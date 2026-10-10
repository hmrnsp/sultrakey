//! Getting values from people and pipes, never from command-line arguments (those show
//! up in `ps` and shell history).

pub mod form;
pub mod prompt;
pub mod source;
pub mod stdin_lines;
