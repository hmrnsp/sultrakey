//! Values and key files work with the official `age` tool, both ways: an emergency
//! decryption without sultrakey must always be possible.
//!
//! Skipped when `age` is not installed, unless `SULTRAKEY_REQUIRE_AGE_CLI=1` (CI).

mod common;

use std::fs;
use std::process::Command;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use common::{Env, SECRET, print_env, text};

fn age_available() -> bool {
    let found = Command::new("age")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success());
    if !found {
        assert!(
            std::env::var_os("SULTRAKEY_REQUIRE_AGE_CLI").is_none(),
            "SULTRAKEY_REQUIRE_AGE_CLI is set but the age tool is not installed"
        );
        eprintln!("age tidak terpasang; test kompatibilitas dilewati");
    }
    found
}

fn value_of<'a>(dotenv: &'a str, key: &str) -> &'a str {
    dotenv
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .unwrap()
}

#[test]
fn the_age_tool_opens_sultrakey_values() {
    if !age_available() {
        return;
    }
    let env = Env::new();
    env.ready();
    let dotenv = env.read(".env");
    let b64 = value_of(&dotenv, "REDIS_HOST")
        .strip_prefix("enc:")
        .unwrap();
    let cipher = env.path("value.age");
    fs::write(&cipher, STANDARD.decode(b64).unwrap()).unwrap();

    let out = Command::new("age")
        .arg("-d")
        .arg("-i")
        .arg(env.key_file("demo"))
        .arg(&cipher)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), SECRET);
}

#[test]
fn sultrakey_opens_values_made_by_the_age_tool() {
    if !age_available() {
        return;
    }
    let env = Env::new();
    env.ready();
    let dotenv = env.read(".env");
    let public_key = value_of(&dotenv, "SULTRAKEY_PUBLIC_KEY");
    env.write("plain.txt", "dibuat-dengan-age");
    let cipher = env.path("made.age");
    let out = Command::new("age")
        .args(["-r", public_key, "-o"])
        .arg(&cipher)
        .arg(env.path("plain.txt"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
    let b64 = STANDARD.encode(fs::read(&cipher).unwrap());
    let old = value_of(&dotenv, "REDIS_HOST").to_string();
    env.write(".env", &dotenv.replace(&old, &format!("enc:{b64}")));

    let mut args = vec!["run", "--"];
    args.extend(print_env());
    let output = env.run(&args, "");
    assert!(text(&output.stdout).contains("REDIS_HOST=dibuat-dengan-age"));
}
