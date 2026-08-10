//! The quantize surface (#165).
//!
//! The engine was complete and unreachable. These cover the surface's
//! own claims: that the plan is previewable before anything is written,
//! that "why did that hit not move" is answerable, and that it works for
//! MIDI notes as well as audio transients.

use expression_editor_tools::event::Timed;
use expression_editor_tools::quantize::QuantizeConfig;
use expression_editor_ui::quantize_panel::{
    Group, QuantizePanel, WriteMode, excluded_count, histogram, preview, threshold_position,
};

/// A hit with a strength — stands in for an audio transient.
#[derive(Clone, Copy, Debug)]
struct Hit {
    at: f64,
    weight: f64,
}

impl Timed for Hit {
    fn onset(&self) -> f64 {
        self.at
    }
    fn move_to(&mut self, to: f64) {
        self.at = to;
    }
    fn strength(&self) -> f64 {
        self.weight
    }
}

/// A MIDI note — the other half of "this is a tool panel, not an audio
/// panel".
#[derive(Clone, Copy, Debug)]
struct Note {
    start: f64,
    velocity: f64,
}

impl Timed for Note {
    fn onset(&self) -> f64 {
        self.start
    }
    fn move_to(&mut self, to: f64) {
        self.start = to;
    }
    fn strength(&self) -> f64 {
        self.velocity
    }
}

fn panel(grid: f64) -> QuantizePanel {
    QuantizePanel {
        config: QuantizeConfig {
            grid,
            ..QuantizeConfig::default()
        },
        ..Default::default()
    }
}

fn late_hits() -> Vec<Hit> {
    (1..5)
        .map(|i| Hit {
            at: i as f64 + 0.02,
            weight: 0.9,
        })
        .collect()
}

#[test]
fn a_plan_is_previewable_before_anything_is_written() {
    let (plan, view) = preview(&late_hits(), &panel(1.0), 400.0, (0.0, 5.0));
    assert_eq!(plan.moves.len(), 4);
    assert_eq!(view.lines.len(), 4);
    assert!(view.lines.iter().all(|l| l.moves()), "every line shows a move");
}

#[test]
fn the_grid_is_drawn_so_the_target_is_visible_not_implied() {
    let (_, view) = preview(&late_hits(), &panel(1.0), 500.0, (0.0, 5.0));
    assert_eq!(view.divisions.len(), 6, "0 through 5 inclusive");
    assert!(view.divisions.iter().all(|x| (0.0..=500.0).contains(x)));
}

#[test]
fn an_unmatched_hit_is_shown_rather_than_omitted() {
    // `Plan::unmatched` exists so "why did that hit not move" is
    // answerable, and a line that is simply absent answers nothing.
    let hits = vec![
        Hit {
            at: 1.0,
            weight: 0.9,
        },
        Hit {
            at: 1.5,
            weight: 0.05,
        },
    ];
    let mut p = panel(1.0);
    p.config.min_strength = 0.5;

    let (plan, view) = preview(&hits, &p, 400.0, (0.0, 3.0));
    assert!(!plan.unmatched.is_empty(), "the weak hit was left alone");
    let weak: Vec<_> = view.lines.iter().filter(|l| l.unmatched).collect();
    assert_eq!(weak.len(), 1, "and it is on screen");
    assert!(!weak[0].moves(), "drawn where it is, not where it would go");
}

#[test]
fn lines_are_ordered_left_to_right() {
    let hits = vec![
        Hit {
            at: 3.02,
            weight: 0.9,
        },
        Hit {
            at: 1.02,
            weight: 0.9,
        },
    ];
    let (_, view) = preview(&hits, &panel(1.0), 400.0, (0.0, 4.0));
    for w in view.lines.windows(2) {
        assert!(w[0].x <= w[1].x, "a preview that jumps around is unreadable");
    }
}

#[test]
fn the_panel_works_for_midi_notes_too() {
    // Quantize is a tool, so this is a tool panel — not an audio panel.
    let notes: Vec<Note> = (1..4)
        .map(|i| Note {
            start: i as f64 + 0.03,
            velocity: 0.8,
        })
        .collect();
    let (plan, view) = preview(&notes, &panel(1.0), 300.0, (0.0, 4.0));
    assert_eq!(plan.moves.len(), 3);
    assert_eq!(view.lines.len(), 3);
}

#[test]
fn geometry_stays_inside_the_panel() {
    let hits = vec![
        Hit {
            at: -5.0,
            weight: 0.9,
        },
        Hit {
            at: 500.0,
            weight: 0.9,
        },
    ];
    let (_, view) = preview(&hits, &panel(1.0), 200.0, (0.0, 5.0));
    for l in &view.lines {
        assert!((0.0..=200.0).contains(&l.x), "x escaped: {}", l.x);
        assert!((0.0..=200.0).contains(&l.to_x));
    }
}

// ── The sensitivity histogram ────────────────────────────────────────

#[test]
fn the_histogram_shows_where_the_hits_actually_sit() {
    let hits = vec![
        Hit { at: 0.0, weight: 0.05 },
        Hit { at: 1.0, weight: 0.09 },
        Hit { at: 2.0, weight: 0.90 },
        Hit { at: 3.0, weight: 0.95 },
    ];
    let bins = histogram(&hits, 10);
    assert_eq!(bins.len(), 10);
    assert_eq!(bins[0].count, 2, "the quiet cluster");
    assert_eq!(bins[9].count, 2, "and the loud one");
    assert_eq!(bins.iter().map(|b| b.count).sum::<usize>(), 4);
}

#[test]
fn the_top_of_the_range_lands_in_the_last_bin_not_off_the_end() {
    let hits = vec![Hit {
        at: 0.0,
        weight: 1.0,
    }];
    let bins = histogram(&hits, 4);
    assert_eq!(bins[3].count, 1);
}

#[test]
fn the_threshold_is_placed_on_the_same_scale_as_the_bins() {
    // What makes the slider legible against the material's own floor.
    let mut cfg = QuantizeConfig::default();
    cfg.min_strength = 0.3;
    assert_eq!(threshold_position(&cfg), 0.3);
    cfg.min_strength = 5.0;
    assert_eq!(threshold_position(&cfg), 1.0, "clamped onto the axis");
}

#[test]
fn the_panel_can_say_how_much_it_is_excluding() {
    let hits = vec![
        Hit { at: 0.0, weight: 0.1 },
        Hit { at: 1.0, weight: 0.2 },
        Hit { at: 2.0, weight: 0.9 },
    ];
    let mut cfg = QuantizeConfig::default();
    cfg.min_strength = 0.5;
    assert_eq!(excluded_count(&hits, &cfg), 2);
}

// ── Mode and group ───────────────────────────────────────────────────

#[test]
fn split_is_the_default_mode() {
    // Drum editing is done by splitting, because it is phase-coherent
    // across a mic group.
    assert_eq!(QuantizePanel::default().mode, WriteMode::Split);
}

#[test]
fn a_group_needs_its_trigger_to_be_one_of_its_members() {
    // Otherwise the group is edited against a reference nobody can hear.
    let mut g = Group {
        members: vec!["snare".into(), "oh-l".into()],
        trigger: Some("snare".into()),
    };
    assert!(g.is_valid());

    g.trigger = Some("a-track-not-in-the-group".into());
    assert!(!g.is_valid());

    g.trigger = None;
    assert!(!g.is_valid(), "something has to be detected from");
}
