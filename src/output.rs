//! Everything printed for people. Problems and warnings go to stderr, so `run` never mixes
//! them into the application's own output; results go to stdout.
//!
//! Nothing here ever receives a secret value: callers pass key names only.

use std::io::{self, Write};

use crate::error::{Abort, Fail, Issue};

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
