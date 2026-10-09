//! Reads `.env` and `.env.example` files.
//!
//! Accepted, as common dotenv parsers do: CRLF line endings, an `export ` prefix, single
//! quotes (literal), double quotes (escapes `\\ \" \n \r \t \$`, may span lines), and a
//! comment after a value (` # ...`). Refused with a line number: lines without `=`, bad key
//! names, a key written twice, unknown `# @...` annotations, and an unclosed quote.

use std::collections::HashMap;
use std::fmt;

use super::{
    Document, ENC_PREFIX, Entry, Flags, HEADER_APP, HEADER_PUBLIC_KEY, Header, Item,
    RESERVED_PREFIX, Value, valid_key,
};
use crate::secret::Secret;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "baris {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

fn err(line: usize, message: impl Into<String>) -> ParseError {
    ParseError {
        line,
        message: message.into(),
    }
}

pub fn parse(text: &str) -> Result<Document, ParseError> {
    let text = text
        .strip_prefix('\u{feff}')
        .unwrap_or(text)
        .replace("\r\n", "\n");
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }

    let mut doc = Document::default();
    let mut app: Option<(String, usize)> = None;
    let mut public_key: Option<(String, usize)> = None;
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut pending: Vec<String> = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line_no = i + 1;
        let trimmed = lines[i].trim();

        if trimmed.is_empty() {
            flush(&mut doc, &mut pending);
            doc.items.push(Item::Blank);
            i += 1;
            continue;
        }
        if trimmed.starts_with('#') {
            annotations(trimmed).map_err(|message| err(line_no, message))?;
            pending.push(trimmed.to_string());
            i += 1;
            continue;
        }

        let start = lines[i].trim_start();
        let start = strip_export(start);
        let Some(eq) = start.find('=') else {
            return Err(err(
                line_no,
                "tidak dikenali; setiap baris harus berbentuk KEY=value, komentar (#), atau kosong",
            ));
        };
        let key = start[..eq].trim();
        if !valid_key(key) {
            return Err(err(
                line_no,
                format!(
                    "nama key '{key}' tidak valid; pakai huruf, angka, dan garis bawah, \
                     tidak diawali angka"
                ),
            ));
        }
        let (content, quoted, next) = read_value(&lines, i, start[eq + 1..].trim_start())?;
        i = next;

        if let Some(name) = key.strip_prefix(RESERVED_PREFIX) {
            let slot = match key {
                HEADER_APP => &mut app,
                HEADER_PUBLIC_KEY => &mut public_key,
                _ => {
                    return Err(err(
                        line_no,
                        format!(
                            "key {RESERVED_PREFIX}{name} tidak dikenal; awalan {RESERVED_PREFIX} \
                             dicadangkan untuk sultrakey"
                        ),
                    ));
                }
            };
            if let Some((_, first)) = slot {
                return Err(err(
                    line_no,
                    format!("{key} muncul dua kali (baris {first} dan {line_no})"),
                ));
            }
            *slot = Some((content, line_no));
            flush(&mut doc, &mut pending);
            continue;
        }

        if let Some(first) = seen.insert(key.to_string(), line_no) {
            return Err(err(
                line_no,
                format!("{key} muncul dua kali (baris {first} dan {line_no})"),
            ));
        }
        let comments = std::mem::take(&mut pending);
        let flags = flags_of(&comments);
        doc.items.push(Item::Entry(Entry {
            key: key.to_string(),
            value: value_of(content, quoted),
            comments,
            flags,
            line: line_no,
        }));
    }
    flush(&mut doc, &mut pending);

    doc.header = match (app, public_key) {
        (None, None) => None,
        (Some((app, _)), Some((public_key, _))) => Some(Header { app, public_key }),
        (Some((_, line)), None) => {
            return Err(err(
                line,
                format!("header tidak lengkap: {HEADER_PUBLIC_KEY} tidak ada"),
            ));
        }
        (None, Some((_, line))) => {
            return Err(err(
                line,
                format!("header tidak lengkap: {HEADER_APP} tidak ada"),
            ));
        }
    };
    Ok(doc)
}

/// Comments not followed directly by a key stay as free comments.
fn flush(doc: &mut Document, pending: &mut Vec<String>) {
    doc.items.extend(pending.drain(..).map(Item::Comment));
}

fn strip_export(line: &str) -> &str {
    match line.strip_prefix("export") {
        Some(rest) if rest.starts_with([' ', '\t']) => rest.trim_start(),
        _ => line,
    }
}

