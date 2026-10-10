//! Users and file permissions. Real on Unix; on Windows (developer laptops only) there is
//! no root, no `--owner` and no permission check.

#[cfg(unix)]
pub use unix::*;
#[cfg(windows)]
pub use windows::*;

/// The uid/gid an `--owner` value names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owner {
    pub uid: u32,
    pub gid: u32,
    /// As typed, for messages.
    pub label: String,
}

#[cfg(unix)]
mod unix {
    use std::fs;
    use std::io;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::path::Path;

    use nix::unistd::{Gid, Group, Uid, User};

    use super::Owner;
    use crate::error::Fail;

    pub fn is_root() -> bool {
        Uid::effective().is_root()
    }

    /// The login name of the current user, for messages (`--owner budi`).
    pub fn current_user_name() -> Option<String> {
        User::from_uid(Uid::effective())
            .ok()
            .flatten()
            .map(|user| user.name)
    }

    pub fn user_name(uid: u32) -> Option<String> {
        User::from_uid(Uid::from_raw(uid))
            .ok()
            .flatten()
            .map(|user| user.name)
    }

    /// `user`, `user:group`, `uid`, `uid:gid`. Without a group, the user's own group.
    /// Names are looked up in `/etc/passwd` and `/etc/group` (the static musl build cannot
    /// ask LDAP/SSSD; use numbers for such users).
    pub fn resolve_owner(spec: &str) -> Result<Owner, Fail> {
        let (user_part, group_part) = match spec.split_once(':') {
            Some((user, group)) => (user, Some(group)),
            None => (spec, None),
        };
        let unknown_user = || {
            Fail::usage(
                format!("User '{user_part}' not found on this server."),
                "create the application user first (useradd), or give numbers uid:gid, for example: --owner 1001:1001",
            )
        };
        let (uid, default_gid) = if let Ok(uid) = user_part.parse::<u32>() {
            let gid = User::from_uid(Uid::from_raw(uid))
                .ok()
                .flatten()
                .map(|user| user.gid.as_raw());
            (uid, gid)
        } else {
            let user = User::from_name(user_part)
                .ok()
                .flatten()
                .ok_or_else(unknown_user)?;
            (user.uid.as_raw(), Some(user.gid.as_raw()))
        };
        let gid = match group_part {
            Some(group) => match group.parse::<u32>() {
                Ok(gid) => gid,
                Err(_) => Group::from_name(group)
                    .ok()
                    .flatten()
                    .map(|group| group.gid.as_raw())
                    .ok_or_else(|| {
                        Fail::usage(
                            format!("Group '{group}' not found on this server."),
                            "check the group name, or give numbers uid:gid",
                        )
                    })?,
            },
            None => default_gid.ok_or_else(|| {
                Fail::usage(
                    format!("uid {uid} has no primary group."),
                    format!("give the group too, for example: --owner {uid}:{uid}"),
                )
            })?,
        };
        Ok(Owner {
            uid,
            gid,
            label: spec.to_string(),
        })
    }

    /// The permission bits that let group or others in, when there are any.
    pub fn open_to_others(path: &Path) -> io::Result<Option<u32>> {
        let mode = fs::metadata(path)?.permissions().mode() & 0o777;
        Ok((mode & 0o077 != 0).then_some(mode))
    }

    /// Whether group or others may write the file.
    pub fn writable_by_others(path: &Path) -> io::Result<bool> {
        let mode = fs::metadata(path)?.permissions().mode();
        Ok(mode & 0o022 != 0)
    }

    /// The name (or uid) of the file's owner, for `sudo -u <owner>` hints.
    pub fn owner_name(path: &Path) -> Option<String> {
        let uid = fs::metadata(path).ok()?.uid();
        Some(user_name(uid).unwrap_or_else(|| uid.to_string()))
    }

    pub fn current_ids() -> (u32, u32) {
        (Uid::effective().as_raw(), Gid::effective().as_raw())
    }
}

#[cfg(windows)]
mod windows {
    use std::io;
    use std::path::Path;

    use super::Owner;
    use crate::error::Fail;

    pub fn is_root() -> bool {
        false
    }

    pub fn current_user_name() -> Option<String> {
        std::env::var("USERNAME").ok()
    }

    pub fn resolve_owner(_spec: &str) -> Result<Owner, Fail> {
        Err(Fail::usage(
            "--owner is not used on Windows.",
            "run without --owner",
        ))
    }

    pub fn open_to_others(_path: &Path) -> io::Result<Option<u32>> {
        Ok(None)
    }

    pub fn writable_by_others(_path: &Path) -> io::Result<bool> {
        Ok(false)
    }

    pub fn owner_name(_path: &Path) -> Option<String> {
        None
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn resolves_numbers_and_root() {
        let owner = resolve_owner("0:0").unwrap();
        assert_eq!((owner.uid, owner.gid), (0, 0));
        let root = resolve_owner("root").unwrap();
        assert_eq!(root.uid, 0);
        assert!(resolve_owner("no-such-user-xyz").is_err());
        assert!(resolve_owner("root:no-such-group-xyz").is_err());
    }

    #[test]
    fn permission_checks() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("k");
        std::fs::write(&path, "x").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o400)).unwrap();
        assert_eq!(open_to_others(&path).unwrap(), None);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(open_to_others(&path).unwrap(), Some(0o644));
        assert!(!writable_by_others(&path).unwrap());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o664)).unwrap();
        assert!(writable_by_others(&path).unwrap());
    }
}
