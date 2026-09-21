//! Telling the desktop that a phase is over.

use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use crate::timer::{Phase, Transition};

const ICON: &str = "utilities-terminal";
const TIMEOUT_MS: &str = "5000";
const SOUNDS: &str = "/usr/share/sounds/freedesktop/stereo";

/// A notification and the sound that goes with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alert {
    /// Urgency as `notify-send` spells it.
    pub urgency: &'static str,
    /// The line the user reads.
    pub body: String,
    /// Absolute path of the sound file to play.
    pub sound: String,
}

impl Alert {
    /// Describes what the user should be told about `transition`.
    pub fn new(transition: Transition) -> Self {
        let body = format!(
            "{} — {}",
            started_label(transition.started),
            minutes(transition.length)
        );
        match transition.started {
            // Being called back to work is the one that must survive a
            // do-not-disturb rule, so it is the one marked critical.
            Phase::Work => Self {
                urgency: "critical",
                body,
                sound: sound("bell"),
            },
            Phase::Break | Phase::LongBreak => Self {
                urgency: "normal",
                body,
                sound: sound("complete"),
            },
        }
    }

    /// Returns the arguments `notify-send` is called with.
    pub fn args(&self) -> Vec<String> {
        [
            "-u",
            self.urgency,
            "-t",
            TIMEOUT_MS,
            "-i",
            ICON,
            "pomctl",
            &self.body,
        ]
        .map(str::to_owned)
        .to_vec()
    }
}

/// Fires the alert without holding up the clock.
///
/// Both commands run on a thread of their own so that the several seconds
/// `paplay` takes to finish do not freeze the countdown, and so that the
/// children are reaped rather than left as zombies for the length of a
/// working day. Output goes nowhere: a missing `notify-send` or a broken
/// sound file must not print over the screen the clock is drawn on, and it is
/// no reason to interrupt a session either.
pub fn send(alert: Alert) {
    thread::spawn(move || {
        run("notify-send", &alert.args());
        run("paplay", &[alert.sound]);
    });
}

fn run<S: AsRef<std::ffi::OsStr>>(program: &str, args: &[S]) {
    let _ = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn sound(name: &str) -> String {
    format!("{SOUNDS}/{name}.oga")
}

fn started_label(phase: Phase) -> &'static str {
    match phase {
        Phase::Work => "Back to work",
        Phase::Break => "Break time",
        Phase::LongBreak => "Long break",
    }
}

/// Writes a phase length the way a notification should read it.
///
/// Rounded up, because a notification that says `0 min` would be worse than
/// one off by a few seconds.
fn minutes(length: Duration) -> String {
    let minutes = length.as_secs().div_ceil(60).max(1);
    format!("{minutes} min")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transition(started: Phase, minutes: u64) -> Transition {
        Transition {
            started,
            length: Duration::from_secs(minutes * 60),
        }
    }

    #[test]
    fn the_end_of_work_announces_the_break_and_how_long_it_is() {
        let alert = Alert::new(transition(Phase::Break, 5));
        assert_eq!(alert.body, "Break time — 5 min");
        assert_eq!(alert.urgency, "normal");
        assert!(alert.sound.ends_with("complete.oga"));
    }

    #[test]
    fn a_long_break_is_named_as_one() {
        let alert = Alert::new(transition(Phase::LongBreak, 15));
        assert_eq!(alert.body, "Long break — 15 min");
        assert_eq!(alert.urgency, "normal");
    }

    #[test]
    fn being_called_back_to_work_is_critical_so_it_is_not_held_back() {
        let alert = Alert::new(transition(Phase::Work, 25));
        assert_eq!(alert.body, "Back to work — 25 min");
        assert_eq!(alert.urgency, "critical");
        assert!(alert.sound.ends_with("bell.oga"));
    }

    #[test]
    fn the_body_is_passed_as_one_argument_however_it_is_worded() {
        let alert = Alert::new(transition(Phase::Break, 5));
        let args = alert.args();
        assert_eq!(
            args.last().map(String::as_str),
            Some("Break time — 5 min"),
            "a body split across arguments would show as a truncated message"
        );
        assert!(
            args.contains(&"pomctl".to_owned()),
            "the summary is the app name"
        );
    }

    #[test]
    fn the_urgency_reaches_notify_send_next_to_its_flag() {
        let args = Alert::new(transition(Phase::Work, 25)).args();
        let flag = args
            .iter()
            .position(|arg| arg == "-u")
            .expect("urgency flag");
        assert_eq!(args[flag + 1], "critical");
    }

    #[test]
    fn a_length_of_seconds_never_reads_as_no_time_at_all() {
        let alert = Alert::new(Transition {
            started: Phase::Break,
            length: Duration::from_secs(30),
        });
        assert_eq!(alert.body, "Break time — 1 min");
    }
}
