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
            "Jendela terlalu kecil. Perbesar jendela terminal (minimal {MIN_WIDTH}×{MIN_HEIGHT})."
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
        Mode::Help { .. } => draw_help(frame),
    }
}

fn draw_list(frame: &mut Frame, form: &Form, area: Rect) {
    let area = pad(area);
    let [head, rows] = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).areas(area);
    let count = format!("{}/{}", form.answered(), form.fields.len());
    let gap = (head.width as usize).saturating_sub(width("Key kosong") + width(&count));
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::from("Key kosong").bold(),
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
            let symbol = match answer {
                Some(Reply::Value(..)) => Span::from("✓ ").green(),
                Some(Reply::Empty) => Span::from("✓ ").dark_gray(),
                None if selected => Span::from("● ").cyan(),
                None => Span::from("○ ").dark_gray(),
            };
            let key = if selected {
                Span::from(field.key.clone()).bold()
            } else {
                Span::from(field.key.clone())
            };
            let mut line = vec![marker, symbol, key];
            let used = 4 + width(&field.key);
            if field.optional && used + 9 <= rows.width as usize {
                line.push(Span::from(" opsional").dark_gray());
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
    // The input right under the help; any room left stays at the bottom.
    let fixed = 3 + 1 + input_rows + 1 + 2;
    let help_rows = wrapped_rows(&help_text, area.width).min(area.height.saturating_sub(fixed));
    let [title, labels, rule1, help, rule2, input, _, status, _] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(help_rows.max(1)),
        Constraint::Length(1),
        Constraint::Length(input_rows),
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Min(0),
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
            "Isi    › ",
            &form.input,
            true,
            editing && !repeat,
        );
        draw_input(
            frame,
            second,
            "Ulangi › ",
            &form.again,
            true,
            editing && repeat,
        );
    } else {
        draw_input(frame, input, "Isi › ", &form.input, false, editing);
    }
    frame.render_widget(
        Paragraph::new(status_line(&form.status())).wrap(Wrap { trim: true }),
        status,
    );
}

fn label_line(field: &Field) -> Line<'static> {
    let (shown, shown_bg) = if field.masked {
        ("RAHASIA", Color::Yellow)
    } else {
        ("TERLIHAT", Color::Cyan)
    };
    let (need, need_bg) = if field.optional {
        ("OPSIONAL", Color::Gray)
    } else {
        ("WAJIB", Color::LightMagenta)
    };
    let (kept, kept_bg) = if field.plain {
        ("POLOS", Color::Gray)
    } else {
        ("TERENKRIPSI", Color::Green)
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
        vec![
            Line::from("Belum ada keterangan untuk key ini.").dark_gray(),
            Line::from("Tambahkan di .env.example, contoh:").dark_gray(),
            Line::from("  # Penjelasan singkat").dark_gray(),
            Line::from(format!("  {}=", field.key)).dark_gray(),
        ]
    } else {
        field
            .help
            .iter()
            .map(|line| Line::from(printable(line)))
            .collect()
    };
    if let Some(default) = &field.default {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::from("Bawaan template: ").dark_gray(),
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
            ("Enter", "lanjut"),
            ("↑↓", "pindah"),
            ("Ctrl+S", "simpan"),
            ("F1", "bantuan"),
            ("Esc", "batal"),
        ],
        Mode::Review { .. } => &[
            ("←→", "pilih"),
            ("Enter", "jalankan"),
            ("↑↓", "gulir"),
            ("Esc", "batal"),
        ],
        Mode::ConfirmCancel { .. } => &[("y", "ya, batal"), ("n", "kembali")],
        Mode::Help { .. } => &[("tombol apa saja", "tutup bantuan")],
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
                None if field.optional => Span::from("(belum diisi)").dark_gray(),
                None => Span::from("(belum diisi)").yellow(),
                Some(Reply::Empty) => Span::from("(kosong)").dark_gray(),
                Some(Reply::Value(_, Some(path))) => Span::from(format!(
                    "(isi file {})",
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
        .filter(|(field, answer)| answer.is_none() && !field.optional)
        .map(|(field, _)| field.key.as_str())
        .collect();

    let area = frame.area();
    let warn_rows = if missing.is_empty() { 0 } else { 2 };
    let height = rows.len() as u16 + 4 + warn_rows;
    let popup = centered(area, area.width.saturating_sub(4).min(76), height);
    frame.render_widget(Clear, popup);
    let block = popup_block(" Periksa sebelum disimpan ");
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
    if !missing.is_empty() {
        frame.render_widget(
            Paragraph::new(format!(
                "! Belum diisi: {}. Tetap kosong; isi nanti dengan sultrakey setup.",
                missing.join(", ")
            ))
            .yellow()
            .wrap(Wrap { trim: true }),
            warn,
        );
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
            button("[ Simpan ]", save),
            Span::from("      "),
            button("[ Kembali ubah ]", !save),
        ]))
        .centered(),
        buttons,
    );
}

