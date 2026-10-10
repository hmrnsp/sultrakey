//! SHA-256 checks of downloaded binaries. Pure. Adapted from lopi `src/update/checksum.rs`.

use anyhow::{Result, bail};
use sha2::{Digest, Sha256};

/// Lowercase hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Fails unless `bytes` has the digest `expected` (lowercase hex).
pub fn verify(bytes: &[u8], expected: &str, name: &str) -> Result<()> {
    let actual = sha256_hex(bytes);
    if actual != expected {
        bail!(
            "{name} is not the published file (SHA-256 {actual}, expected {expected}); \
             nothing was changed"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    fn hashes() {
        assert_eq!(sha256_hex(b""), EMPTY);
        assert!(verify(b"", EMPTY, "a").is_ok());
        let err = verify(b"x", EMPTY, "a").unwrap_err().to_string();
        assert!(err.contains("is not the published file"), "{err}");
    }
}
