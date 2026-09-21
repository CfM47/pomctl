//! Reading the command line and running a session from it.

use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

use crate::input::Command;
use crate::notify::{self, Alert};
use crate::render::live::{self, Live, View};
use crate::render::screen::Screen;
use crate::timer::{Plan, Summary, Timer};

/// How long the session waits for a key before redrawing.
///
/// Short enough that a key feels immediate and that the clock never shows a
/// second it has already left, long enough that an idle timer is four wakeups
/// a second rather than a spin.
const TICK: Duration = Duration::from_millis(250);

/// The longest phase that can be asked for, in minutes.
const MAX_MINUTES: u64 = 24 * 60;

const USAGE: &str = "pomctl — a pomodoro timer for the terminal

usage: pomctl [work] [break] [long]

  work    minutes of work in a phase        (default 25)
  break   minutes of the short break        (default 5)
  long    minutes of the long break, taken
          after every fourth work phase     (default 15)

keys: space pause · s skip · q quit";

/// What the command line asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Run a session to this plan.
    Run(Plan),
    /// Print the usage and stop.
    Help,
}

/// Runs whatever the arguments asked for.
pub fn run(arguments: impl Iterator<Item = String>) -> ExitCode {
    match parse(arguments) {
        Ok(Action::Help) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Action::Run(plan)) => {
            session(plan);
            ExitCode::SUCCESS
        }
        Err(complaint) => {
            eprintln!("pomctl: {complaint}\n\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}

/// Reads the arguments as a plan, or says what is wrong with them.
///
/// Durations are positional and optional from the right, so the common case of
/// a longer work phase is `pomctl 50` and nothing else has to be repeated.
pub fn parse(arguments: impl Iterator<Item = String>) -> Result<Action, String> {
    let mut plan = Plan::default();
    let mut lengths = Vec::new();

    for argument in arguments {
        match argument.as_str() {
            "-h" | "--help" => return Ok(Action::Help),
            _ => lengths.push(minutes(&argument)?),
        }
    }

    let slots: [&mut Duration; 3] = [&mut plan.work, &mut plan.short_break, &mut plan.long_break];
    if lengths.len() > slots.len() {
        return Err(format!(
            "expected at most {} durations, got {}",
            slots.len(),
            lengths.len()
        ));
    }
    for (slot, length) in slots.into_iter().zip(lengths) {
        *slot = length;
    }

    Ok(Action::Run(plan))
}

/// Reads one argument as a phase length.
fn minutes(argument: &str) -> Result<Duration, String> {
    let minutes: u64 = argument
        .parse()
        .map_err(|_| format!("{argument:?} is not a number of minutes"))?;

    // Zero would leave a phase that ends the instant it starts, notifying in a
    // loop as fast as the terminal can be drawn.
    if minutes == 0 || minutes > MAX_MINUTES {
        return Err(format!(
            "a phase is between 1 and {MAX_MINUTES} minutes, got {minutes}"
        ));
    }
    Ok(Duration::from_secs(minutes * 60))
}

/// Runs a session, drawing it if there is a terminal to draw on.
fn session(plan: Plan) {
    let mut timer = Timer::start(plan, Instant::now());

    match Screen::enter() {
        Some(screen) => {
            let mut live = Live::new(screen);
            let quit = watched(&mut timer, &mut live);
            // The terminal is handed back before anything is printed to it,
            // or the summary would be painted onto the alternate screen and
            // vanish with it.
            drop(live);
            println!("{}", summary_line(timer.summary(quit)));
        }
        None => logged(&mut timer),
    }
}

/// Draws the session until the user quits, returning the instant they did.
fn watched(timer: &mut Timer, live: &mut Live) -> Instant {
    loop {
        let now = Instant::now();
        announce(timer, now);
        live.show(View::of(timer, now));

        let Some(key) = live.read_key(TICK) else {
            continue;
        };
        // The clock is read again here rather than reused: the key arrived up
        // to a tick after the frame was drawn.
        match Command::from_key(key) {
            Some(Command::Quit) => return Instant::now(),
            Some(Command::Pause) => timer.toggle_pause(Instant::now()),
            Some(Command::Skip) => timer.skip(Instant::now()),
            None => {}
        }
    }
}

/// Runs the session as lines of output, for a pomctl whose stdout is a pipe.
///
/// There is no keyboard to read here and no screen to restore, so the session
/// runs until it is killed and the summary never comes: what a log wants is
/// the phases, which are printed as they start.
fn logged(timer: &mut Timer) -> ! {
    println!("{}", phase_line(timer));
    loop {
        let now = Instant::now();
        if announce(timer, now) {
            println!("{}", phase_line(timer));
        }
        thread::sleep(TICK);
    }
}

/// Moves the cycle on if the phase is over, notifying if it was.
///
/// Returns whether a phase changed.
fn announce(timer: &mut Timer, now: Instant) -> bool {
    match timer.tick(now) {
        Some(transition) => {
            notify::send(Alert::new(transition));
            true
        }
        None => false,
    }
}

fn phase_line(timer: &Timer) -> String {
    format!("{} {}", timer.phase().label(), live::clock(timer.length()))
}

/// Writes what the session came to.
pub fn summary_line(summary: Summary) -> String {
    let minutes = summary.focused.as_secs() / 60;
    let focused = if minutes >= 60 {
        format!("{}h {}m", minutes / 60, minutes % 60)
    } else {
        format!("{minutes}m")
    };
    let pomodoros = match summary.pomodoros {
        1 => "1 pomodoro".to_owned(),
        count => format!("{count} pomodoros"),
    };
    format!("{pomodoros} completed — {focused} focused")
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: Duration = Duration::from_secs(60);

    fn parse_args(arguments: &[&str]) -> Result<Action, String> {
        parse(arguments.iter().map(|argument| (*argument).to_owned()))
    }

    fn plan_of(arguments: &[&str]) -> Plan {
        match parse_args(arguments).expect("the arguments were accepted") {
            Action::Run(plan) => plan,
            Action::Help => panic!("expected a plan, got help"),
        }
    }

    fn summary(pomodoros: u32, focused: Duration) -> Summary {
        Summary { pomodoros, focused }
    }

    #[test]
    fn no_arguments_run_the_classic_plan() {
        assert_eq!(plan_of(&[]), Plan::default());
        assert_eq!(plan_of(&[]).work, 25 * MINUTE);
    }

    #[test]
    fn a_single_duration_only_changes_the_work_phase() {
        let plan = plan_of(&["50"]);
        assert_eq!(plan.work, 50 * MINUTE);
        assert_eq!(plan.short_break, 5 * MINUTE);
        assert_eq!(plan.long_break, 15 * MINUTE);
    }

    #[test]
    fn the_durations_are_read_in_the_order_the_usage_lists_them() {
        let plan = plan_of(&["50", "10", "20"]);
        assert_eq!(plan.work, 50 * MINUTE);
        assert_eq!(plan.short_break, 10 * MINUTE);
        assert_eq!(plan.long_break, 20 * MINUTE);
    }

    #[test]
    fn help_is_asked_for_by_either_spelling() {
        assert_eq!(parse_args(&["-h"]), Ok(Action::Help));
        assert_eq!(parse_args(&["--help"]), Ok(Action::Help));
        assert_eq!(parse_args(&["25", "--help"]), Ok(Action::Help));
    }

    #[test]
    fn something_that_is_not_a_number_is_refused_rather_than_ignored() {
        let complaint = parse_args(&["ten"]).expect_err("\"ten\" is not minutes");
        assert!(complaint.contains("\"ten\""), "got {complaint:?}");
    }

    #[test]
    fn a_zero_length_phase_is_refused() {
        // It would end the instant it began, notifying in a loop.
        assert!(parse_args(&["0"]).is_err());
        assert!(parse_args(&["25", "0"]).is_err());
    }

    #[test]
    fn a_phase_longer_than_a_day_is_refused() {
        assert!(parse_args(&["1440"]).is_ok());
        assert!(parse_args(&["1441"]).is_err());
    }

    #[test]
    fn a_negative_duration_is_refused_as_the_typo_it_is() {
        assert!(parse_args(&["-5"]).is_err());
    }

    #[test]
    fn a_fourth_duration_is_refused_rather_than_dropped() {
        let complaint = parse_args(&["25", "5", "15", "4"]).expect_err("three is the limit");
        assert!(complaint.contains("at most 3"), "got {complaint:?}");
    }

    #[test]
    fn the_summary_reports_the_count_and_the_time() {
        assert_eq!(
            summary_line(summary(3, 82 * MINUTE)),
            "3 pomodoros completed — 1h 22m focused"
        );
    }

    #[test]
    fn one_pomodoro_is_not_reported_in_the_plural() {
        assert_eq!(
            summary_line(summary(1, 25 * MINUTE)),
            "1 pomodoro completed — 25m focused"
        );
    }

    #[test]
    fn quitting_early_reports_nothing_rather_than_pretending() {
        assert_eq!(
            summary_line(summary(0, Duration::from_secs(30))),
            "0 pomodoros completed — 0m focused"
        );
    }

    #[test]
    fn a_whole_number_of_hours_keeps_its_zero_minutes() {
        assert_eq!(
            summary_line(summary(4, 120 * MINUTE)),
            "4 pomodoros completed — 2h 0m focused"
        );
    }
}
