//! `host-automation/1` across a real C ABI boundary: a `clap_host` in
//! memory whose `get_extension` answers the vtable, called through the
//! plugin's client as a plugin would.

use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_void};
use std::sync::{Arc, Mutex};

use clap_sys::host::clap_host;
use daw_clap_ext::automation::{
    AutomationPoint, By, Caller, HostAutomation, HostAutomationClient, HostAutomationLookup,
    HostAutomationVtable, LaneInfo, LaneMode, TimeRange, WriteFlags, WriteReport, merge_points,
};
use daw_clap_ext::{ExtError, sys};

/// A host keeping lanes per (plugin, param), with an undo list.
#[derive(Default)]
struct Host {
    /// Which params each plugin owns.
    owns: HashMap<Caller, Vec<u32>>,
    lanes: Mutex<HashMap<(Caller, u32), Vec<AutomationPoint>>>,
    open: Mutex<Option<String>>,
    /// Undo steps: their labels, and how many changes each grouped.
    undo: Mutex<Vec<(String, u32)>>,
}

impl Host {
    fn owned(&self, caller: Caller, param: u32) -> Result<(), ExtError> {
        self.owns
            .get(&caller)
            .is_some_and(|ps| ps.contains(&param))
            .then_some(())
            .ok_or_else(|| ExtError::NotFound(format!("parameter {param}")))
    }

    fn changed(&self, what: &str) {
        let open = self.open.lock().unwrap().clone();
        let mut undo = self.undo.lock().unwrap();
        match (open, undo.last_mut()) {
            (Some(label), Some((l, n))) if *l == label => *n += 1,
            (Some(label), _) => undo.push((label, 1)),
            (None, _) => undo.push((what.into(), 1)),
        }
    }
}

impl HostAutomation for Host {
    fn lanes(&self, caller: Caller) -> Result<Vec<LaneInfo>, ExtError> {
        let lanes = self.lanes.lock().unwrap();
        let mut out: Vec<LaneInfo> = lanes
            .iter()
            .filter(|((c, _), _)| *c == caller)
            .map(|((_, p), pts)| LaneInfo {
                param_id: *p,
                mode: LaneMode::Read,
                shown: true,
                points: u32::try_from(pts.len()).unwrap(),
                user_points: u32::try_from(pts.iter().filter(|p| p.by == By::User).count())
                    .unwrap(),
            })
            .collect();
        out.sort_by_key(|l| l.param_id);
        Ok(out)
    }

    fn read(
        &self,
        caller: Caller,
        param: u32,
        range: TimeRange,
    ) -> Result<Vec<AutomationPoint>, ExtError> {
        self.owned(caller, param)?;
        let lanes = self.lanes.lock().unwrap();
        let lane = lanes
            .get(&(caller, param))
            .ok_or_else(|| ExtError::NotFound("no lane".into()))?;
        Ok(lane
            .iter()
            .filter(|p| range.contains(p.t))
            .copied()
            .collect())
    }

    fn write(
        &self,
        caller: Caller,
        param: u32,
        range: TimeRange,
        points: &[AutomationPoint],
        flags: WriteFlags,
    ) -> Result<WriteReport, ExtError> {
        self.owned(caller, param)?;
        let mut lanes = self.lanes.lock().unwrap();
        let created = !lanes.contains_key(&(caller, param));
        if created && !flags.has(WriteFlags::CREATE_LANE) {
            return Err(ExtError::NotFound("no lane".into()));
        }
        let lane = lanes.entry((caller, param)).or_default();
        let (merged, kept) =
            merge_points(lane, range, points, flags.has(WriteFlags::KEEP_USER_EDITS));
        *lane = merged;
        let written = lane
            .iter()
            .filter(|p| p.by == By::Plugin && range.contains(p.t))
            .count();
        drop(lanes);
        self.changed("Write automation");
        Ok(WriteReport {
            written: u32::try_from(written).unwrap(),
            kept,
            created,
        })
    }

    fn clear(&self, caller: Caller, param: u32, range: TimeRange) -> Result<(), ExtError> {
        self.owned(caller, param)?;
        if let Some(l) = self.lanes.lock().unwrap().get_mut(&(caller, param)) {
            l.retain(|p| !range.contains(p.t));
        }
        self.changed("Clear automation");
        Ok(())
    }

    fn begin_edit(&self, _caller: Caller, label: &str) -> Result<(), ExtError> {
        let mut open = self.open.lock().unwrap();
        if open.is_some() {
            return Err(ExtError::Failed("an edit is open".into()));
        }
        *open = Some(label.into());
        Ok(())
    }

    fn end_edit(&self, _caller: Caller) -> Result<(), ExtError> {
        self.open
            .lock()
            .unwrap()
            .take()
            .map(|_| ())
            .ok_or_else(|| ExtError::Failed("no edit is open".into()))
    }
}

/// `host_data`: the host and the plugin this `clap_host` serves.
struct Data {
    host: Arc<dyn HostAutomation>,
    caller: Caller,
}

struct ByHostData;
impl HostAutomationLookup for ByHostData {
    unsafe fn lookup(host: *const clap_host) -> Option<(Arc<dyn HostAutomation>, Caller)> {
        let data = unsafe { (*host).host_data }.cast::<Data>();
        (!data.is_null()).then(|| unsafe { ((*data).host.clone(), (*data).caller) })
    }
}

