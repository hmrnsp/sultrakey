//! Helpers for integration tests: a temporary app folder with its own key folder, and
//! sultrakey runs that never see the real `/etc/sultrakey`, install folder or releases.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use assert_cmd::Command;
use tempfile::TempDir;

/// The value used to prove no secret ever reaches stdout or stderr.
pub const SECRET: &str = "S3cr3t-uji-xyz";

pub const TEMPLATE: &str = "# Port aplikasi\n# @plain\nPORT=8899\n# Host Redis\nREDIS_HOST=\n\
                            # Password Redis\n# @optional\nREDIS_PASSWORD=\n# Sertifikat\nSSL_CERT=\n";

pub struct Env {
    pub dir: TempDir,
}

impl Env {
    pub fn new() -> Self {
        let env = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        fs::create_dir_all(env.app()).unwrap();
        env
    }

    /// The application folder (working directory of every run).
    pub fn app(&self) -> PathBuf {
        self.dir.path().join("app")
    }

    pub fn keys(&self) -> PathBuf {
        self.dir.path().join("keys")
    }

    pub fn key_file(&self, app: &str) -> PathBuf {
        self.keys().join(format!("{app}.key"))
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.app().join(name)
    }

    pub fn write(&self, name: &str, text: &str) {
        fs::write(self.path(name), text).unwrap();
    }

    pub fn read(&self, name: &str) -> String {
        fs::read_to_string(self.path(name)).unwrap()
    }

    pub fn cmd(&self) -> Command {
        let mut cmd = Command::cargo_bin("sultrakey").unwrap();
        cmd.current_dir(self.app())
            .env("SULTRAKEY_KEY_DIR", self.keys())
            .env("SULTRAKEY_INSTALL_DIR", self.dir.path().join("bin"))
            .env("SULTRAKEY_UPDATE_URL", "http://127.0.0.1:9/none")
            .env_remove("SULTRAKEY_KEY_FILE")
            .env_remove("CREDENTIALS_DIRECTORY");
        cmd
    }

    /// Runs with `stdin`, returns the whole output (any exit code).
    pub fn run(&self, args: &[&str], stdin: &str) -> Output {
        let mut args: Vec<&str> = args.to_vec();
        // As root (CI containers), `init` needs `--owner` (D4): give the files to root.
        if running_as_root() && args.first() == Some(&"init") && !args.contains(&"--owner") {
            args.extend(["--owner", "0:0"]);
        }
        let output = self.cmd().args(&args).write_stdin(stdin).output().unwrap();
        assert_no_secret(&output);
        if args.first() != Some(&"run") {
            let stdout = text(&output.stdout);
            assert!(!stdout.contains(SECRET), "secret on stdout: {stdout}");
        }
        output
    }

    /// Runs, expects exit 0, returns stdout.
    pub fn ok(&self, args: &[&str], stdin: &str) -> String {
        let output = self.run(args, stdin);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{args:?}\nstdout: {}\nstderr: {}",
            text(&output.stdout),
            text(&output.stderr)
        );
        text(&output.stdout)
    }

    /// Runs, expects `code`, returns stderr.
    pub fn fails(&self, args: &[&str], stdin: &str, code: i32) -> String {
        let output = self.run(args, stdin);
        assert_eq!(
            output.status.code(),
            Some(code),
            "{args:?}\nstdout: {}\nstderr: {}",
            text(&output.stdout),
            text(&output.stderr)
        );
        text(&output.stderr)
    }

    /// Template + `init demo` + `fill` with the test secret.
    pub fn ready(&self) {
        self.write(".env.template", TEMPLATE);
        self.write("cert.pem", "-----BEGIN-----\nAAA\n-----END-----\n");
        self.ok(&["init", "demo"], "");
        self.ok(
            &["fill"],
            &format!("REDIS_HOST={SECRET}\nSSL_CERT=@cert.pem\n"),
        );
    }
}

#[cfg(unix)]
pub fn running_as_root() -> bool {
    nix::unistd::Uid::effective().is_root()
}

#[cfg(not(unix))]
pub fn running_as_root() -> bool {
    false
}

pub fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Every sultrakey run in the tests is checked: the secret must never be printed. `run`
/// itself is the exception — there the output belongs to the application.
pub fn assert_no_secret(output: &Output) {
    let stderr = text(&output.stderr);
    assert!(!stderr.contains(SECRET), "secret on stderr: {stderr}");
}

/// A command printing the whole environment, one `NAME=value` per line.
pub fn print_env() -> Vec<&'static str> {
    if cfg!(windows) {
        vec!["cmd", "/C", "set"]
    } else {
        vec!["env"]
    }
}

/// A command exiting with `code`.
pub fn exit_with(code: i32) -> Vec<String> {
    if cfg!(windows) {
        vec!["cmd".into(), "/C".into(), format!("exit {code}")]
    } else {
        vec!["sh".into(), "-c".into(), format!("exit {code}")]
    }
}

pub fn exists(path: &Path) -> bool {
    path.exists()
}
