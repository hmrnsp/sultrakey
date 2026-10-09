//! The child's environment: the current one, minus `SULTRAKEY_*`, plus every value from
//! `.env` (which wins over what was already set).

use std::ffi::OsString;

use crate::envfile::RESERVED_PREFIX;
use crate::secret::Secret;

#[derive(Debug, Default)]
pub struct ChildEnv {
    /// Names to remove (as found in the current environment).
    pub remove: Vec<OsString>,
    pub set: Vec<(String, Secret)>,
    /// Keys that were already set to another value; names only, for a warning.
    pub conflicts: Vec<String>,
}

/// Windows variable names ignore case.
fn same_name(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

fn reserved(name: &str) -> bool {
    if cfg!(windows) {
        name.to_ascii_uppercase().starts_with(RESERVED_PREFIX)
    } else {
        name.starts_with(RESERVED_PREFIX)
    }
}

pub fn plan(
    current: impl IntoIterator<Item = (OsString, OsString)>,
    values: &[(String, Secret)],
) -> ChildEnv {
    let current: Vec<(OsString, OsString)> = current.into_iter().collect();
    let mut env = ChildEnv::default();
    for (name, _) in &current {
        if reserved(&name.to_string_lossy()) {
            env.remove.push(name.clone());
        }
    }
    for (key, value) in values {
        let clash = current.iter().any(|(name, old)| {
            same_name(&name.to_string_lossy(), key) && old.to_str() != Some(value.expose())
        });
        if clash {
            env.conflicts.push(key.clone());
        }
        env.set.push((key.clone(), value.clone()));
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        pairs.iter().map(|(a, b)| (a.into(), b.into())).collect()
    }

    #[test]
    fn removes_reserved_names_and_reports_conflicts_by_name() {
        let current = os(&[
            ("PATH", "/bin"),
            ("SULTRAKEY_KEY_FILE", "/k"),
            ("SULTRAKEY_APP", "x"),
            ("PORT", "1"),
            ("HOST", "same"),
            ("CREDENTIALS_DIRECTORY", "/run/c"),
        ]);
        let values = vec![
            ("PORT".to_string(), Secret::from("2")),
            ("HOST".to_string(), Secret::from("same")),
            ("EMPTY".to_string(), Secret::default()),
        ];
        let env = plan(current, &values);
        assert_eq!(
            env.remove,
            [
                OsString::from("SULTRAKEY_KEY_FILE"),
                OsString::from("SULTRAKEY_APP")
            ]
        );
        assert_eq!(env.conflicts, ["PORT"]);
        assert_eq!(env.set.len(), 3);
        assert_eq!(env.set[2].1.expose(), "");
    }

    #[cfg(windows)]
    #[test]
    fn windows_names_ignore_case() {
        let env = plan(
            os(&[("sultrakey_key_file", "/k"), ("port", "1")]),
            &[("PORT".to_string(), Secret::from("2"))],
        );
        assert_eq!(env.remove, [OsString::from("sultrakey_key_file")]);
        assert_eq!(env.conflicts, ["PORT"]);
    }
}