fn draw_confirm(frame: &mut Frame) {
    let popup = centered(frame.area(), 46, 6);
    frame.render_widget(Clear, popup);
    let block = popup_block(" Batal tanpa menyimpan? ");
    let inner = pad(block.inner(popup));
    frame.render_widget(block, popup);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from("Semua isian di layar ini dibuang."),
            Line::from(""),
            Line::from(vec![
                Span::from("y").cyan().bold(),
                Span::from(" ya, batal      "),
                Span::from("n").cyan().bold(),
                Span::from(" kembali"),
            ]),
        ]),
        inner,
    );
}

fn draw_help(frame: &mut Frame) {
    let section = |title: &'static str| Line::from(title).bold();
    let row = |key: &'static str, what: &'static str| {
        Line::from(vec![
            Span::from(format!("  {key:<14}")).cyan(),
            Span::from(what),
        ])
    };
    let lines = vec![
        section("Label"),
        row("RAHASIA", "diketik sebagai bintang, dua kali"),
        row("TERLIHAT", "diketik terlihat, sekali"),
        row("WAJIB", "harus diisi sebelum aplikasi bisa jalan"),
        row("OPSIONAL", "boleh dikosongkan: hapus isinya, lalu Enter"),
        row("TERENKRIPSI", "disimpan terenkripsi di .env"),
        row("POLOS", "disimpan apa adanya (# @plain)"),
        Line::from(""),
        section("Tombol"),
        row("Enter", "simpan isian, lanjut ke key berikutnya"),
        row(
            "↑ ↓  Tab",
            "pindah key; ketikan yang belum di-Enter dibuang",
        ),
        row("← → Home End", "geser kursor"),
        row("Ctrl+U", "hapus seluruh isian"),
        row("Ctrl+S", "periksa semua jawaban, lalu simpan"),
        row("Esc", "batal tanpa menyimpan"),
        Line::from(""),
        section("Isi dari file"),
        row(
            "@lokasi-file",
            "value = ISI file itu; path-nya tidak disimpan",
        ),
        row("@@teks", "value yang memang diawali @"),
    ];
    let popup = centered(frame.area(), 68, lines.len() as u16 + 4);
    frame.render_widget(Clear, popup);
    let block = popup_block(" Bantuan ");
    let inner = pad(block.inner(popup));
    frame.render_widget(block, popup);
    frame.render_widget(Paragraph::new(lines), inner);
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

    use super::super::tests::field;
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
        host.help = vec!["Alamat server PostgreSQL.".into()];
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
            text.contains("TERLIHAT") && text.contains("WAJIB"),
            "{text}"
        );
        assert!(text.contains("TERENKRIPSI"), "{text}");
        assert!(text.contains("Alamat server PostgreSQL."), "{text}");
        assert!(text.contains("Bawaan template: localhost"), "{text}");
        assert!(text.contains("Isi › localhost"), "{text}");
        assert!(text.contains("0/3"), "{text}");
    }

    #[test]
    fn a_key_without_help_explains_how_to_add_it() {
        let mut form = sample();
        form.select(2);
        let text = screen(&form, 80, 24);
        assert!(
            text.contains("OPSIONAL") && text.contains("POLOS"),
            "{text}"
        );
        assert!(text.contains("Belum ada keterangan"), "{text}");
        assert!(text.contains("REDIS_HOST="), "{text}");
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
        assert!(typing.contains("RAHASIA"), "{typing}");
        assert!(typing.contains("Isi    › **"), "{typing}");
        assert!(!typing.contains("pw"), "{typing}");

        type_line(&mut form, "");
        type_line(&mut form, "pw");
        type_line(&mut form, "");
        let review = screen(&form, 80, 24);
        assert!(review.contains("Periksa sebelum disimpan"), "{review}");
        assert!(review.contains("DB_PASSWORD  ********"), "{review}");
        assert!(review.contains("DB_HOST      localhost"), "{review}");
        assert!(review.contains("(kosong)"), "{review}");
        assert!(!review.contains("pw"), "{review}");
    }

    #[test]
    fn narrow_and_tiny_terminals() {
        let narrow = screen(&sample(), 50, 24);
        assert!(
            narrow.contains("DB_PASSWORD") && narrow.contains("TERLIHAT"),
            "{narrow}"
        );
        let tiny = screen(&sample(), 30, 8);
        assert!(tiny.contains("terlalu kecil"), "{tiny}");
    }
}
