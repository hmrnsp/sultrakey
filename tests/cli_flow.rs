//! init → setup → check → run, and every way they refuse.

mod common;

use std::fs;

use common::{Env, SECRET, TEMPLATE, exit_with, print_env, text};

#[test]
fn init_setup_check_run() {
    let env = Env::new();
    env.write(".env.example", TEMPLATE);

    let out = env.ok(&["init", "demo"], "");
    assert!(out.contains("New key created"), "{out}");
    assert!(env.key_file("demo").exists());
    let dotenv = env.read(".env");
    assert!(dotenv.starts_with("SULTRAKEY_APP=demo\nSULTRAKEY_PUBLIC_KEY=age1"));
    assert!(
        dotenv.contains("# @optional\nREDIS_PASSWORD=\n"),
        "{dotenv}"
    );

    env.write("cert.pem", "-----BEGIN-----\nAAA\n-----END-----\n");
    let out = env.ok(
        &["setup"],
        &format!("REDIS_HOST={SECRET}\nSSL_CERT=@cert.pem\n"),
    );
    assert!(out.contains("Filled: REDIS_HOST, SSL_CERT, PORT"), "{out}");
    let dotenv = env.read(".env");
    assert!(!dotenv.contains(SECRET));
    assert!(dotenv.contains("\nPORT=8899\n"), "default taken: {dotenv}");
    assert!(dotenv.contains("\nREDIS_HOST=enc:"));
    assert!(dotenv.contains("\nSSL_CERT=enc:"));

    let out = env.ok(&["check"], "");
    assert!(out.contains("All good (4 keys"), "{out}");

    let list = env.ok(&["list"], "");
    assert!(list.contains("REDIS_HOST      encrypted"), "{list}");
    assert!(list.contains("REDIS_PASSWORD  empty (optional)"), "{list}");

    let mut args = vec!["run", "--"];
    args.extend(print_env());
    let output = env.run(&args, "");
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    let printed = text(&output.stdout).replace("\r\n", "\n");
    assert!(
        printed.contains(&format!("REDIS_HOST={SECRET}\n")),
        "{printed}"
    );
    assert!(printed.contains("PORT=8899\n"));
    assert!(printed.contains("SSL_CERT=-----BEGIN-----\nAAA\n-----END-----"));
    if !cfg!(windows) {
        // cmd's `set` hides empty variables; `env` shows them.
        assert!(printed.contains("REDIS_PASSWORD=\n"), "D3: sent as empty");
    }
}

#[test]
fn second_setup_keeps_values_and_adds_new_template_keys() {
    let env = Env::new();
    env.ready();
    let before = env.read(".env");
    env.ok(&["setup"], "");
    assert_eq!(env.read(".env"), before, "nothing to do, nothing changed");

    env.write(".env.example", &format!("{TEMPLATE}# Baru\nNEW_KEY=\n"));
    let err_out = env.run(&["setup"], "REDIS_HOST=other\n");
    assert_eq!(err_out.status.code(), Some(0));
    let stderr = text(&err_out.stderr);
    assert!(
        stderr.contains("REDIS_HOST already has a value; not changed"),
        "{stderr}"
    );
    assert!(stderr.contains("Still empty: NEW_KEY"), "{stderr}");
    let stderr = env.fails(&["check"], "", 78);
    assert!(stderr.contains("NEW_KEY is empty."), "{stderr}");
    assert!(stderr.contains("Fix: sultrakey setup"), "{stderr}");
}

#[test]
fn secret_keys_never_take_example_values() {
    let env = Env::new();
    env.write(".env.example", "# @plain\nPORT=8899\nDB_PASSWORD=secret\n");
    env.ok(&["init", "demo"], "");
    let output = env.run(&["setup"], "");
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    let stderr = text(&output.stderr);
    assert!(stderr.contains("Still empty: DB_PASSWORD"), "{stderr}");

    let dotenv = env.read(".env");
    assert!(dotenv.contains("\nPORT=8899\n"), "{dotenv}");
    assert!(dotenv.contains("\nDB_PASSWORD=\n"), "{dotenv}");
    let stderr = env.fails(&["check"], "", 78);
    assert!(stderr.contains("DB_PASSWORD is empty."), "{stderr}");
}

