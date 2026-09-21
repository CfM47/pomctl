//! Reading the command line and running a session from it.

use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

use clap::Parser;

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

/// Phase lengths, in minutes, as the command line gives them.
///
/// Positional rather than flagged, and optional from the right, so the common
/// case of a longer work phase is `pomctl 50` with nothing else repeated.
#[derive(Debug, Parser, PartialEq, Eq)]
#[command(
    version,
    about = "A pomodoro timer for the terminal",
    // Without this the doc comment above becomes the long help, putting the
    // reasoning behind the argument shape in front of the user.
    long_about = None,
    after_help = "keys: space pause · s skip · q quit"
)]
pub struct Cli {
    /// Minutes of work in a phase
    #[arg(default_value_t = 25, value_parser = phase_length())]
    work: u64,

    /// Minutes of the short break after a work phase
    #[arg(value_name = "BREAK", default_value_t = 5, value_parser = phase_length())]
    short_break: u64,

    /// Minutes of the long break, taken after every fourth work phase
    #[arg(value_name = "LONG", default_value_t = 15, value_parser = phase_length())]
    long_break: u64,
}

impl Cli {
    /// Returns the plan the arguments describe.
    pub fn plan(&self) -> Plan {
        Plan {
            work: minutes(self.work),
            short_break: minutes(self.short_break),
            long_break: minutes(self.long_break),
        }
    }
}

/// Accepts a phase length a session can actually be run to.
///
/// Zero would leave a phase that ends the instant it starts, notifying in a
/// loop as fast as the terminal can be drawn, so the floor is one minute
/// rather than none.
fn phase_length() -> clap::builder::RangedU64ValueParser {
    clap::value_parser!(u64).range(1..=MAX_MINUTES)
}

fn minutes(count: u64) -> Duration {
    Duration::from_secs(count * 60)
}

/// Runs a session to the plan on the command line.
pub fn run(arguments: Cli) -> ExitCode {
    session(arguments.plan());
    ExitCode::SUCCESS
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
    use clap::CommandFactory;

    const MINUTE: Duration = Duration::from_secs(60);

    fn parse(arguments: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_from(std::iter::once("pomctl").chain(arguments.iter().copied()))
    }

    fn plan_of(arguments: &[&str]) -> Plan {
        parse(arguments)
            .expect("the arguments were accepted")
            .plan()
    }

    fn complaint(arguments: &[&str]) -> String {
        parse(arguments)
            .expect_err("the arguments were refused")
            .to_string()
    }

    fn summary(pomodoros: u32, focused: Duration) -> Summary {
        Summary { pomodoros, focused }
    }

    #[test]
    fn the_command_definition_holds_together() {
        Cli::command().debug_assert();
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
    fn something_that_is_not_a_number_is_refused_rather_than_ignored() {
        assert!(complaint(&["ten"]).contains("ten"));
    }

    #[test]
    fn a_zero_length_phase_is_refused() {
        // It would end the instant it began, notifying in a loop.
        assert!(parse(&["0"]).is_err());
        assert!(parse(&["25", "0"]).is_err());
    }

    #[test]
    fn a_phase_longer_than_a_day_is_refused() {
        assert!(parse(&["1440"]).is_ok());
        assert!(parse(&["1441"]).is_err());
    }

    #[test]
    fn a_refused_duration_is_told_what_it_should_have_been() {
        let told = complaint(&["0"]);
        assert!(
            told.contains("1") && told.contains(&MAX_MINUTES.to_string()),
            "a range error should name the range, got {told:?}"
        );
    }

    #[test]
    fn a_negative_duration_is_refused_as_the_typo_it_is() {
        assert!(parse(&["-5"]).is_err());
    }

    #[test]
    fn a_fourth_duration_is_refused_rather_than_dropped() {
        assert!(parse(&["25", "5", "15", "4"]).is_err());
    }

    #[test]
    fn help_and_version_are_answered_rather_than_run() {
        for argument in ["-h", "--help", "-V", "--version"] {
            let kind = parse(&[argument])
                .expect_err("help and version stop before a session starts")
                .kind();
            assert!(
                matches!(
                    kind,
                    clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
                ),
                "{argument} gave {kind:?}"
            );
        }
    }

    #[test]
    fn the_help_says_what_the_keys_do() {
        let help = Cli::command().render_long_help().to_string();
        assert!(help.contains("space pause"), "got {help}");
    }

    #[test]
    fn the_help_describes_the_timer_rather_than_its_argument_shape() {
        let help = Cli::command().render_long_help().to_string();
        assert!(help.contains("A pomodoro timer for the terminal"));
        assert!(
            !help.contains("Positional rather than flagged"),
            "the doc comment is for whoever reads the code, got {help}"
        );
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
