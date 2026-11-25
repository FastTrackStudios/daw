//! `host-timeline/1` across a real C ABI boundary: a `clap_host` built in
//! memory answers the extension through [`HostTimelineVtable`], and the
//! plugin's [`HostTimelineClient`] reads it the way a plugin would — the
//! revision from another thread, the timeline through a `clap_ostream`,
//! the playback claim.

use std::ffi::{CStr, c_char, c_void};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use clap_sys::host::clap_host;
use clap_sys::version::CLAP_VERSION;
use daw_clap_ext::host::{HostTimeline, HostTimelineLookup, HostTimelineVtable};
use daw_clap_ext::plugin::HostTimelineClient;
use daw_clap_ext::sys;
use daw_clap_ext::timeline::{Control, Ctrl, Note, Tempo, TempoShape, Timeline};

/// One instance's track, as Studio would answer it.
struct Track {
    revision: AtomicU64,
    timeline: Mutex<Option<Timeline>>,
    claimed: AtomicBool,
}

impl HostTimeline for Track {
    fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }
    fn read(&self) -> Option<Timeline> {
        let mut t = self.timeline.lock().unwrap().clone()?;
        t.revision = self.revision();
        Some(t)
    }
    fn set_playback_claimed(&self, claimed: bool) -> bool {
        self.claimed.store(claimed, Ordering::Release);
        true
    }
}

struct ByHostData;
impl HostTimelineLookup for ByHostData {
    unsafe fn lookup(host: *const clap_host) -> Option<Arc<dyn HostTimeline>> {
        let data = unsafe { (*host).host_data }.cast::<Arc<dyn HostTimeline>>();
        (!data.is_null()).then(|| unsafe { (*data).clone() })
    }
}

unsafe extern "C" fn get_extension(_host: *const clap_host, id: *const c_char) -> *const c_void {
    if unsafe { CStr::from_ptr(id) } == sys::FTS_EXT_HOST_TIMELINE {
        HostTimelineVtable::<ByHostData>::as_ptr()
    } else {
        std::ptr::null()
    }
}

unsafe extern "C" fn no_extension(_host: *const clap_host, _id: *const c_char) -> *const c_void {
    std::ptr::null()
}

unsafe extern "C" fn noop(_host: *const clap_host) {}

fn host(data: *mut c_void, ext: bool) -> clap_host {
    clap_host {
        clap_version: CLAP_VERSION,
        host_data: data,
        name: c"test".as_ptr(),
        vendor: c"test".as_ptr(),
        url: c"".as_ptr(),
        version: c"0".as_ptr(),
        get_extension: Some(if ext { get_extension } else { no_extension }),
        request_restart: Some(noop),
        request_process: Some(noop),
        request_callback: Some(noop),
    }
}

/// A legato line on channel 0 and a second line on channel 1, with a CC,
/// a bend, and a tempo change.
fn song() -> Timeline {
    let note = |start_qn, end_qn, channel, key, velocity| Note {
        start_qn,
        end_qn,
        channel,
        key,
        velocity,
        release_velocity: 64,
    };
    Timeline {
        revision: 0,
        end_qn: 12.0,
        notes: vec![
            note(0.0, 2.1, 0, 60, 90),
            note(0.0, 4.0, 1, 48, 70),
            note(2.0, 4.1, 0, 62, 30),
            note(4.0, 6.0, 0, 65, 110),
        ],
        ctrls: vec![
            Ctrl {
                qn: 0.0,
                channel: 0,
                control: Control::Cc {
                    controller: 58,
                    value: 2,
                },
            },
            Ctrl {
                qn: 3.0,
                channel: 1,
                control: Control::PitchBend(8_192 + 512),
            },
        ],
        tempo: vec![
            Tempo {
                qn: 0.0,
                seconds: 0.0,
                bpm: 120.0,
                numerator: 4,
                denominator: 4,
                shape: TempoShape::Step,
            },
            Tempo {
                qn: 8.0,
                seconds: 4.0,
                bpm: 96.0,
                numerator: 4,
                denominator: 4,
                shape: TempoShape::Step,
            },
        ],
    }
}

#[test]
fn the_plugin_reads_its_track_over_the_c_abi() {
    let track = Arc::new(Track {
        revision: AtomicU64::new(0),
        timeline: Mutex::new(None),
        claimed: AtomicBool::new(false),
    });
    let mut data: Arc<dyn HostTimeline> = track.clone();
    let raw = host(std::ptr::from_mut(&mut data).cast(), true);

    let client = unsafe { HostTimelineClient::query(&raw const raw) }.expect("offered");

    // Not on a track yet: revision 0, nothing to read.
    assert_eq!(client.revision(), 0);
    assert!(client.read().is_err());

    // Placed on a track: the host publishes a timeline and a revision.
    *track.timeline.lock().unwrap() = Some(song());
    track.revision.store(3, Ordering::Release);

    // The revision is thread-safe: read it from another thread, as an
    // audio thread would.
    let seen = std::thread::scope(|s| s.spawn(|| client.revision()).join().unwrap());
    assert_eq!(seen, 3);

    let got = client.read().expect("a timeline");
    let mut want = song();
    want.revision = 3;
    assert_eq!(got, want, "every note, controller and tempo point survives");
    assert_eq!(got.channels(), vec![0, 1]);

    // An edit moves the revision, and the next read is the new content.
    track.timeline.lock().unwrap().as_mut().unwrap().notes.pop();
    track.revision.store(4, Ordering::Release);
    let got = client.read().unwrap();
    assert_eq!((got.revision, got.notes.len()), (4, 3));

    // The playback claim reaches the host, both ways.
    assert!(client.set_playback_claimed(true));
    assert!(track.claimed.load(Ordering::Acquire));
    assert!(client.set_playback_claimed(false));
    assert!(!track.claimed.load(Ordering::Acquire));
}

#[test]
fn a_host_without_the_extension_offers_none() {
    let raw = host(std::ptr::null_mut(), false);
    assert!(unsafe { HostTimelineClient::query(&raw const raw) }.is_none());
}

#[test]
fn a_dense_track_round_trips() {
    // 20 000 notes over 16 channels: a full orchestral track.
    let notes: Vec<Note> = (0..20_000u32)
        .map(|i| Note {
            start_qn: f64::from(i) * 0.25,
            end_qn: f64::from(i).mul_add(0.25, 0.3),
            channel: (i % 16) as u8,
            key: (36 + i % 60) as u8,
            velocity: (1 + i % 127) as u8,
            release_velocity: (i % 128) as u8,
        })
        .collect();
    let t = Timeline {
        revision: 9,
        end_qn: 5_001.0,
        notes,
        ..song()
    };
    let track = Arc::new(Track {
        revision: AtomicU64::new(9),
        timeline: Mutex::new(Some(t.clone())),
        claimed: AtomicBool::new(false),
    });
    let mut data: Arc<dyn HostTimeline> = track;
    let raw = host(std::ptr::from_mut(&mut data).cast(), true);
    let client = unsafe { HostTimelineClient::query(&raw const raw) }.unwrap();
    assert_eq!(client.read().unwrap(), t);
}