#[test]
fn run_refuses_and_passes_on_exit_codes() {
    let env = Env::new();
    env.write(".env.example", "REDIS_HOST=\n");
    env.ok(&["init", "demo"], "");

    // Empty required key: nothing starts, exit 78.
    let mut args = vec!["run".to_string(), "--".to_string()];
    args.extend(exit_with(0));
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let stderr = env.fails(&args, "", 78);
    assert!(stderr.contains("✗ REDIS_HOST is empty."), "{stderr}");

    env.ok(&["setup"], &format!("REDIS_HOST={SECRET}\n"));
    let mut args = vec!["run".to_string(), "--".to_string()];
    args.extend(exit_with(3));
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    env.fails(&args, "", 3);

    let stderr = env.fails(&["run", "--", "no-such-program-xyz"], "", 78);
    assert!(stderr.contains("not found"), "{stderr}");
}

#[test]
fn run_hides_sultrakey_variables_and_warns_by_name() {
    let env = Env::new();
    env.ready();
    let mut args = vec!["run", "--"];
    args.extend(print_env());
    let output = env
        .cmd()
        .args(&args)
        .env("SULTRAKEY_KEY_FILE", env.key_file("demo"))
        .env("REDIS_HOST", "old-value")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    let printed = text(&output.stdout);
    assert!(
        !printed
            .lines()
            .any(|l| l.to_uppercase().starts_with("SULTRAKEY_")),
        "{printed}"
    );
    assert!(printed.contains(&format!("REDIS_HOST={SECRET}")));
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("REDIS_HOST is already in the environment"),
        "{stderr}"
    );
    assert!(!stderr.contains("old-value") && !stderr.contains(SECRET));
}

