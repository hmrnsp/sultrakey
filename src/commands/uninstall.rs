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
        .context("the install location has no folder")?;
    if !yes {
        require_terminal("sultrakey uninstall -y (no confirmation)")?;
        let question = format!("Remove sultrakey from {}?", target.display());
        if !prompt::confirm(&mut Terminal, &question, false)? {
            return Err(Abort::Cancelled.into());
        }
    }

    let mut changed = remove_from_path(dir)?;
    if target.exists() {
        install::remove_exe(&target)?;
        output::ok(&format!("{} removed.", target.display()));
        changed = true;
    }
    if changed {
        if let Some(keys) = keyfile::default_dir() {
            output::info(&format!(
                "Key files are not deleted; they stay in {}.",
                keys.display()
            ));
        }
    } else {
        output::info(&format!(
            "sultrakey is not installed in {}; nothing was removed.",
            target.display()
        ));
    }
    Ok(0)
}

#[cfg(windows)]
fn remove_from_path(dir: &std::path::Path) -> Result<bool> {
    let removed = install::remove_from_path(&mut install::windows::RegistryPath, dir)?;
    if removed {
        output::ok(&format!("{} removed from PATH.", dir.display()));
    }
    Ok(removed)
}

#[cfg(not(windows))]
fn remove_from_path(_dir: &std::path::Path) -> Result<bool> {
    Ok(false)
}
