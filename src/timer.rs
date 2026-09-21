//! Running work and break phases in order, for as long as someone watches.

use std::time::{Duration, Instant};

/// Work phases between long breaks.
pub const SESSIONS_PER_CYCLE: u32 = 4;

/// What the current phase is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Heads down.
    Work,
    /// The short break after a work phase.
    Break,
    /// The long break that closes a cycle.
    LongBreak,
}

impl Phase {
    /// Returns the phase as it is written on screen and in notifications.
    pub fn label(self) -> &'static str {
        match self {
            Self::Work => "work",
            Self::Break => "break",
            Self::LongBreak => "long break",
        }
    }

    /// Returns whether time spent here counts as focused.
    pub fn is_work(self) -> bool {
        self == Self::Work
    }
}

/// How long each kind of phase lasts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    pub work: Duration,
    pub short_break: Duration,
    pub long_break: Duration,
}

impl Plan {
    /// Returns how long `phase` runs for under this plan.
    pub fn length(&self, phase: Phase) -> Duration {
        match phase {
            Phase::Work => self.work,
            Phase::Break => self.short_break,
            Phase::LongBreak => self.long_break,
        }
    }
}

impl Default for Plan {
    fn default() -> Self {
        Self {
            work: Duration::from_secs(25 * 60),
            short_break: Duration::from_secs(5 * 60),
            long_break: Duration::from_secs(15 * 60),
        }
    }
}

/// One phase giving way to the next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Transition {
    /// The phase whose time ran out.
    pub finished: Phase,
    /// The phase now running.
    pub started: Phase,
    /// How long the new phase lasts, for saying so in the notification.
    pub length: Duration,
}

/// What a session amounted to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Summary {
    /// Work phases that ran all the way down.
    pub pomodoros: u32,
    /// Time spent in work phases, paused time excluded.
    pub focused: Duration,
}

/// A pomodoro cycle in progress.
///
/// Every method that cares about time takes `now` rather than reading the
/// clock itself. The phase end is stored as time remaining against the last
/// instant the timer was looked at, so a long frame or a slow notification
/// cannot make the countdown drift the way accumulated sleeps would, and a
/// test can run a whole day of cycles without waiting for one.
pub struct Timer {
    plan: Plan,
    phase: Phase,
    session: u32,
    length: Duration,
    remaining: Duration,
    /// The instant `remaining` was measured at, or `None` while paused.
    since: Option<Instant>,
    pomodoros: u32,
    focused: Duration,
}

impl Timer {
    /// Starts a cycle on its first work phase.
    pub fn start(plan: Plan, now: Instant) -> Self {
        Self {
            plan,
            phase: Phase::Work,
            session: 1,
            length: plan.work,
            remaining: plan.work,
            since: Some(now),
            pomodoros: 0,
            focused: Duration::ZERO,
        }
    }

    /// Returns the phase now running.
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Returns which work phase of the cycle this is, counting from one.
    ///
    /// A break reports the work phase it follows, so the label on screen does
    /// not jump forward before the work it refers to has been done.
    pub fn session(&self) -> u32 {
        self.session
    }

    /// Returns whether the clock is stopped.
    pub fn is_paused(&self) -> bool {
        self.since.is_none()
    }

    /// Returns how long the current phase lasts in full.
    pub fn length(&self) -> Duration {
        self.length
    }

    /// Returns how much of the current phase is left.
    pub fn remaining(&self, now: Instant) -> Duration {
        match self.since {
            None => self.remaining,
            Some(since) => self.remaining.saturating_sub(now.saturating_duration_since(since)),
        }
    }

    /// Advances the cycle if the current phase has run out.
    ///
    /// Returns the transition to announce, or `None` while there is time left.
    /// The next phase starts from its full length rather than from whatever is
    /// left over, so a frame that arrives late shortens nothing.
    pub fn tick(&mut self, now: Instant) -> Option<Transition> {
        if self.is_paused() || !self.remaining(now).is_zero() {
            return None;
        }

        let finished = self.phase;
        if finished.is_work() {
            self.pomodoros += 1;
            self.focused += self.length;
        }
        self.enter(self.next_phase(), now);

        Some(Transition {
            finished,
            started: self.phase,
            length: self.length,
        })
    }

