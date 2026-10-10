//! The `setup` form: every empty key on one screen, the list on the left and the selected
//! key on the right. This file is the state and the keyboard; `view` draws it and
//! `screen` owns the terminal. Answers follow the same rules as `prompt::ask`: secret keys
//! are typed as stars and twice, `@path` reads a file, the template's value is prefilled
//! on visible keys only.

mod screen;
mod view;

use std::fs;
use std::path::PathBuf;

use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use zeroize::{Zeroize, Zeroizing};

pub use screen::run;

use super::prompt::Reply;
use super::source::{self, Answer};
use crate::secret::Secret;

/// Files larger than this are not read while the path is still being typed (Enter still
/// reads them).
const PROBE_LIMIT: u64 = 1024 * 1024;

/// One key to ask about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub key: String,
    /// Comment lines from the template.
    pub help: Vec<String>,
    /// Typed as stars and twice (SECRET) instead of visibly and once (VISIBLE).
    pub masked: bool,
    /// Stored as is (PLAIN) instead of encrypted (ENCRYPTED).
    pub plain: bool,
    /// May be left empty (OPTIONAL) instead of required (REQUIRED).
    pub optional: bool,
    /// The template's value, prefilled. Always `None` for masked keys.
    pub default: Option<String>,
}

/// What the screen is showing on top of the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Typing the value of the selected key.
    Edit,
    /// Typing a secret again, to catch a typo.
    Repeat,
    /// All answers, before saving.
    Review { save: bool, scroll: u16 },
    /// "Cancel without saving?"
    ConfirmCancel { back: Back },
    /// The legend of labels and keys.
    Help { back: Back },
}

/// Where a popup returns to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Back {
    Edit,
    Review,
}

/// A line under the input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Guidance, shown faint.
    Hint(String),
    /// A rejected answer, shown in yellow.
    Problem(String),
    /// An `@path` whose file can be read.
    FileOk(String),
}

/// What the caller does after a key press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Continue,
    Save,
    Cancel,
}

pub struct Form {
    pub fields: Vec<Field>,
    /// `None` while unanswered: saved as empty.
    pub answers: Vec<Option<Reply>>,
    pub selected: usize,
    pub mode: Mode,
    pub input: LineInput,
    /// The second typing of a secret.
    pub again: LineInput,
    /// The first typing of a secret, while `again` is typed.
    first: Option<Secret>,
    /// Edited since the key was selected.
    dirty: bool,
    pub problem: Option<String>,
    /// The last `@path` checked and what was found, so typing does not reread it.
    probe: Option<(String, Result<String, String>)>,
}

impl Form {
    pub fn new(fields: Vec<Field>) -> Self {
        let answers = fields.iter().map(|_| None).collect();
        let mut form = Self {
            fields,
            answers,
            selected: 0,
            mode: Mode::Edit,
            input: LineInput::default(),
            again: LineInput::default(),
            first: None,
            dirty: false,
            problem: None,
            probe: None,
        };
        form.select(0);
        form
    }

    pub fn field(&self) -> &Field {
        &self.fields[self.selected]
    }

    pub fn answered(&self) -> usize {
        self.answers.iter().filter(|a| a.is_some()).count()
    }

    /// The answers in field order; `None` stays empty.
    pub fn into_answers(self) -> Vec<Option<Reply>> {
        self.answers
    }

