//! One module per subcommand, plus what they share: reading and writing `.env`, finding
//! the key, and the exact command to suggest in `Fix:` lines.

pub mod check;
pub mod init;
pub mod install;
pub mod list;
pub mod run;
pub mod set;
pub mod setup;
pub mod uninstall;
pub mod update;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use age::x25519::{Identity, Recipient};
use anyhow::{Context, Result};

use crate::cli::Global;
use crate::crypto;
use crate::envfile::{self, Document, Header, ParseError, valid_app};
use crate::error::Fail;
use crate::fsutil::atomic::{self, Options, Owner as FileOwner};
use crate::input::prompt;
use crate::keyfile::{self, Facts, Located};

/// The global options every command works with.
#[derive(Debug, Clone)]
pub struct Ctx {
    pub env: PathBuf,
    pub template: PathBuf,
    pub key_file: Option<PathBuf>,
}

impl Ctx {
    pub fn new(global: &Global) -> Self {
        Self {
            env: global.env.clone(),
            template: global.template.clone(),
            key_file: global.key_file.clone(),
        }
    }

    /// `sultrakey <sub>` with the same `--env`/`--template`/`--key-file` as now, so a
    /// suggested fix works when copied as is.
    pub fn cmd(&self, sub: &str) -> String {
        let mut out = format!("sultrakey {sub}");
        if self.env != Path::new(".env") {
            out.push_str(&format!(" --env {}", shell_path(&self.env)));
        }
        if self.template != Path::new(".env.example") {
            out.push_str(&format!(" --template {}", shell_path(&self.template)));
        }
        if let Some(key) = &self.key_file {
            out.push_str(&format!(" --key-file {}", shell_path(key)));
        }
        out
    }

    /// How to run `init` here: with sudo and `--owner` on Linux/macOS.
    pub fn init_cmd(&self, app: &str) -> String {
        if cfg!(windows) {
            self.cmd(&format!("init {app}"))
        } else {
            format!(
                "sudo {} --owner <app-user>",
                self.cmd(&format!("init {app}"))
            )
        }
    }
}

fn shell_path(path: &Path) -> String {
    let text = path.display().to_string();
    if text.contains([' ', '\'', '"', '$', '`']) {
        format!("\"{}\"", text.replace('"', "\\\""))
    } else {
        text
    }
}

pub fn parse_fail(path: &Path, err: &ParseError) -> Fail {
    Fail::config(
        format!("{} {err}.", path.display()),
        format!(
            "open {} in a text editor and fix line {}",
            path.display(),
            err.line
        ),
    )
}

/// Reads and parses a file; `None` when it does not exist.
pub fn read_doc(path: &Path) -> Result<Option<Document>, Fail> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) if err.kind() == io::ErrorKind::PermissionDenied => {
            let solution = match crate::system::owner_name(path) {
                Some(owner) => format!(
                    "run as the user who owns the file ({owner}), for example: sudo -u {owner} sultrakey check"
                ),
                None => "run with sudo, or as the user who owns the file".into(),
            };
            return Err(Fail::config(
                format!("No permission to read {}.", path.display()),
                solution,
            ));
        }
        Err(err) => {
            return Err(Fail::config_bare(format!(
                "{} cannot be read: {err}.",
                path.display()
            )));
        }
    };
    envfile::parse(&text)
        .map(Some)
        .map_err(|err| parse_fail(path, &err))
}

/// The `.env`, which must exist.
pub fn load_env(ctx: &Ctx) -> Result<Document, Fail> {
    read_doc(&ctx.env)?.ok_or_else(|| {
        Fail::config(
            format!("File {} not found.", ctx.env.display()),
            format!(
                "run from the application folder or use --env <path>; for a new application: {}",
                ctx.init_cmd("<app-name>")
            ),
        )
    })
}

/// The template, which must exist and must not carry a header.
pub fn load_template(ctx: &Ctx) -> Result<Document, Fail> {
    let template = read_doc(&ctx.template)?.ok_or_else(|| {
        Fail::config(
            format!("Template {} not found.", ctx.template.display()),
            "run from the application folder, or point to it with --template <path>",
        )
    })?;
    if template.header.is_some() {
        return Err(Fail::config(
            format!(
                "Template {} contains a SULTRAKEY_* header; it belongs in .env only.",
                ctx.template.display()
            ),
            "remove the SULTRAKEY_APP and SULTRAKEY_PUBLIC_KEY lines from the template",
        ));
    }
    Ok(template)
}

