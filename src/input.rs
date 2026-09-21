//! Turning key presses into the three things a session can be told to do.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Something the user asked of a running session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// Leave, printing the summary.
    Quit,
    /// Stop or restart the clock where it stands.
    Pause,
    /// Treat the current phase as over, without notifying.
    Skip,
}

impl Command {
    /// Reads a key as a command, or as nothing.
    ///
    /// Raw mode delivers Ctrl-C as a key rather than as a signal, so quitting
    /// on it here is what keeps the habit working; without this the terminal
    /// would only be restored by `q`.
    pub fn from_key(key: KeyEvent) -> Option<Self> {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('c' | 'C') if control => Some(Self::Quit),
            // A modified letter is someone else's shortcut, not ours.
            _ if control || key.modifiers.contains(KeyModifiers::ALT) => None,
            KeyCode::Char('q' | 'Q') => Some(Self::Quit),
            KeyCode::Char(' ') => Some(Self::Pause),
            KeyCode::Char('s' | 'S') => Some(Self::Skip),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(code: KeyCode) -> Option<Command> {
        Command::from_key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn press_with(code: KeyCode, modifiers: KeyModifiers) -> Option<Command> {
        Command::from_key(KeyEvent::new(code, modifiers))
    }

    #[test]
    fn the_documented_keys_do_what_the_help_says() {
        assert_eq!(press(KeyCode::Char('q')), Some(Command::Quit));
        assert_eq!(press(KeyCode::Char(' ')), Some(Command::Pause));
        assert_eq!(press(KeyCode::Char('s')), Some(Command::Skip));
    }

    #[test]
    fn caps_lock_does_not_disable_the_controls() {
        assert_eq!(press(KeyCode::Char('Q')), Some(Command::Quit));
        assert_eq!(press(KeyCode::Char('S')), Some(Command::Skip));
    }

    #[test]
    fn control_c_quits_because_raw_mode_swallows_the_signal() {
        assert_eq!(
            press_with(KeyCode::Char('c'), KeyModifiers::CONTROL),
            Some(Command::Quit)
        );
        assert_eq!(
            press_with(KeyCode::Char('C'), KeyModifiers::CONTROL | KeyModifiers::SHIFT),
            Some(Command::Quit)
        );
    }

    #[test]
    fn a_plain_c_keeps_running() {
        assert_eq!(press(KeyCode::Char('c')), None);
    }

    #[test]
    fn a_shortcut_meant_for_something_else_is_left_alone() {
        // Ctrl-S is flow control and Alt-S a menu key on many terminals;
        // reading either as skip would throw away a phase the user is in.
        assert_eq!(press_with(KeyCode::Char('s'), KeyModifiers::CONTROL), None);
        assert_eq!(press_with(KeyCode::Char('s'), KeyModifiers::ALT), None);
        assert_eq!(press_with(KeyCode::Char('q'), KeyModifiers::ALT), None);
    }

    #[test]
    fn an_unbound_key_is_ignored_rather_than_guessed_at() {
        for code in [
            KeyCode::Enter,
            KeyCode::Esc,
            KeyCode::Up,
            KeyCode::Backspace,
            KeyCode::Char('p'),
        ] {
            assert_eq!(press(code), None, "{code:?} is not a control");
        }
    }
}
