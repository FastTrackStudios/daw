//! The play cursor, the edit cursor, the time selection, and where Stop
//! leaves the cursor.
//!
//! Three different things that all look like a vertical line, and
//! confusing them is the classic DAW bug:
//!
//! - the **play cursor** is where the audio is. It moves on its own and
//!   you cannot put it anywhere; you can only ask the transport to.
//! - the **edit cursor** is where YOU are. It moves when you click and
//!   stays where you left it.
//! - the **time selection** is a span, and it is what most commands act
//!   on when it exists.
//!
//! # Why the play cursor extrapolates
//!
//! The engine reports its position once per audio block — every few
//! milliseconds, and not on a schedule the display shares. Drawn
//! straight from that number, the cursor advances in steps of a block
//! however fast the window presents, which reads as stutter on a 144 Hz
//! display and is the whole reason REAPER users install
//! `ReaSmoothPlayhead` to replace the native one.
//!
//! So the position is a line, not a point: the last report, plus the
//! time since it arrived multiplied by the rate. Between blocks the
//! cursor glides; when the next block lands it corrects. The correction
//! is sub-pixel as long as the extrapolation is honest about the rate,
//! which is why [`Playhead::seek`] exists — a jump is told to the
//! cursor rather than inferred from a position that moved too far.

use std::time::Duration;
use web_time::Instant;

/// Where the audio is, as something that can be asked at any moment.
#[derive(Clone, Copy, Debug)]
pub struct Playhead {
    /// The last position the engine reported, in seconds.
    reported: f64,
    /// When it reported it.
    at: Instant,
    /// Seconds of project per second of wall clock. Zero when stopped;
    /// not always one, because a transport can play at half speed.
    rate: f64,
    playing: bool,
}

impl Playhead {
    #[must_use]
    pub fn stopped(at_seconds: f64) -> Self {
        Self {
            reported: at_seconds,
            at: Instant::now(),
            rate: 0.0,
            playing: false,
        }
    }

    /// A position from the engine.
    ///
    /// Called once per audio block. Everything between two of these is
    /// the extrapolation's job.
    pub fn report(&mut self, seconds: f64, rate: f64, now: Instant) {
        self.reported = seconds;
        self.at = now;
        self.rate = if self.playing { rate } else { 0.0 };
    }

    /// Start or stop moving.
    pub fn set_playing(&mut self, playing: bool, rate: f64, now: Instant) {
        // Freeze where the cursor IS, not where it was last reported —
        // stopping otherwise snaps it backwards by up to a block, which
        // is the most visible glitch a transport can have.
        self.reported = self.at_time(now);
        self.at = now;
        self.playing = playing;
        self.rate = if playing { rate } else { 0.0 };
    }

    /// Jump, without the jump being mistaken for playback.
    ///
    /// A seek reports a position far from the extrapolated one. Told
    /// about it, the cursor moves and carries on; left to infer it, the
    /// next frame would show a cursor that had apparently played several
    /// seconds in one frame.
    pub fn seek(&mut self, seconds: f64, now: Instant) {
        self.reported = seconds;
        self.at = now;
    }

    /// Where to draw it, now.
    #[must_use]
    pub fn at_time(&self, now: Instant) -> f64 {
        if !self.playing {
            return self.reported;
        }
        let elapsed = now.saturating_duration_since(self.at).as_secs_f64();
        // Capped, so a stalled engine does not send the cursor off down
        // the timeline. A gap this long is a dropout, and a cursor
        // frozen at the last real position says so where one sailing
        // away says nothing.
        let elapsed = elapsed.min(MAX_EXTRAPOLATION.as_secs_f64());
        self.rate.mul_add(elapsed, self.reported)
    }

    #[must_use]
    pub const fn playing(&self) -> bool {
        self.playing
    }
}

/// How far ahead of the last report the cursor will ever be drawn.
///
/// Generous next to an audio block (a few milliseconds) and short next
/// to a hang. Anything longer is not latency, it is the engine having
/// stopped talking.
pub const MAX_EXTRAPOLATION: Duration = Duration::from_millis(250);

/// A span of the timeline, in the surface's units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimeSpan {
    pub start: f64,
    pub end: f64,
}

