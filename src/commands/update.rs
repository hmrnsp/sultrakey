//! `update [--check] [-y]`: replaces the installed sultrakey with the latest release.
//! Never runs on its own. Running applications are not touched; they use the new version
//! from their next restart. Adapted from lopi `src/commands/update.rs`.

use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use super::require_terminal;
use crate::error::{Abort, Fail};
use crate::input::prompt::{self, Terminal};
use crate::install;
use crate::output;
use crate::system;
use crate::update::http::{BINARY_LIMIT, Releases, SUMS_LIMIT};
use crate::update::manifest::Manifest;
use crate::update::version::Version;
use crate::update::{RELEASE_TARGET, checksum, sum_for};

/// How the running binary was put in place.
#[derive(Debug, PartialEq, Eq)]
enum Channel {
    /// The installed copy for this user (root: `/usr/bin`).
    Installed(PathBuf),
    /// The system copy, but this user is not root.
    NeedsRoot(PathBuf),
    /// Anything else: a build folder, a copy run where it was downloaded.
    Unknown(PathBuf),
}

fn detect(exe: &Path, root: bool) -> Channel {
    let mine = install::installed_exe(root).ok();
    if mine
        .as_deref()
        .is_some_and(|path| install::same_file(exe, path))
    {
        return Channel::Installed(mine.unwrap_or_default());
    }
    if !root
        && cfg!(unix)
        && let Ok(system_exe) = install::installed_exe(true)
        && install::same_file(exe, &system_exe)
    {
        return Channel::NeedsRoot(system_exe);
    }
    Channel::Unknown(exe.to_path_buf())
}

fn how_to_update(channel: &Channel) -> String {
    match channel {
        Channel::Installed(path) if path.starts_with(install::SYSTEM_DIR) => {
            "run `sudo sultrakey update`".into()
        }
        Channel::Installed(_) => "run `sultrakey update`".into(),
        Channel::NeedsRoot(_) => "run `sudo sultrakey update`".into(),
        Channel::Unknown(_) => {
            "download the latest binary, then run `sudo ./sultrakey install` (Windows: `.\\sultrakey.exe install`), \
             or run the install script again"
                .into()
        }
    }
}

pub fn run(check: bool, yes: bool) -> Result<i32> {
    let exe = env::current_exe().context("cannot find this program's file")?;
    let channel = detect(&exe, system::is_root());

    let releases = Releases::from_env()?;
    let manifest = Manifest::parse(&releases.latest_manifest()?)?;
    let latest = manifest.version()?;
    let current = Version::current();
    if latest == current {
        output::ok(&format!("sultrakey {current} is the latest version."));
        return Ok(0);
    }
    if latest < current {
        output::info(&format!(
            "sultrakey {current} is newer than the latest release ({latest}); nothing to do."
        ));
        return Ok(0);
    }
    output::info(&format!(
        "sultrakey {latest} is available (installed: {current}): {}",
        releases.page(latest)
    ));
    if check {
        output::info(&format!("To update: {}", how_to_update(&channel)));
        return Ok(1);
    }

    let target = match &channel {
        Channel::Installed(path) => path.clone(),
        Channel::NeedsRoot(path) => {
            return Err(Fail::usage(
                format!("Replacing {} needs root.", path.display()),
                "sudo sultrakey update",
            )
            .into());
        }
        Channel::Unknown(path) => {
            return Err(Fail::usage(
                format!(
                    "This sultrakey runs from {}, not from the install location, so it is not replaced.",
                    path.display()
                ),
                how_to_update(&channel),
            )
            .into());
        }
    };
    let Some(triple) = RELEASE_TARGET else {
        bail!("no sultrakey binary is released for this system");
    };
    let file = manifest.file_for(triple)?;

    if !yes {
        require_terminal("sultrakey update -y (no confirmation)")?;
        let question = format!("Update {} to {latest}?", target.display());
        if !prompt::confirm(&mut Terminal, &question, true)? {
            return Err(Abort::Cancelled.into());
        }
    }

    output::info(&format!("Downloading {}", file.name));
    let bytes = releases.asset(latest, &file.name, BINARY_LIMIT)?;
    let sums = releases.asset(latest, "SHA256SUMS", SUMS_LIMIT)?;
    let listed = sum_for(&String::from_utf8_lossy(&sums), &file.name).with_context(|| {
        format!(
            "SHA256SUMS does not list {}; nothing was changed",
            file.name
        )
    })?;
    if listed != file.sha256 {
        bail!(
            "the release lists two different checksums for {}; nothing was changed",
            file.name
        );
    }
    checksum::verify(&bytes, &listed, &file.name)?;
    install::replace_exe(&target, &bytes, |staged| reports_version(staged, latest))
        .context("nothing was changed")?;

    output::ok(&format!(
        "{} updated to sultrakey {latest}.",
        target.display()
    ));
    output::info(
        "Running applications are not affected; they use the new version from their next restart.",
    );
    Ok(0)
}

/// Runs the downloaded binary before it replaces this one: it must start on this system
/// and call itself the expected version.
fn reports_version(exe: &Path, expected: Version) -> Result<()> {
    let output = Command::new(exe)
        .arg("--version")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .context("the downloaded binary cannot run on this system")?;
    let printed = String::from_utf8_lossy(&output.stdout);
    let printed = printed.trim();
    if !output.status.success() || printed != format!("sultrakey {expected}") {
        bail!("the downloaded binary calls itself '{printed}', not 'sultrakey {expected}'");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_copies_are_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("somewhere-else");
        std::fs::write(&exe, "x").unwrap();
        assert_eq!(detect(&exe, false), Channel::Unknown(exe.clone()));
        assert!(how_to_update(&Channel::Unknown(exe)).contains("install"));
        assert!(
            how_to_update(&Channel::Installed(PathBuf::from("/usr/bin/sultrakey")))
                .contains("sudo")
        );
    }
}
