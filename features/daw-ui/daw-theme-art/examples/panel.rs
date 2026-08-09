//! A whole track panel and mixer strip, drawn from the components.
//!
//!     cargo run -p daw-theme-art --example panel
//!     → target/theme-shots/native-panels.png
//!
//! Everything up to now has been one control against one PNG. This is the
//! question those were for: put the controls together the way REAPER lays
//! them out and see whether the result reads as the same panel.
//!
//! The layout follows a 1x REAPER screenshot of this theme rather than
//! anything invented here — a 296-wide track panel over 71-row tracks, an
//! 86-wide mixer strip — so the two can be set side by side and compared
//! honestly. What is *not* here is as much the point as what is: the
//! meters, the dB scale and the input combo have no component yet and are
//! drawn as plain rectangles, marked in the report.
//!
//! It composes by *nesting* each control's own `<svg>` inside a parent
//! with `x`/`y`, rather than re-rendering into a shared coordinate space.
//! That keeps every control's measured cell exactly as it is audited — a
//! control that scores 0.21 alone scores 0.21 here — so this file holds
//! only layout, no geometry.

use daw_theme::Color;
use daw_theme_art::render::{rasterise, render_svg};
use daw_theme_art::vector_controls as v;

const NONE: (Option<u32>, Option<u32>) = (None, None);

/// One control, placed.
///
/// `svg` is a whole document from `render_svg`, and it is nested rather
/// than re-rendered: only `x` and `y` are added, so the control keeps the
/// width, height and viewBox it was audited at. Adding a second `width`
/// beside the one already there is a parse error rather than an override,
/// which is how this was written first.
fn at(x: f32, y: f32, svg: &str) -> String {
    // Gradient ids are fixed per component — `lbM`, `trlit`, `recring` —
    // which is fine for one control in one document and wrong the moment
    // two of the same control share a page: the first definition wins and
    // every later copy silently paints with it. Five mute buttons came
    // out in the first one's grey, with a one-row red sliver where the
    // highlight is drawn from a colour rather than a reference.
    //
    // Uniquing them here rather than in the components keeps the exported
    // PNGs byte-identical, but it is a real constraint on the live UI and
    // wants solving there properly.
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let svg = svg.replace("id=\"", &format!("id=\"u{n}"));
    let svg = svg.replace("url(#", &format!("url(#u{n}"));
    svg.strip_prefix("<svg ")
        .map(|s| format!("<svg x=\"{x}\" y=\"{y}\" {s}"))
        .unwrap_or_else(|| svg.to_string())
}

fn rect(x: f32, y: f32, w: f32, h: f32, fill: &str) -> String {
    format!("<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" fill=\"{fill}\"/>")
}

fn label(x: f32, y: f32, size: f32, fill: &str, s: &str) -> String {
    format!(
        "<text x=\"{x}\" y=\"{y}\" font-family=\"Fira Sans, DejaVu Sans, sans-serif\" \
         font-size=\"{size}\" fill=\"{fill}\">{s}</text>"
    )
}

struct Track {
    name: &'static str,
    tint: Color,
    armed: bool,
    muted: bool,
    soloed: bool,
}

fn mute(cell: (f32, f32), body: (f32, f32), track: bool, on: bool) -> String {
    let t = daw_theme::Theme::default();
    render_svg(
        v::MuteButton,
        v::ToggleProps {
            on,
            cell,
            body,
            unlit: Some(t.chrome.hardware.shade(if track { 0.078 } else { 0.036 })),
            legend: Some(t.chrome.hardware_mark.shade(match (track, on) {
                (true, true) => 0.86,
                (true, false) => 0.23,
                (false, _) => 0.45,
            })),
            sinks: !track,
            hover: 0.25,
            depth: 0.11,
            width: NONE.0,
            height: NONE.1,
            at: v::Interaction::Normal,
        },
    )
}

fn solo(cell: (f32, f32), body: (f32, f32), track: bool, on: bool) -> String {
    let t = daw_theme::Theme::default();
    render_svg(
        v::SoloButton,
        v::SoloProps {
            state: if on { v::Solo::On } else { v::Solo::Off },
            cell,
            body,
            unlit: Some(t.chrome.hardware.shade(if track { 0.078 } else { 0.036 })),
            legend: Some(t.chrome.hardware_mark.shade(match (track, on) {
                (true, true) => 1.0,
                (true, false) => 0.44,
                (false, true) => 0.59,
                (false, false) => 0.45,
            })),
            sinks: !track,
            hover: 0.35,
            depth: 0.11,
            width: NONE.0,
            height: NONE.1,
            at: v::Interaction::Normal,
        },
    )
}

