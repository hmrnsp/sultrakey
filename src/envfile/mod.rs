//! The `.env` file and its template: a header naming the app and its public key, then
//! keys in order, each with the comment block directly above it (help text and
//! `# @plain` / `# @optional` annotations).
//!
//! `parse` reads, `write` renders, `sync` merges a template into a `.env`. All pure.

pub mod parse;
pub mod sync;
pub mod write;

pub use parse::{ParseError, parse};
pub use sync::{SyncResult, sync};
pub use write::render;

use crate::secret::Secret;

pub const HEADER_APP: &str = "SULTRAKEY_APP";
pub const HEADER_PUBLIC_KEY: &str = "SULTRAKEY_PUBLIC_KEY";
/// Keys starting with this belong to sultrakey and never reach the application.
pub const RESERVED_PREFIX: &str = "SULTRAKEY_";
/// Marks an encrypted value: `enc:<base64 of a binary age file>`.
pub const ENC_PREFIX: &str = "enc:";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub app: String,
    pub public_key: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Document {
    pub header: Option<Header>,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Blank,
    /// A comment not attached to a key (a blank line separates it from the next key).
    Comment(String),
    Entry(Entry),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub key: String,
    pub value: Value,
    /// The comment lines directly above the key, as written (each starts with `#`).
    pub comments: Vec<String>,
    pub flags: Flags,
    /// Line number in the file it was read from (0 when built in memory).
    pub line: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Flags {
    /// `@plain`: stored unencrypted (not a secret).
    pub plain: bool,
    /// `@optional`: may stay empty.
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Empty,
    Plain(Secret),
    /// The base64 text after `enc:`.
    Encrypted(String),
}

/// What `list` shows for a key. Never the value itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Encrypted,
    Plain,
    /// A secret key holding a plain value: must be encrypted (`fill` does it).
    PlainSecret,
    EmptyRequired,
    EmptyOptional,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Self::Encrypted => "terenkripsi",
            Self::Plain => "plain",
            Self::PlainSecret => "polos — harus dienkripsi",
            Self::EmptyRequired => "kosong — wajib diisi",
            Self::EmptyOptional => "kosong (opsional)",
        }
    }
}

impl Entry {
    pub fn new(key: impl Into<String>, value: Value) -> Self {
        Self {
            key: key.into(),
            value,
            comments: Vec::new(),
            flags: Flags::default(),
            line: 0,
        }
    }

    /// The help text: comment lines that are not annotations, without the `#`.
    pub fn help(&self) -> Vec<&str> {
        self.comments
            .iter()
            .filter_map(|line| {
                let text = line.trim().strip_prefix('#')?.trim();
                (!text.is_empty() && !text.starts_with('@')).then_some(text)
            })
            .collect()
    }

    pub fn status(&self) -> Status {
        match (&self.value, self.flags) {
            (Value::Encrypted(_), _) => Status::Encrypted,
            (Value::Plain(_), flags) if flags.plain => Status::Plain,
            (Value::Plain(_), _) => Status::PlainSecret,
            (Value::Empty, flags) if flags.optional => Status::EmptyOptional,
            (Value::Empty, _) => Status::EmptyRequired,
        }
    }
}

impl Document {
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.items.iter().filter_map(|item| match item {
            Item::Entry(entry) => Some(entry),
            _ => None,
        })
    }

    pub fn entries_mut(&mut self) -> impl Iterator<Item = &mut Entry> {
        self.items.iter_mut().filter_map(|item| match item {
            Item::Entry(entry) => Some(entry),
            _ => None,
        })
    }

    pub fn get(&self, key: &str) -> Option<&Entry> {
        self.entries().find(|entry| entry.key == key)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Entry> {
        self.entries_mut().find(|entry| entry.key == key)
    }
}

/// `[A-Za-z_][A-Za-z0-9_]*`, the names every shell and runtime accepts.
pub fn valid_key(key: &str) -> bool {
    let mut bytes = key.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// `[a-z0-9-]+`: app names become file names (`/etc/sultrakey/<app>.key`).
pub fn valid_app(app: &str) -> bool {
    !app.is_empty()
        && app
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_and_app_names() {
        for ok in ["A", "_A", "REDIS_HOST", "a1", "x_Y_9"] {
            assert!(valid_key(ok), "{ok}");
        }
        for bad in ["", "1A", "A-B", "A B", "A.B", "ä"] {
            assert!(!valid_key(bad), "{bad}");
        }
        for ok in ["lakupandai", "app-1", "x"] {
            assert!(valid_app(ok), "{ok}");
        }
        for bad in ["", "App", "a_b", "a/b", "a.b", "../x"] {
            assert!(!valid_app(bad), "{bad}");
        }
    }

    #[test]
    fn help_skips_annotations() {
        let mut entry = Entry::new("A", Value::Empty);
        entry.comments = vec!["# Host Redis".into(), "# @optional".into(), "#".into()];
        assert_eq!(entry.help(), ["Host Redis"]);
    }

    #[test]
    fn statuses() {
        let mut entry = Entry::new("A", Value::Plain("x".into()));
        assert_eq!(entry.status(), Status::PlainSecret);
        entry.flags.plain = true;
        assert_eq!(entry.status(), Status::Plain);
        entry.value = Value::Empty;
        assert_eq!(entry.status(), Status::EmptyRequired);
        entry.flags.optional = true;
        assert_eq!(entry.status(), Status::EmptyOptional);
        entry.value = Value::Encrypted("abc".into());
        assert_eq!(entry.status(), Status::Encrypted);
    }
}