    pub fn handle(&mut self, event: Event) -> Step {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => self.key(key),
            Event::Paste(mut text) => {
                self.paste(&text);
                text.zeroize();
                Step::Continue
            }
            _ => Step::Continue,
        }
    }

    fn key(&mut self, key: KeyEvent) -> Step {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        if ctrl && !alt && key.code == KeyCode::Char('c') {
            return Step::Cancel;
        }
        match self.mode {
            Mode::Edit | Mode::Repeat => self.edit_key(key, ctrl, alt),
            Mode::Review { save, scroll } => self.review_key(key, save, scroll),
            Mode::ConfirmCancel { back } => match key.code {
                KeyCode::Char('y' | 'Y') => Step::Cancel,
                KeyCode::Char('n' | 'N') | KeyCode::Esc | KeyCode::Enter => {
                    self.mode = back.mode();
                    Step::Continue
                }
                _ => Step::Continue,
            },
            Mode::Help { back } => {
                self.mode = back.mode();
                Step::Continue
            }
        }
    }

    fn edit_key(&mut self, key: KeyEvent, ctrl: bool, alt: bool) -> Step {
        match key.code {
            KeyCode::Esc => self.mode = Mode::ConfirmCancel { back: Back::Edit },
            KeyCode::F(1) => self.mode = Mode::Help { back: Back::Edit },
            KeyCode::Enter => {
                if self.commit() == Commit::Done {
                    self.advance();
                }
            }
            KeyCode::Up | KeyCode::BackTab => self.select(self.selected.saturating_sub(1)),
            KeyCode::Down | KeyCode::Tab => self.select(self.selected + 1),
            KeyCode::Char('s') if ctrl && !alt => {
                if !self.dirty || self.commit() == Commit::Done {
                    self.review();
                }
            }
            KeyCode::Char('u') if ctrl && !alt => self.edit(LineInput::clear),
            // AltGr arrives as Ctrl+Alt on Windows: `@` on many layouts.
            KeyCode::Char(c) if ctrl == alt => self.edit(|line| line.insert(c)),
            KeyCode::Backspace => self.edit(LineInput::backspace),
            KeyCode::Delete => self.edit(LineInput::delete),
            KeyCode::Left => self.active().left(),
            KeyCode::Right => self.active().right(),
            KeyCode::Home => self.active().home(),
            KeyCode::End => self.active().end(),
            _ => {}
        }
        Step::Continue
    }

    fn review_key(&mut self, key: KeyEvent, save: bool, scroll: u16) -> Step {
        match key.code {
            KeyCode::Enter if save => return Step::Save,
            KeyCode::Enter => self.back_to_edit(),
            KeyCode::Char('s' | 'S' | 'y' | 'Y') => return Step::Save,
            KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
                self.mode = Mode::Review {
                    save: !save,
                    scroll,
                };
            }
            KeyCode::Up => {
                self.mode = Mode::Review {
                    save,
                    scroll: scroll.saturating_sub(1),
                };
            }
            KeyCode::Down => {
                self.mode = Mode::Review {
                    save,
                    scroll: scroll.saturating_add(1),
                };
            }
            KeyCode::Esc => self.mode = Mode::ConfirmCancel { back: Back::Review },
            KeyCode::F(1) => self.mode = Mode::Help { back: Back::Review },
            _ => {}
        }
        Step::Continue
    }

    fn paste(&mut self, text: &str) {
        if !matches!(self.mode, Mode::Edit | Mode::Repeat) {
            return;
        }
        let text = source::strip_one_newline(text);
        if text.contains(['\n', '\r']) {
            self.problem = Some(
                "The pasted text has several lines. For a multi-line value, \
                 save it to a file, then type @file-path."
                    .into(),
            );
            return;
        }
        self.edit(|line| text.chars().for_each(|c| line.insert(c)));
    }

    /// The line being typed: `again` while a secret is repeated.
    fn active(&mut self) -> &mut LineInput {
        if self.mode == Mode::Repeat {
            &mut self.again
        } else {
            &mut self.input
        }
    }

    fn edit(&mut self, change: impl FnOnce(&mut LineInput)) {
        change(self.active());
        self.dirty = true;
        self.problem = None;
        if self.mode == Mode::Edit {
            self.probe_file();
        }
    }

    /// Moves to field `index` (clamped). What was typed and not committed is dropped; the
    /// input starts from the current answer, or the template's value.
    pub fn select(&mut self, index: usize) {
        self.selected = index.min(self.fields.len().saturating_sub(1));
        self.mode = Mode::Edit;
        self.first = None;
        self.again.clear();
        self.dirty = false;
        self.problem = None;
        let field = &self.fields[self.selected];
        let start = match &self.answers[self.selected] {
            _ if field.masked => String::new(),
            Some(Reply::Value(_, Some(path))) => format!("@{}", path.display()),
            Some(Reply::Value(value, None)) => value.expose().to_string(),
            Some(Reply::Empty) => String::new(),
            None => field.default.clone().unwrap_or_default(),
        };
        self.input.set(&start);
        self.probe_file();
    }

    fn advance(&mut self) {
        if self.selected + 1 < self.fields.len() {
            self.select(self.selected + 1);
        } else {
            self.review();
        }
    }

    fn review(&mut self) {
        self.input.clear();
        self.again.clear();
        self.first = None;
        self.mode = Mode::Review {
            save: true,
            scroll: 0,
        };
    }

    fn back_to_edit(&mut self) {
        self.select(self.selected);
    }

    /// Checks the typed line and, when it is a complete answer, stores it.
    fn commit(&mut self) -> Commit {
        if self.mode == Mode::Repeat {
            return self.commit_repeat();
        }
        let field = &self.fields[self.selected];
        let typed = Secret::from(self.input.text());
        // Kept as is: the template's value is used literally, even when it starts with `@`.
        if let Some(default) = &field.default
            && typed.expose() == default
        {
            return self.store(Reply::Value(typed, None));
        }
        if typed.is_empty() {
            if field.masked && self.answers[self.selected].is_some() {
                return Commit::Done;
            }
            if field.optional {
                return self.store(Reply::Empty);
            }
            return self.reject(format!("{} is required.", field.key));
        }
        let answer = match source::interpret(typed.expose()) {
            Ok(answer) => answer,
            Err(fail) => return self.reject(fail.to_string()),
        };
        match answer {
            Answer::File(path) => match source::read_file(&path) {
                Ok(value) => self.store(Reply::Value(value, Some(path))),
                Err(fail) => self.reject(fail.to_string()),
            },
            Answer::Text(value) => {
                if let Err(fail) = source::check_text(&value, &field.key) {
                    return self.reject(fail.to_string());
                }
                if field.masked {
                    self.first = Some(value);
                    self.again.clear();
                    self.mode = Mode::Repeat;
                    return Commit::Pending;
                }
                self.store(Reply::Value(value, None))
            }
        }
    }

    fn commit_repeat(&mut self) -> Commit {
        let again = Secret::from(self.again.text());
        match self.first.take() {
            Some(first) if first == again => self.store(Reply::Value(first, None)),
            _ => {
                self.input.clear();
                self.again.clear();
                self.mode = Mode::Edit;
                self.reject("Not the same. Type it again from the start.".into())
            }
        }
    }

    fn store(&mut self, reply: Reply) -> Commit {
        self.answers[self.selected] = Some(reply);
        self.dirty = false;
        Commit::Done
    }

    fn reject(&mut self, problem: String) -> Commit {
        self.problem = Some(problem);
        Commit::Rejected
    }

    /// The line under the input: a problem, the `@path` check, or what Enter does.
    pub fn status(&self) -> Status {
        if let Some(problem) = &self.problem {
            return Status::Problem(problem.clone());
        }
        let field = self.field();
        if self.mode == Mode::Repeat {
            return Status::Hint("Type it once more; it must be the same.".into());
        }
        // The template's value is taken literally, so an `@` in it names no file.
        let unedited = field.default.as_deref() == Some(self.input.text());
        if let Some((_, found)) = &self.probe
            && !unedited
            && file_path(self.input.text()).is_some()
        {
            return match found {
                Ok(text) => Status::FileOk(text.clone()),
                Err(problem) => Status::Problem(problem.clone()),
            };
        }
        let answered = self.answers[self.selected].is_some();
        let hint = match &field.default {
            _ if field.masked && answered => {
                "Already filled. Enter = keep, or type a new value to replace it.".to_string()
            }
            Some(default) if self.input.text() == default => {
                format!("Enter = use {default} · Backspace = edit")
            }
            _ if field.optional => "May be left empty: clear it, then press Enter.".to_string(),
            _ => "Type @file-path to fill it from a file.".to_string(),
        };
        Status::Hint(hint)
    }

    /// Checks the file of an `@path` being typed. The contents are never shown.
    fn probe_file(&mut self) {
        let Some(path) = file_path(self.input.text()) else {
            self.probe = None;
            return;
        };
        if self.probe.as_ref().is_some_and(|(seen, _)| seen == path) {
            return;
        }
        let found = probe(path);
        self.probe = Some((path.to_string(), found));
    }
}

