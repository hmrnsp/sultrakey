//! Starting the application with the decrypted values. Unix replaces this process
//! (`exec`): the PID, signals and exit code belong to the application, as pm2 and systemd
//! expect. Windows has no exec: the application runs as a child and its exit code is
//! passed on.

pub mod env;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

use std::ffi::OsString;
use std::io;

use anyhow::Result;

pub use env::{ChildEnv, plan};

use crate::error::Fail;

/// Runs `command` (program and arguments) with `env`. Returns only on Windows, or when the
/// program cannot be started.
pub fn launch(command: &[OsString], env: &ChildEnv) -> Result<i32> {
    let Some((program, args)) = command.split_first() else {
        return Err(Fail::usage(
            "Perintah aplikasi tidak ada.",
            "tulis perintahnya setelah --, contoh: sultrakey run -- node dist/main.js",
        )
        .into());
    };
    #[cfg(unix)]
    return unix::exec(program, args, env);
    #[cfg(windows)]
    return windows::spawn(program, args, env);
}

/// The failure for a program that cannot be started. Exit 78: restarting will not help.
pub fn start_fail(program: &std::ffi::OsStr, err: &io::Error) -> Fail {
    let name = program.to_string_lossy();
    match err.kind() {
        io::ErrorKind::NotFound => Fail::config(
            format!("Perintah '{name}' tidak ditemukan."),
            "periksa nama perintahnya, atau tulis lokasi lengkapnya, contoh: /usr/bin/node",
        ),
        io::ErrorKind::PermissionDenied => Fail::config(
            format!("Perintah '{name}' tidak boleh dijalankan oleh user ini (izin)."),
            format!("periksa izin file '{name}' (chmod +x) dan user yang menjalankan aplikasi"),
        ),
        _ => Fail::config_bare(format!("Perintah '{name}' gagal dijalankan: {err}.")),
    }
}
