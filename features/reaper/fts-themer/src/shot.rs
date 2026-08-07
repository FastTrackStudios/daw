//! Screenshotting a real REAPER wearing the theme.
//!
//! The browser preview in the editor is fast but approximate — it draws from
//! the palette and WALTER layout, with vector fallbacks where the theme uses
//! PNG artwork. This is the ground truth: a real REAPER, on a private X
//! display, with real tracks and a real mixer, captured to a file.
//!
//! Two things make it quick enough to run in a loop:
//!
//! - **No plugin scan.** A cold REAPER scanning a few hundred VSTs takes
//!   longer than everything else here combined and shows a modal progress
//!   dialog over the UI we came to photograph. [`Overrides::for_screenshot`]
//!   blanks the scan paths for the duration of the run.
//! - **The overrides are temporary.** When shooting against a real profile
//!   (so the screenshot shows *your* layout), the ini is restored on drop —
//!   the run must not leave the profile without its plugin paths.
//!
//! CLAP paths are deliberately left alone: they're what we actually load, and
//! the CLAP scan is cheap.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::color::Rgb;

/// Which REAPER profile to shoot against.
#[derive(Clone, Debug)]
pub enum Profile {
    /// Build a throwaway profile. Reproducible, but shows REAPER's defaults
    /// rather than your window layout.
    Isolated(PathBuf),
    /// Use an existing resource dir (e.g. `~/fts-dev`), with its ini
    /// temporarily overridden and restored afterwards.
    Existing(PathBuf),
}

impl Profile {
    fn dir(&self) -> &Path {
        match self {
            Self::Isolated(p) | Self::Existing(p) => p,
        }
    }
}

/// A track to put in the generated project.
#[derive(Clone, Debug)]
pub struct TrackSpec {
    pub name: String,
    pub color: Rgb,
}

impl TrackSpec {
    pub fn new(name: impl Into<String>, color: Rgb) -> Self {
        Self {
            name: name.into(),
            color,
        }
    }
}

/// A representative band: a folder's worth of drums plus the usual suspects,
/// in colors far enough apart to judge how the theme tints panels.
pub fn default_tracks() -> Vec<TrackSpec> {
    [
        ("Kick", "#e0567a"),
        ("Snare", "#e0567a"),
        ("OH", "#e0567a"),
        ("Bass", "#5b8def"),
        ("Gtr", "#46b9fe"),
        ("Keys", "#f2b134"),
        ("Lead Vox", "#3ddc97"),
    ]
    .into_iter()
    .map(|(n, c)| TrackSpec::new(n, Rgb::parse_hex(c).expect("literal hex")))
    .collect()
}

/// What to capture.
#[derive(Clone, Debug)]
pub struct ShotOptions {
    /// Theme directory to load (the one holding `<name>.ReaperTheme`).
    pub theme: PathBuf,
    /// Profile to run against.
    pub profile: Profile,
    /// Tracks in the generated project.
    pub tracks: Vec<TrackSpec>,
    /// Where the PNG goes.
    pub out: PathBuf,
    /// Xvfb screen spec.
    pub geometry: String,
    /// X display to use.
    pub display: String,
    /// How long to let REAPER settle before capturing.
    pub settle: Duration,
}

impl ShotOptions {
    pub fn new(theme: impl Into<PathBuf>, out: impl Into<PathBuf>) -> Self {
        let scratch = std::env::temp_dir().join("fts-themer-shot");
        Self {
            theme: theme.into(),
            profile: Profile::Isolated(scratch),
            tracks: default_tracks(),
            out: out.into(),
            geometry: "1920x1200x24".into(),
            display: ":97".into(),
            settle: Duration::from_secs(14),
        }
    }
}

// ── ini overrides ────────────────────────────────────────────────────────

/// Temporary `[REAPER]` ini edits, restored when this drops.
///
/// The restore is the whole point: a screenshot run against a real profile
/// blanks its plugin paths, and leaving them blank would silently break the
/// next real REAPER launch.
pub struct Overrides {
    path: PathBuf,
    original: Option<String>,
    restore: bool,
}

