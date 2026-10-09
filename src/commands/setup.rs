//! `setup`: brings the `.env` in line with the template, encrypts plain values of secret
//! keys (D13), then asks for every empty key (D12). Without a terminal it reads
//! `KEY=value` lines from stdin instead. The file is written once, at the end: stopping
//! halfway changes nothing.

use std::path::PathBuf;

use age::x25519::Recipient;
use anyhow::Result;

use super::{Ctx, header, load_env, load_template, read_stdin, recipient, write_env};
use crate::crypto;
use crate::envfile::{self, Document, Entry, Value};
use crate::error::{Abort, Fail};
use crate::fsutil::atomic::Owner as FileOwner;
use crate::fsutil::lock::{DEFAULT_TIMEOUT, FileLock};
use crate::input::prompt::{self, Prompter, Question, Reply, Terminal};
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
    let interactive = prompt::interactive();
    let filled = if interactive {
        ask_empty(&mut Terminal, &mut doc, &template, &recipient, &mut files)?
    } else {
        from_stdin(ctx, &mut doc, &template, &recipient, &mut files)?
    };

    write_env(ctx, &doc, FileOwner::Keep)?;
    // In a terminal the keys were just reviewed on screen; only stdin needs the list.
    if filled.is_empty() || interactive {
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
        output::info(&format!("Solusi: {}", ctx.cmd("setup")));
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

fn ask_empty(
    prompter: &mut dyn Prompter,
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
    let total = empty.len();
    let defaults = empty.iter().any(|key| {
        doc.get(key)
            .is_some_and(|entry| default_for(template, entry).is_some())
    });
    for line in output::banner("sultrakey setup", concat!("v", env!("CARGO_PKG_VERSION"))) {
        prompter.say(&line);
    }
    prompter.keys("  ", &prompt::general_keys(defaults));

    let mut replies = Vec::with_capacity(total);
    for (i, key) in empty.iter().enumerate() {
        replies.push(ask_one(prompter, doc, template, key, (i + 1, total))?);
    }
    // Nothing is kept until the answers are confirmed: a typo is fixed here, not found
    // later when the application fails to connect.
    loop {
        review(prompter, doc, &empty, &replies);
        if prompt::confirm(prompter, "Simpan?", true)? {
            break;
        }
        let Some(i) = pick(prompter, &empty)? else {
            return Err(Abort::Cancelled.into());
        };
        replies[i] = ask_one(prompter, doc, template, &empty[i], (i + 1, total))?;
    }
    prompter.say("");

    let mut filled = Vec::new();
    for (key, reply) in empty.into_iter().zip(replies) {
        let Some(entry) = doc.get_mut(&key) else {
            continue;
        };
        entry.value = match reply {
            Reply::Value(value, file) => {
                files.extend(file);
                store(entry.flags.plain, value, recipient)?
            }
            Reply::Empty => Value::Empty,
        };
        if entry.value != Value::Empty {
            filled.push(key);
        }
    }
    Ok(filled)
}

fn ask_one(
    prompter: &mut dyn Prompter,
    doc: &Document,
    template: &Document,
    key: &str,
    step: (usize, usize),
) -> Result<Reply> {
    let Some(entry) = doc.get(key) else {
        return Ok(Reply::Empty);
    };
    prompt::ask(
        prompter,
        &Question {
            key,
            help: entry.help(),
            masked: entry.masked(),
            optional: entry.flags.optional,
            default: default_for(template, entry),
            step: Some(step),
        },
    )
}

/// The answers as a numbered list. Secret values are never shown, not even their length.
/// Shown in a terminal only, where the visible values were just typed on screen anyway.
fn review(prompter: &mut dyn Prompter, doc: &Document, keys: &[String], replies: &[Reply]) {
    let width = keys
        .iter()
        .map(|key| key.chars().count())
        .max()
        .unwrap_or(0);
    let digits = keys.len().to_string().len();
    prompter.say("");
    prompter.say("Periksa sebelum disimpan:");
    for (i, (key, reply)) in keys.iter().zip(replies).enumerate() {
        let masked = doc.get(key).is_some_and(Entry::masked);
        let shown = match reply {
            Reply::Empty => "(kosong)".to_string(),
            Reply::Value(_, Some(path)) => format!("(isi file {})", path.display()),
            // Always eight: the real length stays hidden.
            Reply::Value(_, None) if masked => "********".to_string(),
            Reply::Value(value, None) => value.expose().to_string(),
        };
        prompter.say(&format!("  {:>digits$}. {key:<width$}  {shown}", i + 1));
    }
}

/// Which answer to redo, by number or key name; `None` (empty line) stops without saving.
fn pick(prompter: &mut dyn Prompter, keys: &[String]) -> Result<Option<usize>> {
    let total = keys.len();
    loop {
        let reply = prompter.visible(
            &format!("Ubah nomor berapa? (1-{total}, Enter = batal tanpa menyimpan) "),
            "",
        )?;
        let reply = reply.trim();
        if reply.is_empty() {
            return Ok(None);
        }
        let found = match reply.parse::<usize>() {
            Ok(n) => (1..=total).contains(&n).then(|| n - 1),
            Err(_) => keys.iter().position(|key| key.eq_ignore_ascii_case(reply)),
        };
        match found {
            Some(i) => return Ok(Some(i)),
            None => prompter.alert(&format!("Ketik nomor 1 sampai {total}, atau nama key-nya.")),
        }
    }
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
                format!("{key} tidak ada di template {}.", ctx.template.display()),
                format!(
                    "tambahkan {key} ke template, lalu jalankan {} lagi",
                    ctx.cmd("setup")
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
    use crate::input::prompt::tests::Script;

    #[test]
    fn secret_looking_keys_are_masked_and_everything_else_is_visible() {
        let template = envfile::parse(
            "# @plain\nPORT=8899\nREDIS_HOST=\nREDIS_PASSWORD=\n# @masked\nDATABASE_URL=\n\
             JWT_SECRET=changeme\n",
        )
        .unwrap();
        let mut doc = envfile::sync(&template, &Document::default()).doc;
        let recipient = crypto::generate().to_public();
        let mut script = Script::new(&[
            "8899",
            "10.0.0.5",
            "pw",
            "pw",
            "pg://u:p@h",
            "pg://u:p@h",
            "jwt",
            "jwt",
            "",
        ]);
        let mut files = Vec::new();

        let filled = ask_empty(&mut script, &mut doc, &template, &recipient, &mut files).unwrap();

        assert_eq!(
            filled,
            [
                "PORT",
                "REDIS_HOST",
                "REDIS_PASSWORD",
                "DATABASE_URL",
                "JWT_SECRET"
            ]
        );
        let asked: Vec<&str> = script
            .log
            .iter()
            .filter(|l| {
                l.starts_with("visible:") || l.starts_with("masked:") || l.starts_with("initial:")
            })
            .map(String::as_str)
            .collect();
        assert_eq!(
            asked,
            [
                "visible:      › ",
                "initial:8899",
                "visible:      › ",
                "masked:      ›        ",
                "masked:      ulangi › ",
                "masked:      ›        ",
                "masked:      ulangi › ",
                "masked:      ›        ",
                "masked:      ulangi › ",
                "visible:Simpan? [Y/n] ",
            ],
            "the example secret is never prefilled"
        );
        assert!(script.log.iter().any(|l| l == "[1/5] PORT"));
        assert!(script.log.iter().any(|l| l == "[5/5] JWT_SECRET"));
        assert!(script.log.iter().all(|l| !l.contains("changeme")));
        let value = |key| doc.get(key).unwrap().value.clone();
        assert_eq!(value("PORT"), Value::Plain(Secret::from("8899")));
        for key in ["REDIS_HOST", "REDIS_PASSWORD", "DATABASE_URL", "JWT_SECRET"] {
            assert!(
                matches!(value(key), Value::Encrypted(_)),
                "{key}: visible while typed, still encrypted"
            );
        }
    }

    fn db_template() -> Document {
        envfile::parse("DB_HOST=localhost\nDB_USER=\nDB_PASSWORD=\n").unwrap()
    }

    #[test]
    fn the_review_shows_typed_values_but_never_secrets() {
        let template = db_template();
        let mut doc = envfile::sync(&template, &Document::default()).doc;
        let recipient = crypto::generate().to_public();
        let mut script = Script::new(&["locahost", "postgres", "rahasia1", "rahasia1", ""]);

        ask_empty(
            &mut script,
            &mut doc,
            &template,
            &recipient,
            &mut Vec::new(),
        )
        .unwrap();

        let review: Vec<&str> = script
            .log
            .iter()
            .skip_while(|l| *l != "Periksa sebelum disimpan:")
            .skip(1)
            .take(3)
            .map(String::as_str)
            .collect();
        assert_eq!(
            review,
            [
                "  1. DB_HOST      locahost",
                "  2. DB_USER      postgres",
                "  3. DB_PASSWORD  ********",
            ]
        );
        assert!(script.log.iter().all(|l| !l.contains("rahasia1")));
    }

    #[test]
    fn a_reviewed_answer_can_be_redone_by_number_or_name() {
        let template = db_template();
        let mut doc = envfile::sync(&template, &Document::default()).doc;
        let recipient = crypto::generate().to_public();
        let first_round = ["locahost", "postgres", "pw", "pw"];
        let fix_host_by_number_after_a_bad_pick = ["n", "9", "1", "localhost"];
        let fix_user_by_name = ["t", "db_user", "admin"];
        let save = [""];
        let mut script = Script::new(
            &[
                &first_round[..],
                &fix_host_by_number_after_a_bad_pick,
                &fix_user_by_name,
                &save,
            ]
            .concat(),
        );

        let filled = ask_empty(
            &mut script,
            &mut doc,
            &template,
            &recipient,
            &mut Vec::new(),
        )
        .unwrap();

        assert_eq!(filled, ["DB_HOST", "DB_USER", "DB_PASSWORD"]);
        assert!(
            script
                .log
                .iter()
                .any(|l| l.contains("Ketik nomor 1 sampai 3"))
        );
        let review = script
            .log
            .iter()
            .rposition(|l| l == "Periksa sebelum disimpan:")
            .unwrap();
        assert_eq!(script.log[review + 1], "  1. DB_HOST      localhost");
        assert_eq!(script.log[review + 2], "  2. DB_USER      admin");
        for key in ["DB_HOST", "DB_USER", "DB_PASSWORD"] {
            assert!(matches!(doc.get(key).unwrap().value, Value::Encrypted(_)));
        }
    }

    #[test]
    fn declining_the_review_without_a_pick_changes_nothing() {
        let template = db_template();
        let mut doc = envfile::sync(&template, &Document::default()).doc;
        let recipient = crypto::generate().to_public();
        let mut script = Script::new(&["localhost", "postgres", "pw", "pw", "n", ""]);
        let mut files = Vec::new();

        let err = ask_empty(&mut script, &mut doc, &template, &recipient, &mut files).unwrap_err();

        assert_eq!(err.downcast_ref::<Abort>(), Some(&Abort::Cancelled));
        assert!(doc.entries().all(|entry| entry.value == Value::Empty));
    }
}
