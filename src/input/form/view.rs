//! Draws the form. Secret values are never drawn: masked input shows one `*` per character
//! while typed, and the review always shows eight.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};

use super::{Field, Form, LineInput, Mode, Status};
use crate::input::prompt::Reply;

/// Below this width the key list goes above the selected key instead of beside it.
const SIDE_BY_SIDE: u16 = 70;
const MIN_WIDTH: u16 = 40;
const MIN_HEIGHT: u16 = 12;

pub fn draw(frame: &mut Frame, form: &Form) {
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        let text = format!(
            "The window is too small. Make the terminal window larger (at least {MIN_WIDTH}×{MIN_HEIGHT})."
        );
        frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), area);
        return;
    }
    let [main, footer] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(faint())
        .title(Line::from(" sultrakey setup ").cyan().bold())
        .title_top(
            Line::from(concat!(" v", env!("CARGO_PKG_VERSION"), " "))
                .right_aligned()
                .dark_gray(),
        );
    let inner = block.inner(main);
    frame.render_widget(block, main);

    let (list, detail) = if inner.width >= SIDE_BY_SIDE {
        let longest = form.fields.iter().map(|f| width(&f.key)).max().unwrap_or(0);
        let list_width = (longest.min(200) as u16 + 15).clamp(22, inner.width / 2);
        let [list, detail] =
            Layout::horizontal([Constraint::Length(list_width), Constraint::Min(0)]).areas(inner);
        (list, detail)
    } else {
        let rows = (form.fields.len() as u16 + 2).min(inner.height / 3);
        let [list, detail] =
            Layout::vertical([Constraint::Length(rows), Constraint::Min(0)]).areas(inner);
        (list, detail)
    };
    let side = inner.width >= SIDE_BY_SIDE;
    let list_block = Block::new()
        .borders(if side {
            Borders::RIGHT
        } else {
            Borders::BOTTOM
        })
        .border_style(faint());
    draw_list(frame, form, list_block.inner(list));
    frame.render_widget(list_block, list);
    draw_detail(frame, form, pad(detail));
    frame.render_widget(Paragraph::new(footer_line(form.mode)), footer);

    match form.mode {
        Mode::Edit | Mode::Repeat => {}
        Mode::Review { save, scroll } => draw_review(frame, form, save, scroll),
        Mode::ConfirmCancel { .. } => draw_confirm(frame),
        Mode::Help { scroll, .. } => draw_help(frame, scroll),
    }
}

fn draw_list(frame: &mut Frame, form: &Form, area: Rect) {
    let area = pad(area);
    let [head, rows] = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(area);
    let count = format!("{}/{}", form.answered(), form.fields.len());
    let gap = (head.width as usize).saturating_sub(width("Keys") + width(&count));
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::from("Keys").bold(),
            Span::from(" ".repeat(gap)),
            Span::from(count).dark_gray(),
        ])),
        head,
    );

    let visible = rows.height as usize;
    let skip = (form.selected + 1).saturating_sub(visible);
    let lines: Vec<Line> = form
        .fields
        .iter()
        .zip(&form.answers)
        .enumerate()
        .skip(skip)
        .take(visible)
        .map(|(i, (field, answer))| {
            let selected = i == form.selected;
            let marker = if selected {
                Span::from("▸ ").cyan().bold()
            } else {
                Span::from("  ")
            };
            // Filled in .env and not replaced here: dimmed, and kept when saved.
            let kept = field.filled && answer.is_none();
            let symbol = match answer {
                Some(Reply::Value(..)) => Span::from("✓ ").green(),
                Some(Reply::Empty) => Span::from("✓ ").dark_gray(),
                None if kept => Span::from("✓ ").dark_gray(),
                None if selected => Span::from("● ").cyan(),
                None => Span::from("○ ").dark_gray(),
            };
            let key = if selected {
                Span::from(field.key.clone()).bold()
            } else if kept {
                Span::from(field.key.clone()).dark_gray()
            } else {
                Span::from(field.key.clone())
            };
            let mut line = vec![marker, symbol, key];
            let used = 4 + width(&field.key);
            let optional = " optional";
            if field.optional && used + width(optional) <= rows.width as usize {
                line.push(Span::from(optional).dark_gray());
            }
            Line::from(line)
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), rows);
}

