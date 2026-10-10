//! Questions in a terminal. Secret-looking keys (see `Entry::masked`) show one `*` per
//! character and are typed twice (a typo shows up now, not when the application fails to
//! log in); every other key is typed visibly, once, starting from the template's value,
//! which can be edited in place. Encryption is decided elsewhere.

use std::env;
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;

use anyhow::{Result, bail};
use rustyline::config::{Behavior, Config};
use rustyline::error::ReadlineError;

use super::source::{self, Answer};
use crate::error::Abort;
use crate::output;
use crate::secret::Secret;

/// The terminal, or a script in tests.
pub trait Prompter {
    /// Typed visibly; the line starts out holding `initial`, editable.
    fn visible(&mut self, label: &str, initial: &str) -> Result<String>;
    /// Typed as stars.
    fn masked(&mut self, label: &str) -> Result<Secret>;
    /// A line of guidance (stderr), never a secret value.
    fn say(&mut self, text: &str);
    /// Like `say`, for explanations under a key: shown faint (dim italics) on a terminal.
    fn note(&mut self, text: &str) {
        self.say(text);
    }
    /// Like `say`, for an answer that was rejected: shown in yellow on a terminal.
    fn alert(&mut self, text: &str) {
        self.say(text);
    }
    /// `(key, what it does)` pairs, one per line after `indent`, the descriptions lined
    /// up; the keys stand out on a terminal.
    fn keys(&mut self, indent: &str, keys: &[(&str, &str)]) {
        let width = key_width(keys);
        for (key, what) in keys {
            self.say(&format!("{indent}{key:<width$}  {what}"));
        }
    }
}

pub struct Terminal;

impl Prompter for Terminal {
    fn visible(&mut self, label: &str, initial: &str) -> Result<String> {
        // PreferTerm: keep editing on the terminal even when stdout is redirected, like
        // rpassword does. No history: nothing typed is kept or written anywhere.
        let config = Config::builder()
            .behavior(Behavior::PreferTerm)
            .auto_add_history(false)
            .build();
        let mut editor = rustyline::DefaultEditor::with_config(config)?;
        match editor.readline_with_initial(label, (initial, "")) {
            Ok(line) => Ok(line),
            Err(ReadlineError::Interrupted) => Err(Abort::Cancelled.into()),
            Err(ReadlineError::Eof) => bail!("input ended before the question was answered"),
            Err(err) => Err(err.into()),
        }
    }

    fn masked(&mut self, label: &str) -> Result<Secret> {
        // The label goes through stderr, not rpassword: on Windows rpassword writes raw
        // UTF-8 to the console, which shows `›` as `ΓÇ║`.
        let mut stderr = io::stderr();
        write!(stderr, "{label}")?;
        stderr.flush()?;
        let config = rpassword::ConfigBuilder::new()
            .password_feedback_mask('*')
            .build();
        Ok(Secret::new(rpassword::read_password_with_config(config)?))
    }

    fn say(&mut self, text: &str) {
        eprintln!("{text}");
    }

    fn note(&mut self, text: &str) {
        eprintln!("{}", output::faint(text));
    }

    fn alert(&mut self, text: &str) {
        eprintln!("{}", output::caution(text));
    }

    fn keys(&mut self, indent: &str, keys: &[(&str, &str)]) {
        let width = key_width(keys);
        for (key, what) in keys {
            eprintln!("{indent}{}", output::key_hint(key, what, width));
        }
    }
}

fn key_width(keys: &[(&str, &str)]) -> usize {
    keys.iter()
        .map(|(key, _)| key.chars().count())
        .max()
        .unwrap_or(0)
}

/// Prompts need a keyboard (stdin) and a screen to draw on (stderr).
pub fn interactive() -> bool {
    io::stdin().is_terminal() && io::stderr().is_terminal()
}

/// Why prompts are unavailable, when there is a known fix.
pub fn not_interactive_hint() -> Option<&'static str> {
    let mintty = env::var_os("TERM_PROGRAM").is_some_and(|t| t == "mintty");
    let git_bash = env::var_os("MSYSTEM").is_some();
    (cfg!(windows) && (mintty || git_bash)).then_some(
        "the default Git Bash window (mintty) cannot show questions; \
         use Windows Terminal, PowerShell, or cmd",
    )
}

/// `y`/`yes` or `n`/`no`; Enter takes `default`.
pub fn confirm(prompter: &mut dyn Prompter, question: &str, default: bool) -> Result<bool> {
    let choices = if default { "[Y/n]" } else { "[y/N]" };
    loop {
        let reply = prompter.visible(&format!("{question} {choices} "), "")?;
        match reply.trim().to_lowercase().as_str() {
            "" => return Ok(default),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => prompter.alert("Answer y (yes) or n (no)."),
        }
    }
}

