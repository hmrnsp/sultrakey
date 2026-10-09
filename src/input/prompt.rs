//! Questions in a terminal. Secrets are typed hidden and twice (a typo shows up now, not
//! when the application fails to log in); `@plain` values are typed visibly, once.

use std::env;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::PathBuf;

use anyhow::{Result, bail};

use super::source::{self, Answer};
use crate::secret::Secret;

/// The terminal, or a script in tests.
pub trait Prompter {
    fn visible(&mut self, label: &str) -> Result<String>;
    fn hidden(&mut self, label: &str) -> Result<Secret>;
    /// A line of guidance (stderr), never a value.
    fn say(&mut self, text: &str);
}

pub struct Terminal;

impl Prompter for Terminal {
    fn visible(&mut self, label: &str) -> Result<String> {
        eprint!("{label}");
        io::stderr().flush()?;
        let mut line = String::new();
        if io::stdin().lock().read_line(&mut line)? == 0 {
            bail!("input berakhir sebelum pertanyaan dijawab");
        }
        Ok(source::strip_one_newline(&line).to_string())
    }

    fn hidden(&mut self, label: &str) -> Result<Secret> {
        Ok(Secret::new(rpassword::prompt_password(label)?))
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
        let reply = prompter.visible(&format!("{question} {choices} "))?;
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
    pub plain: bool,
    pub optional: bool,
    /// The template's value, taken on Enter.
    pub default: Option<&'a str>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Reply {
    /// The value, and the file it came from (to remind the user to delete it).
    Value(Secret, Option<PathBuf>),
    /// Enter on an optional key without a default.
    Empty,
}

pub fn ask(prompter: &mut dyn Prompter, question: &Question<'_>) -> Result<Reply> {
    let key = question.key;
    prompter.say("");
    prompter.say(key);
    for line in &question.help {
        prompter.say(&format!("  {line}"));
    }
    let mut hints = vec![if question.plain {
        "terlihat saat diketik".to_string()
    } else {
        "rahasia: ketikan tidak terlihat, diketik dua kali".to_string()
    }];
    hints.push("@lokasi-file = isi dari file".into());
    if let Some(default) = question.default {
        hints.push(format!("Enter = {default}"));
    } else if question.optional {
        hints.push("Enter = biarkan kosong".into());
    }
    prompter.say(&format!("  ({})", hints.join("; ")));

    let label = format!("  Isi {key}: ");
    loop {
        let typed = if question.plain {
            Secret::new(prompter.visible(&label)?)
        } else {
            prompter.hidden(&label)?
        };
        if typed.is_empty() {
            if let Some(default) = question.default {
                return Ok(Reply::Value(Secret::from(default), None));
            }
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
                if !question.plain {
                    let again = prompter.hidden(&format!("  Ulangi {key}: "))?;
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
        fn visible(&mut self, label: &str) -> Result<String> {
            self.next(label, "visible:")
        }
        fn hidden(&mut self, label: &str) -> Result<Secret> {
            Ok(Secret::new(self.next(label, "hidden:")?))
        }
        fn say(&mut self, text: &str) {
            self.log.push(text.to_string());
        }
    }

    fn question(plain: bool, optional: bool, default: Option<&'static str>) -> Question<'static> {
        Question {
            key: "K",
            help: vec!["Bantuan"],
            plain,
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
        let reply = ask(&mut script, &question(false, false, None)).unwrap();
        assert_eq!(value(reply), "abc");
        assert!(script.log.iter().any(|l| l.contains("Tidak sama")));
        assert!(
            script.log.iter().all(|l| !l.contains("abc")),
            "never echoed"
        );
        assert!(script.log.iter().any(|l| l == "  Bantuan"));
    }

    #[test]
    fn plain_values_are_typed_once_and_visible() {
        let mut script = Script::new(&["8080"]);
        let reply = ask(&mut script, &question(true, false, None)).unwrap();
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
    fn enter_takes_the_default_or_leaves_optional_empty() {
        let mut script = Script::new(&[""]);
        let reply = ask(&mut script, &question(false, false, Some("8899"))).unwrap();
        assert_eq!(value(reply), "8899");

        let mut script = Script::new(&[""]);
        assert_eq!(
            ask(&mut script, &question(false, true, None)).unwrap(),
            Reply::Empty
        );

        let mut script = Script::new(&["", "x", "x"]);
        let reply = ask(&mut script, &question(false, false, None)).unwrap();
        assert_eq!(value(reply), "x");
        assert!(script.log.iter().any(|l| l.contains("wajib diisi")));
    }

    #[test]
    fn files_are_read_once_without_repeating() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cert.pem");
        std::fs::write(&path, "A\nB\n").unwrap();
        let at = format!("@{}", path.display());
        let mut script = Script::new(&["@/does/not/exist", &at]);
        let reply = ask(&mut script, &question(false, false, None)).unwrap();
        assert_eq!(reply, Reply::Value(Secret::from("A\nB"), Some(path)));
        assert!(script.log.iter().any(|l| l.contains("tidak ditemukan")));
        assert_eq!(
            script
                .log
                .iter()
                .filter(|l| l.starts_with("hidden:"))
                .count(),
            2
        );
    }

    #[test]
    fn double_at_is_a_literal_value() {
        let mut script = Script::new(&["@@x", "@@x"]);
        let reply = ask(&mut script, &question(false, false, None)).unwrap();
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
