//! The terminal while the form is open: raw keys, a separate screen drawn on stderr (stdout
//! stays free, like the line prompts), and everything put back on the way out, also on an
//! error or a panic. Closing the separate screen also takes the typed values off screen.

use std::io::{self, Stderr};
use std::panic;

use anyhow::Result;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, DisableBracketedPaste, EnableBracketedPaste};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

use super::{Field, Form, Step, view};
use crate::error::Abort;
use crate::input::prompt::Reply;

/// Shows the form until the answers are saved (`Ok`, in field order, `None` = left
/// unanswered) or the user cancels (`Abort::Cancelled`).
pub fn run(fields: Vec<Field>) -> Result<Vec<Option<Reply>>> {
    let mut screen = Screen::open()?;
    let mut form = Form::new(fields);
    loop {
        screen.terminal.draw(|frame| view::draw(frame, &form))?;
        match form.handle(event::read()?) {
            Step::Continue => {}
            Step::Save => return Ok(form.into_answers()),
            Step::Cancel => return Err(Abort::Cancelled.into()),
        }
    }
}

struct Screen {
    terminal: Terminal<CrosstermBackend<Stderr>>,
}

impl Screen {
    fn open() -> Result<Self> {
        let terminal = Terminal::new(CrosstermBackend::new(io::stderr()))?;
        // A panic message printed on the separate screen would vanish with it.
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            restore();
            previous(info);
        }));
        // From here on, a failure drops `screen`, which puts the terminal back.
        let screen = Self { terminal };
        enable_raw_mode()?;
        execute!(io::stderr(), EnterAlternateScreen, EnableBracketedPaste)?;
        Ok(screen)
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        restore();
        let _ = self.terminal.show_cursor();
        let _ = panic::take_hook();
    }
}

fn restore() {
    let _ = execute!(io::stderr(), DisableBracketedPaste, LeaveAlternateScreen);
    let _ = disable_raw_mode();
}
