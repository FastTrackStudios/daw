//! The engine as a window sees it: one vox service, [`EngineView`], beside
//! the daw facade on the same link.
//!
//! A window never reaches into the engine. It reads the song through the
//! daw facade (`daw::rpc::Daw`) and everything the facade does not carry —
//! the set, the hit markers, the waveforms as the engine draws them, the
//! automation lanes with the leveler's, the streaming state, what the
//! engine asks of a window, the Action List — through this service. The
//! link is a Unix socket to `studio engine` on the desktop, and an
//! in-memory one to an engine in the same process on iOS and in a browser
//! (`architect::LocalServer`): the same client either way, so no window
//! code knows which.
//!
//! Live state comes as `#[subscribe]` streams from the engine's hubs (the
//! set, the overlays, the streaming state, what changed, what a window is
//! asked to do); what a window reads on demand (a waveform's detail at the
//! zoom it draws, a song's lanes) is a call. A frame never waits on
//! either: the window keeps a replica of what it draws
//! (`crate::view_client`).
//!
//! The engine side is `crate::studio_engine::view_host`; the window side
//! `crate::view_client`. See `docs/design/engine-boundary.md`.

use facet::Facet;

/// Why the engine would not do what a window asked.
#[repr(u8)]
#[derive(Debug, Clone, PartialEq, Eq, Facet)]
pub enum ViewError {
    /// The action refused, and why, for a person.
    Refused(String),
    /// No song by that name is open.
    NoSong(String),
}

impl std::fmt::Display for ViewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(why) => f.write_str(why),
            Self::NoSong(song) => write!(f, "no song {song} is open"),
        }
    }
}

impl std::error::Error for ViewError {}

/// One Action List call: its id and its arguments as JSON (empty for
/// none).
#[derive(Debug, Clone, PartialEq, Eq, Facet)]
pub struct ActionCall {
    pub id: String,
    pub args: String,
}

/// A window's edits, in the order it made them, against `song` (empty: the
/// engine's current song).
#[derive(Debug, Clone, PartialEq, Facet)]
pub struct EditBatch {
    pub song: String,
    pub edits: Vec<crate::engine::Edit>,
}

/// What a batch came to: the items it made, by the name the window gave
/// each (a split's right half, a copy) and the engine's.
#[derive(Debug, Clone, Default, PartialEq, Eq, Facet)]
pub struct Applied {
    pub made: Vec<(String, String)>,
    /// Whether any edit changed an item (the window reads the changed
    /// items back).
    pub items: bool,
}

/// Coarse waveforms of `items` of `song`, as the engine draws them: through
/// each take's volume curve, cut from a recording already read.
#[derive(Debug, Clone, PartialEq, Eq, Facet)]
pub struct WavesAsk {
    pub song: String,
    pub items: Vec<String>,
}

/// One item's waveform as read.
#[repr(u8)]
#[derive(Debug, Clone, PartialEq, Facet)]
pub enum WaveRead {
    /// Its whole waveform.
    Whole {
        item: String,
        wave: crate::midi::Wave,
    },
    /// Read before its recording could be read whole (it is still
    /// streaming in): read it again once it has arrived.
    Partial { item: String },
    /// No audio to draw (a MIDI item, a missing file).
    None { item: String },
}

/// A stretch of audio, fine, as a lane draws it at its zoom.
#[derive(Debug, Clone, PartialEq, Facet)]
pub struct DetailAsk {
    pub song: String,
    pub source: crate::wave_detail::Source,
    /// Project seconds.
    pub from: f64,
    pub to: f64,
    /// Seconds a point stands for, at most.
    pub step: f64,
}

/// A song's automation as the window draws it: every shown lane (the
/// leveler's stage lanes among them), the take volume curves shown on
/// items, and the leveled items.
#[derive(Debug, Clone, Default, PartialEq, Facet)]
pub struct AutomationRead {
    pub lanes: Vec<WireLane>,
    /// `(item, curve)`.
    pub takes: Vec<(String, crate::automation::TakeCurve)>,
    /// `(track, its leveled items)`.
    pub leveled: Vec<(String, Vec<WireLeveled>)>,
}

