//! Questions in a terminal. Secret-looking keys (see `Entry::masked`) show one `*` per
//! character and are typed twice (a typo shows up now, not when the application fails to
//! log in); every other key is typed visibly, once, starting from the template's value,
//! which can be edited in place. Encryption is decided elsewhere.

use std::env;
use std::io::{self, IsTerminal};
use std::path::PathBuf;

use anyhow::{Result, bail};
use rustyline::config::{Behavior, Config};
use rustyline::error::ReadlineError;

use super::source::{self, Answer};
use crate::error::Abort;
use crate::secret::Secret;

/// The terminal, or a script in tests.
pub trait Prompter {
    /// Typed visibly; the line starts out holding `initial`, editable.
    fn visible(&mut self, label: &str, initial: &str) -> Result<String>;
    /// Typed as stars.
    fn masked(&mut self, label: &str) -> Result<Secret>;
    /// A line of guidance (stderr), never a value.
    fn say(&mut self, text: &str);
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
            Err(ReadlineError::Eof) => bail!("input berakhir sebelum pertanyaan dijawab"),
            Err(err) => Err(err.into()),
        }
    }

    fn masked(&mut self, label: &str) -> Result<Secret> {
        let config = rpassword::ConfigBuilder::new()
            .password_feedback_mask('*')
            .build();
        Ok(Secret::new(rpassword::prompt_password_with_config(
            label, config,
        )?))
    }

    fn say(&mut self, text: &str) {
        eprintln!("{text}");
    }
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
        "jendela bawaan Git Bash (mintty) tidak bisa menampilkan pertanyaan; \
         pakai Windows Terminal, PowerShell, atau cmd",
    )
}

/// `y`/`ya` or `n`/`tidak`; Enter takes `default`.
pub fn confirm(prompter: &mut dyn Prompter, question: &str, default: bool) -> Result<bool> {
    let choices = if default { "[Y/n]" } else { "[y/N]" };
    loop {
        let reply = prompter.visible(&format!("{question} {choices} "), "")?;
        match reply.trim().to_lowercase().as_str() {
            "" => return Ok(default),
            "y" | "ya" | "yes" => return Ok(true),
            "n" | "t" | "tidak" | "no" => return Ok(false),
            _ => prompter.say("Jawab y (ya) atau n (tidak)."),
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
    prompter.say("");
    prompter.say(key);
    for line in &question.help {
        prompter.say(&format!("  {line}"));
    }
    let mut hints = vec![if question.masked {
        "rahasia: tampil sebagai *, diketik dua kali".to_string()
    } else {
        "terlihat saat diketik".to_string()
    }];
    hints.push("@lokasi-file = isi dari file".into());
    if default.is_some() {
        hints.push("nilai bawaan sudah terisi: Enter = pakai, Backspace = ganti".into());
    }
    if question.optional {
        hints.push("kosongkan = biarkan kosong".into());
    }
    prompter.say(&format!("  ({})", hints.join("; ")));

    let label = format!("  Isi {key}: ");
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
            prompter.say(&format!("  {key} wajib diisi."));
            continue;
        }
        let answer = match source::interpret(typed.expose()) {
            Ok(answer) => answer,
            Err(fail) => {
                prompter.say(&format!("  {fail}"));
                continue;
            }
        };
        match answer {
            Answer::File(path) => match source::read_file(&path) {
                Ok(value) => return Ok(Reply::Value(value, Some(path))),
                Err(fail) => prompter.say(&format!("  {fail}")),
            },
            Answer::Text(value) => {
                if let Err(fail) = source::check_text(&value, key) {
                    prompter.say(&format!("  {fail}"));
                    continue;
                }
                if question.masked {
                    let again = prompter.masked(&format!("  Ulangi {key}: "))?;
                    if again != typed {
                        prompter.say("  Tidak sama. Ulangi.");
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
            help: vec!["Bantuan"],
            masked,
            optional,
            default,
        }
    }

    fn value(reply: Reply) -> String {
        match reply {
            Reply::Value(secret, _) => secret.expose().to_string(),
            Reply::Empty => "<kosong>".into(),
        }
    }

    #[test]
    fn secrets_are_typed_twice_until_they_match() {
        let mut script = Script::new(&["abc", "abd", "abc", "abc"]);
        let reply = ask(&mut script, &question(true, false, None)).unwrap();
        assert_eq!(value(reply), "abc");
        assert!(script.log.iter().any(|l| l.contains("Tidak sama")));
        assert!(
            script.log.iter().all(|l| !l.contains("abc")),
            "never echoed"
        );
        assert!(script.log.iter().any(|l| l == "  Bantuan"));
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
        assert!(script.log.iter().any(|l| l.contains("Enter = pakai")));

        // Cleared on a required key: asked again, prefilled again.
        let mut script = Script::new(&["", "3000"]);
        let reply = ask(&mut script, &question(false, false, Some("8899"))).unwrap();
        assert_eq!(value(reply), "3000");
        assert!(script.log.iter().any(|l| l.contains("wajib diisi")));
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
        assert!(script.log.iter().any(|l| l.contains("wajib diisi")));
    }

    #[test]
    fn masked_keys_never_show_or_take_the_default() {
        let mut script = Script::new(&["", "baru", "baru"]);
        let reply = ask(&mut script, &question(true, false, Some("rahasia-bawaan"))).unwrap();
        assert_eq!(value(reply), "baru");
        assert!(script.log.iter().any(|l| l.contains("wajib diisi")));
        assert!(
            script
                .log
                .iter()
                .all(|l| !l.contains("rahasia-bawaan") && !l.starts_with("initial:")),
            "{:?}",
            script.log
        );
    }

    #[test]
    fn an_unedited_default_is_taken_literally() {
        let mut script = Script::new(&["@bukan-file"]);
        let reply = ask(&mut script, &question(false, false, Some("@bukan-file"))).unwrap();
        assert_eq!(reply, Reply::Value(Secret::from("@bukan-file"), None));
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
        assert!(script.log.iter().any(|l| l.contains("tidak ditemukan")));
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
        let mut script = Script::new(&["", "maybe", "ya"]);
        assert!(confirm(&mut script, "Lanjut?", true).unwrap());
        assert!(confirm(&mut script, "Lanjut?", false).unwrap());
        let mut script = Script::new(&["n"]);
        assert!(!confirm(&mut script, "Lanjut?", true).unwrap());
    }
}
