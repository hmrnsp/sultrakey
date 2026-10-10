//! `install`: copies this binary to the install folder (no internet needed; running it
//! again replaces the installed version). Adapted from lopi `src/commands/install.rs`.

use std::env;
use std::path::Path;

use anyhow::{Context, Result};

use crate::install::{self, Copied};
use crate::output;
use crate::system;

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn run() -> Result<i32> {
    let root = system::is_root();
    let source = env::current_exe().context("cannot find this program's file")?;
    let target = install::installed_exe(root)?;
    let dir = target
        .parent()
        .context("the install location has no folder")?;

    match install::copy_exe(&source, &target)? {
        Copied::Installed => output::ok(&format!(
            "sultrakey {VERSION} installed in {}",
            target.display()
        )),
        Copied::Replaced => output::ok(&format!(
            "{} updated to sultrakey {VERSION}",
            target.display()
        )),
        Copied::AlreadyThere => output::ok(&format!(
            "sultrakey {VERSION} is already installed in {}",
            target.display()
        )),
    }
    if root {
        prepare_key_dir();
    } else if cfg!(target_os = "linux") {
        output::info(
            "Note: on a server, install with sudo (sudo ./sultrakey install) so that \
             sultrakey is in /usr/bin, where pm2/systemd can use it.",
        );
    }
    let path_changed = add_to_path(dir)?;
    if path_changed {
        output::info("Done: open a new terminal, then run `sultrakey --version`.");
    } else {
        output::info("Done: run `sultrakey --version` from any terminal.");
    }
    Ok(0)
}

/// `/etc/sultrakey` (0755, root) for the key files.
#[cfg(unix)]
fn prepare_key_dir() {
    use std::os::unix::fs::DirBuilderExt;
    let Some(dir) = crate::keyfile::default_dir() else {
        return;
    };
    if dir.exists() {
        return;
    }
    match std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o755)
        .create(&dir)
    {
        Ok(()) => output::ok(&format!("Key folder {} created.", dir.display())),
        Err(err) => output::warn(&format!(
            "Key folder {} cannot be created: {err}",
            dir.display()
        )),
    }
}

#[cfg(not(unix))]
fn prepare_key_dir() {}

#[cfg(windows)]
fn add_to_path(dir: &Path) -> Result<bool> {
    let added = install::add_to_path(&mut install::windows::RegistryPath, dir)?;
    if added {
        output::ok(&format!("{} added to PATH.", dir.display()));
    }
    Ok(added)
}

/// Linux and macOS: shell startup files are not edited; when the folder is not on PATH,
/// the line to add is printed.
#[cfg(not(windows))]
fn add_to_path(dir: &Path) -> Result<bool> {
    let on_path =
        env::var_os("PATH").is_some_and(|path| env::split_paths(&path).any(|entry| entry == dir));
    if !on_path {
        output::warn(&format!(
            "{} is not on PATH; add this line to ~/.bashrc or ~/.zshrc:\n  export PATH=\"{}:$PATH\"",
            dir.display(),
            dir.display()
        ));
    }
    Ok(false)
}