/// A meter. **No component draws this yet** — the one thing in the panel
/// still assembled out of rectangles, and the largest single gap.
fn meter(x: f32, y: f32, w: f32, h: f32, level: f32) -> String {
    let t = daw_theme::Theme::default();
    let lit = h * level;
    let mut s = rect(x, y, w, h, "#101010");
    s.push_str(&rect(x, y + h - lit, w, lit, &t.signal.meter_safe.css()));
    s
}

fn track_row(y: f32, n: u32, tk: &Track) -> String {
    let t = daw_theme::Theme::default();
    let mut s = String::new();

    // The track colour is the row: REAPER tints the whole panel, not a
    // stripe on it.
    s.push_str(&rect(0.0, y, 296.0, 70.0, &tk.tint.css()));
    s.push_str(&rect(0.0, y + 70.0, 296.0, 1.0, &t.chrome.hardware.shade(-0.72).css()));
    s.push_str(&label(9.0, y + 44.0, 11.0, &t.chrome.hardware_mark.shade(0.1).css(), &n.to_string()));

    // Row one: arm, name, pan, meter, FX, IN.
    s.push_str(&at(
        26.0,
        y + 6.0,
        &render_svg(
            v::RecordArmButton,
            v::RecordArmProps {
                state: if tk.armed { v::RecordArm::On } else { v::RecordArm::Off },
                cell: (20.0, 20.0),
                housing: false,
                width: NONE.0,
                height: NONE.1,
                at: v::Interaction::Normal,
            },
        ),
    ));
    s.push_str(&rect(50.0, y + 7.0, 130.0, 17.0, &t.chrome.hardware.shade(-0.40).css()));
    s.push_str(&label(56.0, y + 20.0, 11.5, &t.chrome.hardware_mark.shade(0.62).css(), tk.name));
    s.push_str(&at(
        186.0,
        y + 3.0,
        &render_svg(
            v::PanningKnob,
            // Centred, like every track in the reference shot. A panned
            // knob here is honest demo data and reads as a broken
            // pointer, which is not what this render is for.
            v::PanProps { position: 0.0, large: false, indicator: true, width: NONE.0, height: NONE.1 },
        ),
    ));
    s.push_str(&meter(214.0, y + 8.0, 5.0, 15.0, 0.7));
    s.push_str(&meter(221.0, y + 8.0, 5.0, 15.0, 0.5));
    s.push_str(&meter(228.0, y + 8.0, 5.0, 15.0, 0.6));
    s.push_str(&at(
        242.0,
        y + 6.0,
        &render_svg(
            v::FxInButton,
            v::FxInProps {
                loaded: n % 2 == 0,
                cell: (29.0, 20.0),
                width: NONE.0,
                height: NONE.1,
                at: v::Interaction::Normal,
            },
        ),
    ));
    s.push_str(&at(
        274.0,
        y + 6.0,
        &render_svg(
            v::InputMonitorIndicator,
            v::MonitoringProps {
                state: if tk.armed { v::Monitoring::On } else { v::Monitoring::Off },
                cell: (15.0, 24.0),
                axis: v::Axis::Horizontal,
                width: NONE.0,
                height: NONE.1,
                at: v::Interaction::Normal,
            },
        ),
    ));

    // Row two: envelope, the FX slot, the input combo.
    s.push_str(&at(
        26.0,
        y + 32.0,
        &render_svg(
            v::EnvelopeButton,
            v::EnvelopeProps {
                mode: v::EnvelopeMode::Off,
                cell: (22.0, 20.0),
                width: NONE.0,
                height: NONE.1,
                at: v::Interaction::Normal,
            },
        ),
    ));
    s.push_str(&rect(56.0, y + 34.0, 34.0, 17.0, &t.chrome.hardware.shade(-0.40).css()));
    s.push_str(&label(62.0, y + 47.0, 10.0, &t.chrome.hardware_mark.shade(-0.1).css(), "FX"));
    // The input combo: no component, so a rectangle and a caret.
    s.push_str(&rect(94.0, y + 34.0, 186.0, 17.0, &t.chrome.hardware.shade(-0.40).css()));
    s.push_str(&label(100.0, y + 47.0, 11.0, &t.chrome.hardware_mark.shade(0.5).css(), "Input 1"));
    s.push_str(&format!(
        "<path d=\"M {} {} h 7 l -3.5 4 z\" fill=\"{}\"/>",
        266.0,
        y + 40.0,
        t.chrome.hardware_mark.shade(0.2).css()
    ));

    // Mute and solo live outside the tint, stacked.
    s.push_str(&rect(296.0, y, 44.0, 71.0, &t.chrome.hardware.shade(-0.40).css()));
    s.push_str(&at(306.0, y + 4.0, &mute((21.0, 24.0), (1.0 / 24.0, 20.0 / 24.0), true, tk.muted)));
    s.push_str(&at(306.0, y + 36.0, &solo((21.0, 24.0), (1.0 / 24.0, 20.0 / 24.0), true, tk.soloed)));
    s
}