/// The annotations of a comment line: `# @plain @optional @masked`. A comment whose text
/// does not start with `@` has none. Unknown names are refused, so a typo cannot silently
/// turn a secret into a plain value or a required key into an optional one.
fn annotations(comment: &str) -> Result<Flags, String> {
    let text = comment.trim_start_matches('#').trim();
    let mut flags = Flags::default();
    if !text.starts_with('@') {
        return Ok(flags);
    }
    for word in text.split_whitespace() {
        match word {
            "@plain" => flags.plain = true,
            "@optional" => flags.optional = true,
            "@masked" => flags.masked = true,
            other => {
                return Err(format!(
                    "anotasi '{other}' tidak dikenal; yang dikenal hanya @plain, @optional, dan @masked"
                ));
            }
        }
    }
    Ok(flags)
}

fn flags_of(comments: &[String]) -> Flags {
    comments.iter().fold(Flags::default(), |acc, line| {
        let flags = annotations(line).unwrap_or_default();
        Flags {
            plain: acc.plain || flags.plain,
            optional: acc.optional || flags.optional,
            masked: acc.masked || flags.masked,
        }
    })
}

fn value_of(content: String, quoted: bool) -> Value {
    if content.is_empty() {
        Value::Empty
    } else if !quoted && let Some(b64) = content.strip_prefix(ENC_PREFIX) {
        Value::Encrypted(b64.to_string())
    } else {
        Value::Plain(Secret::new(content))
    }
}

/// Reads the value that starts on line `index` (`rest` is the text after `=`). Returns the
/// value, whether it was quoted, and the index of the line after it.
fn read_value(
    lines: &[&str],
    index: usize,
    rest: &str,
) -> Result<(String, bool, usize), ParseError> {
    let line_no = index + 1;
    let Some(quote) = rest.chars().next().filter(|c| matches!(c, '"' | '\'')) else {
        return Ok((unquoted(rest), false, index + 1));
    };

    let mut out = String::new();
    let mut current = &rest[1..];
    let mut at = index;
    loop {
        let mut chars = current.char_indices();
        while let Some((pos, c)) = chars.next() {
            if c == quote {
                let after = current[pos + 1..].trim_start();
                if !after.is_empty() && !after.starts_with('#') {
                    return Err(err(
                        at + 1,
                        "ada teks setelah tanda kutip penutup; tambahkan # bila itu komentar",
                    ));
                }
                return Ok((out, true, at + 1));
            }
            if quote == '"' && c == '\\' {
                match chars.next() {
                    Some((_, 'n')) => out.push('\n'),
                    Some((_, 'r')) => out.push('\r'),
                    Some((_, 't')) => out.push('\t'),
                    Some((_, e @ ('\\' | '"' | '$'))) => out.push(e),
                    Some((_, other)) => {
                        out.push('\\');
                        out.push(other);
                    }
                    None => out.push('\\'),
                }
                continue;
            }
            out.push(c);
        }
        at += 1;
        if at >= lines.len() {
            return Err(err(line_no, "tanda kutip tidak ditutup"));
        }
        out.push('\n');
        current = lines[at];
    }
}