impl Overrides {
    /// The settings that make REAPER start fast and photograph cleanly.
    ///
    /// `clap_path` is intentionally absent — CLAP is what we load, and its
    /// scan is cheap.
    pub fn for_screenshot() -> Vec<(&'static str, String)> {
        vec![
            // The expensive one: a few hundred VSTs, behind a modal dialog.
            ("vstpath", String::new()),
            ("vstpath64", String::new()),
            ("lv2path_linux", String::new()),
            // "A new REAPER is available" pops over the arrange view.
            ("verchk", "0".into()),
            // No device to open — and no chance of stealing the live rig's.
            ("audiosys", "0".into()),
            ("undo_max_mem", "0".into()),
        ]
    }

    /// Apply `values` to the `[REAPER]` section of `path`.
    ///
    /// `restore` false is for a throwaway profile, where putting the old
    /// values back is pointless work.
    pub fn apply(path: &Path, values: &[(&str, String)], restore: bool) -> Result<Self> {
        let original = std::fs::read_to_string(path).ok();
        let text = original.clone().unwrap_or_else(|| "[REAPER]\n".into());
        std::fs::write(path, patch_reaper_section(&text, values))
            .with_context(|| format!("write {}", path.display()))?;
        Ok(Self {
            path: path.to_path_buf(),
            original,
            restore,
        })
    }
}

impl Drop for Overrides {
    fn drop(&mut self) {
        if !self.restore {
            return;
        }
        if let Some(original) = &self.original {
            // Best effort: a failed restore must not mask the run's own error,
            // but it does need saying — the profile is left modified.
            if let Err(e) = std::fs::write(&self.path, original) {
                eprintln!(
                    "WARNING: could not restore {}: {e}. Its plugin paths are \
                     still blank — fix before launching REAPER normally.",
                    self.path.display()
                );
            }
        }
    }
}

/// Set `values` in the `[REAPER]` section, preserving every other line.
fn patch_reaper_section(text: &str, values: &[(&str, String)]) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let mut in_reaper = false;
    let mut section_end = lines.len();
    let mut seen: Vec<&str> = Vec::new();

    for i in 0..lines.len() {
        let t = lines[i].trim().to_string();
        if let Some(section) = t.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            if in_reaper {
                section_end = i;
                in_reaper = false;
            } else if section.eq_ignore_ascii_case("REAPER") {
                in_reaper = true;
                section_end = i + 1;
            }
            continue;
        }
        if in_reaper
            && let Some((k, _)) = t.split_once('=')
            && let Some((key, v)) = values
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(k.trim()))
        {
            lines[i] = format!("{key}={v}");
            seen.push(key);
            section_end = i + 1;
        } else if in_reaper && t.contains('=') {
            section_end = i + 1;
        }
    }

    // Keys the file didn't have yet.
    let missing: Vec<String> = values
        .iter()
        .filter(|(k, _)| !seen.contains(k))
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    lines.splice(section_end..section_end, missing);

    let mut out = lines.join("\n");
    out.push('\n');
    out
}

// ── project generation ───────────────────────────────────────────────────

/// A minimal `.RPP` with the requested tracks.
///
/// Hand-written rather than routed through the RPP writer: this needs a few
/// named, colored tracks and nothing else, and keeping it dependency-free
/// keeps the screenshot tool light.
pub fn project_rpp(tracks: &[TrackSpec]) -> String {
    let mut out = String::from("<REAPER_PROJECT 0.1 \"7.0\" 0\n  TEMPO 120 4 4\n");
    for t in tracks {
        // PEAKCOL is a native color: 0x01000000 marks "custom colour set",
        // and the low bytes are BGR like every other REAPER colour.
        let col = 0x0100_0000_u32 | (t.color.to_colorref() as u32 & 0x00ff_ffff);
        out.push_str(&format!(
            "  <TRACK\n    NAME \"{}\"\n    PEAKCOL {col}\n    TRACKHEIGHT 0 0\n  >\n",
            t.name.replace('"', "'")
        ));
    }
    out.push_str(">\n");
    out
}

