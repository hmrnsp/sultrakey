//! `setup`: brings the `.env` in line with the template, encrypts plain values of secret
//! keys (D13), then asks for every empty key (D12) on one screen. Without a terminal it reads
//! `KEY=value` lines from stdin instead. The file is written once, at the end: stopping
//! halfway changes nothing.

use std::path::PathBuf;

use age::x25519::Recipient;
use anyhow::Result;

use super::{Ctx, header, load_env, load_template, read_stdin, recipient, write_env};
use crate::crypto;
use crate::envfile::{self, Document, Entry, Value};
use crate::error::Fail;
use crate::fsutil::atomic::Owner as FileOwner;
use crate::fsutil::lock::{DEFAULT_TIMEOUT, FileLock};
use crate::input::form::{self, Field};
use crate::input::prompt::{self, Reply};
use crate::input::{source, stdin_lines};
use crate::output;
use crate::secret::Secret;

pub fn run(ctx: &Ctx) -> Result<i32> {
    let _lock = FileLock::acquire(&ctx.env, DEFAULT_TIMEOUT)?;
    let env = load_env(ctx)?;
    let header = header(ctx, &env)?;
    let recipient = recipient(ctx, &header)?;
    let template = load_template(ctx)?;

    let synced = envfile::sync(&template, &env);
    let mut doc = synced.doc;
    if !synced.added.is_empty() {
        output::info(&format!(
            "New keys from the template: {}",
            synced.added.join(", ")
        ));
    }
    for key in &synced.extra {
        output::warn(&format!(
            "{key} is in .env but not in the template; kept at the end of the file."
        ));
    }

    let protected = encrypt_plain_secrets(&mut doc, &recipient)?;
    if !protected.is_empty() {
        output::ok(&format!("Plain values encrypted: {}", protected.join(", ")));
    }

    let mut files = Vec::new();
    let filled = if prompt::interactive() {
        fill_in_form(&mut doc, &template, &recipient, &mut files)?
    } else {
        from_stdin(ctx, &mut doc, &template, &recipient, &mut files)?
    };

    write_env(ctx, &doc, FileOwner::Keep)?;
    if filled.is_empty() {
        output::ok(&format!("{} saved.", ctx.env.display()));
    } else {
        output::ok(&format!(
            "{} saved. Filled: {}",
            ctx.env.display(),
            filled.join(", ")
        ));
    }
    for file in &files {
        output::warn(&format!("Delete the file {} now.", file.display()));
    }
    let missing: Vec<&str> = doc
        .entries()
        .filter(|entry| entry.value == Value::Empty && !entry.flags.optional)
        .map(|entry| entry.key.as_str())
        .collect();
    if missing.is_empty() {
        output::info(&format!("Next: {}", ctx.cmd("check")));
    } else {
        output::warn(&format!("Still empty: {}", missing.join(", ")));
        output::info(&format!("Fix: {}", ctx.cmd("setup")));
    }
    Ok(0)
}

/// D13: plain values in keys that are not `@plain` get encrypted.
fn encrypt_plain_secrets(doc: &mut Document, recipient: &Recipient) -> Result<Vec<String>> {
    let mut keys = Vec::new();
    for entry in doc.entries_mut() {
        if let Value::Plain(value) = &entry.value
            && !entry.flags.plain
        {
            entry.value = Value::Encrypted(crypto::encrypt(recipient, value)?);
            keys.push(entry.key.clone());
        }
    }
    Ok(keys)
}

/// The template's value for `entry`, offered as the default. Secret keys never take it: an
/// example password in the template must not end up as the real one.
fn default_for<'a>(template: &'a Document, entry: &Entry) -> Option<&'a str> {
    if entry.masked() {
        return None;
    }
    match &template.get(&entry.key)?.value {
        Value::Plain(value) => Some(value.expose()),
        _ => None,
    }
}

fn store(plain: bool, value: Secret, recipient: &Recipient) -> Result<Value> {
    Ok(if value.is_empty() {
        Value::Empty
    } else if plain {
        Value::Plain(value)
    } else {
        Value::Encrypted(crypto::encrypt(recipient, &value)?)
    })
}

/// Every empty key on one screen (`input::form`). Nothing is stored before the answers
/// are reviewed and saved there.
fn fill_in_form(
    doc: &mut Document,
    template: &Document,
    recipient: &Recipient,
    files: &mut Vec<PathBuf>,
) -> Result<Vec<String>> {
    let fields = fields(doc, template);
    if fields.is_empty() {
        output::info("Every key already has a value.");
        return Ok(Vec::new());
    }
    let keys: Vec<String> = fields.iter().map(|field| field.key.clone()).collect();
    let answers = form::run(fields)?;
    apply(doc, keys, answers, recipient, files)
}

/// What the form asks about each empty key, in file order.
fn fields(doc: &Document, template: &Document) -> Vec<Field> {
    doc.entries()
        .filter(|entry| entry.value == Value::Empty)
        .map(|entry| Field {
            key: entry.key.clone(),
            help: entry.help().into_iter().map(String::from).collect(),
            masked: entry.masked(),
            plain: entry.flags.plain,
            optional: entry.flags.optional,
            default: default_for(template, entry).map(String::from),
        })
        .collect()
}

