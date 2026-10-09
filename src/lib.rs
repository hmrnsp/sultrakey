//! sultrakey: encrypted `.env` files and a launcher that hands the decrypted values to any
//! application. See `catatan.md` for the full specification.

pub mod app;
pub mod cli;
pub mod commands;
pub mod crypto;
pub mod envfile;
pub mod error;
pub mod fsutil;
pub mod input;
pub mod install;
pub mod keyfile;
pub mod launch;
pub mod output;
pub mod secret;
pub mod system;
pub mod time;
pub mod update;