fn draw_detail(frame: &mut Frame, form: &Form, area: Rect) {
    let field = form.field();
    let input_rows = if field.masked { 2 } else { 1 };
    let help_text = help_text(field);
    // A problem or the `@file` check sits right under the input, where the eye is; a hint
    // about the keys sits at the very bottom, out of the way.
    let status = status_line(&form.status());
    let (notice, hint) = match form.status() {
        Status::Hint(_) => (None, Some(status)),
        _ => (Some(status), None),
    };
    let hint_rows = hint.as_ref().map_or(0, |line| {
        wrapped_rows(&Text::from(line.clone()), area.width)
    });
    // The input right under the help; any room left stays above the hint.
    let fixed = 3 + 1 + input_rows + 1 + 2 + hint_rows;
    let help_rows = wrapped_rows(&help_text, area.width).min(area.height.saturating_sub(fixed));
    let [
        title,
        labels,
        rule1,
        help,
        rule2,
        input,
        _,
        notice_area,
        _,
        hint_area,
    ] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(help_rows.max(1)),
        Constraint::Length(1),
        Constraint::Length(input_rows),
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Min(0),
        Constraint::Length(hint_rows),
    ])
    .areas(area);

    frame.render_widget(
        Paragraph::new(Line::from(field.key.clone()).cyan().bold()),
        title,
    );
    frame.render_widget(Paragraph::new(label_line(field)), labels);
    for rule in [rule1, rule2] {
        frame.render_widget(
            Paragraph::new("─".repeat(rule.width as usize)).style(faint()),
            rule,
        );
    }
    frame.render_widget(Paragraph::new(help_text).wrap(Wrap { trim: false }), help);

    let editing = matches!(form.mode, Mode::Edit | Mode::Repeat);
    if field.masked {
        let [first, second] =
            Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(input);
        let repeat = form.mode == Mode::Repeat;
        draw_input(
            frame,
            first,
            "Value  › ",
            &form.input,
            true,
            editing && !repeat,
        );
        draw_input(
            frame,
            second,
            "Repeat › ",
            &form.again,
            true,
            editing && repeat,
        );
    } else {
        draw_input(frame, input, "Value › ", &form.input, false, editing);
    }
    for (line, area) in [(notice, notice_area), (hint, hint_area)] {
        if let Some(line) = line {
            frame.render_widget(Paragraph::new(line).wrap(Wrap { trim: true }), area);
        }
    }
}

fn label_line(field: &Field) -> Line<'static> {
    let (shown, shown_bg) = if field.masked {
        ("SECRET", Color::Yellow)
    } else {
        ("VISIBLE", Color::Cyan)
    };
    let (need, need_bg) = if field.optional {
        ("OPTIONAL", Color::Gray)
    } else {
        ("REQUIRED", Color::LightMagenta)
    };
    let (kept, kept_bg) = if field.plain {
        ("PLAIN", Color::Gray)
    } else {
        ("ENCRYPTED", Color::Green)
    };
    let badge = |text: &'static str, bg: Color| {
        Span::styled(format!(" {text} "), Style::new().fg(Color::Black).bg(bg))
    };
    Line::from(vec![
        badge(shown, shown_bg),
        Span::from(" "),
        badge(need, need_bg),
        Span::from(" "),
        badge(kept, kept_bg),
    ])
}

fn help_text(field: &Field) -> Text<'static> {
    let mut lines: Vec<Line> = if field.help.is_empty() {
        vec![Line::from("No description for this key yet.").dark_gray()]
    } else {
        field
            .help
            .iter()
            .map(|line| Line::from(printable(line)).dark_gray())
            .collect()
    };
    if let Some(default) = &field.default {
        // A filled key does not start from it, and its own value is not this one.
        let label = if field.filled {
            "Example: "
        } else {
            "Default: "
        };
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::from(label).dark_gray(),
            Span::from(printable(default)),
        ]));
    }
    Text::from(lines)
}

/// `label` then the line, scrolled so the cursor stays in view. Places the terminal cursor
/// when `active`.
fn draw_input(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    line: &LineInput,
    masked: bool,
    active: bool,
) {
    let label_width = width(label) as u16;
    let room = area.width.saturating_sub(label_width + 1).max(1) as usize;
    let skip = (line.cursor() + 1).saturating_sub(room);
    let shown: String = if masked {
        "*".repeat(line.len().saturating_sub(skip).min(room))
    } else {
        printable(line.text())
            .chars()
            .skip(skip)
            .take(room)
            .collect()
    };
    let label = if active {
        Span::from(label.to_string()).cyan()
    } else {
        Span::from(label.to_string()).dark_gray()
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![label, Span::from(shown)])),
        area,
    );
    if active {
        let x = area.x + label_width + (line.cursor() - skip) as u16;
        frame.set_cursor_position(Position::new(x, area.y));
    }
}