/// The path of an `@path` answer; `None` for text, including `@@`.
fn file_path(typed: &str) -> Option<&str> {
    if typed.starts_with("@@") {
        return None;
    }
    typed.strip_prefix('@').map(str::trim)
}

fn probe(path: &str) -> Result<String, String> {
    if path.is_empty() {
        return Err("Write the file path after the @.".into());
    }
    let meta = match fs::metadata(path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err("✗ File not found.".into());
        }
        Err(_) => return Err("✗ The file cannot be opened.".into()),
    };
    if meta.is_dir() {
        return Err("✗ That is a folder, not a file.".into());
    }
    if !meta.is_file() {
        return Err("✗ Not a regular file.".into());
    }
    if meta.len() > PROBE_LIMIT {
        return Ok(format!(
            "✓ File found ({} KB). Its contents are used.",
            meta.len() / 1024
        ));
    }
    match source::read_file(&PathBuf::from(path)) {
        Ok(value) => {
            let lines = value.expose().lines().count().max(1);
            Ok(format!(
                "✓ File found ({lines} lines). Its contents are used."
            ))
        }
        Err(fail) => Err(format!("✗ {fail}")),
    }
}

impl Back {
    fn mode(self) -> Mode {
        match self {
            Self::Edit => Mode::Edit,
            Self::Review => Mode::Review {
                save: true,
                scroll: 0,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Commit {
    Done,
    /// A secret waits for its second typing.
    Pending,
    Rejected,
}

/// One editable line, wiped from memory when cleared or dropped. It grows by copying into
/// a new wiped buffer, so no unwiped copy is left behind by a reallocation.
pub struct LineInput {
    text: Zeroizing<String>,
    /// In characters, not bytes.
    cursor: usize,
}

impl Default for LineInput {
    fn default() -> Self {
        Self {
            text: Zeroizing::new(String::with_capacity(256)),
            cursor: 0,
        }
    }
}

impl LineInput {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn len(&self) -> usize {
        self.text.chars().count()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    fn set(&mut self, text: &str) {
        self.clear();
        self.reserve(text.len());
        self.text.push_str(text);
        self.cursor = self.len();
    }

    fn clear(&mut self) {
        self.text.zeroize();
        self.cursor = 0;
    }

    fn reserve(&mut self, extra: usize) {
        if self.text.len() + extra <= self.text.capacity() {
            return;
        }
        let mut grown = Zeroizing::new(String::with_capacity((self.text.len() + extra) * 2));
        grown.push_str(&self.text);
        self.text = grown;
    }

    fn byte_at(&self, cursor: usize) -> usize {
        self.text
            .char_indices()
            .nth(cursor)
            .map_or(self.text.len(), |(i, _)| i)
    }

    fn insert(&mut self, c: char) {
        self.reserve(c.len_utf8());
        let at = self.byte_at(self.cursor);
        self.text.insert(at, c);
        self.cursor += 1;
    }

    fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            let at = self.byte_at(self.cursor);
            self.text.remove(at);
        }
    }

    fn delete(&mut self) {
        if self.cursor < self.len() {
            let at = self.byte_at(self.cursor);
            self.text.remove(at);
        }
    }

    fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.len());
    }

    fn home(&mut self) {
        self.cursor = 0;
    }

    fn end(&mut self) {
        self.cursor = self.len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn field(key: &str, masked: bool, optional: bool, default: Option<&str>) -> Field {
        Field {
            key: key.into(),
            help: vec![format!("Help for {key}")],
            masked,
            plain: false,
            optional,
            default: default.map(String::from),
        }
    }

    fn press(form: &mut Form, code: KeyCode) -> Step {
        form.handle(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)))
    }

