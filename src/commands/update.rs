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
            "jalankan `sudo sultrakey update`".into()
        }
        Channel::Installed(_) => "jalankan `sultrakey update`".into(),
        Channel::NeedsRoot(_) => "jalankan `sudo sultrakey update`".into(),
        Channel::Unknown(_) => {
            "unduh binary terbaru, lalu jalankan `sudo ./sultrakey install` (Windows: `.\\sultrakey.exe install`), \
             atau jalankan lagi skrip pasang"
                .into()
        }
    }
}

pub fn run(check: bool, yes: bool) -> Result<i32> {
    let exe = env::current_exe().context("file program ini tidak ditemukan")?;
    let channel = detect(&exe, system::is_root());

    let releases = Releases::from_env()?;
    let manifest = Manifest::parse(&releases.latest_manifest()?)?;
    let latest = manifest.version()?;
    let current = Version::current();
    if latest == current {
        output::ok(&format!("sultrakey {current} sudah versi terbaru."));
        return Ok(0);
    }
    if latest < current {
        output::info(&format!(
            "sultrakey {current} lebih baru dari rilis terakhir ({latest}); tidak ada yang dilakukan."
        ));
        return Ok(0);
    }
    output::info(&format!(
        "sultrakey {latest} tersedia (terpasang: {current}): {}",
        releases.page(latest)
    ));
    if check {
        output::info(&format!("Untuk memperbarui: {}", how_to_update(&channel)));
        return Ok(1);
    }

    let target = match &channel {
        Channel::Installed(path) => path.clone(),
        Channel::NeedsRoot(path) => {
            return Err(Fail::usage(
                format!("Butuh hak root untuk mengganti {}.", path.display()),
                "sudo sultrakey update",
            )
            .into());
        }
        Channel::Unknown(path) => {
            return Err(Fail::usage(
                format!(
                    "sultrakey ini berjalan dari {}, bukan dari lokasi pasang, jadi tidak diganti.",
                    path.display()
                ),
                how_to_update(&channel),
            )
            .into());
        }
    };
    let Some(triple) = RELEASE_TARGET else {
        bail!("tidak ada binary sultrakey yang dirilis untuk sistem ini");
    };
    let file = manifest.file_for(triple)?;

    if !yes {
        require_terminal("sultrakey update -y (tanpa konfirmasi)")?;
        let question = format!("Perbarui {} ke {latest}?", target.display());
        if !prompt::confirm(&mut Terminal, &question, true)? {
            return Err(Abort::Cancelled.into());
        }
    }

    output::info(&format!("Mengunduh {}", file.name));
    let bytes = releases.asset(latest, &file.name, BINARY_LIMIT)?;
    let sums = releases.asset(latest, "SHA256SUMS", SUMS_LIMIT)?;
    let listed = sum_for(&String::from_utf8_lossy(&sums), &file.name).with_context(|| {
        format!(
            "SHA256SUMS tidak memuat {}; tidak ada yang diubah",
            file.name
        )
    })?;
    if listed != file.sha256 {
        bail!(
            "rilis mencantumkan dua checksum berbeda untuk {}; tidak ada yang diubah",
            file.name
        );
    }
    checksum::verify(&bytes, &listed, &file.name)?;
    install::replace_exe(&target, &bytes, |staged| reports_version(staged, latest))
        .context("tidak ada yang diubah")?;

    output::ok(&format!(
        "{} diperbarui ke sultrakey {latest}.",
        target.display()
    ));
    output::info(
        "Aplikasi yang sedang berjalan tidak terganggu; versi baru dipakai saat aplikasi di-restart berikutnya.",
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
        .context("binary yang diunduh tidak bisa berjalan di sistem ini")?;
    let printed = String::from_utf8_lossy(&output.stdout);
    let printed = printed.trim();
    if !output.status.success() || printed != format!("sultrakey {expected}") {
        bail!("binary yang diunduh menyebut dirinya '{printed}', bukan 'sultrakey {expected}'");
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
