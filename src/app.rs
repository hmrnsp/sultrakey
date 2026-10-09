//! Parses the command line and runs the chosen command. Usage errors exit with 64, not
//! clap's usual 2, so scripts can tell them apart from configuration problems (78).

use anyhow::Result;
use clap::Parser;
use clap::error::ErrorKind;

use crate::cli::{Cli, Command};
use crate::commands::{self, Ctx};
use crate::error::Code;

pub fn run() -> Result<i32> {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => return Ok(clap_exit(&err)),
    };
    let ctx = Ctx::new(&cli.global);
    match cli.command {
        Command::Init { app, owner } => commands::init::run(&ctx, &app, owner.as_deref()),
        Command::Fill => commands::fill::run(&ctx),
        Command::Set { key, stdin, file } => commands::set::run(&ctx, &key, stdin, file.as_deref()),
        Command::List => commands::list::run(&ctx),
        Command::Check => commands::check::run(&ctx),
        Command::Run { command } => commands::run::run(&ctx, &command),
        Command::Install => commands::install::run(),
        Command::Update { check, yes } => commands::update::run(check, yes),
        Command::Uninstall { yes } => commands::uninstall::run(yes),
    }
}

/// Prints clap's message; help and version are not errors.
fn clap_exit(err: &clap::Error) -> i32 {
    let _ = err.print();
    match err.kind() {
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => 0,
        _ => {
            eprintln!("Solusi: sultrakey --help");
            Code::Usage.exit_code()
        }
    }
}
