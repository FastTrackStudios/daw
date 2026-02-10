use dawfile_reaper::{parse_rpp_file, ReaperProject};

fn parse_marker_position_label(label: &str) -> Option<(i32, i32, f64)> {
    let mut parts = label.split('.');
    let measure = parts.next()?.parse::<i32>().ok()?;
    let beat = parts.next()?.parse::<i32>().ok()?;
    let frac_raw = parts.next()?.parse::<i32>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((measure, beat, frac_raw as f64 / 1000.0))
}

#[test]
fn advanced_tempo_map_marker_positions_match_labels() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("tempo-map-advanced.RPP");

    let content = std::fs::read_to_string(&fixture)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", fixture.display()));
    let rpp = parse_rpp_file(&content).expect("parse_rpp_file failed");
    let project = ReaperProject::from_rpp_project(&rpp).expect("from_rpp_project failed");

    let envelope = project
        .tempo_envelope
        .as_ref()
        .expect("fixture must contain TEMPOENVEX");

    let mut checked = 0usize;
    let mut failures = Vec::new();
    for marker in &project.markers_regions.markers {
        if !marker.is_marker() {
            continue;
        }

        let Some((exp_measure, exp_beat, exp_frac)) = parse_marker_position_label(&marker.name) else {
            continue;
        };

        let (act_measure, act_beat, act_frac) = envelope.musical_position_at_time(marker.position);

        if act_measure != exp_measure
            || act_beat != exp_beat
            || (act_frac - exp_frac).abs() > 0.02
        {
            failures.push(format!(
                "marker={} time={:.12} expected={}.{}.{:03} actual={}.{}.{:03}",
                marker.name,
                marker.position,
                exp_measure,
                exp_beat,
                (exp_frac * 1000.0).round() as i32,
                act_measure,
                act_beat,
                (act_frac * 1000.0).round() as i32
            ));
        }

        checked += 1;
    }

    assert!(checked >= 8, "expected to validate many marker labels, got {checked}");
    assert!(
        failures.is_empty(),
        "tempo marker mismatches:\n{}",
        failures.join("\n")
    );
}

#[test]
fn advanced_tempo_map_beats_monotonic() {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("tempo-map-advanced.RPP");

    let content = std::fs::read_to_string(&fixture)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", fixture.display()));
    let rpp = parse_rpp_file(&content).expect("parse_rpp_file failed");
    let project = ReaperProject::from_rpp_project(&rpp).expect("from_rpp_project failed");

    let envelope = project
        .tempo_envelope
        .as_ref()
        .expect("fixture must contain TEMPOENVEX");

    let mut previous = 0.0f64;
    for i in 0..=500 {
        let t = (i as f64) * 0.06;
        let beats = envelope.beats_at_time(t);
        assert!(
            beats + 1e-9 >= previous,
            "beats must be monotonic: t={t:.3} previous={previous:.6} current={beats:.6}"
        );
        previous = beats;
    }
}
