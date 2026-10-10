//! Cross-process lock around read-modify-write of `.env`, using a separate `.env.lock`
//! (a lock on `.env` itself would vanish with the atomic rename). Adapted from lopi
//! `src/lock.rs`. The lock file is never deleted: deleting it would let two processes
//! lock two different files.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use crate::error::Fail;

/// How long `setup`/`set` wait for another one to finish saving.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
const RETRY_EVERY: Duration = Duration::from_millis(50);

/// Held while the guarded file is being changed; the OS releases the lock when this is
/// dropped (or when the process dies), so a crash never leaves a stale lock.
#[derive(Debug)]
pub struct FileLock {
    _file: File,
}

impl FileLock {
    /// Waits up to `timeout` for the lock that guards `path`.
    pub fn acquire(path: &Path, timeout: Duration) -> Result<Self> {
        let started = Instant::now();
        loop {
            if let Some(lock) = Self::try_acquire(path)? {
                return Ok(lock);
            }
            if started.elapsed() >= timeout {
                return Err(Fail::other(format!(
                    "Another sultrakey process is changing {}. Wait a moment, then try again.",
                    path.display()
                ))
                .into());
            }
            thread::sleep(RETRY_EVERY);
        }
    }

    /// Takes the lock if it is free, without waiting.
    pub fn try_acquire(path: &Path) -> Result<Option<Self>> {
        let lock_path = lock_path(path);
        let file = open(&lock_path)
            .with_context(|| format!("cannot open the lock file {}", lock_path.display()))?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(err)) => {
                Err(err).with_context(|| format!("cannot lock {}", lock_path.display()))
            }
        }
    }
}

/// A lock file created earlier by root (`sudo sultrakey setup`) may not be writable for the
/// app user; a read-only handle locks just as well.
fn open(path: &Path) -> io::Result<File> {
    match OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
    {
        Err(err) if err.kind() == io::ErrorKind::PermissionDenied && path.exists() => {
            fs::File::open(path)
        }
        result => result,
    }
}

pub fn lock_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".lock");
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_lock_waits_then_times_out() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        let held = FileLock::acquire(&path, DEFAULT_TIMEOUT).unwrap();
        assert!(lock_path(&path).ends_with(".env.lock"));

        assert!(FileLock::try_acquire(&path).unwrap().is_none());
        let started = Instant::now();
        let err = FileLock::acquire(&path, Duration::from_millis(120)).unwrap_err();
        assert!(started.elapsed() >= Duration::from_millis(120));
        assert!(err.to_string().contains("is changing"), "{err}");

        drop(held);
        assert!(FileLock::try_acquire(&path).unwrap().is_some());
    }
}