/// An unquoted value ends where a comment starts: a `#` at its start or after whitespace.
fn unquoted(rest: &str) -> String {
    let mut end = rest.len();
    let mut previous_space = true;
    for (pos, c) in rest.char_indices() {
        if c == '#' && previous_space {
            end = pos;
            break;
        }
        previous_space = c.is_whitespace();
    }
    rest[..end].trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(text: &str) -> Vec<(String, Value)> {
        parse(text)
            .unwrap()
            .entries()
            .map(|e| (e.key.clone(), e.value.clone()))
            .collect()
    }

    fn plain(text: &str) -> Value {
        Value::Plain(Secret::from(text))
    }

    fn error(text: &str) -> ParseError {
        parse(text).unwrap_err()
    }

    #[test]
    fn reads_the_example_env() {
        let doc = parse(
            "SULTRAKEY_APP=lakupandai\nSULTRAKEY_PUBLIC_KEY=age1xyz\n# Port aplikasi\n# @plain\n\
             PORT=8899\n# Host Redis\nREDIS_HOST=enc:QUJD\n# Password\n# @optional\nREDIS_PASSWORD=\n",
        )
        .unwrap();
        assert_eq!(
            doc.header,
            Some(Header {
                app: "lakupandai".into(),
                public_key: "age1xyz".into()
            })
        );
        let port = doc.get("PORT").unwrap();
        assert_eq!(port.value, plain("8899"));
        assert_eq!(
            port.flags,
            Flags {
                plain: true,
                optional: false,
                masked: false
            }
        );
        assert_eq!(port.help(), ["Port aplikasi"]);
        assert_eq!(port.line, 5);
        assert_eq!(
            doc.get("REDIS_HOST").unwrap().value,
            Value::Encrypted("QUJD".into())
        );
        let password = doc.get("REDIS_PASSWORD").unwrap();
        assert_eq!(password.value, Value::Empty);
        assert!(password.flags.optional && !password.flags.plain);
    }

    #[test]
    fn crlf_bom_and_export() {
        assert_eq!(
            entries("\u{feff}export A=1\r\nB = two words \r\n\texport\tC=3\r\n"),
            [
                ("A".into(), plain("1")),
                ("B".into(), plain("two words")),
                ("C".into(), plain("3"))
            ]
        );
        // `export` without a space is a key named export...
        assert_eq!(entries("exportX=1"), [("exportX".into(), plain("1"))]);
    }

    #[test]
    fn quotes_and_escapes() {
        assert_eq!(
            entries(
                "A='it is # literal \\n'\nB=\"a\\nb\\t\\\"c\\\" \\\\ \\$x \\q\"\nC=\"\" # empty\nD=''\n"
            ),
            [
                ("A".into(), plain("it is # literal \\n")),
                ("B".into(), plain("a\nb\t\"c\" \\ $x \\q")),
                ("C".into(), Value::Empty),
                ("D".into(), Value::Empty),
            ]
        );
    }

    #[test]
    fn multi_line_values() {
        let doc = parse("CERT=\"-----BEGIN-----\r\nAAA  \r\n-----END-----\"\nNEXT=1\nS='a\nb'\n")
            .unwrap();
        assert_eq!(
            doc.get("CERT").unwrap().value,
            plain("-----BEGIN-----\nAAA  \n-----END-----")
        );
        assert_eq!(doc.get("NEXT").unwrap().line, 4);
        assert_eq!(doc.get("S").unwrap().value, plain("a\nb"));
    }

    #[test]
    fn inline_comments_on_unquoted_values() {
        assert_eq!(
            entries("A=x # note\nB=x#not-a-comment\nC=#only comment\nD= \nE=a  b\n"),
            [
                ("A".into(), plain("x")),
                ("B".into(), plain("x#not-a-comment")),
                ("C".into(), Value::Empty),
                ("D".into(), Value::Empty),
                ("E".into(), plain("a  b")),
            ]
        );
    }

    #[test]
    fn quoted_enc_is_plain() {
        assert_eq!(
            entries("A=enc:xyz\nB=\"enc:xyz\"\n"),
            [
                ("A".into(), Value::Encrypted("xyz".into())),
                ("B".into(), plain("enc:xyz"))
            ]
        );
    }

    #[test]
    fn comments_attach_to_the_next_key_only_without_a_blank_line() {
        let doc = parse("# free\n\n# help A\nA=1\n# trailing\n").unwrap();
        assert_eq!(
            doc.items,
            [
                Item::Comment("# free".into()),
                Item::Blank,
                Item::Entry(Entry {
                    key: "A".into(),
                    value: plain("1"),
                    comments: vec!["# help A".into()],
                    flags: Flags::default(),
                    line: 4,
                }),
                Item::Comment("# trailing".into()),
            ]
        );
    }

    #[test]
    fn annotations_on_one_line_or_many() {
        let doc = parse(
            "# @plain @optional\nA=\n#   @optional\n# @plain\nB=\n# email @ kantor\nC=\n\
             # @masked\nD=\n# @plain @masked\nE=\n",
        )
        .unwrap();
        let flags = |k| doc.get(k).unwrap().flags;
        assert_eq!(
            flags("A"),
            Flags {
                plain: true,
                optional: true,
                masked: false
            }
        );
        assert_eq!(
            flags("B"),
            Flags {
                plain: true,
                optional: true,
                masked: false
            }
        );
        assert_eq!(flags("C"), Flags::default());
        assert_eq!(
            flags("D"),
            Flags {
                masked: true,
                ..Flags::default()
            }
        );
        assert_eq!(
            flags("E"),
            Flags {
                plain: true,
                optional: false,
                masked: true
            }
        );
    }

    #[test]
    fn refusals_name_the_line() {
        let e = error("A=1\n# @optinal\nB=2\n");
        assert_eq!(e.line, 2);
        assert!(e.message.contains("@optinal"), "{e}");

        let e = error("A=1\nB=2\nA=3\n");
        assert_eq!(e.line, 3);
        assert!(e.message.contains("baris 1 dan 3"), "{e}");

        assert_eq!(error("A=1\njust text\n").line, 2);
        assert_eq!(error("1A=x\n").line, 1);
        assert_eq!(error("A-B=x\n").line, 1);
        assert_eq!(error("A=\"open\nB=1\n").line, 1);
        assert_eq!(error("A=\"x\" y\n").line, 1);
        assert_eq!(error("SULTRAKEY_OTHER=1\n").line, 1);
        assert_eq!(error("SULTRAKEY_APP=a\n").line, 1);
        assert_eq!(error("SULTRAKEY_PUBLIC_KEY=a\n").line, 1);
        let e = error("SULTRAKEY_APP=a\nSULTRAKEY_PUBLIC_KEY=b\nSULTRAKEY_APP=c\n");
        assert_eq!(e.line, 3);
    }

    #[test]
    fn empty_file() {
        assert_eq!(parse("").unwrap(), Document::default());
        assert_eq!(parse("\n").unwrap().items, [Item::Blank]);
    }
}
