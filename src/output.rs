//! Everything printed for people. Problems and warnings go to stderr, so `run` never mixes
//! them into the application's own output; results go to stdout.
//!
//! Nothing here ever receives a secret value: callers pass key names only.

use std::env;
use std::io::{self, IsTerminal, Write};
use std::sync::OnceLock;

use crate::error::{Abort, Fail, Issue};

/// Guidance text (stderr), set in dim italics so the key and the input stand out.
pub fn faint(text: &str) -> String {
    paint("2;3", text)
}

/// A rejected answer (stderr), in yellow so it is not missed among the guidance.
pub fn caution(text: &str) -> String {
    paint("33", text)
}

/// `Enter   pakai nilai yang ada`: the key bold cyan, padded to `width` so a list of them
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

fn styled() -> bool {
    static STYLED: OnceLock<bool> = OnceLock::new();
    *STYLED.get_or_init(|| {
        io::stderr().is_terminal()
            && env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
            && ansi_supported()
    })
}

/// Windows consoles show escape codes literally unless asked to interpret them.
#[cfg(windows)]
fn ansi_supported() -> bool {
    use windows_sys::Win32::System::Console::{
        ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, GetStdHandle, STD_ERROR_HANDLE,
        SetConsoleMode,
    };
    // SAFETY: the handle comes from GetStdHandle and `mode` outlives the call.
    unsafe {
        let handle = GetStdHandle(STD_ERROR_HANDLE);
        let mut mode = 0;
        GetConsoleMode(handle, &mut mode) != 0
            && (mode & ENABLE_VIRTUAL_TERMINAL_PROCESSING != 0
                || SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) != 0)
    }
}

#[cfg(not(windows))]
fn ansi_supported() -> bool {
    env::var_os("TERM").is_none_or(|term| term != "dumb")
}

/// `✓ <text>` on stdout.
pub fn ok(text: &str) {
    println!("✓ {text}");
}

/// A plain line on stdout.
pub fn info(text: &str) {
    println!("{text}");
}

/// `! <text>` on stderr: worth knowing, but nothing failed.
pub fn warn(text: &str) {
    eprintln!("! {text}");
}

/// `✗ <problem>` and `Solusi: <command>` on stderr.
pub fn issue(issue: &Issue) {
    eprintln!("✗ {}", issue.problem);
    if let Some(solution) = &issue.solution {
        eprintln!("Solusi: {solution}");
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
    eprintln!("✗ Terjadi kesalahan: {err:#}");
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
        let err = anyhow::Error::new(Fail::config("A kosong.", "sultrakey setup"));
        assert_eq!(report(&err), 78);
        assert_eq!(report(&anyhow::anyhow!("boom")), 1);
    }
}
