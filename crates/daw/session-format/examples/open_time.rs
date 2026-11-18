//! How long a project's song takes to load from its store, before and
//! after its history is compacted into a snapshot.
//!
//! `cargo run --release -p session-format --example open_time -- <a COPY of Song.session>`
//!
//! Compacts the copy it is given: never point it at the original.

use std::time::Instant;

use session_format::store::{MAIN, Store};

fn main() {
    let path = std::env::args().nth(1).expect("a copy of a .session file");
    let store = Store::open(std::path::Path::new(&path)).expect("the store opens");
    let t = Instant::now();
    let doc = store.load(MAIN).expect("main loads");
    println!(
        "load as stored:    {:>8.1} ms",
        t.elapsed().as_secs_f64() * 1e3
    );
    let t = Instant::now();
    store.compact(MAIN, &doc).expect("compacts");
    println!(
        "compact:           {:>8.1} ms",
        t.elapsed().as_secs_f64() * 1e3
    );
    drop(doc);
    let t = Instant::now();
    let doc = store.load(MAIN).expect("main loads again");
    println!(
        "load compacted:    {:>8.1} ms",
        t.elapsed().as_secs_f64() * 1e3
    );
    let t = Instant::now();
    let value = doc.get_deep_value();
    println!(
        "read every value:  {:>8.1} ms  ({} top-level)",
        t.elapsed().as_secs_f64() * 1e3,
        match value {
            loro::LoroValue::Map(m) => m.len(),
            _ => 0,
        }
    );
}
