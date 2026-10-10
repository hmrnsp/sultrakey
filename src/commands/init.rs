//! `init <app> [--owner user[:group]]`: creates the key (never replaces one), then creates
//! the `.env` from the template, or takes over an old plain `.env` by encrypting its
//! values (D2).

use anyhow::Result;

use super::{Ctx, header as checked_header, load_template, read_doc, write_env};
use crate::crypto;
use crate::envfile::{self, Document, Header, Value, valid_app};
use crate::error::Fail;
use crate::fsutil::atomic::Owner as FileOwner;
use crate::fsutil::lock::{DEFAULT_TIMEOUT, FileLock};
use crate::keyfile::{self, ReadError};
use crate::output;
use crate::system::{self, Owner};

/// What `--owner` means here.
#[derive(Debug, PartialEq, Eq)]
pub enum OwnerPlan {
    /// Windows: no owners; `warn` when `--owner` was given anyway.
    Ignore { warn: bool },
    /// Not root (Unix): files belong to the current user.
    NotRoot,
    /// Root with `--owner`: files are given to that user.
    Root(String),
}

/// D4 and D10 as a pure decision: root needs `--owner`, `--owner` needs root.
pub fn plan_owner(
    windows: bool,
    root: bool,
    owner: Option<&str>,
    app: &str,
) -> Result<OwnerPlan, Fail> {
    if windows {
        return Ok(OwnerPlan::Ignore {
            warn: owner.is_some(),
        });
    }
    match (root, owner) {
        (true, Some(spec)) => Ok(OwnerPlan::Root(spec.to_string())),
        (true, None) => Err(Fail::usage(
            "With sudo, --owner is required: the user that runs the application.",
            format!("sudo sultrakey init {app} --owner <app-user>"),
        )),
        (false, Some(spec)) => Err(Fail::usage(
            "--owner works only with sudo.",
            format!("sudo sultrakey init {app} --owner {spec}"),
        )),
        (false, None) => Ok(OwnerPlan::NotRoot),
    }
}

pub fn run(ctx: &Ctx, app: &str, owner: Option<&str>) -> Result<i32> {
    if !valid_app(app) {
        return Err(Fail::usage(
            format!("App name '{app}' is not valid."),
            "use lowercase letters, digits, and dashes, for example: sultrakey init lakupandai",
        )
        .into());
    }
    let plan = plan_owner(cfg!(windows), system::is_root(), owner, app)?;
    if plan == (OwnerPlan::Ignore { warn: true }) {
        output::warn("--owner is not used on Windows; ignored.");
    }
    let owner: Option<Owner> = match &plan {
        OwnerPlan::Root(spec) => Some(system::resolve_owner(spec)?),
        _ => None,
    };

    let _lock = FileLock::acquire(&ctx.env, DEFAULT_TIMEOUT)?;
    let existing = read_doc(&ctx.env)?;
    if let Some(doc) = &existing
        && doc.header.is_some()
    {
        let header = checked_header(ctx, doc)?;
        if header.app != app {
            return Err(Fail::config(
                format!(
                    "{} belongs to app '{}', not '{app}'.",
                    ctx.env.display(),
                    header.app
                ),
                ctx.init_cmd(&header.app),
            )
            .into());
        }
    }

    let located = super::key_location(ctx, app)?;
    let identity = match keyfile::read(&located.path) {
        Ok(identity) => {
            output::ok(&format!(
                "Using the existing key: {}",
                located.path.display()
            ));
            identity
        }
        Err(ReadError::Missing) => {
            if existing.as_ref().is_some_and(|doc| doc.header.is_some()) {
                return Err(Fail::config(
                    format!(
                        "Key file {} is missing, but {} is already encrypted with a key. \
                         No new key is created, because the existing values would become unreadable.",
                        located.path.display(),
                        ctx.env.display()
                    ),
                    super::missing_key_hint(ctx, app, &located.path),
                )
                .into());
            }
            if plan == OwnerPlan::NotRoot && !located.custom {
                let me = system::current_user_name().unwrap_or_else(|| "<your-user>".into());
                return Err(Fail::usage(
                    format!(
                        "Creating a key in {} needs root.",
                        located.path.parent().unwrap_or(&located.path).display()
                    ),
                    format!("sudo sultrakey init {app} --owner {me}"),
                )
                .into());
            }
            let identity = crypto::generate();
            keyfile::create(&located.path, &identity, owner.as_ref())?;
            let whose = owner
                .as_ref()
                .map(|owner| format!(", owner {}", owner.label))
                .unwrap_or_default();
            output::ok(&format!(
                "New key created: {} (mode 400{whose})",
                located.path.display()
            ));
            output::warn(
                "Keep a copy of this key file somewhere safe (offline). \
                 A lost key means every value must be entered again.",
            );
            identity
        }
        Err(err) => {
            return Err(keyfile::read_fail(err, &located.path, "").into());
        }
    };
    let public_key = crypto::public_key(&identity);
    let header = Header {
        app: app.to_string(),
        public_key: public_key.clone(),
    };
    let file_owner = |keep: bool| match &owner {
        Some(owner) => FileOwner::Set {
            uid: owner.uid,
            gid: owner.gid,
        },
        None if keep => FileOwner::Keep,
        None => FileOwner::Current,
    };

    match existing {
        Some(doc) if doc.header.is_some() => {
            if doc.header.as_ref().map(|h| &h.public_key) != Some(&public_key) {
                return Err(super::mismatch(ctx, &located.path).into());
            }
            output::ok(&format!("{} already uses this key.", ctx.env.display()));
        }
        Some(doc) => migrate(ctx, doc, header, file_owner(true))?,
        None => {
            let template = load_template(ctx)?;
            let mut doc = envfile::sync(&template, &Document::default()).doc;
            let count = doc.entries().count();
            doc.header = Some(header);
            write_env(ctx, &doc, file_owner(false))?;
            output::ok(&format!(
                "{} created from {} ({count} keys, all still empty).",
                ctx.env.display(),
                ctx.template.display()
            ));
        }
    }

    next_steps(ctx, owner.as_ref());
    Ok(0)
}

