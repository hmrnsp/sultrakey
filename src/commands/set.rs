//! `set <KEY> [--stdin | --file <lokasi>]`: replaces one value. Never from a command-line
//! argument, which would show up in `ps` and shell history.

use std::path::Path;

use anyhow::Result;

use super::{Ctx, header, load_env, read_stdin, recipient, require_terminal, write_env};
use crate::crypto;
use crate::envfile::Value;
use crate::error::Fail;
use crate::fsutil::atomic::Owner as FileOwner;
use crate::fsutil::lock::{DEFAULT_TIMEOUT, FileLock};
use crate::input::prompt::{self, Question, Reply, Terminal};
use crate::input::source;
use crate::output;
use crate::secret::Secret;

pub fn run(ctx: &Ctx, key: &str, stdin: bool, file: Option<&Path>) -> Result<i32> {
    let _lock = FileLock::acquire(&ctx.env, DEFAULT_TIMEOUT)?;
    let mut doc = load_env(ctx)?;
    let header = header(ctx, &doc)?;
    let recipient = recipient(ctx, &header)?;
    let Some(entry) = doc.get(key) else {
        return Err(Fail::usage(
            format!("{key} tidak ada di {}.", ctx.env.display()),
            format!(
                "tambahkan {key} ke {}, lalu jalankan {}",
                ctx.template.display(),
                ctx.cmd("setup")
            ),
        )
        .into());
    };
    let flags = entry.flags;

    let mut used_file = file.map(Path::to_path_buf);
    let value: Option<Secret> = if stdin {
        let text = read_stdin()?;
        let value = Secret::from(source::strip_one_newline(&text));
        source::check_text(&value, key)?;
        Some(value)
    } else if let Some(file) = file {
        Some(source::read_file(file)?)
    } else {
        require_terminal(&format!(
            "{} --stdin, atau {} --file <lokasi>",
            ctx.cmd(&format!("set {key}")),
            ctx.cmd(&format!("set {key}"))
        ))?;
        let question = Question {
            key,
            help: entry.help(),
            masked: entry.masked(),
            optional: flags.optional,
            default: None,
            step: None,
        };
        match prompt::ask(&mut Terminal, &question)? {
            Reply::Value(value, from) => {
                used_file = from;
                Some(value)
            }
            Reply::Empty => None,
        }
    };

    let new_value = match value {
        Some(value) if !value.is_empty() => {
            if flags.plain {
                Value::Plain(value)
            } else {
                Value::Encrypted(crypto::encrypt(&recipient, &value)?)
            }
        }
        _ if flags.optional => Value::Empty,
        _ => {
            return Err(Fail::usage(
                format!("{key} wajib diisi; value kosong tidak disimpan."),
                "isi dengan value yang benar",
            )
            .into());
        }
    };
    if let Some(entry) = doc.get_mut(key) {
        entry.value = new_value;
    }
    write_env(ctx, &doc, FileOwner::Keep)?;
    output::ok(&format!("{key} diperbarui."));
    if let Some(file) = used_file {
        output::warn(&format!("Hapus file {} sekarang.", file.display()));
    }
    Ok(0)
}