// ── the run ──────────────────────────────────────────────────────────────

/// Set up a profile, launch REAPER on a private display, and capture it.
pub fn capture(opts: &ShotOptions) -> Result<PathBuf> {
    let theme = crate::ThemeDir::open(&opts.theme)?;
    let dir = opts.profile.dir().to_path_buf();
    let restore = matches!(opts.profile, Profile::Existing(_));

    if !restore {
        // Throwaway profile: (re)build it from scratch, theme linked in.
        let themes = dir.join("ColorThemes");
        std::fs::create_dir_all(&themes)?;
        link_force(&theme.images_dir(), &themes.join(&theme.name))?;
        link_force(
            &theme.ini_path(),
            &themes.join(format!("{}.ReaperTheme", theme.name)),
        )?;

        match install_license(&dir) {
            Some(from) => println!("  license: copied from {}", from.display()),
            None => eprintln!(
                "  NOTE: no {LICENSE_FILE} found in any known resource dir — \
                 REAPER's evaluation splash will appear in the capture."
            ),
        }
    }

    let ini = dir.join("reaper.ini");
    let mut values = Overrides::for_screenshot();
    // Point REAPER at the theme under test.
    let theme_ini = std::fs::canonicalize(theme.ini_path())?;
    values.push(("lastthemefn5", theme_ini.display().to_string()));
    let _guard = Overrides::apply(&ini, &values, restore)?;

    let project = dir.join("fts-themer-shot.rpp");
    std::fs::write(&project, project_rpp(&opts.tracks))?;

    let display = Display::start(&opts.display, &opts.geometry)?;
    let mut reaper = launch_reaper(&ini, &project, display.name())?;

    // REAPER restores dialogs *after* it starts, so sweep for a while rather
    // than once — and the update nag can appear seconds in.
    let deadline = Instant::now() + opts.settle;
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(700));
        display.close_stray_dialogs();
    }
    display.maximize_main_window();
    std::thread::sleep(Duration::from_millis(800));

    if let Some(parent) = opts.out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let result = display.screenshot(&opts.out);

    let _ = reaper.kill();
    let _ = reaper.wait();
    result?;

    Ok(opts.out.clone())
}

/// Resource dirs searched for an existing REAPER license, in order.
///
/// A throwaway profile has no license, so REAPER shows its "Still Evaluating"
/// splash over the arrange view — in the middle of the screenshot. Copying the
/// license the user already has into their own scratch profile is what makes
/// an unattended capture clean.
fn license_search_paths() -> Vec<PathBuf> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let mut dirs = Vec::new();
    if let Ok(dev) = std::env::var("FTS_DEV") {
        dirs.push(PathBuf::from(dev));
    }
    dirs.extend([
        home.join("fts-dev"),
        home.join(".config/REAPER"),
        home.join(".fasttrackstudio/Reaper"),
        home.join("fasttrackstudio"),
    ]);
    dirs
}

/// The name REAPER stores its license under, in the resource dir.
const LICENSE_FILE: &str = "reaper-license.rk";

/// Find an installed REAPER license, if there is one.
pub fn find_license() -> Option<PathBuf> {
    license_search_paths()
        .into_iter()
        .map(|d| d.join(LICENSE_FILE))
        .find(|p| p.is_file())
}

/// Copy an existing license into `profile`, so the evaluation splash stays out
/// of the shot. Returns where it came from.
///
/// Copied, not symlinked: REAPER rewrites this file, and a symlink would let a
/// throwaway profile write through to the real one.
fn install_license(profile: &Path) -> Option<PathBuf> {
    let src = find_license()?;
    let dst = profile.join(LICENSE_FILE);
    // Don't clobber a license the profile already has.
    if dst.is_file() {
        return Some(dst);
    }
    std::fs::copy(&src, &dst).ok()?;
    Some(src)
}