/// D2: an existing `.env` without a header. With a template, its order, comments and
/// annotations are taken over; every filled value that is not `@plain` is encrypted.
fn migrate(ctx: &Ctx, env: Document, header: Header, owner: FileOwner) -> Result<()> {
    let template = if ctx.template.exists() {
        Some(load_template(ctx)?)
    } else {
        None
    };
    let mut doc = match template {
        Some(template) => {
            let result = envfile::sync(&template, &env);
            for key in &result.extra {
                output::warn(&format!(
                    "{key} is in .env but not in the template; kept at the end of the file."
                ));
            }
            result.doc
        }
        None => {
            output::warn(&format!(
                "Template {} not found: @plain/@optional annotations are not copied, \
                 every filled value is encrypted.",
                ctx.template.display()
            ));
            env
        }
    };
    let recipient = crypto::parse_recipient(&header.public_key).map_err(anyhow::Error::msg)?;
    let mut encrypted = Vec::new();
    for entry in doc.entries_mut() {
        if let Value::Plain(value) = &entry.value
            && !entry.flags.plain
        {
            entry.value = Value::Encrypted(crypto::encrypt(&recipient, value)?);
            encrypted.push(entry.key.clone());
        }
    }
    doc.header = Some(header);
    write_env(ctx, &doc, owner)?;
    output::ok(&format!(
        "{} is now managed by sultrakey.",
        ctx.env.display()
    ));
    if encrypted.is_empty() {
        output::info("No plain values to encrypt.");
    } else {
        output::ok(&format!("Encrypted: {}", encrypted.join(", ")));
    }
    Ok(())
}

fn next_steps(ctx: &Ctx, owner: Option<&Owner>) {
    let (setup, check) = match owner {
        Some(owner) => {
            let user = owner.label.split(':').next().unwrap_or(&owner.label);
            (
                format!("sudo {}", ctx.cmd("setup")),
                format!("sudo -u {user} {}", ctx.cmd("check")),
            )
        }
        None => (ctx.cmd("setup"), ctx.cmd("check")),
    };
    output::info(&format!("Next: {setup}, then {check}"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_rules() {
        assert_eq!(
            plan_owner(true, false, Some("x"), "a").unwrap(),
            OwnerPlan::Ignore { warn: true }
        );
        assert_eq!(
            plan_owner(true, false, None, "a").unwrap(),
            OwnerPlan::Ignore { warn: false }
        );
        assert_eq!(
            plan_owner(false, true, Some("app"), "a").unwrap(),
            OwnerPlan::Root("app".into())
        );
        let need_owner = plan_owner(false, true, None, "demo").unwrap_err();
        assert_eq!(need_owner.exit_code(), 64);
        assert_eq!(
            need_owner.issues[0].solution.as_deref(),
            Some("sudo sultrakey init demo --owner <app-user>")
        );
        let need_root = plan_owner(false, false, Some("app"), "demo").unwrap_err();
        assert_eq!(need_root.exit_code(), 64);
        assert_eq!(
            need_root.issues[0].solution.as_deref(),
            Some("sudo sultrakey init demo --owner app")
        );
        assert_eq!(
            plan_owner(false, false, None, "a").unwrap(),
            OwnerPlan::NotRoot
        );
    }
}
