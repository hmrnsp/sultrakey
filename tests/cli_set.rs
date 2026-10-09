//! `set`: from stdin, from a file, never from an argument.

mod common;

use common::{Env, SECRET, print_env, text};

fn run_env(env: &Env) -> String {
    let mut args = vec!["run", "--"];
    args.extend(print_env());
    let output = env.run(&args, "");
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    text(&output.stdout).replace("\r\n", "\n")
}

#[test]
fn set_from_stdin_and_file() {
    let env = Env::new();
    env.ready();

    let out = env.ok(&["set", "REDIS_HOST", "--stdin"], "10.0.0.5\n");
    assert!(out.contains("REDIS_HOST diperbarui."), "{out}");
    assert!(run_env(&env).contains("REDIS_HOST=10.0.0.5\n"));

    env.write("new.pem", "X\nY\n");
    let output = env.run(&["set", "SSL_CERT", "--file", "new.pem"], "");
    assert_eq!(output.status.code(), Some(0));
    assert!(text(&output.stderr).contains("Hapus file new.pem sekarang."));
    assert!(run_env(&env).contains("SSL_CERT=X\nY\n"));

    // @plain stays readable in the file.
    env.ok(&["set", "PORT", "--stdin"], "9000");
    assert!(env.read(".env").contains("\nPORT=9000\n"));
}

#[test]
fn set_refuses_unknown_keys_and_empty_required_values() {
    let env = Env::new();
    env.ready();
    let stderr = env.fails(&["set", "NOPE", "--stdin"], "x", 64);
    assert!(stderr.contains("NOPE tidak ada di .env"), "{stderr}");
    let stderr = env.fails(&["set", "REDIS_HOST", "--stdin"], "\n", 64);
    assert!(stderr.contains("wajib diisi"), "{stderr}");
    env.ok(&["set", "REDIS_PASSWORD", "--stdin"], "");
    // Without a terminal and without --stdin/--file, set cannot ask.
    let stderr = env.fails(&["set", "REDIS_HOST"], "", 64);
    assert!(stderr.contains("--stdin"), "{stderr}");
    env.ok(&["check"], "");
    assert!(!env.read(".env").contains(SECRET));
}

#[test]
fn fill_from_stdin_refuses_unknown_keys_and_binary_files() {
    let env = Env::new();
    env.ready();
    env.write(".env.template", &format!("{}EXTRA=\n", common::TEMPLATE));
    let stderr = env.fails(&["fill"], "NOT_IN_TEMPLATE=1\n", 64);
    assert!(
        stderr.contains("NOT_IN_TEMPLATE tidak ada di template"),
        "{stderr}"
    );
    std::fs::write(env.path("bin.dat"), b"\x00\x01\xff").unwrap();
    let stderr = env.fails(&["fill"], "EXTRA=@bin.dat\n", 64);
    assert!(stderr.contains("bukan teks"), "{stderr}");
    let stderr = env.fails(&["fill"], "garbage\n", 64);
    assert!(stderr.contains("bukan KEY=value"), "{stderr}");
}
