//! `list`: key names and their status. Never a value. Needs no key file.
//!
//! On a terminal: a table drawn once with ratatui (colored status, how each key is stored and
//! typed, and a summary), left in the scrollback like any other output. Piped or redirected:
//! the plain `KEY  STATUS` columns, unchanged, for scripts.

use std::io::{self, IsTerminal};

use anyhow::Result;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Row, Table, Widget};

use super::{Ctx, load_env};
use crate::envfile::{Document, Entry, Status};
use crate::output;

pub fn run(ctx: &Ctx) -> Result<i32> {
    let doc = load_env(ctx)?;
    if doc.header.is_none() {
        output::warn(&format!(
            "{} is not managed by sultrakey yet (no SULTRAKEY_APP line).",
            ctx.env.display()
        ));
    }
    if doc.entries().next().is_none() {
        output::info("No keys.");
        return Ok(0);
    }
    let text = if io::stdout().is_terminal() {
        let width = ratatui::crossterm::terminal::size().map_or(80, |(cols, _)| cols);
        table(&doc, &ctx.cmd("setup"), width, output::stdout_styled())
    } else {
        plain(&doc)
    };
    output::print(&text)?;
    Ok(0)
}

/// `KEY  STATUS` columns, the format scripts read.
fn plain(doc: &Document) -> String {
    let rows: Vec<(&str, &str)> = doc
        .entries()
        .map(|entry| (entry.key.as_str(), entry.status().label()))
        .collect();
    let width = rows
        .iter()
        .map(|(key, _)| key.len())
        .max()
        .unwrap_or(0)
        .max(3);
    let mut text = format!("{:<width$}  STATUS\n", "KEY");
    for (key, status) in rows {
        text.push_str(&format!("{key:<width$}  {status}\n"));
    }
    text
}

/// The status with a mark, colored: filled green, needs `setup` yellow, optional and empty dim.
fn status_cell(status: Status) -> Span<'static> {
    let text = |mark: &str| format!("{mark} {}", status.label());
    match status {
        Status::Encrypted | Status::Plain => Span::from(text("✓")).green(),
        Status::PlainSecret | Status::EmptyRequired => Span::from(text("✗")).yellow(),
        Status::EmptyOptional => Span::from(text("○")).dark_gray(),
    }
}

/// How the key is stored and typed: the labels of the `setup` screen.
fn label_cells(entry: &Entry) -> [Span<'static>; 2] {
    let stored = if entry.flags.plain {
        Span::from("PLAIN").gray()
    } else {
        Span::from("ENCRYPTED").green()
    };
    let typed = if entry.masked() {
        Span::from("SECRET").yellow()
    } else {
        Span::from("VISIBLE").cyan()
    };
    [stored, typed]
}

fn needs_setup(status: Status) -> bool {
    matches!(status, Status::PlainSecret | Status::EmptyRequired)
}

/// The table and a one-line summary, as text to print (`color`: with SGR codes).
fn table(doc: &Document, setup: &str, width: u16, color: bool) -> String {
    let entries: Vec<&Entry> = doc.entries().collect();
    let key_width = entries
        .iter()
        .map(|entry| entry.key.chars().count())
        .max()
        .unwrap_or(0)
        .max(3);
    let status_width = entries
        .iter()
        .map(|entry| entry.status().label().chars().count() + 2)
        .max()
        .unwrap_or(0)
        .max(6);
    let widths = [
        Constraint::Length(key_width as u16),
        Constraint::Length(status_width as u16),
        Constraint::Length("ENCRYPTED".len() as u16),
        Constraint::Length("VISIBLE".len() as u16),
    ];
    let header = Row::new(["KEY", "STATUS", "STORED", "TYPED"]).style(Style::new().bold());
    let rows = entries.iter().map(|entry| {
        let [stored, typed] = label_cells(entry);
        Row::new([
            Cell::from(entry.key.clone()),
            Cell::from(status_cell(entry.status())),
            Cell::from(stored),
            Cell::from(typed),
        ])
    });
    let spacing = 3;
    let needed = key_width + status_width + "ENCRYPTED".len() + "VISIBLE".len() + 3 * spacing + 1;
    let table_width = width.min(needed.min(usize::from(u16::MAX)) as u16).max(20);
    let area = Rect::new(0, 0, table_width, entries.len() as u16 + 1);
    let mut buffer = Buffer::empty(area);
    // A leading column of one space, like the `setup` screen's padding.
    let inner = Rect::new(1, 0, table_width - 1, area.height);
    Table::new(rows, widths)
        .header(header)
        .column_spacing(spacing as u16)
        .render(inner, &mut buffer);

    let mut text = output::buffer_text(&buffer, color);
    let summary = summary_line(&entries, setup);
    let mut line = Buffer::empty(Rect::new(
        0,
        0,
        table_width.max(summary.width() as u16 + 1),
        1,
    ));
    summary.render(Rect::new(1, 0, line.area.width - 1, 1), &mut line);
    text.push('\n');
    text.push_str(&output::buffer_text(&line, color));
    text
}

