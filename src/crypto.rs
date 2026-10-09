//! age X25519, used as is (no home-made cryptography). One value = one binary age file,
//! base64-encoded (standard alphabet, padded), stored after `enc:`. Key files are standard
//! age identity files, so the official `age` tool can open a value in an emergency:
//! `echo <b64> | base64 -d | age -d -i /etc/sultrakey/<app>.key`.

use std::str::FromStr;

use age::secrecy::ExposeSecret;
use age::x25519::{Identity, Recipient};
use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use zeroize::Zeroizing;

use crate::secret::Secret;

pub fn generate() -> Identity {
    Identity::generate()
}

/// `age1...`
pub fn public_key(identity: &Identity) -> String {
    identity.to_public().to_string()
}

/// The key file as `age-keygen` writes it.
pub fn identity_file_text(identity: &Identity, created: &str) -> Zeroizing<String> {
    Zeroizing::new(format!(
        "# created: {created}\n# public key: {}\n{}\n",
        public_key(identity),
        identity.to_string().expose_secret()
    ))
}

/// Reads a key file: comment lines (`#`) and blank lines are skipped; exactly one
/// `AGE-SECRET-KEY-1...` line must remain.
pub fn parse_identity_file(text: &str) -> Result<Identity, String> {
    let mut keys = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    let (Some(line), None) = (keys.next(), keys.next()) else {
        return Err("harus berisi tepat satu kunci AGE-SECRET-KEY-1...".into());
    };
    Identity::from_str(line).map_err(|_| "bukan kunci age X25519 (AGE-SECRET-KEY-1...)".into())
}

pub fn parse_recipient(text: &str) -> Result<Recipient, String> {
    Recipient::from_str(text.trim()).map_err(|_| format!("'{}' bukan public key age", text.trim()))
}

/// Encrypts `plaintext` to `recipient`; returns the base64 text that follows `enc:`.
pub fn encrypt(recipient: &Recipient, plaintext: &Secret) -> Result<String> {
    let ciphertext =
        age::encrypt(recipient, plaintext.expose().as_bytes()).context("enkripsi gagal")?;
    Ok(STANDARD.encode(ciphertext))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecryptFailure {
    /// The text after `enc:` is not base64.
    NotBase64,
    /// Another key, or damaged data.
    CannotOpen,
    /// Opened, but not UTF-8 text without NUL bytes.
    NotText,
}

pub fn decrypt(identity: &Identity, b64: &str) -> Result<Secret, DecryptFailure> {
    let ciphertext = STANDARD
        .decode(b64.trim())
        .map_err(|_| DecryptFailure::NotBase64)?;
    let plaintext = Zeroizing::new(
        age::decrypt(identity, &ciphertext).map_err(|_| DecryptFailure::CannotOpen)?,
    );
    let text = std::str::from_utf8(&plaintext).map_err(|_| DecryptFailure::NotText)?;
    if text.contains('\0') {
        return Err(DecryptFailure::NotText);
    }
    Ok(Secret::from(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let identity = generate();
        let recipient = parse_recipient(&public_key(&identity)).unwrap();
        for text in [
            "",
            "S3cr3t-uji-xyz",
            "-----BEGIN-----\nAAA\n-----END-----\n",
            "ünï ✓",
        ] {
            let b64 = encrypt(&recipient, &Secret::from(text)).unwrap();
            assert!(!b64.contains(text) || text.is_empty());
            assert!(
                b64.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b))
            );
            assert_eq!(decrypt(&identity, &b64).unwrap().expose(), text);
        }
    }

    #[test]
    fn same_value_encrypts_differently_each_time() {
        let identity = generate();
        let recipient = identity.to_public();
        let a = encrypt(&recipient, &Secret::from("x")).unwrap();
        let b = encrypt(&recipient, &Secret::from("x")).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn failures() {
        let identity = generate();
        let other = generate();
        let b64 = encrypt(&identity.to_public(), &Secret::from("x")).unwrap();
        assert_eq!(
            decrypt(&other, &b64).unwrap_err(),
            DecryptFailure::CannotOpen
        );
        assert_eq!(
            decrypt(&identity, "not base64!").unwrap_err(),
            DecryptFailure::NotBase64
        );
        assert_eq!(
            decrypt(&identity, &STANDARD.encode(b"garbage")).unwrap_err(),
            DecryptFailure::CannotOpen
        );
        let binary = STANDARD.encode(age::encrypt(&identity.to_public(), b"\xff\x00").unwrap());
        assert_eq!(
            decrypt(&identity, &binary).unwrap_err(),
            DecryptFailure::NotText
        );
    }

    #[test]
    fn identity_files() {
        let identity = generate();
        let text = identity_file_text(&identity, "2026-10-09T00:00:00Z");
        assert!(text.starts_with("# created: 2026-10-09T00:00:00Z\n# public key: age1"));
        let back = parse_identity_file(&text).unwrap();
        assert_eq!(public_key(&back), public_key(&identity));
        let crlf = text.replace('\n', "\r\n");
        assert!(parse_identity_file(&crlf).is_ok());

        assert!(parse_identity_file("").is_err());
        assert!(parse_identity_file("# only comments\n").is_err());
        assert!(parse_identity_file(&format!("{}\n{}", *text, *text)).is_err());
        assert!(parse_identity_file("AGE-SECRET-KEY-1NOPE\n").is_err());
        assert!(parse_recipient("age1nope").is_err());
    }
}