impl TimeSpan {
    /// A span from two points, in either order.
    ///
    /// Dragging right-to-left is the same selection as left-to-right,
    /// and every consumer wants `start <= end`.
    #[must_use]
    pub fn between(a: f64, b: f64) -> Self {
        Self {
            start: a.min(b).max(0.0),
            end: a.max(b).max(0.0),
        }
    }

    #[must_use]
    pub fn length(self) -> f64 {
        self.end - self.start
    }

    /// Whether this is a real selection or an accidental click.
    ///
    /// A click that moves two pixels is a click, not a drag, and a
    /// selection of four milliseconds is one nobody meant to make —
    /// commands that act on the time selection would act on nothing.
    #[must_use]
    pub fn is_meaningful(self) -> bool {
        self.length() > MIN_SELECTION
    }
}

/// The shortest span treated as a selection rather than a click, in
/// seconds (the arrangement's units).
pub const MIN_SELECTION: f64 = 0.01;

/// Where the user is on the timeline.
#[derive(Clone, Copy, Debug, Default)]
pub struct EditCursor {
    /// The edit cursor, in seconds.
    pub at: f64,
    /// The time selection, when there is one.
    pub selection: Option<TimeSpan>,
}

impl EditCursor {
    /// Click: move the cursor, drop the selection.
    ///
    /// REAPER's behaviour, and the one that makes a stale selection
    /// impossible to leave lying around — a command acting on a
    /// selection you had forgotten is worse than one acting on nothing.
    pub fn click(&mut self, seconds: f64) {
        self.at = seconds.max(0.0);
        self.selection = None;
    }

