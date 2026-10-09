//! Where the private key lives, and reading/creating it.
//!
//! Search order: `--key-file` → `$SULTRAKEY_KEY_FILE` → `$CREDENTIALS_DIRECTORY/sultrakey.key`
//! (systemd `LoadCredential=`, systemd ≥ 247) → `$SULTRAKEY_KEY_DIR/<app>.key` → the default
//! folder: `/etc/sultrakey/<app>.key` (Linux, macOS) or `%APPDATA%\sultrakey\<app>.key`
//! (Windows).

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use age::x25519::Identity;
use anyhow::{Context, Result};

use crate::crypto;
use crate::error::Fail;
use crate::fsutil::atomic::{self, Options, Owner as FileOwner};
use crate::system::{self, Owner};
use crate::time;

pub const KEY_FILE_ENV: &str = "SULTRAKEY_KEY_FILE";
/// Replaces the default folder (tests, unusual setups).
pub const KEY_DIR_ENV: &str = "SULTRAKEY_KEY_DIR";
pub const CREDENTIAL_NAME: &str = "sultrakey.key";

/// Everything the search looks at, gathered once so the search itself is pure.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub flag: Option<PathBuf>,
    pub key_file_env: Option<PathBuf>,
    pub credentials_dir: Option<PathBuf>,
    pub key_dir_env: Option<PathBuf>,
    pub default_dir: Option<PathBuf>,
}

impl Facts {
    pub fn gather(flag: Option<&Path>) -> Self {
        let var = |name: &str| {
            env::var_os(name)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        };
        Self {
            flag: flag.map(Path::to_path_buf),
            key_file_env: var(KEY_FILE_ENV),
            credentials_dir: var("CREDENTIALS_DIRECTORY"),
            key_dir_env: var(KEY_DIR_ENV),
            default_dir: default_dir(),
        }
    }
}

/// `/etc/sultrakey`, or `%APPDATA%\sultrakey` on Windows.
pub fn default_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        dirs::config_dir().map(|dir| dir.join("sultrakey"))
    } else {
        Some(PathBuf::from("/etc/sultrakey"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    pub path: PathBuf,
    /// Not the default folder: chosen by flag or environment. Creating a key there does
    /// not need root.
    pub custom: bool,
}

/// The key file for `app`, in search order. `exists` is only asked about the systemd
/// credential, which is used only when present.
pub fn locate(facts: &Facts, app: &str, exists: impl Fn(&Path) -> bool) -> Result<Located, Fail> {
    let custom = |path: PathBuf| Located { path, custom: true };
    if let Some(path) = &facts.flag {
        return Ok(custom(path.clone()));
    }
    if let Some(path) = &facts.key_file_env {
        return Ok(custom(path.clone()));
    }
    if let Some(dir) = &facts.credentials_dir {
        let path = dir.join(CREDENTIAL_NAME);
        if exists(&path) {
            return Ok(custom(path));
        }
    }
    let file = format!("{app}.key");
    if let Some(dir) = &facts.key_dir_env {
        return Ok(custom(dir.join(file)));
    }
    match &facts.default_dir {
        Some(dir) => Ok(Located {
            path: dir.join(file),
            custom: false,
        }),
        None => Err(Fail::other(
            "Folder data aplikasi (%APPDATA%) tidak ditemukan.",
        )),
    }
}

/// Why a key file could not be used.
#[derive(Debug)]
pub enum ReadError {
    Missing,
    Denied,
    Invalid(String),
    Io(io::Error),
}

pub fn read(path: &Path) -> Result<Identity, ReadError> {
    let text =
        zeroize::Zeroizing::new(fs::read_to_string(path).map_err(|err| match err.kind() {
            io::ErrorKind::NotFound => ReadError::Missing,
            io::ErrorKind::PermissionDenied => ReadError::Denied,
            io::ErrorKind::InvalidData => ReadError::Invalid("bukan file teks".into()),
            _ => ReadError::Io(err),
        })?);
    crypto::parse_identity_file(&text).map_err(ReadError::Invalid)
}

/// The failure to show for a key that cannot be read. `missing_hint` is the fix when the
/// file does not exist (it depends on whether the `.env` already uses a key).
pub fn read_fail(err: ReadError, path: &Path, missing_hint: &str) -> Fail {
    let shown = path.display();
    match err {
        ReadError::Missing => {
            Fail::config(format!("File kunci {shown} tidak ditemukan."), missing_hint)
        }
        ReadError::Denied => {
            let owner = system::owner_name(path).unwrap_or_else(|| "<pemilik-kunci>".into());
            Fail::config(
                format!("Tidak punya izin membaca file kunci {shown} (pemiliknya: {owner})."),
                format!(
                    "jalankan sebagai user pemilik kunci, contoh: sudo -u {owner} sultrakey check \
                     (di pm2/systemd/Docker: jalankan aplikasi sebagai user {owner})"
                ),
            )
        }
        ReadError::Invalid(why) => Fail::config(
            format!("File kunci {shown} rusak: {why}."),
            "pulihkan file kunci dari backup",
        ),
        ReadError::Io(err) => {
            Fail::config_bare(format!("File kunci {shown} tidak bisa dibaca: {err}."))
        }
    }
}

/// Writes a new key file: mode 0400, owned by `owner` (Unix), never replacing a file.
/// The folder is created (0755) when missing.
pub fn create(path: &Path, identity: &Identity, owner: Option<&Owner>) -> Result<()> {
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty())
        && !dir.exists()
    {
        create_dir(dir).with_context(|| format!("tidak bisa membuat folder {}", dir.display()))?;
    }
    let contents = crypto::identity_file_text(identity, &time::now_rfc3339());
    let options = Options {
        mode: 0o400,
        owner: match owner {
            Some(owner) => FileOwner::Set {
                uid: owner.uid,
                gid: owner.gid,
            },
            None => FileOwner::Current,
        },
    };
    atomic::write_new(path, contents.as_bytes(), options)
        .with_context(|| format!("tidak bisa menulis file kunci {}", path.display()))
}

#[cfg(unix)]
fn create_dir(dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o755)
        .create(dir)
}