fn status_line(status: &Status) -> Line<'static> {
    match status {
        Status::Hint(text) => Line::from(text.clone()).dark_gray().italic(),
        Status::FileOk(text) => Line::from(text.clone()).green(),
        Status::Problem(text) if text.starts_with('✗') => Line::from(text.clone()).yellow(),
        Status::Problem(text) => Line::from(format!("! {text}")).yellow(),
    }
}

fn footer_line(mode: Mode) -> Line<'static> {
    let keys: &[(&str, &str)] = match mode {
        Mode::Edit | Mode::Repeat => &[
            ("Enter", "next"),
            ("↑↓", "move"),
            ("Ctrl+S", "save"),
            ("F1", "help"),
            ("Esc", "cancel"),
        ],
        Mode::Review { .. } => &[
            ("←→", "choose"),
            ("Enter", "confirm"),
            ("↑↓", "scroll"),
            ("Esc", "cancel"),
        ],
        Mode::ConfirmCancel { .. } => &[("y", "yes, cancel"), ("n", "go back")],
        Mode::Help { .. } => &[("↑↓", "scroll"), ("any other key", "close help")],
    };
    let mut spans = vec![Span::from(" ")];
    for (i, (key, what)) in keys.iter().enumerate() {
        if i > 0 {
            spans.push(Span::from("   "));
        }
        spans.push(Span::from(*key).cyan().bold());
        spans.push(Span::from(format!(" {what}")).dark_gray());
    }
    Line::from(spans)
}

fn draw_review(frame: &mut Frame, form: &Form, save: bool, scroll: u16) {
    let longest = form.fields.iter().map(|f| width(&f.key)).max().unwrap_or(0);
    let rows: Vec<Line> = form
        .fields
        .iter()
        .zip(&form.answers)
        .map(|(field, answer)| {
            let shown = match answer {
                None if field.filled => Span::from("(unchanged)").dark_gray(),
                None if field.optional => Span::from("(not filled)").dark_gray(),
                None => Span::from("(not filled)").yellow(),
                Some(Reply::Empty) => Span::from("(empty)").dark_gray(),
                Some(Reply::Value(_, Some(path))) => Span::from(format!(
                    "(contents of {})",
                    printable(&path.display().to_string())
                ))
                .dark_gray(),
                // Always eight: the real length stays hidden.
                Some(Reply::Value(..)) if field.masked => Span::from("********"),
                Some(Reply::Value(value, None)) => Span::from(printable(value.expose())),
            };
            Line::from(vec![
                Span::from(format!("{:<longest$}  ", field.key)).cyan(),
                shown,
            ])
        })
        .collect();
    let missing: Vec<&str> = form
        .fields
        .iter()
        .zip(&form.answers)
        .filter(|(field, answer)| answer.is_none() && !field.optional && !field.filled)
        .map(|(field, _)| field.key.as_str())
        .collect();

    let area = frame.area();
    let popup_width = area.width.saturating_sub(4).min(76);
    let warning = (!missing.is_empty()).then(|| {
        Line::from(format!(
            "! Not filled: {}. They stay empty; fill them later with sultrakey setup.",
            missing.join(", ")
        ))
        .yellow()
    });
    // As many rows as the warning wraps to, so no key name is cut off.
    let warn_rows = warning.as_ref().map_or(0, |line| {
        wrapped_rows(&Text::from(line.clone()), popup_width.saturating_sub(4))
    });
    let height = rows.len() as u16 + 4 + warn_rows;
    let popup = centered(area, popup_width, height);
    frame.render_widget(Clear, popup);
    let block = popup_block(" Review before saving ");
    let inner = pad(block.inner(popup));
    frame.render_widget(block, popup);

    let [list, _, warn, buttons] = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(warn_rows),
        Constraint::Length(1),
    ])
    .areas(inner);
    let scroll = scroll.min((rows.len() as u16).saturating_sub(list.height));
    frame.render_widget(Paragraph::new(rows).scroll((scroll, 0)), list);
    if let Some(warning) = warning {
        frame.render_widget(Paragraph::new(warning).wrap(Wrap { trim: true }), warn);
    }
    let button = |text: &'static str, on: bool| {
        if on {
            Span::styled(
                text,
                Style::new()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::from(text)
        }
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            button("[ Save ]", save),
            Span::from("      "),
            button("[ Back to edit ]", !save),
        ]))
        .centered(),
        buttons,
    );
}