/// An automation lane on the wire ([`crate::automation::Lane`] without its
/// leveled items, which come once per track in [`AutomationRead`]).
#[derive(Debug, Clone, PartialEq, Facet)]
pub struct WireLane {
    pub track: String,
    pub target: daw_proto::automation::EnvelopeRef,
    pub name: String,
    pub kind: crate::automation::Kind,
    pub height: f64,
    pub mode: daw_proto::primitives::AutomationMode,
    pub points: Vec<daw_proto::automation::EnvelopePoint>,
    pub items: Vec<(
        daw_proto::automation::AutomationItem,
        Vec<daw_proto::automation::EnvelopePoint>,
    )>,
    pub played: Vec<daw_proto::automation::EnvelopePoint>,
    pub rest: f64,
    pub fx_name: Option<String>,
    pub choices: Vec<(f64, String)>,
}

/// One point of a leveler stage: item seconds, dB, held to the next.
#[derive(Debug, Clone, Copy, PartialEq, Facet)]
pub struct WirePoint {
    pub t: f64,
    pub db: f64,
    pub hold: bool,
}

/// A span of one gain (the de-esser's).
#[derive(Debug, Clone, Copy, PartialEq, Facet)]
pub struct WireSpan {
    pub from: f64,
    pub to: f64,
    pub db: f64,
}

/// One leveled item: where it is, its four stages as made and which are
/// bypassed, and the track's dials (`(on, amount)` per stage, in
/// [`crate::leveler::Stage::ALL`] order). The window remakes the curves
/// from these, as the engine does.
#[derive(Debug, Clone, PartialEq, Facet)]
pub struct WireLeveled {
    pub track: String,
    pub item: String,
    pub position: f64,
    pub length: f64,
    pub stretch: f64,
    pub origin: f64,
    pub gate: Vec<WirePoint>,
    pub breath: Vec<WirePoint>,
    pub sibilance: Vec<WireSpan>,
    pub ride: Vec<WirePoint>,
    /// gate, breath, sibilance, ride.
    pub bypass: [bool; 4],
    pub dials: [(bool, f64); 4],
}

/// The items of `song` changed since the journal stood at `since`.
#[derive(Debug, Clone, PartialEq, Eq, Facet)]
pub struct ChangesAsk {
    pub song: String,
    pub since: u64,
}

/// One item as it is now, for the window's model of the song.
#[derive(Debug, Clone, PartialEq, Facet)]
pub struct ItemNow {
    pub item: daw_proto::Item,
    /// Its active take's name, when it has one.
    pub take_name: Option<String>,
    pub midi: bool,
}

/// What changed: where the journal stands now, and the items written since
/// (`None`: it cannot say — read the whole song). `gone` are the changed
/// items no longer in the song.
#[derive(Debug, Clone, Default, PartialEq, Facet)]
pub struct ItemChanges {
    pub cursor: u64,
    pub changed: Option<Vec<ItemNow>>,
    pub gone: Vec<String>,
}

/// What a window needs of a song that is not on the facade: its tracks'
/// kinds (as the row plan reads them) and its chart, written from its
/// sections.
#[derive(Debug, Clone, Default, PartialEq, Eq, Facet)]
pub struct SongFacts {
    /// The tracks the template marked: what a scene's selectors match.
    pub kinds: Vec<TrackKind>,
    /// The chart as keyflow text; empty when the song has none.
    pub chart: String,
    /// The song's file on the engine's disk, when it has one.
    pub path: String,
}

/// A track as the template marked it (`dynamic_template`'s `TrackExt`).
#[derive(Debug, Clone, PartialEq, Eq, Facet)]
pub struct TrackKind {
    pub guid: String,
    pub name: String,
    /// Its kind's name (`Kind::as_str`), when it has one.
    pub kind: Option<String>,
    pub group: Option<String>,
}

/// Where one song's playhead is.
#[derive(Debug, Clone, PartialEq, Facet)]
pub struct Playhead {
    pub song: String,
    /// Seconds.
    pub at: f64,
    pub playing: bool,
}

/// The set as the engine has it: its songs in order, which is current, the
/// ones still arriving, and an open under way.
#[derive(Debug, Clone, Default, PartialEq, Facet)]
pub struct SetFrame {
    /// Bumped by every change.
    pub revision: u64,
    /// Which set: bumped when one is installed or closed.
    pub epoch: u64,
    /// `(title, project)`, in order.
    pub songs: Vec<(String, String)>,
    /// Each song's colour, as its tab shows it (`#rrggbb`, or empty).
    pub colors: Vec<String>,
    /// The current song, an index into `songs` (none with no set).
    pub current: Option<u32>,
    /// Titles still on their way, after `songs`.
    pub arriving: Vec<String>,
    pub opening: SetOpening,
}