fn mixer_strip(x: f32, n: u32, tk: &Track) -> String {
    let t = daw_theme::Theme::default();
    let g = |v: f32| t.chrome.hardware.shade(v);
    let mut s = String::new();

    s.push_str(&rect(x, 0.0, 84.0, 212.0, &t.chrome.hardware.shade(-0.40).css()));
    // REAPER tints the top of the strip with the track colour, behind the
    // pan row, and repeats it in the number band at the foot.
    s.push_str(&rect(x, 22.0, 84.0, 28.0, &tk.tint.css()));

    // The FX list — a real three-pill strip, drawn at one pill's height.
    s.push_str(&at(
        x + 3.0,
        4.0,
        &render_svg(
            v::ListStrip,
            v::ListStripProps {
                // One pill, not three: the mixer shows a single FX row
                // at the top of the strip and the three-state sprite is
                // only how REAPER packs it on disk.
                pills: vec![(g(0.09), g(-0.10), 1.0)],
                cell: (38.0, 19.0),
                rows: (2.0, 17.0),
                pill: 15.0,
                inset: 1.0,
                edge: None,
                width: Some(78),
                height: Some(19),
                at: v::Interaction::Normal,
            },
        ),
    ));

    // Pan, and the monitor icon beside it.
    s.push_str(&at(
        x + 8.0,
        26.0,
        &render_svg(
            v::PanningKnob,
            v::PanProps { position: 0.0, large: false, indicator: true, width: NONE.0, height: NONE.1 },
        ),
    ));
    s.push_str(&at(
        x + 58.0,
        24.0,
        &render_svg(
            v::RecordArmButton,
            v::RecordArmProps {
                state: if tk.armed { v::RecordArm::On } else { v::RecordArm::Off },
                cell: (20.0, 20.0),
                housing: false,
                width: NONE.0,
                height: NONE.1,
                at: v::Interaction::Normal,
            },
        ),
    ));
    s.push_str(&at(
        x + 58.0,
        50.0,
        &render_svg(
            v::InputMonitorIndicator,
            v::MonitoringProps {
                state: if tk.armed { v::Monitoring::On } else { v::Monitoring::Off },
                cell: (21.0, 20.0),
                axis: v::Axis::Vertical,
                width: NONE.0,
                height: NONE.1,
                at: v::Interaction::Normal,
            },
        ),
    ));

    // The scale, the fader and the meter. Neither the scale nor the meter
    // has a component.
    for (i, db) in ["-inf", "-6-", "-18-", "-30-", "-42-", "-54-"].iter().enumerate() {
        s.push_str(&label(
            x + 2.0,
            62.0 + i as f32 * 18.0,
            8.0,
            &t.chrome.hardware_mark.shade(-0.35).css(),
            db,
        ));
    }
    // The mixer's fader is its own pair of components — `VolumeFaderTrack`
    // and `VolumeFaderCap`, which draw `mcp_volbg` and `mcp_volthumb`.
    // `PanelSlider`'s thumb is the *track panel's* cap: 27 by 29, lying on
    // its side with a seam down it, where the mixer's is 27 by 53 and
    // ribbed across. Standing the wrong one in the mixer read as two pale
    // bars where the cap should be.
    s.push_str(&at(
        x + 24.0,
        54.0,
        &render_svg(
            v::VolumeFaderTrack,
            v::FaderCapProps {
                accent: None,
                full: true,
                width: Some(23),
                height: Some(104),
            },
        ),
    ));
    s.push_str(&at(
        x + 22.0,
        84.0,
        &render_svg(
            v::VolumeFaderCap,
            v::FaderCapProps { accent: None, full: false, width: Some(27), height: Some(53) },
        ),
    ));
    s.push_str(&meter(x + 46.0, 54.0, 5.0, 104.0, 0.62));
    s.push_str(&meter(x + 52.0, 54.0, 5.0, 104.0, 0.55));

    // Mute, solo, and the routing button under them.
    s.push_str(&at(x + 60.0, 74.0, &mute((21.0, 20.0), (0.0, 1.0), false, tk.muted)));
    s.push_str(&at(x + 60.0, 96.0, &solo((21.0, 20.0), (0.0, 1.0), false, tk.soloed)));
    s.push_str(&at(
        x + 60.0,
        120.0,
        &render_svg(
            v::RoutingButton,
            v::RoutingProps {
                has_sends: true,
                has_receives: false,
                disabled: false,
                axis: v::Axis::Vertical,
                cell: (23.0, 15.0),
                width: NONE.0,
                height: NONE.1,
                at: v::Interaction::Normal,
            },
        ),
    ));

    s.push_str(&label(
        x + 22.0,
        180.0,
        11.0,
        &t.chrome.hardware_mark.shade(0.5).css(),
        tk.name,
    ));
    // The number strip in the track's colour.
    s.push_str(&rect(x + 1.0, 190.0, 82.0, 17.0, &tk.tint.css()));
    s.push_str(&label(x + 39.0, 203.0, 11.0, "#f0f0f0", &n.to_string()));
    s
}