fn draw_confirm(frame: &mut Frame) {
    let title = " Cancel without saving? ";
    let text = Text::from(vec![
        Line::from("Everything entered on this screen is discarded."),
        Line::from(""),
        Line::from(vec![
            Span::from("y").cyan().bold(),
            Span::from(" yes, cancel      "),
            Span::from("n").cyan().bold(),
            Span::from(" go back"),
        ]),
    ]);
    // As wide as the text needs (plus border and padding); narrow terminals wrap it.
    let area = frame.area();
    let widest = text.width().max(width(title)).min(usize::from(u16::MAX)) as u16;
    let popup_width = (widest + 4).min(area.width);
    let rows = wrapped_rows(&text, popup_width.saturating_sub(4));
    let popup = centered(area, popup_width, rows + 2);
    frame.render_widget(Clear, popup);
    let block = popup_block(title);
    let inner = pad(block.inner(popup));
    frame.render_widget(block, popup);
    frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), inner);
}

/// The legend shown by F1: labels, the annotations behind them, keys, and `@file`.
pub(super) fn help_lines() -> Vec<Line<'static>> {
    let section = |title: &'static str| Line::from(title).bold();
    let row = |key: &'static str, what: &'static str| {
        Line::from(vec![
            Span::from(format!("  {key:<14}")).cyan(),
            Span::from(what),
        ])
    };
    let more = |what: &'static str| Line::from(format!("  {:<14}{what}", ""));
    let example = |text: &'static str| Line::from(format!("  {:<14}{text}", "")).dark_gray();
    vec![
        section("Labels"),
        row("SECRET", "typed as stars, twice"),
        row("VISIBLE", "typed visibly, once"),
        row("REQUIRED", "must be filled before the application can run"),
        row("OPTIONAL", "may be left empty: clear it, then press Enter"),
        row("ENCRYPTED", "stored encrypted in .env"),
        row("PLAIN", "stored as is, readable in .env"),
        Line::from(""),
        section("Annotations"),
        more("Comment lines right above a key in .env.example"),
        more("set its labels. Several may share one line, for"),
        more("example: # @plain @optional"),
        row("# @plain", "store the value as is, not encrypted (PLAIN)."),
        more("For values that are not secret: a port, a log level."),
        row(
            "# @optional",
            "the key may stay empty (OPTIONAL); the application",
        ),
        more("then gets the variable with an empty value."),
        row(
            "# @masking",
            "type as stars, twice (SECRET), for a secret whose",
        ),
        more("name looks harmless, such as DATABASE_URL."),
        row(
            "no annotation",
            "encrypted and required. Typed as stars when the",
        ),
        more("name has PASSWORD, PASS, PWD, SECRET, TOKEN, AUTH,"),
        more("or SALT (REDIS_PASSWORD); otherwise visible."),
        row("Example", "# Redis password; leave empty if there is none"),
        example("# @optional"),
        example("REDIS_PASSWORD="),
        more("→ SECRET, OPTIONAL, ENCRYPTED"),
        Line::from(""),
        section("Keys"),
        row("Enter", "keep the value, go to the next empty key"),
        more("On a filled key, an empty line keeps the value"),
        more("already in .env (it is never shown)."),
        row(
            "↑ ↓  Tab",
            "move between keys; typing without Enter is dropped",
        ),
        row("← → Home End", "move the cursor"),
        row("Ctrl+U", "clear the whole line"),
        row("Ctrl+S", "review every answer, then save"),
        row("Esc", "cancel without saving"),
        Line::from(""),
        section("Fill from a file"),
        row(
            "@file-path",
            "value = the file's CONTENTS; the path is not saved",
        ),
        row("@@text", "a value that really starts with @"),
    ]
}

fn draw_help(frame: &mut Frame, scroll: u16) {
    let lines = help_lines();
    let rows = lines.len() as u16;
    // Above the footer, which says how to scroll and close.
    let area = frame.area();
    let above_footer = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(1));
    let popup = centered(above_footer, 72, rows + 2);
    frame.render_widget(Clear, popup);
    let visible = popup.height.saturating_sub(2);
    let block = if rows > visible {
        popup_block(" Help (↑↓ to scroll) ")
    } else {
        popup_block(" Help ")
    };
    let inner = pad(block.inner(popup));
    frame.render_widget(block, popup);
    let scroll = scroll.min(rows.saturating_sub(inner.height));
    frame.render_widget(Paragraph::new(lines).scroll((scroll, 0)), inner);
}