/// Replace `dst` with a symlink to `src`, whatever `dst` currently is.
fn link_force(src: &Path, dst: &Path) -> Result<()> {
    if dst.exists() || std::fs::symlink_metadata(dst).is_ok() {
        let _ = std::fs::remove_file(dst);
        let _ = std::fs::remove_dir_all(dst);
    }
    let src = std::fs::canonicalize(src).with_context(|| format!("resolve {}", src.display()))?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(&src, dst)
        .with_context(|| format!("link {} -> {}", dst.display(), src.display()))?;
    Ok(())
}

/// Spawn REAPER through the FHS wrapper when one is configured.
///
/// Without it SWELL cannot dlopen its GUI libraries and REAPER dies inside
/// GDK — which looks exactly like a missing display.
fn launch_reaper(ini: &Path, project: &Path, display: &str) -> Result<Child> {
    let exe = reaper_binary()?;
    let fhs = reaper_fhs();

    let mut cmd = match &fhs {
        Some(wrapper) => {
            let mut c = Command::new(wrapper);
            c.arg(&exe);
            c
        }
        None => {
            eprintln!(
                "WARNING: no reaper-env FHS wrapper found — REAPER's GUI libs \
                 may fail to load, which presents as a missing display."
            );
            Command::new(&exe)
        }
    };

    cmd.args(["-cfgfile"])
        .arg(ini)
        .args(["-nosplash", "-ignoreerrors"])
        .arg(project)
        .env("DISPLAY", display)
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    cmd.spawn()
        .with_context(|| format!("launch {}", exe.display()))
}

/// Locate REAPER: the fts-dev test profile's pinned build, else `$PATH`.
fn reaper_binary() -> Result<PathBuf> {
    let launch = PathBuf::from(
        std::env::var("FTS_DEV")
            .unwrap_or_else(|_| format!("{}/fts-dev", std::env::var("HOME").unwrap_or_default())),
    )
    .join("launch.json");

    if let Ok(text) = std::fs::read_to_string(&launch)
        && let Some(exe) = json_string(&text, "reaper_executable")
    {
        let p = PathBuf::from(exe);
        // launch.json can name a garbage-collected store path.
        if p.is_file() {
            return Ok(p);
        }
    }
    which("reaper").context("no REAPER found (not in launch.json, not on PATH)")
}

/// The FHS wrapper, from `$FTS_REAPER_FHS` or scraped from the `fts-dev` script.
fn reaper_fhs() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("FTS_REAPER_FHS") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let script = which("fts-dev")?;
    let text = std::fs::read_to_string(script).ok()?;
    let marker = "FTS_REAPER_FHS=\"";
    let start = text.find(marker)? + marker.len();
    let end = start + text[start..].find('"')?;
    let p = PathBuf::from(&text[start..end]);
    p.is_file().then_some(p)
}

/// Naive JSON string-field lookup — enough for launch.json, no dep needed.
fn json_string(text: &str, key: &str) -> Option<String> {
    let at = text.find(&format!("\"{key}\""))?;
    let rest = &text[at + key.len() + 2..];
    let open = rest.find('"')? + 1;
    let close = open + rest[open..].find('"')?;
    Some(rest[open..close].to_string())
}

fn which(bin: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join(bin))
            .find(|p| p.is_file())
    })
}

// ── the display ──────────────────────────────────────────────────────────

/// A private Xvfb with a window manager, torn down on drop.
///
/// Deliberately small and self-contained rather than reaching for
/// `daw::test::VirtualDisplay`: this is a theme tool, and taking a dependency
/// on the whole DAW crate to spawn an Xvfb is not a trade worth making. The
/// hard-won details it encodes — run a WM, sweep for dialogs — are shared as
/// knowledge, not as code.
struct Display {
    name: String,
    xvfb: Option<Child>,
    wm: Option<Child>,
}

