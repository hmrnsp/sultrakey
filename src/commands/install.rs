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
    let source = env::current_exe().context("file program ini tidak ditemukan")?;
    let target = install::installed_exe(root)?;
    let dir = target
        .parent()
        .context("lokasi pasang tidak punya folder")?;

    match install::copy_exe(&source, &target)? {
        Copied::Installed => output::ok(&format!(
            "sultrakey {VERSION} dipasang di {}",
            target.display()
        )),
        Copied::Replaced => output::ok(&format!(
            "{} diperbarui ke sultrakey {VERSION}",
            target.display()
        )),
        Copied::AlreadyThere => output::ok(&format!(
            "sultrakey {VERSION} sudah terpasang di {}",
            target.display()
        )),
    }
    if root {
        prepare_key_dir();
    } else if cfg!(target_os = "linux") {
        output::info(
            "Catatan: untuk server, pasang dengan sudo (sudo ./sultrakey install) supaya \
             sultrakey ada di /usr/bin dan bisa dipakai pm2/systemd.",
        );
    }
    let path_changed = add_to_path(dir)?;
    if path_changed {
        output::info("Selesai: buka terminal baru, lalu jalankan `sultrakey --version`.");
    } else {
        output::info("Selesai: jalankan `sultrakey --version` dari terminal mana pun.");
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
        Ok(()) => output::ok(&format!("Folder kunci {} dibuat.", dir.display())),
        Err(err) => output::warn(&format!(
            "Folder kunci {} tidak bisa dibuat: {err}",
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
        output::ok(&format!("{} ditambahkan ke PATH.", dir.display()));
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
            "{} belum ada di PATH; tambahkan baris ini ke ~/.bashrc atau ~/.zshrc:\n  export PATH=\"{}:$PATH\"",
            dir.display(),
            dir.display()
        ));
    }
    Ok(false)
}
