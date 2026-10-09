//! Plaintext values. Wiped from memory when dropped, and never shown by `{:?}`, so a stray
//! debug print or error message cannot leak one.

use std::fmt;

use zeroize::Zeroizing;

#[derive(Clone, Default, PartialEq, Eq)]
pub struct Secret(Zeroizing<String>);

impl Secret {
    pub fn new(text: String) -> Self {
        Self(Zeroizing::new(text))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<&str> for Secret {
    fn from(text: &str) -> Self {
        Self::new(text.to_string())
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Secret([disembunyikan])")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_hides_the_value() {
        let secret = Secret::from("S3cr3t");
        assert!(!format!("{secret:?}").contains("S3cr3t"));
        assert_eq!(secret.expose(), "S3cr3t");
    }
}