#[test]
fn usage_errors_exit_64() {
    let env = Env::new();
    env.fails(&["fil"], "", 64);
    env.fails(&["fill"], "", 64);
    env.fails(&["run"], "", 64);
    env.fails(&["init", "Bad_Name"], "", 64);
    env.fails(&["set", "A", "--stdin", "--file", "x"], "", 64);
    let out = env.ok(&["--version"], "");
    assert_eq!(
        out.trim(),
        format!("sultrakey {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn missing_files_are_configuration_problems() {
    let env = Env::new();
    let stderr = env.fails(&["check"], "", 78);
    assert!(stderr.contains(".env not found"), "{stderr}");
    let stderr = env.fails(&["init", "demo"], "", 78);
    assert!(
        stderr.contains("Template .env.example not found"),
        "{stderr}"
    );

    env.write(".env", "A=1\n");
    let stderr = env.fails(&["setup"], "", 78);
    assert!(stderr.contains("not managed by sultrakey yet"), "{stderr}");
}

#[test]
fn wrong_damaged_or_lost_keys() {
    let env = Env::new();
    env.ready();
    let good_key = fs::read(env.key_file("demo")).unwrap();

    // Another app's key.
    let other = Env::new();
    other.ready();
    let stderr = env.fails(
        &[
            "check",
            "--key-file",
            other.key_file("demo").to_str().unwrap(),
        ],
        "",
        78,
    );
    assert!(stderr.contains("does not belong to"), "{stderr}");

    // A damaged value.
    let dotenv = env.read(".env");
    let damaged = dotenv.replace("REDIS_HOST=enc:", "REDIS_HOST=enc:AAAA");
    env.write(".env", &damaged);
    let stderr = env.fails(&["check"], "", 78);
    assert!(
        stderr.contains("REDIS_HOST cannot be decrypted"),
        "{stderr}"
    );
    assert!(stderr.contains("Fix: sultrakey set REDIS_HOST"), "{stderr}");
    env.write(".env", &dotenv);

    // Lost key: check fails, init refuses to make a new one.
    fs::remove_file(env.key_file("demo")).unwrap();
    let stderr = env.fails(&["check"], "", 78);
    assert!(stderr.contains("not found"), "{stderr}");
    assert!(
        stderr.contains("restore the key file from a backup"),
        "{stderr}"
    );
    let stderr = env.fails(&["init", "demo"], "", 78);
    assert!(stderr.contains("No new key is created"), "{stderr}");
    assert!(!env.key_file("demo").exists());

    fs::write(env.key_file("demo"), good_key).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(env.key_file("demo"), fs::Permissions::from_mode(0o400)).unwrap();
    }
    env.ok(&["check"], "");
}

#[test]
fn plain_secret_values_block_run_until_setup_encrypts_them() {
    let env = Env::new();
    env.ready();
    let dotenv = env.read(".env");
    let start = dotenv.find("REDIS_HOST=").unwrap();
    let end = start + dotenv[start..].find('\n').unwrap();
    let edited = format!(
        "{}REDIS_HOST=typed-by-hand{}",
        &dotenv[..start],
        &dotenv[end..]
    );
    env.write(".env", &edited);

    let stderr = env.fails(&["check"], "", 78);
    assert!(stderr.contains("REDIS_HOST is stored plain"), "{stderr}");
    assert!(env.ok(&["list"], "").contains("plain — must be encrypted"));

    let out = env.ok(&["setup"], "");
    assert!(out.contains("Plain values encrypted: REDIS_HOST"), "{out}");
    assert!(!env.read(".env").contains("typed-by-hand"));
    env.ok(&["check"], "");
}

#[test]
fn unknown_annotations_are_refused_with_the_line() {
    let env = Env::new();
    env.write(".env.example", "# @optinal\nA=\n");
    let stderr = env.fails(&["init", "demo"], "", 78);
    assert!(stderr.contains("line 1"), "{stderr}");
    assert!(stderr.contains("@optinal"), "{stderr}");

    env.write(".env.example", "# @masked\nDATABASE_URL=\n");
    let stderr = env.fails(&["init", "demo"], "", 78);
    assert!(stderr.contains("renamed to @masking"), "{stderr}");

    env.write(".env.example", "# @masking\nDATABASE_URL=\n");
    env.ok(&["init", "demo"], "");
    assert!(env.read(".env").contains("# @masking\nDATABASE_URL=\n"));
}

#[test]
fn init_migrates_a_plain_env() {
    let env = Env::new();
    env.write(".env.example", TEMPLATE);
    env.write(
        ".env",
        &format!("PORT=3000\r\nREDIS_HOST={SECRET}\r\nLEGACY_TOKEN=abc def\r\n"),
    );
    let output = env.run(&["init", "demo"], "");
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    let stdout = text(&output.stdout);
    assert!(
        stdout.contains("Encrypted: REDIS_HOST, LEGACY_TOKEN"),
        "{stdout}"
    );
    assert!(text(&output.stderr).contains("LEGACY_TOKEN is in .env but not in the template"));

    let dotenv = env.read(".env");
    assert!(!dotenv.contains(SECRET) && !dotenv.contains("abc def"));
    assert!(
        dotenv.contains("# @plain\nPORT=3000\n"),
        "plain stays: {dotenv}"
    );
    assert!(!dotenv.contains('\r'));
    assert_eq!(dotenv.matches("SULTRAKEY_APP=").count(), 1);

    // Running init again changes nothing.
    let out = env.ok(&["init", "demo"], "");
    assert!(out.contains("already uses this key"), "{out}");
    assert_eq!(env.read(".env"), dotenv);

    let stderr = env.fails(&["check"], "", 78);
    assert!(stderr.contains("SSL_CERT is empty"), "{stderr}");
}

#[test]
fn init_refuses_another_apps_env() {
    let env = Env::new();
    env.ready();
    let stderr = env.fails(&["init", "other"], "", 78);
    assert!(stderr.contains("belongs to app 'demo'"), "{stderr}");
}

#[cfg(unix)]
#[test]
fn key_files_open_to_others_are_refused() {
    use std::os::unix::fs::PermissionsExt;
    let env = Env::new();
    env.ready();
    let key = env.key_file("demo");
    let mode = fs::metadata(&key).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o400);
    let dotenv_mode = fs::metadata(env.path(".env")).unwrap().permissions().mode() & 0o777;
    assert_eq!(dotenv_mode, 0o600);

    fs::set_permissions(&key, fs::Permissions::from_mode(0o644)).unwrap();
    let stderr = env.fails(&["check"], "", 78);
    assert!(stderr.contains("too open (644)"), "{stderr}");
    assert!(stderr.contains("sudo chmod 400"), "{stderr}");
}