/// Where opening a set has got to.
#[repr(u8)]
#[derive(Debug, Clone, Default, PartialEq, Facet)]
pub enum SetOpening {
    /// Nothing is being opened.
    #[default]
    Idle,
    /// Opening: where it has got to.
    Busy(crate::open_progress::Progress),
    /// The last open failed, and why, for a person.
    Failed(String),
}

/// The kit's hit markers, zones, comps and take ranks for `song`, as the
/// engine reads them.
#[derive(Debug, Clone, Default, PartialEq, Facet)]
pub struct OverlayFrame {
    pub song: String,
    /// Bumped by every read.
    pub version: u64,
    pub overlay: crate::hit_overlay::Overlay,
}

/// The tracks' own markers and regions of `song`, by track.
#[derive(Debug, Clone, Default, PartialEq, Facet)]
pub struct MarksFrame {
    pub song: String,
    pub version: u64,
    pub marks: Vec<(String, crate::track_regions::Marks)>,
    /// The song's points of interest (`crate::poi`), and the one stepped to.
    pub pois: Vec<crate::poi::Poi>,
    pub poi_current: Option<String>,
}

/// What streams in: each item's share arrived (by item guid, those not all
/// here) and each song's state.
#[derive(Debug, Clone, Default, PartialEq, Facet)]
pub struct StreamsFrame {
    pub version: u64,
    pub items: Vec<(String, f32)>,
    pub songs: Vec<(String, StreamSong)>,
    /// A play pressed on a song still streaming waits for it.
    pub waiting: bool,
    /// How the streamed songs are heard, as Settings offers it.
    pub listening: Listening,
}

/// How the streamed songs are heard: by their stems, once asked for
/// (`crate::reference_play::load_multitracks`), and whether any is heard
/// by its reference now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Facet)]
pub struct Listening {
    pub stems: bool,
    pub referenced: bool,
}

/// What the engine is doing in the background, as the bottom bar shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Facet)]
pub struct JobsFrame {
    pub version: u64,
    pub jobs: Vec<JobRow>,
}

/// The engine's live session, as a window's collaboration control shows
/// it (`crate::collab`, `crate::live_host`): the set it is in, who is
/// there, this device's lock, and why the last share, join or Task
/// hosting failed. The session is the engine's: a window starts none.
#[derive(Debug, Clone, Default, PartialEq, Eq, Facet)]
pub struct LiveFrame {
    pub version: u64,
    /// The live session, if the engine is in one.
    pub status: Option<LiveStatus>,
    /// A session is being joined.
    pub joining: bool,
    /// Why the last share or join failed.
    pub error: Option<String>,
    /// This device is locked: not sending its edits, not taking others'.
    pub locked: bool,
    /// The set the engine hosts in Task by itself, by its title.
    pub hosted: Option<String>,
    /// Why hosting in Task last failed.
    pub host_error: Option<String>,
    /// Everyone else in the set, and the song each is on.
    pub people: Vec<LivePerson>,
    /// What the person should know about the session: a song kept out of
    /// the set, a version of theirs kept aside (`crate::collab::notices`).
    pub notices: Vec<String>,
}

/// A live session, as [`crate::collab::Status`] has it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Facet)]
pub struct LiveStatus {
    pub hosting: bool,
    pub ticket: String,
    pub peers: u32,
    pub shared_transport: bool,
    pub set: String,
    pub name: String,
    pub color: u32,
    /// Seconds until a playground set starts over.
    pub resets_in_secs: Option<u64>,
    pub locked: bool,
    pub held: u32,
}

/// Someone in the set (`crate::presence::Person`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Facet)]
pub struct LivePerson {
    /// Their song's project in the engine.
    pub project: String,
    /// Their song's name everywhere.
    pub song: String,
    pub name: String,
    pub color: u32,
}