/// The header of a managed `.env`, checked.
pub fn header(ctx: &Ctx, doc: &Document) -> Result<Header, Fail> {
    let Some(header) = doc.header.clone() else {
        return Err(Fail::config(
            format!(
                "{} is not managed by sultrakey yet (no SULTRAKEY_APP line).",
                ctx.env.display()
            ),
            ctx.init_cmd("<app-name>"),
        ));
    };
    if !valid_app(&header.app) {
        return Err(Fail::config(
            format!(
                "SULTRAKEY_APP in {} is not valid ('{}').",
                ctx.env.display(),
                header.app
            ),
            "restore .env from a backup; an app name has only lowercase letters, digits, and dashes",
        ));
    }
    Ok(header)
}

pub fn recipient(ctx: &Ctx, header: &Header) -> Result<Recipient, Fail> {
    crypto::parse_recipient(&header.public_key).map_err(|why| {
        Fail::config(
            format!(
                "SULTRAKEY_PUBLIC_KEY in {} is broken: {why}.",
                ctx.env.display()
            ),
            "restore .env from a backup",
        )
    })
}

pub fn key_location(ctx: &Ctx, app: &str) -> Result<Located, Fail> {
    keyfile::locate(&Facts::gather(ctx.key_file.as_deref()), app, Path::exists)
}

/// The fix for a missing key when the `.env` already depends on one.
pub fn missing_key_hint(ctx: &Ctx, app: &str, path: &Path) -> String {
    format!(
        "restore the key file from a backup to {}; without a backup: delete {}, then run {} \
         and sultrakey setup (every value is entered again)",
        path.display(),
        ctx.env.display(),
        ctx.init_cmd(app)
    )
}

/// The private key for a managed `.env`, which must match its public key.
pub fn load_key(ctx: &Ctx, header: &Header) -> Result<(Identity, PathBuf), Fail> {
    let located = key_location(ctx, &header.app)?;
    let identity = keyfile::read(&located.path).map_err(|err| {
        keyfile::read_fail(
            err,
            &located.path,
            &missing_key_hint(ctx, &header.app, &located.path),
        )
    })?;
    if crypto::public_key(&identity) != header.public_key {
        return Err(mismatch(ctx, &located.path));
    }
    Ok((identity, located.path))
}

pub fn mismatch(ctx: &Ctx, key: &Path) -> Fail {
    Fail::config(
        format!(
            "Key file {} does not belong to {} (different public key).",
            key.display(),
            ctx.env.display()
        ),
        "use the right key file (--key-file <path>), or restore this application's key file from a backup",
    )
}

/// Writes the `.env` atomically: mode 0600, owner as given.
pub fn write_env(ctx: &Ctx, doc: &Document, owner: FileOwner) -> Result<()> {
    let text = zeroize::Zeroizing::new(envfile::render(doc));
    atomic::write(&ctx.env, text.as_bytes(), Options { mode: 0o600, owner }).map_err(|err| {
        let fail = match err.kind() {
            io::ErrorKind::PermissionDenied => Fail::config(
                format!(
                    "No permission to write {} or its folder.",
                    ctx.env.display()
                ),
                if cfg!(windows) {
                    "close other programs that have the file open, then try again".to_string()
                } else {
                    format!("sudo {}", ctx.cmd("setup"))
                },
            ),
            _ => Fail::other(format!("{} cannot be written: {err}.", ctx.env.display())),
        };
        anyhow::Error::new(fail)
    })
}

/// Prompts need a terminal; `instead` says what to use without one.
pub fn require_terminal(instead: &str) -> Result<()> {
    if prompt::interactive() {
        return Ok(());
    }
    let hint = prompt::not_interactive_hint()
        .map(|hint| format!(" ({hint})"))
        .unwrap_or_default();
    Err(Fail::usage(
        format!("Cannot ask questions because this is not a terminal{hint}."),
        instead,
    )
    .into())
}

/// Reads all of stdin (for `--stdin` and non-terminal `setup`).
pub fn read_stdin() -> Result<zeroize::Zeroizing<String>> {
    let mut text = zeroize::Zeroizing::new(String::new());
    io::Read::read_to_string(&mut io::stdin(), &mut text).context("cannot read stdin")?;
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(env: &str, template: &str, key: Option<&str>) -> Ctx {
        Ctx {
            env: env.into(),
            template: template.into(),
            key_file: key.map(PathBuf::from),
        }
    }

    #[test]
    fn suggested_commands_keep_the_options() {
        assert_eq!(
            ctx(".env", ".env.example", None).cmd("setup"),
            "sultrakey setup"
        );
        assert_eq!(
            ctx("/opt/a b/.env", "t.env", Some("/k.key")).cmd("setup"),
            "sultrakey setup --env \"/opt/a b/.env\" --template t.env --key-file /k.key"
        );
        let init = ctx(".env", ".env.example", None).init_cmd("demo");
        if cfg!(windows) {
            assert_eq!(init, "sultrakey init demo");
        } else {
            assert_eq!(init, "sudo sultrakey init demo --owner <app-user>");
        }
    }
}