/// Stores the answers; unanswered keys stay empty. Returns the keys that got a value.
fn apply(
    doc: &mut Document,
    keys: Vec<String>,
    answers: Vec<Option<Reply>>,
    recipient: &Recipient,
    files: &mut Vec<PathBuf>,
) -> Result<Vec<String>> {
    let mut filled = Vec::new();
    for (key, answer) in keys.into_iter().zip(answers) {
        let Some(entry) = doc.get_mut(&key) else {
            continue;
        };
        entry.value = match answer {
            Some(Reply::Value(value, file)) => {
                files.extend(file);
                store(entry.flags.plain, value, recipient)?
            }
            Some(Reply::Empty) | None => Value::Empty,
        };
        if entry.value != Value::Empty {
            filled.push(key);
        }
    }
    Ok(filled)
}

/// `KEY=value` lines. Keys that already have a value are not changed (that is what `set`
/// is for); empty keys not sent take the template's default, except secret keys, which
/// stay empty until sent.
fn from_stdin(
    ctx: &Ctx,
    doc: &mut Document,
    template: &Document,
    recipient: &Recipient,
    files: &mut Vec<PathBuf>,
) -> Result<Vec<String>> {
    let text = read_stdin()?;
    let lines = stdin_lines::parse(&text)?;
    let mut filled = Vec::new();
    for (key, answer) in lines {
        let Some(entry) = doc.get_mut(&key) else {
            return Err(Fail::usage(
                format!("{key} is not in the template {}.", ctx.template.display()),
                format!(
                    "add {key} to the template, then run {} again",
                    ctx.cmd("setup")
                ),
            )
            .into());
        };
        if entry.value != Value::Empty {
            output::warn(&format!(
                "{key} already has a value; not changed (to replace it: {}).",
                ctx.cmd(&format!("set {key}"))
            ));
            continue;
        }
        if let source::Answer::File(path) = &answer {
            files.push(path.clone());
        }
        let value = source::resolve(answer)?;
        entry.value = store(entry.flags.plain, value, recipient)?;
        if entry.value != Value::Empty {
            filled.push(key);
        }
    }
    for entry in doc.entries_mut() {
        if entry.value == Value::Empty
            && let Some(default) = default_for(template, entry)
        {
            entry.value = store(entry.flags.plain, Secret::from(default), recipient)?;
            filled.push(entry.key.clone());
        }
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_looking_keys_are_masked_and_never_take_the_example_value() {
        let template = envfile::parse(
            "# Port aplikasi\n# @plain\nPORT=8899\nREDIS_HOST=\n# @optional\nREDIS_PASSWORD=\n\
             # @masking\nDATABASE_URL=\nJWT_SECRET=changeme\n",
        )
        .unwrap();
        let doc = envfile::sync(&template, &Document::default()).doc;

        let fields = fields(&doc, &template);

        let summary: Vec<(&str, bool, bool, bool, Option<&str>)> = fields
            .iter()
            .map(|f| {
                (
                    f.key.as_str(),
                    f.masked,
                    f.plain,
                    f.optional,
                    f.default.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("PORT", false, true, false, Some("8899")),
                ("REDIS_HOST", false, false, false, None),
                ("REDIS_PASSWORD", true, false, true, None),
                ("DATABASE_URL", true, false, false, None),
                ("JWT_SECRET", true, false, false, None),
            ],
            "the example secret is never prefilled"
        );
        assert_eq!(fields[0].help, ["Port aplikasi"]);
    }

    #[test]
    fn keys_with_a_value_are_not_asked() {
        let template = envfile::parse("A=\nB=\n").unwrap();
        let mut doc = envfile::sync(&template, &Document::default()).doc;
        doc.get_mut("A").unwrap().value = Value::Encrypted("QQ==".into());
        let keys: Vec<String> = fields(&doc, &template).into_iter().map(|f| f.key).collect();
        assert_eq!(keys, ["B"]);
    }

    #[test]
    fn answers_are_encrypted_unless_plain_and_unanswered_keys_stay_empty() {
        let template = envfile::parse("# @plain\nPORT=\nREDIS_HOST=\nSSL_CERT=\nLATER=\n").unwrap();
        let mut doc = envfile::sync(&template, &Document::default()).doc;
        let recipient = crypto::generate().to_public();
        let mut files = Vec::new();
        let keys = ["PORT", "REDIS_HOST", "SSL_CERT", "LATER"]
            .map(String::from)
            .to_vec();
        let answers = vec![
            Some(Reply::Value(Secret::from("8899"), None)),
            Some(Reply::Value(Secret::from("10.0.0.5"), None)),
            Some(Reply::Value(
                Secret::from("A\nB"),
                Some(PathBuf::from("cert.pem")),
            )),
            None,
        ];

        let filled = apply(&mut doc, keys, answers, &recipient, &mut files).unwrap();

        assert_eq!(filled, ["PORT", "REDIS_HOST", "SSL_CERT"]);
        assert_eq!(files, [PathBuf::from("cert.pem")]);
        let value = |key| doc.get(key).unwrap().value.clone();
        assert_eq!(value("PORT"), Value::Plain(Secret::from("8899")));
        assert!(matches!(value("REDIS_HOST"), Value::Encrypted(_)));
        assert!(matches!(value("SSL_CERT"), Value::Encrypted(_)));
        assert_eq!(value("LATER"), Value::Empty);
    }
}