    /// Drag: select, and take the cursor to the start.
    ///
    /// The cursor goes to the START rather than to where the mouse
    /// ended, because what you do next is usually play what you just
    /// selected.
    pub fn drag(&mut self, from: f64, to: f64) {
        let span = TimeSpan::between(from, to);
        if span.is_meaningful() {
            self.at = span.start;
            self.selection = Some(span);
        } else {
            self.click(from);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// The point of the whole module: between two engine reports the
    /// cursor keeps moving, so a 144 Hz window does not draw the same
    /// position several frames running.
    #[test]
    fn the_cursor_glides_between_reports() {
        let start = Instant::now();
        let mut head = Playhead::stopped(0.0);
        head.set_playing(true, 1.0, start);
        head.report(10.0, 1.0, start);

        let a = head.at_time(start + ms(3));
        let b = head.at_time(start + ms(6));
        assert!(a > 10.0 && b > a, "the cursor stalled: {a} then {b}");
        // One second of project per second of wall clock.
        assert!((b - 10.006).abs() < 1e-9, "drifted: {b}");
    }

    /// And it respects the rate — a transport at half speed moves the
    /// cursor at half speed, which a naive "advance by frame time" does
    /// not.
    #[test]
    fn it_follows_the_transport_rate() {
        let start = Instant::now();
        let mut head = Playhead::stopped(0.0);
        head.set_playing(true, 0.5, start);
        head.report(4.0, 0.5, start);
        assert!((head.at_time(start + ms(100)) - 4.05).abs() < 1e-9);
    }

    /// A stopped cursor does not move, however long the frame took.
    #[test]
    fn a_stopped_cursor_stays_put() {
        let start = Instant::now();
        let mut head = Playhead::stopped(7.5);
        assert!((head.at_time(start + ms(500)) - 7.5).abs() < f64::EPSILON);
        head.set_playing(true, 1.0, start);
        head.set_playing(false, 1.0, start + ms(200));
        let stopped_at = head.at_time(start + ms(200));
        assert!(
            (head.at_time(start + ms(900)) - stopped_at).abs() < f64::EPSILON,
            "a stopped cursor drifted"
        );
    }

    /// Stopping freezes the cursor where it was DRAWN, not where it was
    /// last reported — otherwise it jumps backwards by up to a block
    /// every time you hit stop.
    #[test]
    fn stopping_does_not_snap_backwards() {
        let start = Instant::now();
        let mut head = Playhead::stopped(0.0);
        head.set_playing(true, 1.0, start);
        head.report(2.0, 1.0, start);

        let shown = head.at_time(start + ms(40));
        head.set_playing(false, 1.0, start + ms(40));
        assert!(
            (head.at_time(start + ms(40)) - shown).abs() < 1e-9,
            "stop moved the cursor from {shown} to {}",
            head.at_time(start + ms(40))
        );
        assert!(shown > 2.0, "nothing was extrapolated to preserve");
    }

    /// An engine that stops reporting freezes the cursor rather than
    /// sending it down the timeline — a dropout should look like one.
    #[test]
    fn a_silent_engine_does_not_run_away() {
        let start = Instant::now();
        let mut head = Playhead::stopped(0.0);
        head.set_playing(true, 1.0, start);
        head.report(1.0, 1.0, start);

        let far = head.at_time(start + Duration::from_secs(30));
        assert!(
            far <= 1.0 + MAX_EXTRAPOLATION.as_secs_f64() + 1e-9,
            "ran away to {far}"
        );
    }

    /// A seek is told, not inferred.
    #[test]
    fn a_seek_moves_without_being_taken_for_playback() {
        let start = Instant::now();
        let mut head = Playhead::stopped(0.0);
        head.set_playing(true, 1.0, start);
        head.report(1.0, 1.0, start);
        head.seek(90.0, start + ms(5));
        assert!((head.at_time(start + ms(5)) - 90.0).abs() < 1e-9);
    }

    /// A span is the same span dragged either way.
    #[test]
    fn a_selection_has_no_direction() {
        assert_eq!(TimeSpan::between(9.0, 2.0), TimeSpan::between(2.0, 9.0));
        assert!((TimeSpan::between(9.0, 2.0).length() - 7.0).abs() < f64::EPSILON);
    }

    /// And it never starts before the project does.
    #[test]
    fn a_selection_cannot_be_negative() {
        let span = TimeSpan::between(-5.0, 3.0);
        assert!(span.start >= 0.0 && span.end >= 0.0);
    }

    /// A click clears the selection; a drag makes one and parks the
    /// cursor at its start, because what you do next is play it.
    #[test]
    fn clicking_clears_and_dragging_selects() {
        let mut edit = EditCursor::default();
        edit.drag(4.0, 9.0);
        assert_eq!(edit.selection, Some(TimeSpan::between(4.0, 9.0)));
        assert!((edit.at - 4.0).abs() < f64::EPSILON);

        edit.click(6.0);
        assert_eq!(edit.selection, None);
        assert!((edit.at - 6.0).abs() < f64::EPSILON);
    }

    /// A drag too short to mean anything is a click — otherwise every
    /// click leaves a four-millisecond selection behind for the next
    /// command to act on.
    #[test]
    fn a_twitch_is_a_click() {
        let mut edit = EditCursor::default();
        edit.drag(3.0, 3.0 + MIN_SELECTION / 2.0);
        assert_eq!(edit.selection, None);
        assert!((edit.at - 3.0).abs() < f64::EPSILON);
    }
}

// ── where Stop leaves the cursor ────────────────────────────────────────
//
// "Return to where play started" is REAPER's "stop moves the edit cursor
// back to the last start position" — start playing at bar 5, stop, and
// the cursor is at bar 5 again, so a passage can be played over and over
// without going back to place the cursor each time. A locate made while
// playing (a click on the ruler, a jump to a section) is where play
// starts from now, so it is what Stop returns to.

/// What Stop does with the cursor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StopMode {
    /// Stop where it is.
    Stay,
    /// Stop and go back to where play started.
    Return,
}

impl StopMode {
    /// Every behaviour, in the order the menu lists them.
    pub const ALL: [Self; 2] = [Self::Stay, Self::Return];

    /// The name the Action List and the saved file use.
    #[must_use]
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Stay => "stay",
            Self::Return => "return",
        }
    }

    /// What the menu calls it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Stay => "Stop where it is",
            Self::Return => "Return to where play started",
        }
    }

    /// The behaviour a slug names.
    #[must_use]
    pub fn from_slug(slug: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|each| each.slug().eq_ignore_ascii_case(slug.trim()))
    }
}