#[cfg(not(unix))]
fn create_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> Facts {
        Facts {
            default_dir: Some(PathBuf::from("/etc/sultrakey")),
            ..Facts::default()
        }
    }

    fn found(facts: &Facts, credential_exists: bool) -> Located {
        locate(facts, "demo", |_| credential_exists).unwrap()
    }

    #[test]
    fn search_order() {
        let mut f = facts();
        assert_eq!(
            found(&f, true),
            Located {
                path: PathBuf::from("/etc/sultrakey/demo.key"),
                custom: false
            }
        );

        f.key_dir_env = Some("/tmp/keys".into());
        assert_eq!(found(&f, true).path, PathBuf::from("/tmp/keys/demo.key"));
        assert!(found(&f, true).custom);

        f.credentials_dir = Some("/run/credentials/app.service".into());
        assert_eq!(found(&f, false).path, PathBuf::from("/tmp/keys/demo.key"));
        assert_eq!(
            found(&f, true).path,
            PathBuf::from("/run/credentials/app.service/sultrakey.key")
        );

        f.key_file_env = Some("/env/k.key".into());
        assert_eq!(found(&f, true).path, PathBuf::from("/env/k.key"));

        f.flag = Some("/flag/k.key".into());
        assert_eq!(found(&f, true).path, PathBuf::from("/flag/k.key"));

        let none = Facts::default();
        assert!(locate(&none, "demo", |_| false).is_err());
    }

    #[test]
    fn create_then_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keys").join("demo.key");
        let identity = crypto::generate();
        create(&path, &identity, None).unwrap();
        let back = read(&path).unwrap();
        assert_eq!(crypto::public_key(&back), crypto::public_key(&identity));
        assert!(
            create(&path, &crypto::generate(), None).is_err(),
            "never replaced"
        );
        assert_eq!(
            crypto::public_key(&read(&path).unwrap()),
            crypto::public_key(&identity)
        );
        assert!(matches!(
            read(&dir.path().join("none")),
            Err(ReadError::Missing)
        ));
        fs::write(dir.path().join("bad"), "hello").unwrap();
        assert!(matches!(
            read(&dir.path().join("bad")),
            Err(ReadError::Invalid(_))
        ));
    }
}