/// Tried in order; all are tiny and need no session bus or compositor.
const WINDOW_MANAGERS: [&str; 4] = [
    "openbox",
    "fluxbox",
    "herbstluftwm",
    "matchbox-window-manager",
];

impl Display {
    fn start(name: &str, geometry: &str) -> Result<Self> {
        let socket = PathBuf::from(format!("/tmp/.X11-unix/X{}", name.trim_start_matches(':')));
        if socket.exists() {
            bail!(
                "display {name} is already in use ({}) — pass a free one",
                socket.display()
            );
        }

        let xvfb = Command::new("Xvfb")
            .args([name, "-screen", "0", geometry, "-nolisten", "tcp"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("start Xvfb (is it on PATH? try `nix develop .#reaper-test`)")?;

        let deadline = Instant::now() + Duration::from_secs(10);
        while !socket.exists() {
            if Instant::now() > deadline {
                bail!("Xvfb did not come up on {name} within 10s");
            }
            std::thread::sleep(Duration::from_millis(100));
        }

        let mut display = Self {
            name: name.to_string(),
            xvfb: Some(xvfb),
            wm: None,
        };
        display.start_window_manager();
        Ok(display)
    }

    /// A window manager is not optional. Unmanaged windows are never
    /// positioned or stacked, so REAPER's dialogs sit on top of the arrange
    /// view and the capture shows nothing a user would recognise.
    fn start_window_manager(&mut self) {
        for wm in WINDOW_MANAGERS {
            if let Ok(child) = Command::new(wm)
                .env("DISPLAY", &self.name)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                std::thread::sleep(Duration::from_millis(500));
                self.wm = Some(child);
                return;
            }
        }
        eprintln!(
            "WARNING: no window manager ({}) — windows will be unmanaged and \
             the screenshot will not reflect real layout.",
            WINDOW_MANAGERS.join(", ")
        );
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn xdotool(&self, args: &[&str]) -> Option<String> {
        let out = Command::new("xdotool")
            .env("DISPLAY", &self.name)
            .args(args)
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// Politely close REAPER's restored dialogs and update nag.
    ///
    /// Politely matters: these are all REAPER's own X clients, so killing one
    /// takes REAPER with it.
    fn close_stray_dialogs(&self) {
        for pattern in ["REAPER Update", "ReaPack", "Notice", "Warning", "Error"] {
            if let Some(ids) = self.xdotool(&["search", "--name", pattern]) {
                for id in ids.lines().filter(|l| !l.is_empty()) {
                    self.xdotool(&["windowclose", id]);
                }
            }
        }
    }

    /// REAPER's main window as `0x…`, the form `import -window` accepts.
    fn main_window_hex(&self) -> Option<String> {
        let ids = self.xdotool(&["search", "--name", "REAPER"])?;
        // Largest id is the most recently created — the main window, once the
        // splash and any dialogs have gone.
        let id: u64 = ids
            .lines()
            .filter_map(|l| l.trim().parse::<u64>().ok())
            .max()?;
        Some(format!("0x{id:x}"))
    }

    /// Fill the screen with REAPER's main window, so the capture isn't
    /// mostly root-window black.
    fn maximize_main_window(&self) {
        let Some(ids) = self.xdotool(&["search", "--name", "REAPER"]) else {
            return;
        };
        if let Some(id) = ids.lines().find(|l| !l.is_empty()) {
            self.xdotool(&["windowsize", id, "100%", "100%"]);
            self.xdotool(&["windowmove", id, "0", "0"]);
            self.xdotool(&["windowactivate", id]);
        }
    }

    /// Capture REAPER's own window, falling back to the whole root.
    ///
    /// Without a window manager `windowsize 100%` has no effect, so a root
    /// capture is mostly black surround. Grabbing the window by id gives a
    /// tight shot either way.
    ///
    /// `import` wants the id in **hex**; xdotool prints decimal, and handing
    /// the decimal straight over fails with a bare "missing an image filename",
    /// which reads like a path problem rather than a window one.
    fn screenshot(&self, path: &Path) -> Result<()> {
        let window = self.main_window_hex();
        let target = window.as_deref().unwrap_or("root");
        let status = Command::new("import")
            .env("DISPLAY", &self.name)
            .args(["-window", target])
            .arg(path)
            .status()
            .context("run `import` (ImageMagick)")?;
        if !status.success() {
            bail!("import failed capturing {} (window {target})", self.name);
        }
        Ok(())
    }
}

impl Drop for Display {
    fn drop(&mut self) {
        for child in [self.wm.as_mut(), self.xvfb.as_mut()].into_iter().flatten() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overrides_replace_existing_keys_in_place() {
        let ini = "[REAPER]\nvstpath=~/.vst\nother=keep\n\n[other]\nvstpath=notme\n";
        let out = patch_reaper_section(&ini, &[("vstpath", String::new())]);
        assert!(out.contains("vstpath=\n"));
        assert!(out.contains("other=keep"));
        // The same key in another section is not ours to touch.
        assert!(out.contains("vstpath=notme"));
    }

    #[test]
    fn overrides_append_missing_keys_inside_the_section() {
        let ini = "[REAPER]\nother=keep\n\n[tail]\nx=1\n";
        let out = patch_reaper_section(&ini, &[("verchk", "0".into())]);
        let verchk = out.lines().position(|l| l.starts_with("verchk")).unwrap();
        let tail = out.lines().position(|l| l == "[tail]").unwrap();
        assert!(verchk < tail, "key escaped its section:\n{out}");
    }

    #[test]
    fn screenshot_overrides_blank_vst_but_leave_clap_alone() {
        // The whole point: CLAP is what we load, and its scan is cheap.
        let keys: Vec<&str> = Overrides::for_screenshot()
            .iter()
            .map(|(k, _)| *k)
            .collect();
        assert!(keys.contains(&"vstpath"));
        assert!(!keys.iter().any(|k| k.contains("clap")));
    }

    #[test]
    fn restore_puts_the_original_back() {
        let dir = std::env::temp_dir().join(format!("fts-shot-ini-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ini = dir.join("reaper.ini");
        let original = "[REAPER]\nvstpath=~/.vst;~/.vst3\n";
        std::fs::write(&ini, original).unwrap();

        {
            let _g = Overrides::apply(&ini, &Overrides::for_screenshot(), true).unwrap();
            assert!(
                std::fs::read_to_string(&ini)
                    .unwrap()
                    .contains("vstpath=\n")
            );
        }
        // Dropped — a run must never leave a real profile without its paths.
        assert_eq!(std::fs::read_to_string(&ini).unwrap(), original);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn no_restore_leaves_the_throwaway_profile_alone() {
        let dir = std::env::temp_dir().join(format!("fts-shot-nore-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ini = dir.join("reaper.ini");
        std::fs::write(&ini, "[REAPER]\nvstpath=x\n").unwrap();
        {
            let _g = Overrides::apply(&ini, &Overrides::for_screenshot(), false).unwrap();
        }
        assert!(
            std::fs::read_to_string(&ini)
                .unwrap()
                .contains("vstpath=\n")
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn project_has_a_track_per_spec_with_its_colour() {
        let rpp = project_rpp(&[TrackSpec::new("Kick", Rgb::new(0xe0, 0x56, 0x7a))]);
        assert_eq!(rpp.matches("<TRACK").count(), 1);
        assert!(rpp.contains("NAME \"Kick\""));
        // 0x01000000 | BGR(0x7a56e0)
        let want = 0x0100_0000_u32 | 0x007a_56e0;
        assert!(rpp.contains(&format!("PEAKCOL {want}")), "{rpp}");
    }

    #[test]
    fn project_quotes_survive_a_track_name() {
        let rpp = project_rpp(&[TrackSpec::new("The \"Big\" One", Rgb::new(0, 0, 0))]);
        // An unescaped quote would truncate the NAME field and corrupt the RPP.
        assert!(rpp.contains("NAME \"The 'Big' One\""), "{rpp}");
    }
}
