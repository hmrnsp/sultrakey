//! `check`: everything `run` needs, checked at once; every problem is reported, not just
//! the first. `run` uses [`inspect`] before starting the application.

use std::path::PathBuf;

use anyhow::Result;

use super::{Ctx, header, key_location, load_env, mismatch, missing_key_hint};
use crate::crypto::{self, DecryptFailure};
use crate::envfile::Value;
use crate::error::{Code, Fail, Issue};
use crate::keyfile;
use crate::output;
use crate::secret::Secret;
use crate::system;

/// The decrypted values, in file order. Empty optional keys are `""` (D3).
pub struct Opened {
    pub values: Vec<(String, Secret)>,
    pub key_path: PathBuf,
}

pub fn inspect(ctx: &Ctx) -> Result<Opened, Fail> {
    let doc = load_env(ctx)?;
    let header = header(ctx, &doc)?;
    let mut issues: Vec<Issue> = Vec::new();

    let located = key_location(ctx, &header.app)?;
    let key_path = located.path;
    if let Ok(Some(mode)) = system::open_to_others(&key_path) {
        issues.push(Issue::new(
            format!(
                "Key file {} is too open ({mode:o}); other users on this server can read it.",
                key_path.display()
            ),
            format!("sudo chmod 400 {}", key_path.display()),
        ));
    }
    let identity = match keyfile::read(&key_path) {
        Ok(identity) if crypto::public_key(&identity) == header.public_key => Some(identity),
        Ok(_) => {
            issues.extend(mismatch(ctx, &key_path).issues);
            None
        }
        Err(err) => {
            let hint = missing_key_hint(ctx, &header.app, &key_path);
            issues.extend(keyfile::read_fail(err, &key_path, &hint).issues);
            None
        }
    };

    let mut values = Vec::new();
    for entry in doc.entries() {
        let key = &entry.key;
        match &entry.value {
            Value::Empty if entry.flags.optional => values.push((key.clone(), Secret::default())),
            Value::Empty => issues.push(Issue::new(format!("{key} is empty."), ctx.cmd("setup"))),
            Value::Plain(value) if entry.flags.plain => values.push((key.clone(), value.clone())),
            Value::Plain(_) => issues.push(Issue::new(
                format!("{key} is stored plain (not encrypted), but it is not @plain."),
                format!("{} (it encrypts that value)", ctx.cmd("setup")),
            )),
            Value::Encrypted(b64) => {
                let Some(identity) = &identity else { continue };
                match crypto::decrypt(identity, b64) {
                    Ok(value) => values.push((key.clone(), value)),
                    Err(failure) => {
                        let why = match failure {
                            DecryptFailure::NotBase64 => "not base64",
                            DecryptFailure::CannotOpen => "broken, or encrypted with another key",
                            DecryptFailure::NotText => "not text",
                        };
                        issues.push(Issue::new(
                            format!("{key} cannot be decrypted ({why})."),
                            ctx.cmd(&format!("set {key}")),
                        ));
                    }
                }
            }
        }
    }

    if issues.is_empty() {
        Ok(Opened { values, key_path })
    } else {
        Err(Fail {
            code: Code::Config,
            issues,
        })
    }
}

pub fn run(ctx: &Ctx) -> Result<i32> {
    let opened = inspect(ctx)?;
    if cfg!(windows) {
        output::info("Key file permissions are not checked on Windows.");
    }
    if system::writable_by_others(&ctx.env).unwrap_or(false) {
        output::warn(&format!(
            "{} can be changed by other users (anyone who can write .env can replace values).",
            ctx.env.display()
        ));
        output::info(&format!("Fix: sudo chmod 600 {}", ctx.env.display()));
    }
    if let Ok(Some(template)) = super::read_doc(&ctx.template)
        && let Ok(doc) = load_env(ctx)
    {
        let new: Vec<&str> = template
            .entries()
            .filter(|entry| doc.get(&entry.key).is_none())
            .map(|entry| entry.key.as_str())
            .collect();
        if !new.is_empty() {
            output::warn(&format!(
                "The template has keys that are not in .env yet: {}",
                new.join(", ")
            ));
            output::info(&format!("Fix: {}", ctx.cmd("setup")));
        }
    }
    output::ok(&format!(
        "All good ({} keys, key file: {}).",
        opened.values.len(),
        opened.key_path.display()
    ));
    Ok(0)
}