/// `5 keys · all set`, or `5 keys · 2 need setup: sultrakey setup`.
fn summary_line(entries: &[&Entry], setup: &str) -> Line<'static> {
    let count = match entries.len() {
        1 => "1 key".to_string(),
        n => format!("{n} keys"),
    };
    let pending = entries
        .iter()
        .filter(|entry| needs_setup(entry.status()))
        .count();
    if pending == 0 {
        return Line::from(vec![
            Span::from(format!("{count} · ")).dark_gray(),
            Span::from("all set").green(),
        ]);
    }
    let what = if pending == 1 {
        "1 needs setup".to_string()
    } else {
        format!("{pending} need setup")
    };
    Line::from(vec![
        Span::from(format!("{count} · ")).dark_gray(),
        Span::from(what).yellow(),
        Span::from(": ").dark_gray(),
        Span::from(setup.to_string()).cyan(),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envfile;

    fn doc() -> Document {
        let mut doc = envfile::parse(
            "# @plain\nPORT=8899\nREDIS_HOST=enc:QUJD\nDB_PASSWORD=\n# @plain @optional\nLOG_LEVEL=\n\
             LEGACY_TOKEN=typed-by-hand\n",
        )
        .unwrap();
        doc.header = None;
        doc
    }

    #[test]
    fn the_table_shows_status_storage_and_typing() {
        let text = table(&doc(), "sultrakey setup", 100, false);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[0],
            " KEY            STATUS                        STORED      TYPED"
        );
        assert_eq!(
            lines[1],
            " PORT           ✓ plain                       PLAIN       VISIBLE"
        );
        assert_eq!(
            lines[3],
            " DB_PASSWORD    ✗ empty — required            ENCRYPTED   SECRET"
        );
        assert_eq!(
            lines[4],
            " LOG_LEVEL      ○ empty (optional)            PLAIN       VISIBLE"
        );
        assert_eq!(
            lines[5],
            " LEGACY_TOKEN   ✗ plain — must be encrypted   ENCRYPTED   SECRET"
        );
        assert_eq!(lines[6], "");
        assert_eq!(lines[7], " 5 keys · 2 need setup: sultrakey setup");
        assert!(!text.contains("typed-by-hand"), "values are never shown");
        assert!(!text.contains('\x1b'), "no codes without color");
    }

    #[test]
    fn colors_mark_what_needs_attention() {
        let text = table(&doc(), "sultrakey setup", 100, true);
        assert!(text.contains("\x1b[0;32m✓ plain"), "{text:?}");
        assert!(text.contains("\x1b[0;33m✗ empty — required"), "{text:?}");
        assert!(text.contains("\x1b[0;90m○ empty (optional)"), "{text:?}");
        assert!(
            text.lines()
                .all(|line| line.is_empty() || line.ends_with("\x1b[0m"))
        );
    }

    #[test]
    fn a_complete_env_is_all_set() {
        let doc = envfile::parse("# @plain\nPORT=1\nA=enc:QUJD\n").unwrap();
        let text = table(&doc, "sultrakey setup", 80, false);
        assert!(text.ends_with(" 2 keys · all set\n"), "{text}");
    }

    #[test]
    fn piped_output_keeps_the_plain_columns() {
        let text = plain(&doc());
        assert!(text.starts_with("KEY           STATUS\n"), "{text}");
        assert!(text.contains("\nREDIS_HOST    encrypted\n"), "{text}");
        assert!(!text.contains("STORED") && !text.contains('✓'));
    }
}
