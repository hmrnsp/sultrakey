//! `check`: everything `run` needs, checked at once; every problem is reported, not just
//! the first. `run` uses [`inspect`] before starting the application.
//!
//! On a terminal, `check` also draws a table (ratatui, printed once) with the result for
//! each key and for the key file. The problems themselves always go to stderr as the usual
//! `✗ ...` / `Fix: ...` lines, which logs and scripts read.

use std::io::{self, IsTerminal};
use std::path::PathBuf;

use anyhow::Result;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::text::Span;
use ratatui::widgets::{Cell, Row, Table, Widget};

use super::{Ctx, header, key_location, load_env, mismatch, missing_key_hint};
use crate::crypto::{self, DecryptFailure};
use crate::envfile::Value;
use crate::error::{Code, Fail, Issue};
use crate::keyfile;
use crate::output;
use crate::secret::Secret;
use crate::system;

/// The decrypted values, in file order. Empty optional keys are `""` (D3).
pub struct Opened {
    pub values: Vec<(String, Secret)>,
    pub key_path: PathBuf,
}

/// What was found for one key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Decrypted,
    Plain,
    EmptyOptional,
    Empty,
    /// A secret key holding a plain value.
    StoredPlain,
    CannotDecrypt(&'static str),
    /// Encrypted, but the key file cannot be used, so it was not tried.
    NotChecked,
}

/// What was found for the key file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyState {
    Ok,
    TooOpen(u32),
    Mismatch,
    Unreadable,
}

/// Everything `check` found, problems included.
struct Report {
    values: Vec<(String, Secret)>,
    key_path: PathBuf,
    key: KeyState,
    keys: Vec<(String, Outcome)>,
    issues: Vec<Issue>,
}

/// Fails right away only when there is nothing to check: no `.env`, no header, no key path.
fn examine(ctx: &Ctx) -> Result<Report, Fail> {
    let doc = load_env(ctx)?;
    let header = header(ctx, &doc)?;
    let mut issues: Vec<Issue> = Vec::new();

    let located = key_location(ctx, &header.app)?;
    let key_path = located.path;
    let mut key = KeyState::Ok;
    if let Ok(Some(mode)) = system::open_to_others(&key_path) {
        key = KeyState::TooOpen(mode);
        issues.push(Issue::new(
            format!(
                "Key file {} is too open ({mode:o}); other users on this server can read it.",
                key_path.display()
            ),
            format!("sudo chmod 400 {}", key_path.display()),
        ));
    }
    let identity = match keyfile::read(&key_path) {
        Ok(identity) if crypto::public_key(&identity) == header.public_key => Some(identity),
        Ok(_) => {
            key = KeyState::Mismatch;
            issues.extend(mismatch(ctx, &key_path).issues);
            None
        }
        Err(err) => {
            key = KeyState::Unreadable;
            let hint = missing_key_hint(ctx, &header.app, &key_path);
            issues.extend(keyfile::read_fail(err, &key_path, &hint).issues);
            None
        }
    };

    let mut values = Vec::new();
    let mut keys = Vec::new();
    for entry in doc.entries() {
        let key = &entry.key;
        let outcome = match &entry.value {
            Value::Empty if entry.flags.optional => {
                values.push((key.clone(), Secret::default()));
                Outcome::EmptyOptional
            }
            Value::Empty => {
                issues.push(Issue::new(format!("{key} is empty."), ctx.cmd("setup")));
                Outcome::Empty
            }
            Value::Plain(value) if entry.flags.plain => {
                values.push((key.clone(), value.clone()));
                Outcome::Plain
            }
            Value::Plain(_) => {
                issues.push(Issue::new(
                    format!("{key} is stored plain (not encrypted), but it is not @plain."),
                    format!("{} (it encrypts that value)", ctx.cmd("setup")),
                ));
                Outcome::StoredPlain
            }
            Value::Encrypted(b64) => match &identity {
                None => Outcome::NotChecked,
                Some(identity) => match crypto::decrypt(identity, b64) {
                    Ok(value) => {
                        values.push((key.clone(), value));
                        Outcome::Decrypted
                    }
                    Err(failure) => {
                        let why = match failure {
                            DecryptFailure::NotBase64 => "not base64",
                            DecryptFailure::CannotOpen => "broken, or encrypted with another key",
                            DecryptFailure::NotText => "not text",
                        };
                        issues.push(Issue::new(
                            format!("{key} cannot be decrypted ({why})."),
                            ctx.cmd(&format!("set {key}")),
                        ));
                        Outcome::CannotDecrypt(why)
                    }
                },
            },
        };
        keys.push((key.clone(), outcome));
    }

    Ok(Report {
        values,
        key_path,
        key,
        keys,
        issues,
    })
}

