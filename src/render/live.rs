//! Drawing a session while it runs.

use std::time::{Duration, Instant};

use crate::render::screen::Screen;
use crate::render::text;
use crate::timer::{Phase, SESSIONS_PER_CYCLE, Timer};

/// Columns the progress bar is drawn in when there is room for it.
const GAUGE_WIDTH: usize = 32;

/// The narrowest frame the key reminder is worth spending a row on.
const HINT_WIDTH: usize = 34;

const HINT: &str = "space pause · s skip · q quit";

/// A timer as the screen needs to see it.
///
/// Reading the timer once per frame keeps every row of that frame agreeing
/// about the time: taking the clock again between the digits and the gauge
/// could draw a bar that has moved on past the number above it.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub phase: Phase,
    pub session: u32,
    pub remaining: Duration,
    pub length: Duration,
    pub paused: bool,
}

impl View {
    /// Takes the reading a frame is drawn from.
    pub fn of(timer: &Timer, now: Instant) -> Self {
        Self {
            phase: timer.phase(),
            session: timer.session(),
            remaining: timer.remaining(now),
            length: timer.length(),
            paused: timer.is_paused(),
        }
    }
}

/// Paints a session onto a terminal it has taken over.
pub struct Live {
    screen: Screen,
}

impl Live {
    /// Draws onto `screen` until dropped, restoring the terminal with it.
    pub fn new(screen: Screen) -> Self {
        Self { screen }
    }

    /// Waits up to `timeout` for a key, returning it unread.
    pub fn read_key(&self, timeout: Duration) -> Option<crossterm::event::KeyEvent> {
        self.screen.read_key(timeout)
    }

    /// Redraws the frame for `view`.
    ///
    /// A terminal that has gone away mid-session is ignored rather than
    /// reported: losing the display is no reason to abandon a cycle that is
    /// otherwise still running.
    pub fn show(&mut self, view: View) {
        let (width, height) = self.screen.size();
        let _ = self
            .screen
            .paint(&text::center(&frame(view, width), width, height));
    }
}

/// Writes how long is left as a clock reads it.
///
/// Rounded up, so a phase opens on its full length and the last second is
/// shown as `00:01` rather than as a zero the timer has not reached yet.
pub fn clock(remaining: Duration) -> String {
    let seconds = remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0);
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

/// Names the phase, and which of the cycle it is.
fn label(view: View) -> String {
    match view.phase {
        Phase::Work => format!("WORK {}/{SESSIONS_PER_CYCLE}", view.session),
        other => other.label().to_uppercase(),
    }
}

/// Builds the rows of one frame, widest row no wider than `width`.
fn frame(view: View, width: u16) -> Vec<String> {
    let time = clock(view.remaining);
    let width = width as usize;
    let mut block = vec![label(view)];

    let (font, drawn_width) = (text::font(), text::enlarged_width(&time));
    if font.can_draw(&time) && drawn_width <= width {
        block.push(String::new());
        block.extend(text::enlarge(&time));
        block.push(String::new());
    } else {
        // Wrapping the art would look like a fault, so the display gives up
        // size rather than the reading.
        block.push(time);
    }

    block.push(gauge(elapsed_fraction(view), width));

    // Only worth a row once there is something to say: an unpaused clock is
    // already telling the user it is running by counting down.
    if view.paused {
        block.push(String::new());
        block.push("PAUSED".to_owned());
    }

    if width >= HINT_WIDTH {
        block.push(String::new());
        block.push(HINT.to_owned());
    }

    block
}

/// Returns how much of the phase has gone, as a fraction.
fn elapsed_fraction(view: View) -> f64 {
    if view.length.is_zero() {
        return 1.0;
    }
    let gone = view.length.saturating_sub(view.remaining);
    (gone.as_secs_f64() / view.length.as_secs_f64()).clamp(0.0, 1.0)
}

