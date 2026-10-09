//! Crash-safe file replacement: readers see either the old or the new file, never a
//! half-written one. Adapted from lopi `src/atomic.rs`, plus file mode and owner on Unix
//! (a `.env` rewritten by `sudo sultrakey setup` must stay readable by the app user).

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use tempfile::NamedTempFile;

/// Who owns the written file (Unix; ignored on Windows).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    /// The current user (the default for new files).
    Current,
    /// Whoever owns the file being replaced; the current user when there is none.
    Keep,
    /// This uid and gid (needs root unless they are the current user's).
    Set { uid: u32, gid: u32 },
}

#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Unix permission bits, e.g. `0o600`.
    pub mode: u32,
    pub owner: Owner,
}

/// Writes `contents` to a temporary file in the same folder, flushes it to disk, then
/// renames it over `path`.
pub fn write(path: &Path, contents: &[u8], options: Options) -> io::Result<()> {
    let dir = folder_of(path);
    let tmp = prepared(path, dir, contents, options)?;
    persist(tmp, path)?;
    sync_dir(dir);
    Ok(())
}

/// Like [`write`], but never replaces an existing file: fails with `AlreadyExists`.
pub fn write_new(path: &Path, contents: &[u8], options: Options) -> io::Result<()> {
    let dir = folder_of(path);
    let tmp = prepared(path, dir, contents, options)?;
    tmp.persist_noclobber(path).map_err(|err| err.error)?;
    sync_dir(dir);
    Ok(())
}

fn folder_of(path: &Path) -> &Path {
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    }
}

fn prepared(
    path: &Path,
    dir: &Path,
    contents: &[u8],
    options: Options,
) -> io::Result<NamedTempFile> {
    let mut tmp = NamedTempFile::new_in(dir)?;
    tmp.write_all(contents)?;
    apply(tmp.as_file(), path, options)?;
    tmp.as_file().sync_all()?;
    Ok(tmp)
}

#[cfg(unix)]
fn apply(file: &fs::File, target: &Path, options: Options) -> io::Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    file.set_permissions(fs::Permissions::from_mode(options.mode))?;
    let wanted = match options.owner {
        Owner::Current => None,
        Owner::Set { uid, gid } => Some((uid, gid)),
        Owner::Keep => match fs::metadata(target) {
            Ok(meta) => Some((meta.uid(), meta.gid())),
            Err(err) if err.kind() == io::ErrorKind::NotFound => None,
            Err(err) => return Err(err),
        },
    };
    let Some((uid, gid)) = wanted else {
        return Ok(());
    };
    let meta = file.metadata()?;
    if meta.uid() == uid && meta.gid() == gid {
        return Ok(());
    }
    nix::unistd::fchown(
        file,
        Some(nix::unistd::Uid::from_raw(uid)),
        Some(nix::unistd::Gid::from_raw(gid)),
    )
    .map_err(io::Error::from)
}

#[cfg(not(unix))]
fn apply(_file: &fs::File, _target: &Path, _options: Options) -> io::Result<()> {
    Ok(())
}

/// On Windows a virus scanner, indexer or editor may hold the target open for a moment,
/// making the rename fail with "access denied"; retry for about a second before giving up.
fn persist(mut tmp: NamedTempFile, path: &Path) -> io::Result<()> {
    const ATTEMPTS: u32 = 10;
    let mut delay = std::time::Duration::from_millis(10);
    for attempt in 1..=ATTEMPTS {
        match tmp.persist(path) {
            Ok(_) => return Ok(()),
            Err(err)
                if cfg!(windows)
                    && err.error.kind() == io::ErrorKind::PermissionDenied
                    && attempt < ATTEMPTS =>
            {
                tmp = err.file;
                std::thread::sleep(delay);
                delay = (delay * 2).min(std::time::Duration::from_millis(200));
            }
            Err(err) => return Err(err.error),
        }
    }
    unreachable!("the last attempt always returns")
}

/// Makes the rename itself durable on Unix. Best effort: some file systems refuse it.
pub fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(dir) = fs::File::open(dir) {
        let _ = dir.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIVATE: Options = Options {
        mode: 0o600,
        owner: Owner::Keep,
    };

    #[test]
    fn replaces_file_and_leaves_no_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        write(&path, b"one", PRIVATE).unwrap();
        write(&path, b"two", PRIVATE).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "two");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn write_new_never_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.key");
        write_new(&path, b"one", PRIVATE).unwrap();
        let err = write_new(&path, b"two", PRIVATE).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&path).unwrap(), "one");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn sets_the_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("k");
        let options = Options {
            mode: 0o400,
            owner: Owner::Current,
        };
        write_new(&path, b"x", options).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o400);
    }
}