pub fn inspect(ctx: &Ctx) -> Result<Opened, Fail> {
    let report = examine(ctx)?;
    if report.issues.is_empty() {
        Ok(Opened {
            values: report.values,
            key_path: report.key_path,
        })
    } else {
        Err(Fail {
            code: Code::Config,
            issues: report.issues,
        })
    }
}

pub fn run(ctx: &Ctx) -> Result<i32> {
    let report = examine(ctx)?;
    if io::stdout().is_terminal() {
        let width = ratatui::crossterm::terminal::size().map_or(80, |(cols, _)| cols);
        output::print(&table(&report, width, output::stdout_styled()))?;
    }
    if !report.issues.is_empty() {
        return Err(Fail {
            code: Code::Config,
            issues: report.issues,
        }
        .into());
    }
    if cfg!(windows) {
        output::info("Key file permissions are not checked on Windows.");
    }
    if system::writable_by_others(&ctx.env).unwrap_or(false) {
        output::warn(&format!(
            "{} can be changed by other users (anyone who can write .env can replace values).",
            ctx.env.display()
        ));
        output::fix(&format!("sudo chmod 600 {}", ctx.env.display()));
    }
    if let Ok(Some(template)) = super::read_doc(&ctx.template)
        && let Ok(doc) = load_env(ctx)
    {
        let new: Vec<&str> = template
            .entries()
            .filter(|entry| doc.get(&entry.key).is_none())
            .map(|entry| entry.key.as_str())
            .collect();
        if !new.is_empty() {
            output::warn(&format!(
                "The template has keys that are not in .env yet: {}",
                new.join(", ")
            ));
            output::fix(&ctx.cmd("setup"));
        }
    }
    output::ok(&format!(
        "All good ({} keys, key file: {}).",
        report.values.len(),
        report.key_path.display()
    ));
    Ok(0)
}

/// The result for one key: passed green, failed yellow, not needed or not tried dim.
fn outcome_cell(outcome: Outcome) -> Span<'static> {
    match outcome {
        Outcome::Decrypted => Span::from("✓ decrypted").green(),
        Outcome::Plain => Span::from("✓ plain").green(),
        Outcome::EmptyOptional => Span::from("○ empty (optional)").dark_gray(),
        Outcome::Empty => Span::from("✗ empty").yellow(),
        Outcome::StoredPlain => Span::from("✗ stored plain, must be encrypted").yellow(),
        Outcome::CannotDecrypt(why) => {
            Span::from(format!("✗ cannot be decrypted ({why})")).yellow()
        }
        Outcome::NotChecked => Span::from("· not checked (key file unusable)").dark_gray(),
    }
}

fn key_cell(key: KeyState) -> Span<'static> {
    match key {
        KeyState::Ok => Span::from("✓ readable, matches .env").green(),
        KeyState::TooOpen(mode) => Span::from(format!("✗ too open ({mode:o})")).yellow(),
        KeyState::Mismatch => Span::from("✗ does not belong to .env").yellow(),
        KeyState::Unreadable => Span::from("✗ cannot be read").yellow(),
    }
}

