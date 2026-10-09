//! One module per subcommand, plus what they share: reading and writing `.env`, finding
//! the key, and the exact command to suggest in `Solusi:` lines.

pub mod check;
pub mod fill;
pub mod init;
pub mod install;
pub mod list;
pub mod run;
pub mod set;
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
        if self.template != Path::new(".env.template") {
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
                "sudo {} --owner <user-aplikasi>",
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
            "buka {} dengan editor teks dan perbaiki baris {}",
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
                    "jalankan sebagai user pemilik file ({owner}), contoh: sudo -u {owner} sultrakey check"
                ),
                None => "jalankan dengan sudo, atau sebagai user pemilik file".into(),
            };
            return Err(Fail::config(
                format!("Tidak punya izin membaca {}.", path.display()),
                solution,
            ));
        }
        Err(err) => {
            return Err(Fail::config_bare(format!(
                "{} tidak bisa dibaca: {err}.",
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
            format!("File {} tidak ditemukan.", ctx.env.display()),
            format!(
                "jalankan dari folder aplikasi atau pakai --env <lokasi>; untuk aplikasi baru: {}",
                ctx.init_cmd("<nama-app>")
            ),
        )
    })
}

/// The template, which must exist and must not carry a header.
pub fn load_template(ctx: &Ctx) -> Result<Document, Fail> {
    let template = read_doc(&ctx.template)?.ok_or_else(|| {
        Fail::config(
            format!("Template {} tidak ditemukan.", ctx.template.display()),
            "jalankan dari folder aplikasi, atau tunjuk lokasinya dengan --template <lokasi>",
        )
    })?;
    if template.header.is_some() {
        return Err(Fail::config(
            format!(
                "Template {} berisi header SULTRAKEY_*; itu hanya boleh ada di .env.",
                ctx.template.display()
            ),
            "hapus baris SULTRAKEY_APP dan SULTRAKEY_PUBLIC_KEY dari template",
        ));
    }
    Ok(template)
}

/// The header of a managed `.env`, checked.
pub fn header(ctx: &Ctx, doc: &Document) -> Result<Header, Fail> {
    let Some(header) = doc.header.clone() else {
        return Err(Fail::config(
            format!(
                "{} belum dikelola sultrakey (tidak ada baris SULTRAKEY_APP).",
                ctx.env.display()
            ),
            ctx.init_cmd("<nama-app>"),
        ));
    };
    if !valid_app(&header.app) {
        return Err(Fail::config(
            format!(
                "SULTRAKEY_APP di {} tidak valid ('{}').",
                ctx.env.display(),
                header.app
            ),
            "pulihkan .env dari backup; nama app hanya huruf kecil, angka, dan tanda minus",
        ));
    }
    Ok(header)
}

pub fn recipient(ctx: &Ctx, header: &Header) -> Result<Recipient, Fail> {
    crypto::parse_recipient(&header.public_key).map_err(|why| {
        Fail::config(
            format!(
                "SULTRAKEY_PUBLIC_KEY di {} rusak: {why}.",
                ctx.env.display()
            ),
            "pulihkan .env dari backup",
        )
    })
}

pub fn key_location(ctx: &Ctx, app: &str) -> Result<Located, Fail> {
    keyfile::locate(&Facts::gather(ctx.key_file.as_deref()), app, Path::exists)
}

/// The fix for a missing key when the `.env` already depends on one.
pub fn missing_key_hint(ctx: &Ctx, app: &str, path: &Path) -> String {
    format!(
        "pulihkan file kunci dari backup ke {}; bila tidak ada backup: hapus {}, lalu jalankan {} \
         dan sultrakey fill (semua value diisi ulang)",
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
            "File kunci {} bukan pasangan {} (public key berbeda).",
            key.display(),
            ctx.env.display()
        ),
        "pakai file kunci yang benar (--key-file <lokasi>), atau pulihkan file kunci aplikasi ini dari backup",
    )
}

/// Writes the `.env` atomically: mode 0600, owner as given.
pub fn write_env(ctx: &Ctx, doc: &Document, owner: FileOwner) -> Result<()> {
    let text = zeroize::Zeroizing::new(envfile::render(doc));
    atomic::write(&ctx.env, text.as_bytes(), Options { mode: 0o600, owner }).map_err(|err| {
        let fail = match err.kind() {
            io::ErrorKind::PermissionDenied => Fail::config(
                format!(
                    "Tidak punya izin menulis {} atau folder tempatnya.",
                    ctx.env.display()
                ),
                if cfg!(windows) {
                    "tutup program lain yang membuka file itu, lalu ulangi".to_string()
                } else {
                    format!("sudo {}", ctx.cmd("fill"))
                },
            ),
            _ => Fail::other(format!("{} tidak bisa ditulis: {err}.", ctx.env.display())),
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
        format!("Tidak bisa bertanya karena ini bukan terminal{hint}."),
        instead,
    )
    .into())
}

/// Reads all of stdin (for `--stdin` and non-terminal `fill`).
pub fn read_stdin() -> Result<zeroize::Zeroizing<String>> {
    let mut text = zeroize::Zeroizing::new(String::new());
    io::Read::read_to_string(&mut io::stdin(), &mut text).context("stdin tidak bisa dibaca")?;
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
            ctx(".env", ".env.template", None).cmd("fill"),
            "sultrakey fill"
        );
        assert_eq!(
            ctx("/opt/a b/.env", "t.env", Some("/k.key")).cmd("fill"),
            "sultrakey fill --env \"/opt/a b/.env\" --template t.env --key-file /k.key"
        );
        let init = ctx(".env", ".env.template", None).init_cmd("demo");
        if cfg!(windows) {
            assert_eq!(init, "sultrakey init demo");
        } else {
            assert_eq!(init, "sudo sultrakey init demo --owner <user-aplikasi>");
        }
    }
}