/// Draws a bar of `filled` progress, narrowed to fit a `width` column frame.
fn gauge(filled: f64, width: usize) -> String {
    let columns = GAUGE_WIDTH.min(width);
    let full = (filled * columns as f64).round() as usize;
    format!(
        "{}{}",
        "█".repeat(full),
        "░".repeat(columns.saturating_sub(full))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: Duration = Duration::from_secs(60);

    fn view(phase: Phase, remaining: Duration, length: Duration) -> View {
        View {
            phase,
            session: 2,
            remaining,
            length,
            paused: false,
        }
    }

    fn working() -> View {
        view(
            Phase::Work,
            12 * MINUTE + Duration::from_secs(7),
            25 * MINUTE,
        )
    }

    #[test]
    fn a_full_phase_reads_as_its_whole_length() {
        assert_eq!(clock(25 * MINUTE), "25:00");
    }

    #[test]
    fn a_part_second_is_rounded_up_so_the_clock_never_shows_a_zero_early() {
        assert_eq!(clock(Duration::from_millis(200)), "00:01");
        assert_eq!(clock(Duration::from_millis(59_500)), "01:00");
    }

    #[test]
    fn a_spent_phase_reads_as_zero() {
        assert_eq!(clock(Duration::ZERO), "00:00");
    }

    #[test]
    fn an_hour_long_phase_keeps_counting_in_minutes() {
        assert_eq!(clock(100 * MINUTE), "100:00");
    }

    #[test]
    fn a_work_phase_says_where_it_sits_in_the_cycle() {
        assert_eq!(label(working()), "WORK 2/4");
    }

    #[test]
    fn a_break_is_named_without_a_count_it_does_not_have() {
        assert_eq!(label(view(Phase::Break, MINUTE, 5 * MINUTE)), "BREAK");
        assert_eq!(
            label(view(Phase::LongBreak, MINUTE, 15 * MINUTE)),
            "LONG BREAK"
        );
    }

    #[test]
    fn a_running_phase_is_drawn_large() {
        let drawn = frame(working(), 80);
        assert_eq!(drawn[0], "WORK 2/4");
        assert!(
            drawn.iter().any(|line| line.contains('█')),
            "the time is shown as block digits"
        );
    }

    #[test]
    fn a_window_too_narrow_for_the_art_still_shows_the_time() {
        let drawn = frame(working(), 20);
        assert!(
            drawn.iter().any(|line| line.contains("12:07")),
            "giving up the block digits must not mean giving up the clock"
        );
    }

    #[test]
    fn no_frame_is_wider_than_the_screen_it_was_built_for() {
        for width in [10u16, 20, 34, 40, 80, 200] {
            for paused in [false, true] {
                let drawn = frame(
                    View {
                        paused,
                        ..working()
                    },
                    width,
                );
                let widest = drawn.iter().map(|line| line.chars().count()).max().unwrap();
                assert!(
                    widest <= width as usize,
                    "a {width} column screen was given a {widest} column frame, which wraps"
                );
            }
        }
    }

    #[test]
    fn a_narrow_frame_drops_the_hint_before_it_drops_the_clock() {
        let drawn = frame(working(), 20);
        assert!(!drawn.iter().any(|line| line.contains("pause")));
        assert!(drawn.iter().any(|line| line.contains("12:07")));
    }

    #[test]
    fn a_wide_frame_says_what_the_keys_do() {
        let drawn = frame(working(), 80);
        assert!(drawn.contains(&HINT.to_owned()));
    }

    #[test]
    fn a_paused_clock_says_so_on_the_screen() {
        let running = frame(working(), 80);
        let paused = frame(
            View {
                paused: true,
                ..working()
            },
            80,
        );

        assert!(paused.contains(&"PAUSED".to_owned()));
        assert!(
            !running.contains(&"PAUSED".to_owned()),
            "a running clock must not claim to be paused"
        );
    }

    #[test]
    fn the_gauge_fills_as_the_phase_goes_by() {
        let start = elapsed_fraction(view(Phase::Work, 25 * MINUTE, 25 * MINUTE));
        let middle = elapsed_fraction(view(
            Phase::Work,
            12 * MINUTE + 30 * Duration::from_secs(1),
            25 * MINUTE,
        ));
        let end = elapsed_fraction(view(Phase::Work, Duration::ZERO, 25 * MINUTE));

        assert_eq!(start, 0.0);
        assert!(
            (middle - 0.5).abs() < 0.01,
            "half way through, got {middle}"
        );
        assert_eq!(end, 1.0);
    }

    #[test]
    fn the_gauge_is_the_same_width_however_full_it_is() {
        for filled in [0.0, 0.33, 0.5, 1.0] {
            assert_eq!(
                gauge(filled, 80).chars().count(),
                GAUGE_WIDTH,
                "a bar that changes width would make the frame jitter"
            );
        }
    }

    #[test]
    fn the_gauge_narrows_to_fit_a_small_window() {
        assert_eq!(gauge(0.5, 10).chars().count(), 10);
    }

    #[test]
    fn a_phase_of_no_length_shows_a_full_gauge_rather_than_dividing_by_zero() {
        let fraction = elapsed_fraction(view(Phase::Work, Duration::ZERO, Duration::ZERO));
        assert_eq!(fraction, 1.0);
    }
}