fn popup_block(title: &'static str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().cyan())
        .title(Line::from(title).bold())
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

/// One column of space on each side.
fn pad(area: Rect) -> Rect {
    Rect::new(
        area.x + 1,
        area.y,
        area.width.saturating_sub(2),
        area.height,
    )
}

fn faint() -> Style {
    Style::new().fg(Color::DarkGray)
}

/// About how many rows `text` takes when wrapped at `width` (words can push a row over).
fn wrapped_rows(text: &Text, width: u16) -> u16 {
    let width = usize::from(width.max(1));
    let rows: usize = text
        .lines
        .iter()
        .map(|line| line.width().div_ceil(width).max(1) + usize::from(line.width() > width))
        .sum();
    rows.min(usize::from(u16::MAX)) as u16
}

fn width(text: &str) -> usize {
    text.chars().count()
}

/// Control characters would move the cursor or restyle the screen; drawn as `?` instead.
fn printable(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

    use super::super::tests::{field, filled};
    use super::*;

    fn screen(form: &Form, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| draw(frame, form)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn type_line(form: &mut Form, text: &str) {
        for c in text.chars() {
            form.handle(Event::Key(KeyEvent::new(
                KeyCode::Char(c),
                KeyModifiers::NONE,
            )));
        }
        form.handle(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        )));
    }

    fn sample() -> Form {
        let mut host = field("DB_HOST", false, false, Some("localhost"));
        host.help = vec!["PostgreSQL server address.".into()];
        let mut bare = field("REDIS_HOST", false, true, None);
        bare.help.clear();
        bare.plain = true;
        Form::new(vec![host, field("DB_PASSWORD", true, false, None), bare])
    }

    #[test]
    fn shows_labels_help_and_the_default() {
        let text = screen(&sample(), 80, 24);
        assert!(text.contains("sultrakey setup"), "{text}");
        assert!(
            text.contains("VISIBLE") && text.contains("REQUIRED"),
            "{text}"
        );
        assert!(text.contains("ENCRYPTED"), "{text}");
        assert!(text.contains("PostgreSQL server address."), "{text}");
        assert!(text.contains("Default: localhost"), "{text}");
        assert!(text.contains("Value › localhost"), "{text}");
        assert!(text.contains("0/3"), "{text}");
    }

    #[test]
    fn a_key_without_help_says_so_in_one_line() {
        let mut form = sample();
        form.select(2);
        let text = screen(&form, 80, 24);
        assert!(
            text.contains("OPTIONAL") && text.contains("PLAIN"),
            "{text}"
        );
        assert!(text.contains("No description for this key yet."), "{text}");
        assert!(!text.contains("Add one in .env.example"), "{text}");
    }

    #[test]
    fn secrets_are_never_drawn() {
        let mut form = sample();
        type_line(&mut form, "");
        form.handle(Event::Key(KeyEvent::new(
            KeyCode::Char('p'),
            KeyModifiers::NONE,
        )));
        form.handle(Event::Key(KeyEvent::new(
            KeyCode::Char('w'),
            KeyModifiers::NONE,
        )));
        let typing = screen(&form, 80, 24);
        assert!(typing.contains("SECRET"), "{typing}");
        assert!(typing.contains("Value  › **"), "{typing}");
        assert!(!typing.contains("pw"), "{typing}");

        type_line(&mut form, "");
        type_line(&mut form, "pw");
        type_line(&mut form, "");
        let review = screen(&form, 80, 24);
        assert!(review.contains("Review before saving"), "{review}");
        assert!(review.contains("DB_PASSWORD  ********"), "{review}");
        assert!(review.contains("DB_HOST      localhost"), "{review}");
        assert!(review.contains("(empty)"), "{review}");
        assert!(!review.contains("pw"), "{review}");
    }

    #[test]
    fn help_explains_the_annotations_and_scrolls() {
        let mut form = sample();
        form.mode = Mode::Help {
            back: super::super::Back::Edit,
            scroll: 0,
        };
        let tall = screen(&form, 80, 60);
        for word in [
            "# @plain",
            "# @optional",
            "# @masking",
            "no annotation",
            "@file-path",
        ] {
            assert!(tall.contains(word), "{word}: {tall}");
        }
        assert!(!tall.contains("to scroll"), "{tall}");

        let short = screen(&form, 80, 24);
        assert!(short.contains("Help (↑↓ to scroll)"), "{short}");
        assert!(
            short.contains("Labels") && !short.contains("@@text"),
            "{short}"
        );
        form.mode = Mode::Help {
            back: super::super::Back::Edit,
            scroll: 99,
        };
        let end = screen(&form, 80, 24);
        assert!(end.contains("@@text") && !end.contains("Labels"), "{end}");
    }

    /// The row (from the top) holding `needle`.
    fn row_of(text: &str, needle: &str) -> Option<usize> {
        text.lines().position(|line| line.contains(needle))
    }

    #[test]
    fn key_hints_sit_at_the_bottom_and_problems_under_the_input() {
        let mut form = sample();
        let text = screen(&form, 80, 24);
        // Row 21 is the last one inside the frame: 22 is its border, 23 the footer.
        assert_eq!(row_of(&text, "Enter = use localhost"), Some(21), "{text}");

        form.handle(Event::Key(KeyEvent::new(
            KeyCode::Char('u'),
            KeyModifiers::CONTROL,
        )));
        form.handle(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        )));
        let text = screen(&form, 80, 24);
        let input = row_of(&text, "Value ›").unwrap();
        assert_eq!(
            row_of(&text, "! DB_HOST is required."),
            Some(input + 2),
            "{text}"
        );
    }

    #[test]
    fn a_long_list_of_unfilled_keys_is_not_cut_off() {
        let keys: Vec<String> = (1..=12)
            .map(|i| format!("SOME_LONG_KEY_NAME_{i:02}"))
            .collect();
        let mut form = Form::new(keys.iter().map(|k| field(k, false, false, None)).collect());
        form.mode = Mode::Review {
            save: true,
            scroll: 0,
        };
        let text = screen(&form, 80, 40);
        assert!(text.contains("SOME_LONG_KEY_NAME_12."), "{text}");
        assert!(
            text.lines().any(|line| line.contains("│ setup.")),
            "the end of the wrapped warning: {text}"
        );
    }

    #[test]
    fn the_cancel_question_is_never_cut_off() {
        let mut form = sample();
        form.mode = Mode::ConfirmCancel {
            back: super::super::Back::Edit,
        };
        let wide = screen(&form, 80, 24);
        assert!(
            wide.contains("Everything entered on this screen is discarded."),
            "{wide}"
        );
        assert!(
            wide.contains("y yes, cancel") && wide.contains("n go back"),
            "{wide}"
        );
        let narrow = screen(&form, 40, 12);
        assert!(narrow.contains("discarded."), "wrapped, not cut: {narrow}");
        assert!(narrow.contains("go back"), "{narrow}");
    }

    #[test]
    fn filled_keys_are_listed_and_kept_in_the_review() {
        let mut form = Form::new(vec![
            filled("DB_HOST", false),
            field("DB_PASSWORD", true, false, None),
            field("REDIS_HOST", false, false, None),
        ]);
        let text = screen(&form, 80, 24);
        assert!(text.contains("Keys"), "{text}");
        assert!(text.contains("1/3"), "{text}");
        assert!(text.contains("✓ DB_HOST"), "{text}");
        assert!(
            text.contains("▸ ● DB_PASSWORD"),
            "starts at the empty key: {text}"
        );

        form.mode = Mode::Review {
            save: true,
            scroll: 0,
        };
        let review = screen(&form, 80, 24);
        assert!(review.contains("DB_HOST      (unchanged)"), "{review}");
        assert!(
            review.contains("Not filled: DB_PASSWORD, REDIS_HOST."),
            "a filled key is not missing: {review}"
        );
    }

    #[test]
    fn a_filled_key_shows_the_default_as_an_example_only() {
        let mut host = filled("DB_HOST", false);
        host.default = Some("localhost".into());
        let mut form = Form::new(vec![host, field("PORT", false, false, None)]);
        form.select(0);
        let text = screen(&form, 80, 24);
        assert!(text.contains("Example: localhost"), "{text}");
        assert!(!text.contains("Default: localhost"), "{text}");
        assert!(!text.contains("Value › localhost"), "not prefilled: {text}");
        assert!(text.contains("Already filled"), "{text}");

        form.handle(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        )));
        assert_eq!(form.answers[0], None, "Enter keeps the value in .env");
    }

    #[test]
    fn narrow_and_tiny_terminals() {
        let narrow = screen(&sample(), 50, 24);
        assert!(
            narrow.contains("DB_PASSWORD") && narrow.contains("VISIBLE"),
            "{narrow}"
        );
        let tiny = screen(&sample(), 30, 8);
        assert!(tiny.contains("too small"), "{tiny}");
    }
}
