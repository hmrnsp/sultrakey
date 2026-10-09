//! `fill`: brings the `.env` in line with the template, encrypts plain values of secret
//! keys (D13), then asks for every empty key (D12). Without a terminal it reads
//! `KEY=value` lines from stdin instead. The file is written once, at the end: stopping
//! halfway changes nothing.

use std::path::PathBuf;

use age::x25519::Recipient;
use anyhow::Result;

use super::{Ctx, header, load_env, load_template, read_stdin, recipient, write_env};
use crate::crypto;
use crate::envfile::{self, Document, Value};
use crate::error::Fail;
use crate::fsutil::atomic::Owner as FileOwner;
use crate::fsutil::lock::{DEFAULT_TIMEOUT, FileLock};
use crate::input::prompt::{self, Question, Reply, Terminal};
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
            "Key baru dari template: {}",
            synced.added.join(", ")
        ));
    }
    for key in &synced.extra {
        output::warn(&format!(
            "{key} ada di .env tetapi tidak ada di template; tetap disimpan di akhir file."
        ));
    }

    let protected = encrypt_plain_secrets(&mut doc, &recipient)?;
    if !protected.is_empty() {
        output::ok(&format!("Value polos dienkripsi: {}", protected.join(", ")));
    }

    let mut files = Vec::new();
    let filled = if prompt::interactive() {
        ask_empty(&mut doc, &template, &recipient, &mut files)?
    } else {
        from_stdin(ctx, &mut doc, &template, &recipient, &mut files)?
    };

    write_env(ctx, &doc, FileOwner::Keep)?;
    if filled.is_empty() {
        output::ok(&format!("{} disimpan.", ctx.env.display()));
    } else {
        output::ok(&format!(
            "{} disimpan. Terisi: {}",
            ctx.env.display(),
            filled.join(", ")
        ));
    }
    for file in &files {
        output::warn(&format!("Hapus file {} sekarang.", file.display()));
    }
    let missing: Vec<&str> = doc
        .entries()
        .filter(|entry| entry.value == Value::Empty && !entry.flags.optional)
        .map(|entry| entry.key.as_str())
        .collect();
    if missing.is_empty() {
        output::info(&format!("Langkah berikut: {}", ctx.cmd("check")));
    } else {
        output::warn(&format!("Masih kosong: {}", missing.join(", ")));
        output::info(&format!("Solusi: {}", ctx.cmd("fill")));
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

/// The template's value for `key`, offered as the default.
fn default_of<'a>(template: &'a Document, key: &str) -> Option<&'a str> {
    match &template.get(key)?.value {
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

fn ask_empty(
    doc: &mut Document,
    template: &Document,
    recipient: &Recipient,
    files: &mut Vec<PathBuf>,
) -> Result<Vec<String>> {
    let empty: Vec<String> = doc
        .entries()
        .filter(|entry| entry.value == Value::Empty)
        .map(|entry| entry.key.clone())
        .collect();
    if empty.is_empty() {
        output::info("Semua key sudah terisi.");
        return Ok(Vec::new());
    }
    output::info(&format!(
        "{} key perlu diisi. Tekan Ctrl+C untuk berhenti tanpa menyimpan apa pun.",
        empty.len()
    ));
    let mut filled = Vec::new();
    for key in empty {
        let Some(entry) = doc.get(&key) else { continue };
        let question = Question {
            key: &key,
            help: entry.help(),
            plain: entry.flags.plain,
            optional: entry.flags.optional,
            default: default_of(template, &key),
        };
        let plain = entry.flags.plain;
        let value = match prompt::ask(&mut Terminal, &question)? {
            Reply::Value(value, file) => {
                files.extend(file);
                store(plain, value, recipient)?
            }
            Reply::Empty => Value::Empty,
        };
        if value != Value::Empty {
            filled.push(key.clone());
        }
        if let Some(entry) = doc.get_mut(&key) {
            entry.value = value;
        }
    }
    Ok(filled)
}

/// `KEY=value` lines. Keys that already have a value are not changed (that is what `set`
/// is for); empty keys not sent take the template's default.
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
                format!("{key} tidak ada di template {}.", ctx.template.display()),
                format!(
                    "tambahkan {key} ke template, lalu jalankan {} lagi",
                    ctx.cmd("fill")
                ),
            )
            .into());
        };
        if entry.value != Value::Empty {
            output::warn(&format!(
                "{key} sudah terisi; tidak diubah (untuk mengganti: {}).",
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
            && let Some(default) = default_of(template, &entry.key)
        {
            entry.value = store(entry.flags.plain, Secret::from(default), recipient)?;
            filled.push(entry.key.clone());
        }
    }
    Ok(filled)
}