/// The per-key table, then the key file, as text to print (`color`: with SGR codes).
fn table(report: &Report, width: u16, color: bool) -> String {
    let cells: Vec<(String, Span<'static>)> = report
        .keys
        .iter()
        .map(|(key, outcome)| (key.clone(), outcome_cell(*outcome)))
        .collect();
    let key_width = cells
        .iter()
        .map(|(key, _)| key.chars().count())
        .chain(["KEY FILE".len()])
        .max()
        .unwrap_or(0);
    let result_width = cells
        .iter()
        .map(|(_, cell)| cell.width())
        .chain([key_cell(report.key).width()])
        .max()
        .unwrap_or(0)
        .max("RESULT".len());
    let spacing = 3;
    let needed = 1 + key_width + spacing + result_width;
    let table_width = width.min(needed.min(usize::from(u16::MAX)) as u16).max(20);
    let draw = |rows: Vec<Row<'static>>, header: Option<Row<'static>>| {
        let height = rows.len() as u16 + u16::from(header.is_some());
        let mut buffer = Buffer::empty(Rect::new(0, 0, table_width, height));
        let mut table = Table::new(
            rows,
            [
                Constraint::Length(key_width as u16),
                Constraint::Length(result_width as u16),
            ],
        )
        .column_spacing(spacing as u16);
        if let Some(header) = header {
            table = table.header(header);
        }
        table.render(Rect::new(1, 0, table_width - 1, height), &mut buffer);
        output::buffer_text(&buffer, color)
    };

    let header = Row::new(["KEY", "RESULT"]).style(Style::new().bold());
    let rows = cells
        .into_iter()
        .map(|(key, cell)| Row::new([Cell::from(key), Cell::from(cell)]))
        .collect();
    let key_row = Row::new([
        Cell::from(Span::from("KEY FILE").bold()),
        Cell::from(key_cell(report.key)),
    ]);
    let mut text = draw(rows, Some(header));
    text.push('\n');
    text.push_str(&draw(vec![key_row], None));
    // The key file's path under its result, then a blank line before what follows.
    let indent = 1 + key_width + spacing;
    text.push_str(&format!("{:indent$}{}\n\n", "", report.key_path.display()));
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(key: KeyState, keys: &[(&str, Outcome)]) -> Report {
        Report {
            values: Vec::new(),
            key_path: PathBuf::from("/etc/sultrakey/example-app.key"),
            key,
            keys: keys.iter().map(|(k, o)| (k.to_string(), *o)).collect(),
            issues: Vec::new(),
        }
    }

    #[test]
    fn the_table_shows_each_key_and_the_key_file() {
        let report = report(
            KeyState::Ok,
            &[
                ("PORT", Outcome::Plain),
                ("REDIS_HOST", Outcome::Decrypted),
                ("DB_PASSWORD", Outcome::Empty),
                ("LOG_LEVEL", Outcome::EmptyOptional),
                ("SSL_CERT", Outcome::CannotDecrypt("not base64")),
            ],
        );
        let text = table(&report, 100, false);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], " KEY           RESULT");
        assert_eq!(lines[1], " PORT          ✓ plain");
        assert_eq!(lines[2], " REDIS_HOST    ✓ decrypted");
        assert_eq!(lines[3], " DB_PASSWORD   ✗ empty");
        assert_eq!(lines[4], " LOG_LEVEL     ○ empty (optional)");
        assert_eq!(
            lines[5],
            " SSL_CERT      ✗ cannot be decrypted (not base64)"
        );
        assert_eq!(lines[6], "");
        assert_eq!(lines[7], " KEY FILE      ✓ readable, matches .env");
        assert_eq!(lines[8], "               /etc/sultrakey/example-app.key");
        assert!(text.ends_with("\n\n"), "a blank line before what follows");
        assert!(!text.contains('\x1b'));
    }

    #[test]
    fn an_unusable_key_file_marks_encrypted_keys_not_checked() {
        let report = report(KeyState::Mismatch, &[("REDIS_HOST", Outcome::NotChecked)]);
        let text = table(&report, 100, true);
        assert!(
            text.contains("\x1b[0;33m✗ does not belong to .env"),
            "{text:?}"
        );
        assert!(
            text.contains("\x1b[0;90m· not checked (key file unusable)"),
            "{text:?}"
        );
    }
}
