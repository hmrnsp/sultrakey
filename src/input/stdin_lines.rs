//! `setup` without a terminal (scripts, tests): one `KEY=value` per line on stdin.
//! `KEY=@/path/to/file` reads the file. The value is taken as is (no quotes, no escapes).

use std::collections::HashSet;

use super::source::{self, Answer};
use crate::envfile::valid_key;
use crate::error::Fail;

pub fn parse(text: &str) -> Result<Vec<(String, Answer)>, Fail> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line_no = index + 1;
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(Fail::usage(
                format!("Line {line_no} of stdin is not KEY=value."),
                "send one KEY=value line per key, for example: printf 'REDIS_HOST=10.0.0.5\\n' | sultrakey setup",
            ));
        };
        let key = key.trim();
        if !valid_key(key) {
            return Err(Fail::usage(
                format!("Line {line_no} of stdin: key name '{key}' is not valid."),
                "use the key name exactly as in .env.example",
            ));
        }
        if !seen.insert(key.to_string()) {
            return Err(Fail::usage(
                format!("{key} was sent twice on stdin."),
                "send each key only once",
            ));
        }
        out.push((key.to_string(), source::interpret(value)?));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret::Secret;

    #[test]
    fn reads_lines() {
        let parsed =
            parse("# komentar\r\nA=1\n\nB= spaced = kept \nC=@/tmp/c.pem\nD=@@x\nE=\n").unwrap();
        assert_eq!(
            parsed,
            [
                ("A".to_string(), Answer::Text(Secret::from("1"))),
                (
                    "B".to_string(),
                    Answer::Text(Secret::from(" spaced = kept "))
                ),
                ("C".to_string(), Answer::File("/tmp/c.pem".into())),
                ("D".to_string(), Answer::Text(Secret::from("@x"))),
                ("E".to_string(), Answer::Text(Secret::from(""))),
            ]
        );
    }

    #[test]
    fn refusals() {
        assert_eq!(parse("no equals\n").unwrap_err().exit_code(), 64);
        assert!(parse("1A=x\n").is_err());
        assert!(parse("A=1\nA=2\n").is_err());
        assert!(parse("A=@\n").is_err());
    }
}
