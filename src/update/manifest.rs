//! `manifest.json`, published with every release by `.github/workflows/release.yml`:
//!
//! ```json
//! {"name":"sultrakey","version":"0.1.0","files":[
//!   {"target":"x86_64-unknown-linux-musl","name":"sultrakey-x86_64-unknown-linux-musl","sha256":"..."}]}
//! ```
//!
//! Pure.

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::version::Version;

const APP_NAME: &str = "sultrakey";

#[derive(Debug, Deserialize)]
pub struct Manifest {
    name: String,
    version: String,
    #[serde(default)]
    files: Vec<FileEntry>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct FileEntry {
    pub target: String,
    pub name: String,
    pub sha256: String,
}

impl Manifest {
    pub fn parse(json: &[u8]) -> Result<Self> {
        let manifest: Self = serde_json::from_slice(json).context("manifest rilis tidak valid")?;
        if manifest.name != APP_NAME {
            bail!(
                "manifest rilis bukan milik {APP_NAME} ('{}')",
                manifest.name
            );
        }
        Ok(manifest)
    }

    pub fn version(&self) -> Result<Version> {
        self.version
            .parse()
            .with_context(|| format!("manifest rilis berisi versi yang salah '{}'", self.version))
    }

    /// The binary for `target`, with a checked name and digest.
    pub fn file_for(&self, target: &str) -> Result<FileEntry> {
        let mut found = self.files.iter().filter(|file| file.target == target);
        let file = match (found.next(), found.next()) {
            (Some(one), None) => one.clone(),
            (None, _) => bail!("rilis terbaru tidak punya binary untuk sistem ini ({target})"),
            (Some(_), Some(_)) => bail!("rilis terbaru punya beberapa binary untuk {target}"),
        };
        check_file_name(&file.name)?;
        if !is_sha256_hex(&file.sha256) {
            bail!(
                "manifest rilis berisi checksum yang salah untuk {}",
                file.name
            );
        }
        Ok(FileEntry {
            sha256: file.sha256.to_ascii_lowercase(),
            ..file
        })
    }
}

pub fn is_sha256_hex(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Names go into download URLs: plain file names only.
fn check_file_name(name: &str) -> Result<()> {
    let plain = !name.is_empty()
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if !plain {
        bail!("manifest rilis berisi nama file yang tidak wajar '{name}'");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUM: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    fn manifest() -> String {
        format!(
            r#"{{"name":"sultrakey","version":"0.2.0","files":[
                {{"target":"x86_64-unknown-linux-musl","name":"sultrakey-x86_64-unknown-linux-musl","sha256":"{}"}},
                {{"target":"x86_64-pc-windows-msvc","name":"sultrakey-x86_64-pc-windows-msvc.exe","sha256":"{SUM}"}}
            ]}}"#,
            SUM.to_ascii_uppercase()
        )
    }

    #[test]
    fn reads_version_and_files() {
        let m = Manifest::parse(manifest().as_bytes()).unwrap();
        assert_eq!(m.version().unwrap(), Version(0, 2, 0));
        let linux = m.file_for("x86_64-unknown-linux-musl").unwrap();
        assert_eq!(linux.name, "sultrakey-x86_64-unknown-linux-musl");
        assert_eq!(linux.sha256, SUM);
        assert!(m.file_for("x86_64-pc-windows-msvc").is_ok());
        let err = m.file_for("riscv64").unwrap_err().to_string();
        assert!(err.contains("tidak punya binary"), "{err}");
    }

    #[test]
    fn refusals() {
        assert!(Manifest::parse(b"not json").is_err());
        assert!(Manifest::parse(br#"{"name":"other","version":"1.0.0"}"#).is_err());
        let bad_version = manifest().replace("0.2.0", "v0.2.0");
        assert!(
            Manifest::parse(bad_version.as_bytes())
                .unwrap()
                .version()
                .is_err()
        );
        let evil = manifest().replace("sultrakey-x86_64-unknown-linux-musl\"", "../x\"");
        assert!(
            Manifest::parse(evil.as_bytes())
                .unwrap()
                .file_for("x86_64-unknown-linux-musl")
                .is_err()
        );
        let bad_sum = manifest().replace(&SUM.to_ascii_uppercase(), "abc");
        assert!(
            Manifest::parse(bad_sum.as_bytes())
                .unwrap()
                .file_for("x86_64-unknown-linux-musl")
                .is_err()
        );
        for name in ["", ".hidden", "a/b", "a b", "a\\b", "ä"] {
            assert!(check_file_name(name).is_err(), "{name}");
        }
    }
}
