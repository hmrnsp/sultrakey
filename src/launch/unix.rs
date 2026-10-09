use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::process::Command;

use anyhow::Result;

use super::{ChildEnv, start_fail};

/// Replaces this process with the application. Returns only when that fails.
pub fn exec(program: &OsString, args: &[OsString], env: &ChildEnv) -> Result<i32> {
    let mut command = Command::new(program);
    command.args(args);
    for name in &env.remove {
        command.env_remove(name);
    }
    for (key, value) in &env.set {
        command.env(key, value.expose());
    }
    let err = command.exec();
    Err(start_fail(program, &err).into())
}
