//! `install` / `uninstall` / `update`: where sultrakey is installed, and swapping the
//! binary safely. Adapted from lopi `src/install/mod.rs`.
//!
//! Locations: root on Linux/macOS → `/usr/bin` (sudo on RHEL does not search
//! `/usr/local/bin`); other users → `~/.local/bin`; Windows → `%LOCALAPPDATA%\Programs\sultrakey`,
//! added to the user `PATH`.

pub mod path_list;
#[cfg(windows)]
pub mod windows;

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

use crate::fsutil::atomic::{self, Options, Owner};

/// Overrides the install folder (tests).
pub const INSTALL_DIR_ENV: &str = "SULTRAKEY_INSTALL_DIR";
pub const SYSTEM_DIR: &str = "/usr/bin";

/// The install folder for this user (`root` = running as root on Linux/macOS).
pub fn install_dir(root: bool) -> Result<PathBuf> {
    if let Some(dir) = env::var_os(INSTALL_DIR_ENV).filter(|dir| !dir.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    if cfg!(windows) {
        let local =
            dirs::data_local_dir().context("the local application data folder was not found")?;
        Ok(local.join("Programs").join("sultrakey"))
    } else if root {
        Ok(PathBuf::from(SYSTEM_DIR))
    } else {
        dirs::executable_dir()
            .or_else(|| dirs::home_dir().map(|home| home.join(".local").join("bin")))
            .context("the ~/.local/bin folder was not found")
    }
}

pub fn exe_name() -> String {
    format!("sultrakey{}", env::consts::EXE_SUFFIX)
}

pub fn installed_exe(root: bool) -> Result<PathBuf> {
    Ok(install_dir(root)?.join(exe_name()))
}

/// The user `PATH` setting, as stored (not the merged, expanded process `PATH`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathValue {
    pub text: String,
    /// Stored as an expandable string (Windows `REG_EXPAND_SZ`), so `%VAR%` keeps working.
    pub expandable: bool,
}

pub trait UserPath {
    fn read(&self) -> Result<Option<PathValue>>;
    fn write(&mut self, value: &PathValue) -> Result<()>;
}

/// Adds `dir` to the user `PATH`. `Ok(false)` when it was already there.
pub fn add_to_path(user_path: &mut dyn UserPath, dir: &Path) -> Result<bool> {
    let current = user_path.read()?.unwrap_or(PathValue {
        text: String::new(),
        expandable: true,
    });
    let Some(text) = path_list::with_dir(&current.text, &path_text(dir)?) else {
        return Ok(false);
    };
    let expandable = current.expandable || text.contains('%');
    user_path.write(&PathValue { text, expandable })?;
    Ok(true)
}

/// Removes `dir` from the user `PATH`. `Ok(false)` when it was not there.
pub fn remove_from_path(user_path: &mut dyn UserPath, dir: &Path) -> Result<bool> {
    let Some(current) = user_path.read()? else {
        return Ok(false);
    };
    let Some(text) = path_list::without_dir(&current.text, &path_text(dir)?) else {
        return Ok(false);
    };
    user_path.write(&PathValue {
        text,
        expandable: current.expandable,
    })?;
    Ok(true)
}

