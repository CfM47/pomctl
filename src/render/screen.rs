//! Borrowing the terminal for the duration of a session.

use std::io::{self, IsTerminal, Stdout, Write};
use std::time::Duration;

use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use crossterm::terminal::{
    Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
    enable_raw_mode, size,
};
use crossterm::{ExecutableCommand, QueueableCommand};

const FALLBACK_SIZE: (u16, u16) = (80, 24);

/// A terminal switched to its alternate screen, in raw mode.
///
/// Holding one is the proof that the switch happened: there is no way to paint
/// or to read a key without having entered, and no way to leave without
/// dropping it. The restore lives in [`Drop`] rather than at the end of the
/// session so that an error path, or a panic while unwinding, cannot strand
/// the user in a screen with no cursor and no echo.
pub struct Screen {
    output: Stdout,
}

impl Screen {
    /// Takes over the terminal.
    ///
    /// Returns `None` when stdout is not a terminal, where the escape codes
    /// would land in whatever is reading the output, and also when the
    /// terminal refuses the switch. Both mean the same thing to a caller:
    /// print plainly instead.
    pub fn enter() -> Option<Self> {
        let mut output = io::stdout();
        if !output.is_terminal() {
            return None;
        }
        // Raw mode comes first because it is what the alternate screen is for:
        // keys reach us unbuffered and unechoed. A terminal that accepts the
        // mode but refuses the switch is restored here rather than left raw,
        // which would otherwise swallow the user's next Ctrl-C.
        enable_raw_mode().ok()?;
        if output.execute(EnterAlternateScreen).is_err() || output.execute(Hide).is_err() {
            let _ = disable_raw_mode();
            return None;
        }
        Some(Self { output })
    }

    /// Returns the frame size in columns and rows.
    pub fn size(&self) -> (u16, u16) {
        usable(size().unwrap_or(FALLBACK_SIZE))
    }

    /// Waits up to `timeout` for a key press.
    ///
    /// Doubles as the clock's pacing: a caller redraws every time this returns,
    /// so a key is acted on at once while an idle second costs one repaint.
    /// Anything that is not a press, a resize included, reads as no key, which
    /// still wakes the caller up to repaint at the new size.
    pub fn read_key(&self, timeout: Duration) -> Option<KeyEvent> {
        if !event::poll(timeout).unwrap_or(false) {
            return None;
        }
        match event::read() {
            // Releases and repeats would act on one press several times, which
            // on a pause key reads as the pause not working.
            Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => Some(key),
            _ => None,
        }
    }

    /// Draws one frame, one line per row from the top.
    ///
    /// Each line is cleared to its right as it is written rather than wiping
    /// the whole screen first, which would show a blank frame between paints.
    pub fn paint(&mut self, frame: &[String]) -> io::Result<()> {
        for (row, line) in frame.iter().enumerate().take(u16::MAX as usize) {
            self.output.queue(MoveTo(0, row as u16))?;
            self.output.write_all(line.as_bytes())?;
            self.output.queue(Clear(ClearType::UntilNewLine))?;
        }
        self.output.flush()
    }
}

/// Replaces a size nothing can be drawn in with one that can.
///
/// A pty opened without a window size, as a test harness or a `script` run
/// does, reports zero columns and rows. A frame of no rows paints nothing at
/// all, which reads as a hung timer rather than as a window too small.
fn usable((width, height): (u16, u16)) -> (u16, u16) {
    if width == 0 || height == 0 {
        FALLBACK_SIZE
    } else {
        (width, height)
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        let _ = self.output.execute(Show);
        let _ = self.output.execute(LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_real_terminal_size_is_used_as_it_is() {
        assert_eq!(usable((120, 40)), (120, 40));
    }

    #[test]
    fn a_pty_with_no_window_size_is_drawn_at_the_fallback() {
        // Zero rows would paint an empty frame, which looks like a hang.
        assert_eq!(usable((0, 0)), FALLBACK_SIZE);
        assert_eq!(usable((80, 0)), FALLBACK_SIZE);
        assert_eq!(usable((0, 24)), FALLBACK_SIZE);
    }
}