    /// Stops the clock, or starts it again.
    pub fn toggle_pause(&mut self, now: Instant) {
        match self.since {
            Some(_) => {
                self.remaining = self.remaining(now);
                self.since = None;
            }
            None => self.since = Some(now),
        }
    }

    /// Ends the current phase early and moves on.
    ///
    /// A skipped work phase is not a pomodoro, though the time already spent
    /// in it still counts as focused: it was worked, just not to the bell.
    pub fn skip(&mut self, now: Instant) {
        if self.phase.is_work() {
            self.focused += self.length - self.remaining(now);
        }
        self.enter(self.next_phase(), now);
    }

    /// Returns what the session has amounted to so far.
    ///
    /// A work phase in progress contributes the time already spent in it, so
    /// quitting twenty minutes in does not report zero.
    pub fn summary(&self, now: Instant) -> Summary {
        let mut focused = self.focused;
        if self.phase.is_work() {
            focused += self.length - self.remaining(now);
        }
        Summary {
            pomodoros: self.pomodoros,
            focused,
        }
    }

    fn next_phase(&self) -> Phase {
        match self.phase {
            Phase::Work if self.session >= SESSIONS_PER_CYCLE => Phase::LongBreak,
            Phase::Work => Phase::Break,
            Phase::Break | Phase::LongBreak => Phase::Work,
        }
    }