fn main() {
    let t = daw_theme::Theme::default();
    let hex = |r, g, b| Color::rgb(r, g, b);
    let tracks = [
        Track { name: "Kick", tint: hex(0xa6, 0x3a, 0x56), armed: true, muted: false, soloed: false },
        Track { name: "Snare", tint: hex(0xa6, 0x3a, 0x56), armed: true, muted: false, soloed: true },
        Track { name: "OH", tint: hex(0xa6, 0x3a, 0x56), armed: false, muted: false, soloed: false },
        Track { name: "Bass", tint: hex(0x3c, 0x5f, 0x9e), armed: false, muted: true, soloed: false },
        Track { name: "Gtr", tint: hex(0x2e, 0x8e, 0xc4), armed: false, muted: false, soloed: false },
    ];

    let (w, h) = (760.0f32, 600.0f32);
    // #333333, sampled from the arrange background. `surface` is
    // #3e3e3e, which is the *toolbar*, and using it made every
    // ground in this panel eleven levels light.
    let ground = t.chrome.hardware.shade(-0.19);
    let mut body = rect(0.0, 0.0, w, h, &ground.css());
    for (i, tk) in tracks.iter().enumerate() {
        body.push_str(&track_row(i as f32 * 71.0, i as u32 + 1, tk));
    }
    // The transport, in the bar under the track panel, as REAPER has it.
    body.push_str(&rect(0.0, 360.0, w, 34.0, &t.chrome.hardware.shade(-0.38).css()));
    for (i, glyph) in [
        v::TransportGlyph::Home,
        v::TransportGlyph::End,
        v::TransportGlyph::Stop,
        v::TransportGlyph::Play,
        v::TransportGlyph::Pause,
        v::TransportGlyph::Record,
    ]
    .into_iter()
    .enumerate()
    {
        body.push_str(&at(
            10.0 + i as f32 * 36.0,
            364.0,
            &render_svg(
                v::TransportButton,
                v::TransportProps {
                    glyph,
                    on: glyph == v::TransportGlyph::Play,
                    cell: (36.0, 26.0),
                    width: NONE.0,
                    height: NONE.1,
                    at: v::Interaction::Normal,
                },
            ),
        ));
    }
    body.push_str(&at(
        238.0,
        365.0,
        &render_svg(
            v::TransportButton,
            v::TransportProps {
                glyph: v::TransportGlyph::Repeat,
                on: false,
                cell: (32.0, 24.0),
                width: NONE.0,
                height: NONE.1,
                at: v::Interaction::Normal,
            },
        ),
    ));
    body.push_str(&rect(270.0, 365.0, 190.0, 24.0, &t.chrome.hardware.shade(-0.40).css()));
    body.push_str(&label(280.0, 381.0, 13.0, &t.chrome.hardware_mark.shade(0.5).css(), "1.1.00 / 0:00.000"));

    // And the mixer along the bottom.
    body.push_str(&rect(0.0, 400.0, w, 222.0, &ground.css()));
    for (i, tk) in tracks.iter().enumerate() {
        let strip = mixer_strip(i as f32 * 86.0 + 4.0, i as u32 + 1, tk);
        body.push_str(&format!("<g transform=\"translate(0 400)\">{strip}</g>"));
    }

    let doc = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" \
         viewBox=\"0 0 {w} {h}\">{body}</svg>"
    );

    let out = std::path::Path::new("target/theme-shots");
    std::fs::create_dir_all(out).unwrap();
    std::fs::write(out.join("native-panels.svg"), &doc).unwrap();
    // Twice the art's size, because the point is that it scales.
    match rasterise(&doc, (w * 2.0) as u32, (h * 2.0) as u32) {
        Ok(img) => {
            img.save(out.join("native-panels.png")).unwrap();
            println!("wrote {}", out.join("native-panels.png").display());
        }
        Err(e) => eprintln!("rasterise failed: {e}"),
    }
}