/// What the transport is to do for a press.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum TransportStep {
    /// Start playing from where the cursor is.
    Play,
    /// Stop where it is.
    Stop,
    /// Stop, and put the cursor at this time.
    StopAt(f64),
}

/// Where play was last started from, while it is playing.
#[derive(Clone, Copy, PartialEq, Default, Debug)]
pub struct Locate {
    start: Option<f64>,
}

impl Locate {
    /// Nothing remembered.
    #[must_use]
    pub const fn new() -> Self {
        Self { start: None }
    }

    /// Play/stop pressed, with the transport `playing` or not and the
    /// cursor at `at` seconds.
    pub const fn press(&mut self, stop: StopMode, playing: bool, at: f64) -> TransportStep {
        if playing {
            self.stop(stop, true)
        } else {
            self.start = Some(at);
            TransportStep::Play
        }
    }

    /// Stop asked for, the transport `playing` or not.
    pub const fn stop(&mut self, stop: StopMode, playing: bool) -> TransportStep {
        let start = self.start.take();
        match (stop, playing, start) {
            (StopMode::Return, true, Some(to)) => TransportStep::StopAt(to),
            _ => TransportStep::Stop,
        }
    }

    /// Play started from `at` seconds on purpose (a move and a play in
    /// one).
    pub const fn started_from(&mut self, at: f64) {
        self.start = Some(at);
    }

    /// The cursor moved to `to`: while playing, that is where play
    /// starts from now. Stopped, nothing is remembered — the next play
    /// starts from wherever the cursor is.
    pub const fn moved(&mut self, playing: bool, to: f64) {
        if playing {
            self.start = Some(to);
        }
    }

    /// Where Stop would return to.
    #[must_use]
    pub const fn start(self) -> Option<f64> {
        self.start
    }
}

#[cfg(test)]
mod stop_tests {
    use super::*;

    #[test]
    fn play_remembers_where_it_started_and_stop_returns_there() {
        let mut locate = Locate::default();
        assert_eq!(
            locate.press(StopMode::Return, false, 8.0),
            TransportStep::Play
        );
        assert_eq!(locate.start(), Some(8.0));
        assert_eq!(
            locate.press(StopMode::Return, true, 20.0),
            TransportStep::StopAt(8.0)
        );
        assert_eq!(locate.start(), None, "a stop forgets the start");
    }

    #[test]
    fn stay_stops_where_it_is() {
        let mut locate = Locate::default();
        locate.press(StopMode::Stay, false, 8.0);
        assert_eq!(
            locate.press(StopMode::Stay, true, 20.0),
            TransportStep::Stop
        );
    }

    #[test]
    fn a_locate_while_playing_is_the_new_start() {
        let mut locate = Locate::default();
        locate.press(StopMode::Return, false, 8.0);
        locate.moved(true, 30.0);
        assert_eq!(
            locate.stop(StopMode::Return, true),
            TransportStep::StopAt(30.0)
        );
    }

    #[test]
    fn a_locate_while_stopped_remembers_nothing() {
        let mut locate = Locate::default();
        locate.moved(false, 30.0);
        assert_eq!(locate.start(), None);
        assert_eq!(
            locate.press(StopMode::Return, false, 30.0),
            TransportStep::Play
        );
        assert_eq!(locate.start(), Some(30.0));
    }

    #[test]
    fn play_from_is_a_start() {
        let mut locate = Locate::default();
        locate.started_from(12.0);
        assert_eq!(
            locate.stop(StopMode::Return, true),
            TransportStep::StopAt(12.0)
        );
    }

    #[test]
    fn stop_with_no_known_start_stays() {
        let mut locate = Locate::default();
        assert_eq!(locate.stop(StopMode::Return, true), TransportStep::Stop);
        assert_eq!(locate.stop(StopMode::Return, false), TransportStep::Stop);
    }

    #[test]
    fn slugs_round_trip() {
        for each in StopMode::ALL {
            assert_eq!(StopMode::from_slug(each.slug()), Some(each));
        }
        assert_eq!(StopMode::from_slug(" Return "), Some(StopMode::Return));
        assert_eq!(StopMode::from_slug("nope"), None);
    }
}
