//! `run [--env FILE] -- <cmd> [args]`: every `check`, then the application starts with
//! the decrypted values. Nothing starts when a check fails (exit 78, so pm2/systemd stop
//! restarting).

use std::env;
use std::ffi::OsString;

use anyhow::Result;

use super::{Ctx, check};
use crate::launch;
use crate::output;

pub fn run(ctx: &Ctx, command: &[OsString]) -> Result<i32> {
    let opened = check::inspect(ctx)?;
    let plan = launch::plan(env::vars_os(), &opened.values);
    for key in &plan.conflicts {
        output::warn(&format!(
            "{key} is already in the environment; the value from .env is used."
        ));
    }
    drop(opened);
    launch::launch(command, &plan)
}