/// What to ask about one key.
pub struct Question<'a> {
    pub key: &'a str,
    pub help: Vec<&'a str>,
    /// Stars and typed twice, instead of visible and once.
    pub masked: bool,
    pub optional: bool,
    /// The template's value: prefilled on the input line, editable. Ignored for masked
    /// keys, so it is never shown.
    pub default: Option<&'a str>,
}

/// The keys that work for every question. `defaults`: some input lines start out filled.
pub fn general_keys(defaults: bool) -> Vec<(&'static str, &'static str)> {
    let mut keys = Vec::new();
    if defaults {
        keys.push(("Enter", "keep the current value"));
        keys.push(("Backspace", "edit"));
    }
    keys.push((
        "@file-path",
        "value = the CONTENTS of that file (without @, the path is saved as text)",
    ));
    keys.push(("Ctrl+C", "cancel without saving"));
    keys
}

#[derive(Debug, PartialEq, Eq)]
pub enum Reply {
    /// The value, and the file it came from (to remind the user to delete it).
    Value(Secret, Option<PathBuf>),
    /// An optional key left empty.
    Empty,
}

pub fn ask(prompter: &mut dyn Prompter, question: &Question<'_>) -> Result<Reply> {
    let key = question.key;
    let default = question.default.filter(|_| !question.masked);
    let pad = "  ";
    prompter.say("");
    prompter.say(key);
    for line in &question.help {
        prompter.note(&format!("{pad}{line}"));
    }
    if question.optional {
        prompter.note(&format!("{pad}may be left empty"));
    }
    prompter.keys(pad, &general_keys(default.is_some()));

    // Both labels the same width, so the two rows of stars line up.
    let (label, again_label) = if question.masked {
        (format!("{pad}›        "), format!("{pad}repeat › "))
    } else {
        (format!("{pad}› "), String::new())
    };
    loop {
        let typed = if question.masked {
            prompter.masked(&label)?
        } else {
            Secret::new(prompter.visible(&label, default.unwrap_or(""))?)
        };
        // Kept as is: the template's value is used literally, even when it starts with `@`.
        if let Some(default) = default
            && typed.expose() == default
        {
            return Ok(Reply::Value(typed, None));
        }
        if typed.is_empty() {
            if question.optional {
                return Ok(Reply::Empty);
            }
            prompter.alert(&format!("{pad}{key} is required."));
            continue;
        }
        let answer = match source::interpret(typed.expose()) {
            Ok(answer) => answer,
            Err(fail) => {
                prompter.alert(&format!("{pad}{fail}"));
                continue;
            }
        };
        match answer {
            Answer::File(path) => match source::read_file(&path) {
                Ok(value) => return Ok(Reply::Value(value, Some(path))),
                Err(fail) => prompter.alert(&format!("{pad}{fail}")),
            },
            Answer::Text(value) => {
                if let Err(fail) = source::check_text(&value, key) {
                    prompter.alert(&format!("{pad}{fail}"));
                    continue;
                }
                if question.masked {
                    let again = prompter.masked(&again_label)?;
                    if again != typed {
                        prompter.alert(&format!("{pad}Not the same. Try again."));
                        continue;
                    }
                }
                return Ok(Reply::Value(value, None));
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use std::collections::VecDeque;

    use super::*;

    /// Answers from a script; records what was said and asked.
    #[derive(Default)]
    pub struct Script {
        pub answers: VecDeque<String>,
        pub log: Vec<String>,
    }

    impl Script {
        pub fn new(answers: &[&str]) -> Self {
            Self {
                answers: answers.iter().map(|a| a.to_string()).collect(),
                log: Vec::new(),
            }
        }

        fn next(&mut self, label: &str, kind: &str) -> Result<String> {
            self.log.push(format!("{kind}{label}"));
            self.answers
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("script ran out at {label}"))
        }
    }

    impl Prompter for Script {
        /// The answer is the line as left after editing `initial`.
        fn visible(&mut self, label: &str, initial: &str) -> Result<String> {
            let answer = self.next(label, "visible:");
            if !initial.is_empty() {
                self.log.push(format!("initial:{initial}"));
            }
            answer
        }
        fn masked(&mut self, label: &str) -> Result<Secret> {
            Ok(Secret::new(self.next(label, "masked:")?))
        }
        fn say(&mut self, text: &str) {
            self.log.push(text.to_string());
        }
    }

    fn question(masked: bool, optional: bool, default: Option<&'static str>) -> Question<'static> {
        Question {
            key: "K",
            help: vec!["Help"],
            masked,
            optional,
            default,
        }
    }

    fn value(reply: Reply) -> String {
        match reply {
            Reply::Value(secret, _) => secret.expose().to_string(),
            Reply::Empty => "<empty>".into(),
        }
    }

    #[test]
    fn secrets_are_typed_twice_until_they_match() {
        let mut script = Script::new(&["abc", "abd", "abc", "abc"]);
        let reply = ask(&mut script, &question(true, false, None)).unwrap();
        assert_eq!(value(reply), "abc");
        assert!(script.log.iter().any(|l| l.contains("Not the same")));
        assert!(
            script.log.iter().all(|l| !l.contains("abc")),
            "never echoed"
        );
        assert!(script.log.iter().any(|l| l == "  Help"));
    }

    #[test]
    fn other_values_are_typed_once_and_visible() {
        let mut script = Script::new(&["8080"]);
        let reply = ask(&mut script, &question(false, false, None)).unwrap();
        assert_eq!(value(reply), "8080");
        assert_eq!(
            script
                .log
                .iter()
                .filter(|l| l.starts_with("visible:"))
                .count(),
            1
        );
    }

    #[test]
    fn prefilled_defaults_are_editable() {
        // Enter without editing keeps the template's value.
        let mut script = Script::new(&["8899"]);
        let reply = ask(&mut script, &question(false, false, Some("8899"))).unwrap();
        assert_eq!(value(reply), "8899");
        assert!(script.log.iter().any(|l| l == "initial:8899"));
        assert!(
            script
                .log
                .iter()
                .any(|l| l.contains("keep the current value"))
        );

        // Cleared on a required key: asked again, prefilled again.
        let mut script = Script::new(&["", "3000"]);
        let reply = ask(&mut script, &question(false, false, Some("8899"))).unwrap();
        assert_eq!(value(reply), "3000");
        assert!(script.log.iter().any(|l| l.contains("is required")));
        assert_eq!(
            script.log.iter().filter(|l| *l == "initial:8899").count(),
            2
        );

        // Cleared on an optional key: stays empty.
        let mut script = Script::new(&[""]);
        assert_eq!(
            ask(&mut script, &question(false, true, Some("info"))).unwrap(),
            Reply::Empty
        );
    }

    #[test]
    fn empty_answers_are_required_unless_optional() {
        let mut script = Script::new(&[""]);
        assert_eq!(
            ask(&mut script, &question(true, true, None)).unwrap(),
            Reply::Empty
        );

        let mut script = Script::new(&["", "x", "x"]);
        let reply = ask(&mut script, &question(true, false, None)).unwrap();
        assert_eq!(value(reply), "x");
        assert!(script.log.iter().any(|l| l.contains("is required")));
    }

    #[test]
    fn masked_keys_never_show_or_take_the_default() {
        let mut script = Script::new(&["", "new", "new"]);
        let reply = ask(&mut script, &question(true, false, Some("default-secret"))).unwrap();
        assert_eq!(value(reply), "new");
        assert!(script.log.iter().any(|l| l.contains("is required")));
        assert!(
            script
                .log
                .iter()
                .all(|l| !l.contains("default-secret") && !l.starts_with("initial:")),
            "{:?}",
            script.log
        );
    }

    #[test]
    fn an_unedited_default_is_taken_literally() {
        let mut script = Script::new(&["@not-a-file"]);
        let reply = ask(&mut script, &question(false, false, Some("@not-a-file"))).unwrap();
        assert_eq!(reply, Reply::Value(Secret::from("@not-a-file"), None));
    }

    #[test]
    fn files_are_read_once_without_repeating() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cert.pem");
        std::fs::write(&path, "A\nB\n").unwrap();
        let at = format!("@{}", path.display());
        let mut script = Script::new(&["@/does/not/exist", &at]);
        let reply = ask(&mut script, &question(true, false, None)).unwrap();
        assert_eq!(reply, Reply::Value(Secret::from("A\nB"), Some(path)));
        assert!(script.log.iter().any(|l| l.contains("not found")));
        assert_eq!(
            script
                .log
                .iter()
                .filter(|l| l.starts_with("masked:"))
                .count(),
            2
        );
    }

    #[test]
    fn double_at_is_a_literal_value() {
        let mut script = Script::new(&["@@x", "@@x"]);
        let reply = ask(&mut script, &question(true, false, None)).unwrap();
        assert_eq!(value(reply), "@x");
    }

    #[test]
    fn confirmations() {
        let mut script = Script::new(&["", "maybe", "ya", "yes"]);
        assert!(confirm(&mut script, "Continue?", true).unwrap());
        assert!(confirm(&mut script, "Continue?", false).unwrap());
        let refused = |script: &Script| {
            script
                .log
                .iter()
                .filter(|l| *l == "Answer y (yes) or n (no).")
                .count()
        };
        assert_eq!(refused(&script), 2, "\"ya\" is not an answer");
        let mut script = Script::new(&["tidak", "n"]);
        assert!(!confirm(&mut script, "Continue?", true).unwrap());
        assert_eq!(refused(&script), 1, "\"tidak\" is not an answer");
    }
}