    fn ctrl(form: &mut Form, c: char) -> Step {
        form.handle(Event::Key(KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::CONTROL,
        )))
    }

    fn typed(form: &mut Form, text: &str) {
        for c in text.chars() {
            press(form, KeyCode::Char(c));
        }
    }

    /// Types `text` and presses Enter.
    fn answer(form: &mut Form, text: &str) -> Step {
        typed(form, text);
        press(form, KeyCode::Enter)
    }

    fn value(reply: &Option<Reply>) -> String {
        match reply {
            Some(Reply::Value(secret, _)) => secret.expose().to_string(),
            Some(Reply::Empty) => "<empty>".into(),
            None => "<unanswered>".into(),
        }
    }

    #[test]
    fn enter_moves_to_the_next_key_then_to_the_review() {
        let mut form = Form::new(vec![
            field("HOST", false, false, None),
            field("PORT", false, false, None),
        ]);
        answer(&mut form, "db");
        assert_eq!(form.selected, 1);
        assert_eq!(form.mode, Mode::Edit);
        answer(&mut form, "5432");
        assert_eq!(
            form.mode,
            Mode::Review {
                save: true,
                scroll: 0
            }
        );
        assert_eq!(press(&mut form, KeyCode::Enter), Step::Save);
        let answers = form.into_answers();
        assert_eq!(value(&answers[0]), "db");
        assert_eq!(value(&answers[1]), "5432");
    }

    #[test]
    fn secrets_are_typed_twice_until_they_match() {
        let mut form = Form::new(vec![field("DB_PASSWORD", true, false, None)]);
        answer(&mut form, "abc");
        assert_eq!(form.mode, Mode::Repeat);
        answer(&mut form, "abd");
        assert_eq!(form.mode, Mode::Edit);
        assert!(matches!(form.status(), Status::Problem(p) if p.contains("Not the same")));
        assert!(form.input.is_empty(), "both typings start over");
        answer(&mut form, "abc");
        answer(&mut form, "abc");
        assert!(matches!(form.mode, Mode::Review { .. }));
        assert_eq!(value(&form.answers[0]), "abc");
    }

    #[test]
    fn required_keys_refuse_an_empty_line_and_optional_keys_take_it() {
        let mut form = Form::new(vec![
            field("A", false, false, None),
            field("B", false, true, Some("info")),
        ]);
        press(&mut form, KeyCode::Enter);
        assert_eq!(form.selected, 0);
        assert!(matches!(form.status(), Status::Problem(p) if p == "A is required."));
        answer(&mut form, "x");
        assert_eq!(
            form.input.text(),
            "info",
            "the template's value is prefilled"
        );
        ctrl(&mut form, 'u');
        press(&mut form, KeyCode::Enter);
        assert_eq!(form.answers[1], Some(Reply::Empty));
    }

    #[test]
    fn prefilled_defaults_are_editable_and_taken_literally() {
        let mut form = Form::new(vec![field("A", false, false, Some("@not-a-file"))]);
        assert!(matches!(form.status(), Status::Hint(h) if h.contains("Enter = use")));
        press(&mut form, KeyCode::Enter);
        assert_eq!(value(&form.answers[0]), "@not-a-file");

        let mut form = Form::new(vec![field("PORT", false, false, Some("8899"))]);
        press(&mut form, KeyCode::Backspace);
        press(&mut form, KeyCode::Home);
        press(&mut form, KeyCode::Delete);
        answer(&mut form, "7");
        assert_eq!(value(&form.answers[0]), "789");
    }

    #[test]
    fn masked_keys_never_start_from_a_value() {
        let mut form = Form::new(vec![
            field("TOKEN", true, false, None),
            field("NEXT", false, false, None),
        ]);
        answer(&mut form, "s3cret");
        answer(&mut form, "s3cret");
        press(&mut form, KeyCode::Up);
        assert_eq!(form.selected, 0);
        assert!(form.input.is_empty(), "a typed secret is never put back");
        assert!(matches!(form.status(), Status::Hint(h) if h.starts_with("Already filled")));
        press(&mut form, KeyCode::Enter);
        assert_eq!(form.selected, 1, "Enter keeps the answer");
        assert_eq!(value(&form.answers[0]), "s3cret");
    }

    #[test]
    fn moving_away_drops_what_was_not_entered() {
        let mut form = Form::new(vec![
            field("A", false, false, None),
            field("B", false, false, None),
        ]);
        typed(&mut form, "half");
        press(&mut form, KeyCode::Down);
        press(&mut form, KeyCode::Up);
        assert!(form.input.is_empty());
        assert_eq!(form.answers[0], None);
    }

    #[test]
    fn ctrl_s_keeps_the_line_being_typed_then_reviews() {
        let mut form = Form::new(vec![
            field("A", false, false, None),
            field("B", false, false, None),
        ]);
        typed(&mut form, "a");
        ctrl(&mut form, 's');
        assert!(matches!(form.mode, Mode::Review { .. }));
        assert_eq!(value(&form.answers[0]), "a");
        assert_eq!(form.answers[1], None, "skipped keys stay unanswered");

        // From the review, back to the list.
        press(&mut form, KeyCode::Right);
        press(&mut form, KeyCode::Enter);
        assert_eq!(form.mode, Mode::Edit);
        assert_eq!(form.input.text(), "a");
    }

    #[test]
    fn files_are_checked_while_typed_and_read_on_enter() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cert.pem");
        fs::write(&path, "A\nB\n").unwrap();
        let mut form = Form::new(vec![field("SSL_CERT", true, false, None)]);

        typed(
            &mut form,
            &format!("@{}", dir.path().join("nope").display()),
        );
        assert!(matches!(form.status(), Status::Problem(p) if p.contains("not found")));
        ctrl(&mut form, 'u');
        typed(&mut form, &format!("@{}", dir.path().display()));
        assert!(matches!(form.status(), Status::Problem(p) if p.contains("folder")));
        ctrl(&mut form, 'u');
        typed(&mut form, &format!("@{}", path.display()));
        assert!(matches!(form.status(), Status::FileOk(t) if t.contains("2 lines")));

        press(&mut form, KeyCode::Enter);
        assert_eq!(
            form.answers[0],
            Some(Reply::Value(Secret::from("A\nB"), Some(path))),
            "a file is not typed twice"
        );
    }

    #[test]
    fn double_at_is_text() {
        let mut form = Form::new(vec![field("A", false, false, None)]);
        typed(&mut form, "@@x");
        assert!(matches!(form.status(), Status::Hint(_)));
        press(&mut form, KeyCode::Enter);
        assert_eq!(value(&form.answers[0]), "@x");
    }

    #[test]
    fn pasting_one_line_inserts_it_and_more_lines_are_refused() {
        let mut form = Form::new(vec![field("A", false, false, None)]);
        form.handle(Event::Paste("abc\n".into()));
        assert_eq!(form.input.text(), "abc");
        form.handle(Event::Paste("x\ny".into()));
        assert_eq!(form.input.text(), "abc");
        assert!(matches!(form.status(), Status::Problem(p) if p.contains("@file-path")));
    }

    #[test]
    fn altgr_types_and_ctrl_c_cancels() {
        let mut form = Form::new(vec![field("A", false, false, None)]);
        form.handle(Event::Key(KeyEvent::new(
            KeyCode::Char('@'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        )));
        assert_eq!(form.input.text(), "@");
        assert_eq!(ctrl(&mut form, 'c'), Step::Cancel);
    }

    #[test]
    fn escape_asks_before_cancelling() {
        let mut form = Form::new(vec![field("A", false, false, None)]);
        typed(&mut form, "keep");
        press(&mut form, KeyCode::Esc);
        assert_eq!(form.mode, Mode::ConfirmCancel { back: Back::Edit });
        press(&mut form, KeyCode::Char('n'));
        assert_eq!(form.mode, Mode::Edit);
        assert_eq!(form.input.text(), "keep");
        press(&mut form, KeyCode::Esc);
        assert_eq!(press(&mut form, KeyCode::Char('y')), Step::Cancel);
    }

    #[test]
    fn key_releases_are_ignored() {
        let mut form = Form::new(vec![field("A", false, false, None)]);
        let mut release = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
        release.kind = KeyEventKind::Release;
        form.handle(Event::Key(release));
        assert!(form.input.is_empty());
    }

    #[test]
    fn line_input_edits_by_character() {
        let mut line = LineInput::default();
        line.set("añb");
        line.left();
        line.backspace();
        assert_eq!(line.text(), "ab");
        line.home();
        line.insert('é');
        assert_eq!(line.text(), "éab");
        assert_eq!(line.cursor(), 1);
        let long = "x".repeat(1000);
        line.set(&long);
        line.insert('y');
        assert_eq!(line.len(), 1001);
    }
}
