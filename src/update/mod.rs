//! `update`: find the latest release, download the binary for this system, check it and
//! replace the installed one. Only `http` touches the network; the rest is pure. Adapted
//! from lopi `src/update/`, with plain binaries instead of archives.

pub mod checksum;
pub mod http;
pub mod manifest;
pub mod version;

/// The release target whose binary fits this build, or `None` where none is released.
/// Linux builds take the static musl binary. Windows on ARM runs the x86_64 one.
pub const RELEASE_TARGET: Option<&str> = if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
    Some("x86_64-unknown-linux-musl")
} else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
    Some("aarch64-unknown-linux-musl")
} else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
    Some("x86_64-apple-darwin")
} else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
    Some("aarch64-apple-darwin")
} else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
    Some("x86_64-pc-windows-msvc")
} else {
    None
};

/// `sultrakey-<target>` (plus `.exe` for Windows), the release file name.
pub fn asset_name(target: &str) -> String {
    let suffix = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    format!("sultrakey-{target}{suffix}")
}

/// The digest listed for `name` in a `SHA256SUMS` file (`<hex>  <name>` or `<hex> *<name>`).
pub fn sum_for(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let mut words = line.split_whitespace();
        let hex = words.next()?;
        let file = words.next()?.trim_start_matches('*');
        (file == name && words.next().is_none() && manifest::is_sha256_hex(hex))
            .then(|| hex.to_ascii_lowercase())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_sums() {
        assert_eq!(
            asset_name("x86_64-unknown-linux-musl"),
            "sultrakey-x86_64-unknown-linux-musl"
        );
        assert_eq!(
            asset_name("x86_64-pc-windows-msvc"),
            "sultrakey-x86_64-pc-windows-msvc.exe"
        );
        let hex = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let sums = format!(
            "{hex}  sultrakey-a\n{}  *sultrakey-b\nbad line\n",
            hex.to_uppercase()
        );
        assert_eq!(sum_for(&sums, "sultrakey-a").as_deref(), Some(hex));
        assert_eq!(sum_for(&sums, "sultrakey-b").as_deref(), Some(hex));
        assert_eq!(sum_for(&sums, "sultrakey-c"), None);
        assert_eq!(sum_for("abc  sultrakey-a\n", "sultrakey-a"), None);
    }
}