fn path_text(dir: &Path) -> Result<String> {
    dir.to_str()
        .map(str::to_string)
        .ok_or_else(|| anyhow!("{} is not valid text", dir.display()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Copied {
    Installed,
    Replaced,
    /// The running program already is the installed copy.
    AlreadyThere,
}

const EXECUTABLE: Options = Options {
    mode: 0o755,
    owner: Owner::Current,
};

/// Copies `source` to `target`. A running `target` cannot be overwritten on Windows, but
/// it can be renamed: it is moved to `<target>.old`, which the next install, update or
/// uninstall deletes.
pub fn copy_exe(source: &Path, target: &Path) -> Result<Copied> {
    if same_file(source, target) {
        return Ok(Copied::AlreadyThere);
    }
    remove_leftover(target);
    if let Some(dir) = target.parent() {
        fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    }
    let bytes = fs::read(source).with_context(|| format!("cannot read {}", source.display()))?;
    let existed = target.exists();
    if let Err(err) = atomic::write(target, &bytes, EXECUTABLE) {
        if !(existed && err.kind() == io::ErrorKind::PermissionDenied) {
            return Err(err).with_context(|| format!("cannot write {}", target.display()));
        }
        fs::rename(target, old_path(target))
            .and_then(|()| atomic::write(target, &bytes, EXECUTABLE))
            .with_context(|| format!("cannot replace {} (in use?)", target.display()))?;
    }
    Ok(if existed {
        Copied::Replaced
    } else {
        Copied::Installed
    })
}

/// Deletes the installed exe. When that is the running program, deletion is handed to
/// `self_replace`, which moves it out of the way now and deletes it after exit.
pub fn remove_exe(target: &Path) -> Result<()> {
    remove_leftover(target);
    if !target.exists() {
        return Ok(());
    }
    let running = env::current_exe().is_ok_and(|exe| same_file(&exe, target));
    if running {
        let dir = target.parent().unwrap_or(target);
        self_replace::self_delete_outside_path(dir)
    } else {
        fs::remove_file(target)
    }
    .with_context(|| format!("cannot delete {}", target.display()))?;
    // Windows: the folder is sultrakey's own; remove it once empty. Never on Linux or
    // macOS, where /usr/bin and ~/.local/bin are shared.
    if cfg!(windows)
        && let Some(dir) = target.parent()
    {
        let _ = fs::remove_dir(dir);
    }
    Ok(())
}

/// Replaces the exe `target` with `bytes` (`update`). The new exe is written to a file of
/// its own in the same folder and renamed over `target`, so the old one is never written
/// to in place. `check` runs on the new file first; when it fails, nothing changes.
pub fn replace_exe(
    target: &Path,
    bytes: &[u8],
    check: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    let dir = target.parent().context("the program file has no folder")?;
    remove_leftover(target);
    let mut staged = tempfile::Builder::new()
        .prefix(".sultrakey-update-")
        .suffix(env::consts::EXE_SUFFIX)
        .tempfile_in(dir)
        .with_context(|| format!("cannot write to {}", dir.display()))?;
    staged
        .write_all(bytes)
        .and_then(|()| staged.as_file().sync_all())
        .with_context(|| format!("cannot write to {}", dir.display()))?;
    // Closed before it runs: Linux refuses to start a file that is open for writing.
    let staged = staged.into_temp_path();
    make_executable(&staged)?;
    check(&staged)?;
    swap_in(&staged, target).with_context(|| format!("cannot replace {}", target.display()))?;
    // Renamed away: nothing left for the guard to delete.
    let _ = staged.keep();
    atomic::sync_dir(dir);
    Ok(())
}

fn swap_in(staged: &Path, target: &Path) -> io::Result<()> {
    match fs::rename(staged, target) {
        Err(err)
            if cfg!(windows)
                && err.kind() == io::ErrorKind::PermissionDenied
                && target.exists() =>
        {
            let old = old_path(target);
            fs::rename(target, &old)?;
            fs::rename(staged, target).inspect_err(|_| {
                let _ = fs::rename(&old, target);
            })
        }
        result => result,
    }
}

fn old_path(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    target.with_file_name(name)
}

/// A `.old` copy may still be running; then it stays until a later attempt.
pub fn remove_leftover(target: &Path) {
    let _ = fs::remove_file(old_path(target));
}

pub fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .with_context(|| format!("cannot make {} executable", path.display()))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// In-memory user `PATH` for tests.
    #[derive(Default)]
    pub struct FakePath(pub Option<PathValue>);

    impl UserPath for FakePath {
        fn read(&self) -> Result<Option<PathValue>> {
            Ok(self.0.clone())
        }
        fn write(&mut self, value: &PathValue) -> Result<()> {
            self.0 = Some(value.clone());
            Ok(())
        }
    }

    fn value(text: &str, expandable: bool) -> Option<PathValue> {
        Some(PathValue {
            text: text.into(),
            expandable,
        })
    }

    #[test]
    fn path_add_and_remove_keep_the_value_type() {
        let dir = Path::new(r"C:\Programs\sultrakey");
        let mut path = FakePath(value(r"C:\a", false));
        assert!(add_to_path(&mut path, dir).unwrap());
        assert_eq!(path.0, value(r"C:\a;C:\Programs\sultrakey", false));
        assert!(!add_to_path(&mut path, dir).unwrap(), "added once only");

        assert!(remove_from_path(&mut path, dir).unwrap());
        assert_eq!(path.0, value(r"C:\a", false));
        assert!(!remove_from_path(&mut path, dir).unwrap());

        let mut empty = FakePath::default();
        add_to_path(&mut empty, dir).unwrap();
        assert_eq!(empty.0, value(r"C:\Programs\sultrakey", true));
    }

    #[test]
    fn copy_install_replace_and_remove() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("download").join("sultrakey.exe");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, b"v2").unwrap();
        let target = dir.path().join("programs").join("sultrakey.exe");

        assert_eq!(copy_exe(&source, &target).unwrap(), Copied::Installed);
        assert_eq!(fs::read(&target).unwrap(), b"v2");
        assert_eq!(copy_exe(&source, &target).unwrap(), Copied::Replaced);
        assert_eq!(copy_exe(&target, &target).unwrap(), Copied::AlreadyThere);

        fs::write(old_path(&target), b"leftover").unwrap();
        remove_exe(&target).unwrap();
        assert!(!target.exists());
        assert!(!old_path(&target).exists());
        assert_eq!(target.parent().unwrap().exists(), !cfg!(windows));
    }

    #[test]
    fn replace_checks_the_new_exe_first() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join(exe_name());
        fs::write(&target, b"v1").unwrap();
        fs::write(old_path(&target), b"leftover").unwrap();
        let only_target = || {
            let names: Vec<_> = fs::read_dir(dir.path())
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect();
            assert_eq!(names, [target.file_name().unwrap()], "no other files left");
        };

        let err = replace_exe(&target, b"v2", |staged| {
            assert_eq!(fs::read(staged).unwrap(), b"v2");
            assert_eq!(staged.parent(), target.parent());
            anyhow::bail!("wrong version")
        })
        .unwrap_err();
        assert_eq!(err.to_string(), "wrong version");
        assert_eq!(fs::read(&target).unwrap(), b"v1");
        only_target();

        replace_exe(&target, b"v2", |_| Ok(())).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"v2");
        only_target();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&target).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o755);
        }
    }
}