/// The performance meter, twice a second (`crate::perf_meter`): the
/// engine as a whole and every track of the current song. CPU of one core
/// is 100 per core; `cpu_pct` is of all cores.
#[derive(Debug, Clone, Default, PartialEq, Facet)]
pub struct PerfFrame {
    pub version: u64,
    /// The song the tracks are of.
    pub song: String,
    pub cores: u32,
    /// The engine's process, of all cores.
    pub cpu_pct: f64,
    /// The audio callback's render (RT CPU), and the render ahead's, of
    /// one core.
    pub rt_cpu_pct: f64,
    pub ahead_cpu_pct: f64,
    /// The longest block of the last two seconds against a block's time.
    pub rt_longest_ms: f64,
    pub block_ms: f64,
    /// Since the engine started: blocks over their budget, xruns the
    /// device reported, the time the device went without audio, and blocks
    /// that played the tracks ahead short.
    pub overruns: u64,
    pub xruns: u64,
    pub gap_ms: f64,
    pub ahead_short: u64,
    /// The engine's disk reads and writes, bytes a second.
    pub disk_read_bps: f64,
    pub disk_write_bps: f64,
    /// The engine's resident memory; the system's available and total.
    pub ram_bytes: u64,
    pub system_free_bytes: u64,
    pub system_total_bytes: u64,
    pub tracks: Vec<TrackPerf>,
}

/// One track in the performance meter.
#[derive(Debug, Clone, Default, PartialEq, Facet)]
pub struct TrackPerf {
    /// Its place in the song.
    pub index: u32,
    pub guid: String,
    pub name: String,
    /// Its FX, and its items (read, resampled, stretched), of one core.
    pub fx_cpu_pct: f64,
    pub media_cpu_pct: f64,
    pub fx_count: u32,
    /// Its FX's delay compensation, samples.
    pub pdc_samples: u32,
}

/// One job, as [`crate::jobs::running`] reads it.
#[derive(Debug, Clone, PartialEq, Eq, Facet)]
pub struct JobRow {
    pub id: u64,
    pub label: String,
    pub done: u64,
    pub total: u64,
}

/// Where a window shows a song's edit cursor and time selection, and the
/// selection an edit would act on — what the engine's actions read
/// (`crate::edit_state`, `crate::edit_actions`).
#[derive(Debug, Clone, Default, PartialEq, Facet)]
pub struct EditShown {
    pub song: String,
    pub at: f64,
    pub selection: Option<daw_edit::scope::Span>,
    pub scope: Option<daw_edit::scope::Scope>,
}

/// Where an action put a song's edit cursor and time selection, for its
/// windows to take up.
#[derive(Debug, Clone, Default, PartialEq, Facet)]
pub struct EditAsked {
    pub song: String,
    pub revision: u64,
    pub at: f64,
    pub selection: Option<daw_edit::scope::Span>,
}

/// A streaming song, as its tab shows it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Facet)]
pub struct StreamSong {
    pub arrived: f32,
    pub ready: bool,
    pub waiting_on: u32,
}

/// Something about a song changed in the engine, for the windows to read
/// back.
#[derive(Debug, Clone, Default, PartialEq, Eq, Facet)]
pub struct Changed {
    pub song: String,
    /// Bumped by every change.
    pub revision: u64,
    pub what: Change,
    /// The items it is about ([`Change::Waves`]), when it says.
    pub items: Vec<String>,
}

/// What changed.
#[repr(u8)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Facet)]
pub enum Change {
    /// Something nobody named: read the whole song back.
    #[default]
    All,
    /// Only items: those the journal says.
    Items,
    /// The automation lanes (and take volume curves).
    Automation,
    /// These items' waveforms no longer draw what they play.
    Waves,
    /// The tracks' own markers and regions.
    Marks,
    /// A Session undo or redo was done: read the whole song back, and an
    /// Editor waiting on one reads again.
    Undone,
}

/// What the engine asks of a window it cannot do itself — an edit the
/// arrangement carries out where it is seen (`crate::window_queue::Asked`).
/// `pending` counts what is waiting; a window takes it with
/// [`EngineView::take_window`] (one window takes each).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Facet)]
pub struct Queued {
    pub pending: u32,
    /// Bumped by every ask.
    pub revision: u64,
}

