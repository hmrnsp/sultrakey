//! Where a typed answer comes from: the text itself, or a file named with `@`
//! (`@/tmp/cert.pem`, `@C:\temp\cert.pem`). `@@` stands for a literal leading `@`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use zeroize::Zeroizing;

use crate::error::Fail;
use crate::secret::Secret;

#[derive(Debug, PartialEq, Eq)]
pub enum Answer {
    Text(Secret),
    File(PathBuf),
}

pub fn interpret(input: &str) -> Result<Answer, Fail> {
    if let Some(rest) = input.strip_prefix("@@") {
        return Ok(Answer::Text(Secret::new(format!("@{rest}"))));
    }
    if let Some(path) = input.strip_prefix('@') {
        let path = path.trim();
        if path.is_empty() {
            return Err(Fail::usage(
                "No file path after the @.",
                "write the path, for example: @/tmp/cert.pem",
            ));
        }
        return Ok(Answer::File(PathBuf::from(path)));
    }
    Ok(Answer::Text(Secret::from(input)))
}

/// The value an answer stands for.
pub fn resolve(answer: Answer) -> Result<Secret, Fail> {
    match answer {
        Answer::Text(text) => {
            check_text(&text, "Value")?;
            Ok(text)
        }
        Answer::File(path) => read_file(&path),
    }
}

/// A file's text as a value: read as is, except one final line break (`\n` or `\r\n`),
/// which editors add on their own. Must be UTF-8 without NUL bytes, because environment
/// variables cannot carry binary data.
pub fn read_file(path: &Path) -> Result<Secret, Fail> {
    let bytes = Zeroizing::new(fs::read(path).map_err(|err| {
        let problem = match err.kind() {
            io::ErrorKind::NotFound => format!("File {} not found.", path.display()),
            io::ErrorKind::PermissionDenied => {
                format!("No permission to read the file {}.", path.display())
            }
            _ => format!("File {} cannot be read: {err}.", path.display()),
        };
        Fail::usage(
            problem,
            "check the path and the file permissions, then try again",
        )
    })?);
    let text = std::str::from_utf8(&bytes).map_err(|_| not_text(path))?;
    let text = text
        .strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .unwrap_or(text);
    let secret = Secret::from(text);
    if secret.expose().contains('\0') {
        return Err(not_text(path));
    }
    Ok(secret)
}

fn not_text(path: &Path) -> Fail {
    Fail::usage(
        format!(
            "File {} is not text (binary data cannot go into the environment).",
            path.display()
        ),
        "store its contents as text, for example base64, then try again",
    )
}

/// Refuses NUL bytes in typed or piped values.
pub fn check_text(text: &Secret, what: &str) -> Result<(), Fail> {
    if text.expose().contains('\0') {
        return Err(Fail::usage(
            format!("{what} contains a NUL character, which cannot go into the environment."),
            "remove that character, then try again",
        ));
    }
    Ok(())
}

/// Removes one final line break (`echo secret | sultrakey set KEY --stdin`).
pub fn strip_one_newline(text: &str) -> &str {
    text.strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .unwrap_or(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(answer: Answer) -> String {
        match answer {
            Answer::Text(secret) => secret.expose().to_string(),
            Answer::File(path) => panic!("file {}", path.display()),
        }
    }

    #[test]
    fn at_means_file_and_double_at_is_literal() {
        assert_eq!(text(interpret("abc").unwrap()), "abc");
        assert_eq!(text(interpret("@@abc").unwrap()), "@abc");
        assert_eq!(text(interpret("").unwrap()), "");
        assert_eq!(
            interpret("@/tmp/cert.pem").unwrap(),
            Answer::File("/tmp/cert.pem".into())
        );
        assert_eq!(
            interpret(r"@ C:\temp\c.pem ").unwrap(),
            Answer::File(r"C:\temp\c.pem".into())
        );
        assert_eq!(interpret("@").unwrap_err().exit_code(), 64);
    }

    #[test]
    fn reads_files_dropping_one_final_line_break() {
        let dir = tempfile::tempdir().unwrap();
        let read = |name: &str, bytes: &[u8]| {
            let path = dir.path().join(name);
            fs::write(&path, bytes).unwrap();
            read_file(&path)
        };
        assert_eq!(
            read("a", b"line1\nline2\n").unwrap().expose(),
            "line1\nline2"
        );
        assert_eq!(read("b", b"x\r\n").unwrap().expose(), "x");
        assert_eq!(read("c", b"x\n\n").unwrap().expose(), "x\n");
        assert_eq!(read("d", b"no newline").unwrap().expose(), "no newline");
        assert!(read("e", b"\xff\xfe").is_err());
        assert!(read("f", b"a\0b").is_err());
        let missing = read_file(&dir.path().join("missing")).unwrap_err();
        assert!(missing.to_string().contains("not found"), "{missing}");
    }

    #[test]
    fn typed_text_without_nul() {
        assert!(check_text(&Secret::from("ok"), "Value").is_ok());
        assert!(check_text(&Secret::from("a\0"), "Value").is_err());
        assert!(resolve(Answer::Text(Secret::from("a\0"))).is_err());
        assert_eq!(strip_one_newline("a\n\n"), "a\n");
        assert_eq!(strip_one_newline("a\r\n"), "a");
    }
}
