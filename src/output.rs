//! Everything printed for people. Problems and warnings go to stderr, so `run` never mixes
//! them into the application's own output; results go to stdout.
//!
//! Nothing here ever receives a secret value: callers pass key names only.

use std::env;
use std::io::{self, IsTerminal, Write};
use std::sync::OnceLock;

use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};

use crate::error::{Abort, Fail, Issue};

/// Guidance text (stderr), set in dim italics so the key and the input stand out.
pub fn faint(text: &str) -> String {
    paint("2;3", text)
}

/// A rejected answer (stderr), in yellow so it is not missed among the guidance.
pub fn caution(text: &str) -> String {
    paint("33", text)
}

/// `Enter   keep the current value`: the key bold cyan, padded to `width` so a list of them
/// lines up, and what it does faint.
pub fn key_hint(key: &str, what: &str, width: usize) -> String {
    format!(
        "{}  {}",
        paint("1;36", &format!("{key:<width$}")),
        faint(what)
    )
}

/// `text` wrapped in the SGR `codes`. Plain when stderr is not a terminal, `NO_COLOR` is
/// set, or the terminal cannot show styles.
fn paint(codes: &str, text: &str) -> String {
    if styled() {
        format!("\x1b[{codes}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

/// Like `paint`, for stdout.
fn paint_out(codes: &str, text: &str) -> String {
    if stdout_styled() {
        format!("\x1b[{codes}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

fn styled() -> bool {
    static STYLED: OnceLock<bool> = OnceLock::new();
    *STYLED.get_or_init(|| io::stderr().is_terminal() && colors_allowed(Stream::Stderr))
}

/// Whether results on stdout may be styled: a terminal that shows styles, `NO_COLOR` unset.
/// Callers check `is_terminal` themselves, since they print plain text otherwise.
pub fn stdout_styled() -> bool {
    static STYLED: OnceLock<bool> = OnceLock::new();
    *STYLED.get_or_init(|| io::stdout().is_terminal() && colors_allowed(Stream::Stdout))
}

#[derive(Clone, Copy)]
enum Stream {
    Stdout,
    Stderr,
}

fn colors_allowed(stream: Stream) -> bool {
    env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()) && ansi_supported(stream)
}

/// Windows consoles show escape codes literally unless asked to interpret them.
#[cfg(windows)]
fn ansi_supported(stream: Stream) -> bool {
    use windows_sys::Win32::System::Console::{
        ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, GetStdHandle, STD_ERROR_HANDLE,
        STD_OUTPUT_HANDLE, SetConsoleMode,
    };
    let which = match stream {
        Stream::Stdout => STD_OUTPUT_HANDLE,
        Stream::Stderr => STD_ERROR_HANDLE,
    };
    // SAFETY: the handle comes from GetStdHandle and `mode` outlives the call.
    unsafe {
        let handle = GetStdHandle(which);
        let mut mode = 0;
        GetConsoleMode(handle, &mut mode) != 0
            && (mode & ENABLE_VIRTUAL_TERMINAL_PROCESSING != 0
                || SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) != 0)
    }
}

#[cfg(not(windows))]
fn ansi_supported(_stream: Stream) -> bool {
    env::var_os("TERM").is_none_or(|term| term != "dumb")
}

/// A drawn ratatui buffer as text to print: one line per row, trailing spaces dropped,
/// with SGR codes for colors, bold and dim when `color`. Nothing is drawn on the terminal
/// itself, so the result stays in the scrollback like any other output.
pub fn buffer_text(buffer: &Buffer, color: bool) -> String {
    let area = buffer.area;
    let mut out = String::new();
    for y in area.top()..area.bottom() {
        let mut line = String::new();
        let mut plain_len = 0;
        let mut current = String::new();
        // The style in force at the last visible character.
        let mut style_at_end = String::new();
        for x in area.left()..area.right() {
            let cell = &buffer[(x, y)];
            if color {
                let codes = sgr(cell.fg, cell.modifier);
                if codes != current {
                    line.push_str(&format!("\x1b[0;{codes}m"));
                    current = codes;
                }
            }
            line.push_str(cell.symbol());
            if cell.symbol() != " " {
                plain_len = line.len();
                style_at_end.clone_from(&current);
            }
        }
        // Drop trailing spaces (and the codes among them), then close any style.
        line.truncate(plain_len);
        if color && !style_at_end.is_empty() {
            line.push_str("\x1b[0m");
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// The SGR parameters for a foreground color and modifiers (`""` for neither).
fn sgr(fg: Color, modifier: Modifier) -> String {
    let mut codes = Vec::new();
    if modifier.contains(Modifier::BOLD) {
        codes.push("1");
    }
    if modifier.contains(Modifier::DIM) {
        codes.push("2");
    }
    let color = match fg {
        Color::Red => "31",
        Color::Green => "32",
        Color::Yellow => "33",
        Color::Blue => "34",
        Color::Magenta => "35",
        Color::Cyan => "36",
        Color::Gray => "37",
        Color::DarkGray => "90",
        _ => "",
    };
    if !color.is_empty() {
        codes.push(color);
    }
    codes.join(";")
}

/// `✓ <text>` on stdout.
pub fn ok(text: &str) {
    println!("{} {text}", paint_out("1;32", "✓"));
}

/// `Fix: <command>` on stdout, for a warning that did not stop the command.
pub fn fix(command: &str) {
    println!("{} {command}", paint_out("1;36", "Fix:"));
}

/// A plain line on stdout.
pub fn info(text: &str) {
    println!("{text}");
}

/// `! <text>` on stderr: worth knowing, but nothing failed.
pub fn warn(text: &str) {
    eprintln!("{} {text}", paint("1;33", "!"));
}

/// `✗ <problem>` and `Fix: <command>` on stderr.
pub fn issue(issue: &Issue) {
    eprintln!("{} {}", paint("1;31", "✗"), issue.problem);
    if let Some(solution) = &issue.solution {
        eprintln!("{} {solution}", paint("1;36", "Fix:"));
    }
}

/// Writes text to stdout, returning the error when the reader closed the pipe
/// (`sultrakey list | head -1`) instead of panicking like `print!`.
pub fn print(text: &str) -> io::Result<()> {
    let mut out = io::stdout().lock();
    out.write_all(text.as_bytes())?;
    out.flush()
}

/// Prints a failure and returns its exit code.
pub fn report(err: &anyhow::Error) -> i32 {
    if is_broken_pipe(err) {
        return 0;
    }
    if let Some(fail) = err.downcast_ref::<Fail>() {
        for item in &fail.issues {
            issue(item);
        }
        return fail.exit_code();
    }
    if let Some(abort) = err.downcast_ref::<Abort>() {
        eprintln!("{abort}");
        return abort.exit_code();
    }
    eprintln!("{} Error: {err:#}", paint("1;31", "✗"));
    1
}

/// Whether `err` (anywhere in its chain) is "the reader closed the pipe".
fn is_broken_pipe(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<io::Error>()
            .is_some_and(|io| io.kind() == io::ErrorKind::BrokenPipe)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Context;

    #[test]
    fn finds_broken_pipe_in_the_chain() {
        let broken: anyhow::Result<()> = Err(io::Error::from(io::ErrorKind::BrokenPipe).into());
        let wrapped = broken.context("cannot print").unwrap_err();
        assert!(is_broken_pipe(&wrapped));
        assert_eq!(report(&wrapped), 0);
        assert!(!is_broken_pipe(&anyhow::anyhow!("x")));
    }

    #[test]
    fn report_uses_the_fail_code() {
        let err = anyhow::Error::new(Fail::config("A is empty.", "sultrakey setup"));
        assert_eq!(report(&err), 78);
        assert_eq!(report(&anyhow::anyhow!("boom")), 1);
    }
}