    fn enter(&mut self, phase: Phase, now: Instant) {
        self.session = match (self.phase, phase) {
            (Phase::Break, Phase::Work) => self.session + 1,
            (Phase::LongBreak, Phase::Work) => 1,
            _ => self.session,
        };
        self.phase = phase;
        self.length = self.plan.length(phase);
        self.remaining = self.length;
        // A phase entered while paused would sit at its full length with the
        // clock stopped, which reads as the timer having hung.
        self.since = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: Duration = Duration::from_secs(60);

    fn plan() -> Plan {
        Plan {
            work: 25 * MINUTE,
            short_break: 5 * MINUTE,
            long_break: 15 * MINUTE,
        }
    }

    /// Runs `timer` to the end of the current phase and returns the transition.
    fn finish_phase(timer: &mut Timer, at: &mut Instant) -> Transition {
        *at += timer.remaining(*at);
        timer.tick(*at).expect("the phase ran out")
    }

    #[test]
    fn a_session_opens_on_the_first_work_phase() {
        let timer = Timer::start(plan(), Instant::now());
        assert_eq!(timer.phase(), Phase::Work);
        assert_eq!(timer.session(), 1);
        assert!(!timer.is_paused());
    }

    #[test]
    fn the_countdown_falls_with_the_clock() {
        let start = Instant::now();
        let timer = Timer::start(plan(), start);
        assert_eq!(timer.remaining(start), 25 * MINUTE);
        assert_eq!(timer.remaining(start + 10 * MINUTE), 15 * MINUTE);
    }

    #[test]
    fn a_phase_with_time_left_announces_nothing() {
        let start = Instant::now();
        let mut timer = Timer::start(plan(), start);
        assert_eq!(timer.tick(start + 24 * MINUTE), None);
        assert_eq!(timer.phase(), Phase::Work);
    }

    #[test]
    fn work_gives_way_to_a_break_and_the_break_to_the_next_work() {
        let mut at = Instant::now();
        let mut timer = Timer::start(plan(), at);

        let to_break = finish_phase(&mut timer, &mut at);
        assert_eq!(to_break.finished, Phase::Work);
        assert_eq!(to_break.started, Phase::Break);
        assert_eq!(to_break.length, 5 * MINUTE);
        assert_eq!(timer.session(), 1, "the break belongs to the work before it");

        let to_work = finish_phase(&mut timer, &mut at);
        assert_eq!(to_work.started, Phase::Work);
        assert_eq!(to_work.length, 25 * MINUTE);
        assert_eq!(timer.session(), 2);
    }

    #[test]
    fn the_fourth_work_phase_earns_the_long_break() {
        let mut at = Instant::now();
        let mut timer = Timer::start(plan(), at);

        let mut phases = Vec::new();
        // Four work phases and the breaks between them, then the long break.
        for _ in 0..8 {
            phases.push(finish_phase(&mut timer, &mut at).started);
        }

        assert_eq!(
            phases,
            vec![
                Phase::Break,
                Phase::Work,
                Phase::Break,
                Phase::Work,
                Phase::Break,
                Phase::Work,
                Phase::LongBreak,
                Phase::Work,
            ]
        );
        assert_eq!(timer.length(), 25 * MINUTE);
        assert_eq!(
            timer.session(),
            1,
            "the cycle starts over after the long break"
        );
    }

    #[test]
    fn a_phase_that_ended_late_still_gets_its_full_length() {
        let start = Instant::now();
        let mut timer = Timer::start(plan(), start);
        // The process was starved past the end of the work phase.
        let late = start + 25 * MINUTE + Duration::from_secs(90);

        timer.tick(late).expect("the phase ran out");
        assert_eq!(
            timer.remaining(late),
            5 * MINUTE,
            "the overshoot must not be taken out of the break"
        );
    }

    #[test]
    fn a_paused_clock_holds_its_reading_and_resumes_from_it() {
        let start = Instant::now();
        let mut timer = Timer::start(plan(), start);

        timer.toggle_pause(start + MINUTE);
        assert!(timer.is_paused());
        assert_eq!(timer.remaining(start + 10 * MINUTE), 24 * MINUTE);

        timer.toggle_pause(start + 10 * MINUTE);
        assert!(!timer.is_paused());
        assert_eq!(timer.remaining(start + 11 * MINUTE), 23 * MINUTE);
    }

    #[test]
    fn a_paused_phase_never_runs_out_on_its_own() {
        let start = Instant::now();
        let mut timer = Timer::start(plan(), start);
        timer.toggle_pause(start);

        assert_eq!(timer.tick(start + 10 * 25 * MINUTE), None);
        assert_eq!(timer.phase(), Phase::Work);
    }

    #[test]
    fn skipping_moves_on_without_crediting_a_pomodoro() {
        let start = Instant::now();
        let mut timer = Timer::start(plan(), start);

        timer.skip(start + 10 * MINUTE);
        assert_eq!(timer.phase(), Phase::Break);
        let summary = timer.summary(start + 10 * MINUTE);
        assert_eq!(summary.pomodoros, 0);
        assert_eq!(
            summary.focused,
            10 * MINUTE,
            "work done before a skip was still work"
        );
    }

    #[test]
    fn a_skip_restarts_the_clock_even_from_a_pause() {
        let start = Instant::now();
        let mut timer = Timer::start(plan(), start);
        timer.toggle_pause(start + MINUTE);
        timer.skip(start + MINUTE);

        assert!(
            !timer.is_paused(),
            "a phase entered paused would look like a hung timer"
        );
        assert_eq!(timer.remaining(start + 2 * MINUTE), 4 * MINUTE);
    }

    #[test]
    fn the_summary_counts_finished_work_phases_only() {
        let mut at = Instant::now();
        let mut timer = Timer::start(plan(), at);

        for _ in 0..3 {
            finish_phase(&mut timer, &mut at);
        }

        let summary = timer.summary(at);
        assert_eq!(summary.pomodoros, 2);
        assert_eq!(summary.focused, 50 * MINUTE);
    }

    #[test]
    fn time_on_the_clock_counts_before_the_phase_is_over() {
        let start = Instant::now();
        let timer = Timer::start(plan(), start);
        assert_eq!(timer.summary(start + 20 * MINUTE).focused, 20 * MINUTE);
    }

    #[test]
    fn a_break_adds_nothing_to_the_focused_total() {
        let mut at = Instant::now();
        let mut timer = Timer::start(plan(), at);
        finish_phase(&mut timer, &mut at);

        let during_break = at + 3 * MINUTE;
        assert_eq!(timer.summary(during_break).focused, 25 * MINUTE);
    }

    #[test]
    fn paused_time_is_not_focused_time() {
        let start = Instant::now();
        let mut timer = Timer::start(plan(), start);

        timer.toggle_pause(start + 5 * MINUTE);
        timer.toggle_pause(start + 65 * MINUTE);

        assert_eq!(
            timer.summary(start + 70 * MINUTE).focused,
            10 * MINUTE,
            "an hour away from the desk must not be reported as focus"
        );
    }
}
