use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "sultrakey",
    version,
    about = "Encrypted .env files, and a launcher that runs applications with the decrypted values",
    after_help = "On a server:\n  \
        sudo sultrakey init <app> --owner <app-user>\n  \
        sultrakey setup\n  \
        sultrakey check\n  \
        sultrakey run -- <application command>"
)]
pub struct Cli {
    #[command(flatten)]
    pub global: Global,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, Args)]
pub struct Global {
    /// Path of the .env file
    #[arg(long, global = true, value_name = "FILE", default_value = ".env")]
    pub env: PathBuf,

    /// Path of the template file
    #[arg(
        long,
        global = true,
        value_name = "FILE",
        default_value = ".env.example"
    )]
    pub template: PathBuf,

    /// Path of the key file (default: /etc/sultrakey/<app>.key; Windows: %APPDATA%\sultrakey\<app>.key)
    #[arg(long, global = true, value_name = "FILE")]
    pub key_file: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create the key (if missing) and .env from the template, or encrypt an old plain .env
    Init {
        /// Application name: lowercase letters, digits, dashes
        app: String,
        /// Owner of the key file and .env: the application's user[:group] (required with sudo)
        #[arg(long, value_name = "USER[:GROUP]")]
        owner: Option<String>,
    },

    /// Bring .env in line with the template, then show every key: fill the empty ones, keep or replace the rest
    Setup,

    /// Replace the value of one key (from a prompt, --stdin, or --file)
    Set {
        /// Key name, exactly as in .env
        key: String,
        /// Read the value from stdin (all of it)
        #[arg(long, conflicts_with = "file")]
        stdin: bool,
        /// Read the value from a file (for multi-line values such as certificates)
        #[arg(long, value_name = "FILE")]
        file: Option<PathBuf>,
    },

    /// Show key names and their status (values are never shown)
    List,

    /// Check that every key is filled, can be decrypted, and the key matches
    Check,

    /// Check, decrypt, then run the application with the values in its environment
    Run {
        /// Application command, written after --, for example: sultrakey run -- node dist/main.js
        #[arg(last = true, required = true, value_name = "COMMAND")]
        command: Vec<OsString>,
    },

    /// Install this sultrakey to /usr/bin (sudo), ~/.local/bin, or the Windows programs folder
    Install,

    /// Update sultrakey to the latest release
    Update {
        /// Only check whether a new version exists (exit 1 if it does)
        #[arg(long)]
        check: bool,
        /// Do not ask for confirmation
        #[arg(short, long)]
        yes: bool,
    },

    /// Remove the installed sultrakey (key files and .env are not touched)
    Uninstall {
        /// Do not ask for confirmation
        #[arg(short, long)]
        yes: bool,
    },
}
