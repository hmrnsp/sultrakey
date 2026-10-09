//! Renders a document back to text: always LF, header first, and values quoted only when
//! a dotenv parser could otherwise misread them.

use super::{Document, ENC_PREFIX, HEADER_APP, HEADER_PUBLIC_KEY, Item, Value};

pub fn render(doc: &Document) -> String {
    let mut out = String::new();
    if let Some(header) = &doc.header {
        out.push_str(&format!("{HEADER_APP}={}\n", header.app));
        out.push_str(&format!("{HEADER_PUBLIC_KEY}={}\n", header.public_key));
    }
    for item in &doc.items {
        match item {
            Item::Blank => out.push('\n'),
            Item::Comment(text) => {
                out.push_str(text);
                out.push('\n');
            }
            Item::Entry(entry) => {
                for comment in &entry.comments {
                    out.push_str(comment);
                    out.push('\n');
                }
                out.push_str(&entry.key);
                out.push('=');
                out.push_str(&format_value(&entry.value));
                out.push('\n');
            }
        }
    }
    out
}

fn format_value(value: &Value) -> String {
    match value {
        Value::Empty => String::new(),
        Value::Encrypted(b64) => format!("{ENC_PREFIX}{b64}"),
        Value::Plain(secret) => quote(secret.expose()),
    }
}

/// Bare when safe; single quotes (literal everywhere) when the text has no `'` or line
/// break; otherwise double quotes with `\\ \" \n \r` escapes. A plain value starting with
/// `enc:` is quoted so it is not read back as an encrypted one.
pub fn quote(text: &str) -> String {
    let special = |c: char| {
        matches!(
            c,
            ' ' | '\t' | '#' | '"' | '\'' | '\\' | '$' | '`' | '\n' | '\r'
        )
    };
    if !text.chars().any(special) && !text.starts_with(ENC_PREFIX) {
        return text.to_string();
    }
    if !text.contains(['\'', '\n', '\r']) {
        return format!("'{text}'");
    }
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::super::{Entry, Flags, Header, parse};
    use super::*;
    use crate::secret::Secret;

    #[test]
    fn quoting_rules() {
        assert_eq!(quote("8899"), "8899");
        assert_eq!(quote("a=b,c:d/e"), "a=b,c:d/e");
        assert_eq!(quote("two words"), "'two words'");
        assert_eq!(quote("p#ss"), "'p#ss'");
        assert_eq!(quote("$HOME"), "'$HOME'");
        assert_eq!(quote("enc:x"), "'enc:x'");
        assert_eq!(quote("it's"), "\"it's\"");
        assert_eq!(quote("a\nb\\c\"d\re"), "\"a\\nb\\\\c\\\"d\\re\"");
    }

    #[test]
    fn values_survive_a_roundtrip() {
        let samples = [
            "simple",
            " leading and trailing ",
            "it's \"quoted\"",
            "back\\slash \\n literal",
            "-----BEGIN CERT-----\nAAAA\n-----END CERT-----\n",
            "crlf\r\nline",
            "#hash",
            "$VAR ${X}",
            "enc:not-really",
            "tab\there",
            "'single'",
            "ünïcödé ✓",
        ];
        for sample in samples {
            let mut doc = Document::default();
            doc.items.push(Item::Entry(Entry::new(
                "K",
                Value::Plain(Secret::from(sample)),
            )));
            let text = render(&doc);
            let back = parse(&text).unwrap();
            assert_eq!(
                back.get("K").unwrap().value,
                Value::Plain(Secret::from(sample)),
                "{sample:?} rendered as {text:?}"
            );
        }
    }

    #[test]
    fn whole_document_roundtrip_is_stable() {
        let text = "SULTRAKEY_APP=demo\nSULTRAKEY_PUBLIC_KEY=age1abc\n# free\n\n# Port\n# @plain\n\
                    PORT=8899\n# @optional\nPASS=\nHOST=enc:QUJD\n";
        let doc = parse(text).unwrap();
        assert_eq!(render(&doc), text);
        assert_eq!(
            doc.header,
            Some(Header {
                app: "demo".into(),
                public_key: "age1abc".into()
            })
        );
        assert_eq!(
            doc.get("PORT").unwrap().flags,
            Flags {
                plain: true,
                optional: false,
                masked: false
            }
        );
    }

    #[test]
    fn crlf_input_renders_as_lf() {
        let doc = parse("# a\r\nA=1\r\n").unwrap();
        assert_eq!(render(&doc), "# a\nA=1\n");
    }
}
