//! What happens to scenario 1's five item envelopes on the way out.
//!
//! #149 left this as the one genuine *unknown* rather than merely
//! unspecified: gating, compression, sibilance and de-breathing are
//! four separate envelopes composited into the item's one volume
//! envelope, and whether the four can be reconstructed after a round
//! trip depends on how REAPER and Bitwig each represent item volume
//! automation. Neither was known when the map was written.
//!
//! The answer, pinned here: **neither format has room for the four.**
//! REAPER's item chunk has exactly one volume envelope, and DAWproject
//! has no home for editor-generated automation at all. So the contract
//! is that the *composite* crosses and the sources stay in the `.daw`.
//!
//! That is a limitation, not a bug, but it must be a *stated* one. The
//! failure it is guarding against is silent: exporting to `.rpp`,
//! reopening, and finding the editor offering to recompute a composite
//! from four envelopes that no longer exist.
//!
//! The DAWproject half is interchange's `tests/composite_envelope.rs`.

use std::path::Path;

use daw_proto::automation::{EnvelopePoint, EnvelopeType};
use dawfile_reaper::session::RppProject;
use session_format::{DawProject, DocumentEdit, DocumentQuery, EntityId};

/// The four concerns scenario 1 generates, in the order the editor
/// composites them.
const SOURCES: [&str; 4] = ["Gating", "Compression", "Sibilance", "De-Breathing"];

fn point(t: f64, v: f64) -> EnvelopePoint {
    EnvelopePoint {
        time: daw_proto::PositionInSeconds::from_seconds(t),
        value: v,
        ..Default::default()
    }
}

/// A project carrying the four sources and their composite on a real
/// item.
///
/// Built on a fixture rather than from scratch because #156's export is
/// verbatim-source plus minimal patch: a document with no `.rpp`
/// provenance has nothing to patch against and refuses to export. That
/// is the design, so the test has to respect it.
fn scenario_one() -> (DawProject, EntityId) {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../features/adapters/reaper/dawfile-reaper/tests/fixtures/setlist/song_a.RPP");
    let (mut project, _) = DawProject::import_rpp_file(&fixture).expect("import fixture");

    let item = project.edit(|d| {
        let item = d
            .tracks
            .iter()
            .find_map(|t| t.items.first())
            .map(|i| i.id.clone())
            .expect("the fixture has an item");

        for name in SOURCES {
            let env = d
                .add_item_envelope(&item, EnvelopeType::Volume, name)
                .expect("source envelope");
            d.set_envelope_points(&env, vec![point(1.0, 0.5), point(2.0, 0.9)])
                .expect("points");
        }

        let composite = d
            .add_item_envelope(&item, EnvelopeType::Volume, "VOLENV")
            .expect("composite");
        d.set_envelope_points(&composite, vec![point(1.0, 0.25), point(2.0, 0.81)])
            .expect("points");
        item
    });

    (project, item)
}

#[test]
fn the_daw_format_keeps_all_five() {
    // The baseline the exports are measured against: natively, nothing
    // is lost, which is why the native format exists.
    let (project, item) = scenario_one();
    assert_eq!(
        project
            .document()
            .item(&item)
            .expect("item")
            .envelopes
            .len(),
        5
    );
}

#[test]
fn only_one_volume_envelope_reaches_the_rpp() {
    // REAPER's item chunk has one volume envelope. Writing five would
    // produce a file REAPER either rejects or silently reads as the
    // last one — a corrupt project either way.
    let (project, _) = scenario_one();
    let text = project.to_rpp_patched().expect("export").0;

    let volenvs = text.matches("<VOLENV").count();
    assert_eq!(
        volenvs, 1,
        "expected exactly one item volume envelope in the .rpp, found \
         {volenvs}:\n{text}"
    );
}

#[test]
fn the_composite_is_the_one_that_crosses() {
    // And it must be the *composite*, not whichever source happened to
    // sort first — the composite is the only one that describes what
    // the item should sound like.
    let (project, _) = scenario_one();
    let text = project.to_rpp_patched().expect("export").0;

    // The composite's second point is 0.81; every source's is 0.9.
    assert!(
        text.contains("0.81"),
        "the composite's points are not in the exported .rpp:\n{text}"
    );
}

#[test]
fn reimporting_reports_the_sources_as_lost() {
    // The silent failure this whole file exists to prevent: a document
    // that came back from a round trip must not look like one that
    // still has four editable sources.
    let (project, _) = scenario_one();
    let text = project.to_rpp_patched().expect("export").0;
    let (back, _) = DawProject::import_rpp(&text, "Composite").expect("reimport");

    let item = back
        .document()
        .tracks
        .iter()
        .find_map(|t| t.items.first())
        .expect("an item")
        .clone();

    let named: Vec<&str> = item
        .envelopes
        .iter()
        .map(|e| e.envelope.name.as_str())
        .collect();
    for source in SOURCES {
        assert!(
            !named.contains(&source),
            "'{source}' came back from a .rpp round trip, but REAPER has \
             nowhere to have stored it — it must be reported lost, not \
             resurrected empty: {named:?}"
        );
    }
    assert_eq!(
        item.envelopes.len(),
        1,
        "the composite alone should survive: {named:?}"
    );
}
