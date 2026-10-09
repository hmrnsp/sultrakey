//! install / uninstall / update, in a temporary install folder, with a local web server
//! standing in for GitHub (pattern from lopi `tests/cli.rs`).

mod common;

use std::path::PathBuf;

use assert_cmd::Command;
use common::{Env, text};
use sultrakey::update::checksum::sha256_hex;
use sultrakey::update::{RELEASE_TARGET, asset_name};

const NEWER: &str = "99.0.0";

/// Answers GET requests for `files` (path → body), 404 for anything else, until the test
/// process ends. Returns the base URL for `SULTRAKEY_UPDATE_URL`.
fn serve(files: Vec<(String, Vec<u8>)>) -> String {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            let _ = reader.read_line(&mut request);
            let mut header = String::new();
            while reader.read_line(&mut header).is_ok_and(|n| n > 2) {
                header.clear();
            }
            let path = request.split_whitespace().nth(1).unwrap_or_default();
            let (status, body) = match files.iter().find(|(file, _)| file == path) {
                Some((_, body)) => ("200 OK", body.as_slice()),
                None => ("404 Not Found", &[][..]),
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(body);
        }
    });
    format!("http://{address}/releases")
}

/// The files of release `version`: manifest under `latest`, binary and SHA256SUMS under
/// the tag. `sums_digest` overrides the digest written in SHA256SUMS.
fn release(version: &str, binary: &[u8], sums_digest: Option<&str>) -> Vec<(String, Vec<u8>)> {
    let target = RELEASE_TARGET.unwrap_or("no-target");
    let name = asset_name(target);
    let digest = sha256_hex(binary);
    let manifest = format!(
        r#"{{"name":"sultrakey","version":"{version}","files":[{{"target":"{target}","name":"{name}","sha256":"{digest}"}}]}}"#
    );
    let tag = format!("/releases/download/v{version}");
    vec![
        (
            "/releases/latest/download/manifest.json".into(),
            manifest.into_bytes(),
        ),
        (format!("{tag}/{name}"), binary.to_vec()),
        (
            format!("{tag}/SHA256SUMS"),
            format!("{}  {name}\n", sums_digest.unwrap_or(&digest)).into_bytes(),
        ),
    ]
}

fn installed(env: &Env) -> PathBuf {
    env.dir
        .path()
        .join("bin")
        .join(format!("sultrakey{}", std::env::consts::EXE_SUFFIX))
}

/// Runs the installed copy (so `update` sees itself as installed).
fn installed_cmd(env: &Env, url: &str) -> Command {
    let mut cmd = Command::new(installed(env));
    cmd.current_dir(env.app())
        .env("SULTRAKEY_INSTALL_DIR", env.dir.path().join("bin"))
        .env("SULTRAKEY_UPDATE_URL", url);
    cmd
}

#[test]
fn install_then_uninstall() {
    let env = Env::new();
    let out = env.ok(&["install"], "");
    assert!(out.contains("dipasang di"), "{out}");
    assert!(installed(&env).exists());
    let out = env.ok(&["install"], "");
    assert!(out.contains("diperbarui ke sultrakey"), "{out}");

    env.fails(&["uninstall"], "", 64);
    let out = env.ok(&["uninstall", "-y"], "");
    assert!(out.contains("dihapus"), "{out}");
    assert!(out.contains("File kunci tidak dihapus"), "{out}");
    assert!(!installed(&env).exists());
    let out = env.ok(&["uninstall", "-y"], "");
    assert!(out.contains("tidak ada yang dihapus"), "{out}");
}

#[test]
fn update_check_reports_a_newer_release() {
    let env = Env::new();
    env.ok(&["install"], "");
    let url = serve(release(NEWER, b"new", None));
    let out = installed_cmd(&env, &url)
        .args(["update", "--check"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(
        stdout.contains(&format!("sultrakey {NEWER} tersedia")),
        "{stdout}"
    );

    let current = env!("CARGO_PKG_VERSION");
    let url = serve(release(current, b"same", None));
    let out = installed_cmd(&env, &url).args(["update"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(text(&out.stdout).contains("sudah versi terbaru"));
}

#[test]
fn update_refuses_copies_outside_the_install_folder() {
    let env = Env::new();
    let url = serve(release(NEWER, b"new", None));
    let out = env
        .cmd()
        .args(["update", "-y"])
        .env("SULTRAKEY_UPDATE_URL", &url)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(64), "{}", text(&out.stderr));
    assert!(text(&out.stderr).contains("bukan dari lokasi pasang"));
}

#[test]
fn update_refuses_a_bad_checksum_and_keeps_the_old_binary() {
    let env = Env::new();
    env.ok(&["install"], "");
    let before = std::fs::read(installed(&env)).unwrap();
    let wrong = "0".repeat(64);
    let url = serve(release(NEWER, b"tampered", Some(&wrong)));
    let out = installed_cmd(&env, &url)
        .args(["update", "-y"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    assert!(text(&out.stderr).contains("dua checksum berbeda"));
    assert_eq!(std::fs::read(installed(&env)).unwrap(), before);
}

/// A real replacement needs a binary that runs here: a shell script on Unix.
#[cfg(unix)]
#[test]
fn update_replaces_the_installed_binary() {
    let env = Env::new();
    env.ok(&["install"], "");
    let script = format!("#!/bin/sh\necho \"sultrakey {NEWER}\"\n");
    let url = serve(release(NEWER, script.as_bytes(), None));
    let out = installed_cmd(&env, &url)
        .args(["update", "-y"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(text(&out.stdout).contains(&format!("diperbarui ke sultrakey {NEWER}")));
    let version = std::process::Command::new(installed(&env))
        .arg("--version")
        .output()
        .unwrap();
    assert_eq!(text(&version.stdout).trim(), format!("sultrakey {NEWER}"));

    // A binary claiming another version is refused, the current one stays. The real
    // sultrakey is installed again first (the fake one above cannot update itself).
    env.ok(&["install"], "");
    let lying = "#!/bin/sh\necho \"sultrakey 1.0.0\"\n";
    let url = serve(release("100.0.0", lying.as_bytes(), None));
    let out = installed_cmd(&env, &url)
        .args(["update", "-y"])
        .output()
        .unwrap();
    assert_ne!(out.status.code(), Some(0));
    assert!(
        text(&out.stderr).contains("menyebut dirinya"),
        "{}",
        text(&out.stderr)
    );
}