unsafe extern "C" fn get_extension(_host: *const clap_host, id: *const c_char) -> *const c_void {
    if unsafe { CStr::from_ptr(id) } == sys::FTS_EXT_HOST_AUTOMATION {
        HostAutomationVtable::<ByHostData>::as_ptr()
    } else {
        std::ptr::null()
    }
}

fn clap_host_for(host: Arc<Host>, caller: Caller) -> clap_host {
    // SAFETY: an all-null clap_host is valid; we fill what the test uses.
    let mut raw: clap_host = unsafe { std::mem::zeroed() };
    raw.host_data = Box::into_raw(Box::new(Data { host, caller })).cast();
    raw.get_extension = Some(get_extension);
    raw
}

fn free(raw: &clap_host) {
    drop(unsafe { Box::from_raw(raw.host_data.cast::<Data>()) });
}

#[test]
fn a_plugin_writes_reads_and_clears_its_own_lanes() {
    let (signal, other) = (Caller(1), Caller(2));
    let host = Arc::new(Host {
        owns: HashMap::from([(signal, vec![1407, 1408]), (other, vec![9])]),
        ..Host::default()
    });
    let raw = clap_host_for(host.clone(), signal);
    let client = unsafe { HostAutomationClient::query(&raw const raw) }.expect("offered");

    let curve: Vec<_> = (0..10)
        .map(|i| AutomationPoint::new(f64::from(i), f64::from(i) / 10.0))
        .collect();
    // No lane, no CREATE_LANE: refused, with the host's words.
    match client.write(1407, TimeRange::ALL, &curve, WriteFlags::default()) {
        Err(ExtError::Failed(why)) => assert!(why.contains("no lane"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert!(client.begin_edit("Analyse the vocal"));
    assert!(!client.begin_edit("again"), "not nested");
    let r = client
        .write(
            1407,
            TimeRange::ALL,
            &curve,
            WriteFlags::CREATE_LANE.with(WriteFlags::SHOW_LANE),
        )
        .unwrap();
    assert_eq!((r.written, r.created), (10, true));
    let r = client
        .write(
            1408,
            TimeRange::new(0.0, 5.0),
            &curve,
            WriteFlags::CREATE_LANE,
        )
        .unwrap();
    assert_eq!(r.written, 5, "only the points inside [0, 5)");
    assert!(client.end_edit());
    assert!(!client.end_edit());
    assert_eq!(
        *host.undo.lock().unwrap(),
        [("Analyse the vocal".to_owned(), 2)],
        "two writes, one undo step"
    );

    let lanes = client.lanes().unwrap();
    assert_eq!(
        lanes
            .iter()
            .map(|l| (l.param_id, l.points))
            .collect::<Vec<_>>(),
        [(1407, 10), (1408, 5)]
    );
    let read = client.read(1407, TimeRange::new(2.0, 4.0)).unwrap();
    assert_eq!(read, curve[2..4]);
    assert!(read.iter().all(|p| p.by == By::Plugin));

    assert!(client.clear(1407, TimeRange::new(5.0, f64::INFINITY)));
    assert_eq!(client.read(1407, TimeRange::ALL).unwrap().len(), 5);

    // Another plugin's parameter is not this one's to touch.
    assert!(client.read(9, TimeRange::ALL).is_err());
    assert!(
        client
            .write(9, TimeRange::ALL, &curve, WriteFlags::CREATE_LANE)
            .is_err()
    );
    assert!(!client.clear(9, TimeRange::ALL));
    free(&raw);

    // A host without the extension.
    let mut bare: clap_host = unsafe { std::mem::zeroed() };
    bare.get_extension = None;
    assert!(unsafe { HostAutomationClient::query(&raw const bare) }.is_none());
}

#[test]
fn keep_user_edits_keeps_a_redrawn_point_and_says_so() {
    let signal = Caller(1);
    let host = Arc::new(Host {
        owns: HashMap::from([(signal, vec![7])]),
        ..Host::default()
    });
    let raw = clap_host_for(host.clone(), signal);
    let client = unsafe { HostAutomationClient::query(&raw const raw) }.unwrap();
    let first: Vec<_> = (0..5)
        .map(|i| AutomationPoint::new(f64::from(i), 0.5))
        .collect();
    client
        .write(7, TimeRange::ALL, &first, WriteFlags::CREATE_LANE)
        .unwrap();
    // The user drags the point at 2 s.
    host.lanes.lock().unwrap().get_mut(&(signal, 7)).unwrap()[2] =
        AutomationPoint::new(2.0, 0.9).by(By::User);
    let read = client.read(7, TimeRange::ALL).unwrap();
    assert_eq!(read[2].by, By::User, "the host says who wrote it");

    let again: Vec<_> = (0..9)
        .map(|i| AutomationPoint::new(f64::from(i) * 0.5, 0.2))
        .collect();
    let r = client
        .write(7, TimeRange::ALL, &again, WriteFlags::KEEP_USER_EDITS)
        .unwrap();
    assert_eq!(r.kept, [AutomationPoint::new(2.0, 0.9).by(By::User)]);
    let now = client.read(7, TimeRange::ALL).unwrap();
    let at = |t: f64| now.iter().find(|p| p.t == t).copied();
    assert_eq!(at(2.0).map(|p| (p.value, p.by)), Some((0.9, By::User)));
    assert!(
        at(1.5).is_none() && at(2.5).is_none(),
        "its stretch is the user's"
    );
    assert_eq!(
        at(1.0).map(|p| p.value),
        Some(0.2),
        "outside it the plugin rewrote"
    );
    free(&raw);
}
