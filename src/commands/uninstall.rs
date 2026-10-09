//! `uninstall`: removes the installed binary (and its PATH entry on Windows). Key files and
//! `.env` files are never touched. Adapted from lopi `src/commands/uninstall.rs`.

use anyhow::{Context, Result};

use super::require_terminal;
use crate::error::Abort;
use crate::input::prompt::{self, Terminal};
use crate::install;
use crate::keyfile;
use crate::output;
use crate::system;

pub fn run(yes: bool) -> Result<i32> {
    let target = install::installed_exe(system::is_root())?;
    let dir = target
        .parent()
        .context("lokasi pasang tidak punya folder")?;
    if !yes {
        require_terminal("sultrakey uninstall -y (tanpa konfirmasi)")?;
        let question = format!("Hapus sultrakey dari {}?", target.display());
        if !prompt::confirm(&mut Terminal, &question, false)? {
            return Err(Abort::Cancelled.into());
        }
    }

    let mut changed = remove_from_path(dir)?;
    if target.exists() {
        install::remove_exe(&target)?;
        output::ok(&format!("{} dihapus.", target.display()));
        changed = true;
    }
    if changed {
        if let Some(keys) = keyfile::default_dir() {
            output::info(&format!(
                "File kunci tidak dihapus dan tetap tersimpan di {}.",
                keys.display()
            ));
        }
    } else {
        output::info(&format!(
            "sultrakey tidak terpasang di {}; tidak ada yang dihapus.",
            target.display()
        ));
    }
    Ok(0)
}

#[cfg(windows)]
fn remove_from_path(dir: &std::path::Path) -> Result<bool> {
    let removed = install::remove_from_path(&mut install::windows::RegistryPath, dir)?;
    if removed {
        output::ok(&format!("{} dihapus dari PATH.", dir.display()));
    }
    Ok(removed)
}

#[cfg(not(windows))]
fn remove_from_path(_dir: &std::path::Path) -> Result<bool> {
    Ok(false)
}