/// One of the window's own actions, as the window's Action List describes
/// it, offered to the engine's ([`EngineView::window_actions`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Facet)]
pub struct WindowAction {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub group: String,
    pub shortcut: String,
    /// `read-only`, `mutating` or `destructive`.
    pub effect: String,
    /// What it takes and gives back, as JSON Schema.
    pub params_schema: Option<String>,
    pub result_schema: Option<String>,
    /// The window that claims it, by name: the engine asks it of that
    /// window alone (`crate::window_queue::claim_as`). Empty: whichever
    /// window takes the engine's asks.
    #[facet(default)]
    pub window: String,
}

/// What a window's action came to ([`EngineView::answer`]): its result as
/// JSON when `ok`, else why not.
#[derive(Debug, Clone, Default, PartialEq, Eq, Facet)]
pub struct WindowAnswer {
    pub ticket: u64,
    pub ok: bool,
    pub json: String,
}

/// The engine, as a window reads and drives it.
#[architect::rpc]
pub trait EngineView {
    /// Run one Action List action on the engine; what it said.
    async fn run(&self, call: ActionCall) -> Result<String, ViewError>;

    /// Carry out a window's edits, in order.
    async fn apply(&self, batch: EditBatch) -> Applied;

    /// What the engine has asked of the window `window` (by name), taken
    /// (each by one window: the one `SESSION_WINDOW_TARGET` names, else the
    /// first to ask).
    async fn take_window(&self, window: String) -> Vec<crate::window_queue::Asked>;

    /// The window's own actions (its view: zoom, page, frame rate), put on
    /// the engine's Action List: run there, each is asked of the window
    /// (`crate::window_queue::ask_window_action`). Those listed already
    /// are left; how many were added.
    async fn window_actions(&self, actions: Vec<WindowAction>) -> u32;

    /// What one of the window's actions, asked of it with a ticket, came to.
    async fn answer(&self, answer: WindowAnswer);

    /// One of the window's own actions ran in the window (a key, a click):
    /// said on the engine's Action List as run, so whoever watches the
    /// engine (`watch`, a guide, a test) sees it as it would one run here.
    /// Not asked of the window again. Whether the list knew it.
    async fn ran_in_window(&self, call: ActionCall) -> bool;

    /// Coarse waveforms, as the engine draws them.
    async fn waves(&self, ask: WavesAsk) -> Vec<WaveRead>;

    /// Fine detail of one stretch, at the zoom asked.
    async fn detail(&self, ask: DetailAsk) -> Option<crate::wave_detail::Detail>;

    /// A song's automation lanes, take curves and leveled items.
    async fn automation(&self, song: String) -> AutomationRead;

    /// The items changed since `since`.
    async fn item_changes(&self, ask: ChangesAsk) -> ItemChanges;

    /// Where the engine's journal stands: what a window reading a song
    /// whole notes first, to ask what changed from there.
    async fn journal_now(&self) -> u64;

    /// What a window needs of a song beside the facade.
    async fn song_facts(&self, song: String) -> SongFacts;

    /// Every song's playhead.
    async fn playheads(&self) -> Vec<Playhead>;

    /// Where the window shows the edit cursor, the time selection and the
    /// selection an edit acts on; whether it moved.
    async fn show_edit(&self, shown: EditShown) -> bool;

    /// The set, now and as it changes.
    #[subscribe]
    fn set(&self) -> SetFrame;

    /// The current song's hit overlay, now and as it is read again.
    #[subscribe]
    fn overlays(&self) -> OverlayFrame;

    /// The tracks' own marks of the current song.
    #[subscribe]
    fn marks(&self) -> MarksFrame;

    /// What streams in.
    #[subscribe]
    fn streams(&self) -> StreamsFrame;

    /// What the engine is doing in the background.
    #[subscribe]
    fn jobs(&self) -> JobsFrame;

    /// The performance meter: the engine's CPU, memory, disk and
    /// dropouts, and every track's FX and media CPU.
    #[subscribe]
    fn perf(&self) -> PerfFrame;

    /// The engine's live session, as it changes.
    #[subscribe]
    fn live(&self) -> LiveFrame;

    /// Where actions put the edit cursor and time selection.
    #[subscribe]
    fn edits(&self) -> EditAsked;

    /// What changed in a song.
    #[subscribe]
    fn changes(&self) -> Changed;

    /// What the engine asks of a window; subscribing is what makes a
    /// window attached.
    #[subscribe]
    fn queued(&self) -> Queued;
}
