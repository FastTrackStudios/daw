//! iOS: the devices are the audio session's ports.
//!
//! cpal's iOS host has one device each way, "Default Device" — the route
//! the shared `AVAudioSession` has chosen. What a player chooses between
//! (a USB interface, the built-in mic, a headset) are the session's input
//! *ports*, so on iOS those are the input devices: listing them reads
//! `availableInputs`, and opening one by name makes it the session's
//! preferred input before the default device opens — which then plays
//! through it. Outputs cannot be chosen by an app (the system routes them,
//! to a connected interface on its own), so the output devices are the
//! ports the current route plays through.
//!
//! `availableInputs` is empty unless the session's category records
//! (`playAndRecord`); the app configures the session before it lists.
//! Dynamic `objc2` messaging, as the app's own session code does — no
//! AVFAudio bindings crate.

use objc2::runtime::{AnyObject, Bool};
use objc2::{class, msg_send};

/// A port's name.
unsafe fn port_name(port: *mut AnyObject) -> String {
    let name: *mut AnyObject = msg_send![port, portName];
    if name.is_null() {
        return String::new();
    }
    let c: *const std::os::raw::c_char = msg_send![name, UTF8String];
    if c.is_null() {
        return String::new();
    }
    std::ffi::CStr::from_ptr(c).to_string_lossy().into_owned()
}

/// How many channels a port carries (0 when it does not say).
unsafe fn port_channels(port: *mut AnyObject) -> u16 {
    let channels: *mut AnyObject = msg_send![port, channels];
    if channels.is_null() {
        return 0;
    }
    let n: usize = msg_send![channels, count];
    n as u16
}

unsafe fn session() -> *mut AnyObject {
    msg_send![class!(AVAudioSession), sharedInstance]
}

/// Each port of an `NSArray` of `AVAudioSessionPortDescription`s.
unsafe fn ports(list: *mut AnyObject) -> Vec<*mut AnyObject> {
    if list.is_null() {
        return Vec::new();
    }
    let count: usize = msg_send![list, count];
    (0..count).map(|i| msg_send![list, objectAtIndex: i]).collect()
}

/// The session's rate now (what the route plays at).
pub(crate) fn sample_rate() -> u32 {
    let rate: f64 = unsafe { msg_send![session(), sampleRate] };
    rate.round() as u32
}

/// The inputs the session can record from: name and channel count.
pub(crate) fn inputs() -> Vec<(String, u16)> {
    unsafe {
        let list: *mut AnyObject = msg_send![session(), availableInputs];
        ports(list)
            .into_iter()
            .map(|p| (port_name(p), port_channels(p)))
            .collect()
    }
}

/// The outputs the current route plays through.
pub(crate) fn outputs() -> Vec<(String, u16)> {
    unsafe {
        let route: *mut AnyObject = msg_send![session(), currentRoute];
        if route.is_null() {
            return Vec::new();
        }
        let list: *mut AnyObject = msg_send![route, outputs];
        ports(list)
            .into_iter()
            .map(|p| (port_name(p), port_channels(p)))
            .collect()
    }
}

/// Make the input whose name contains `name` the session's preferred input.
/// Whether one did.
pub(crate) fn prefer_input(name: &str) -> bool {
    unsafe {
        let list: *mut AnyObject = msg_send![session(), availableInputs];
        let Some(port) = ports(list).into_iter().find(|p| port_name(*p).contains(name)) else {
            return false;
        };
        let set: Bool = msg_send![
            session(),
            setPreferredInput: port,
            error: std::ptr::null_mut::<*mut AnyObject>()
        ];
        set.as_bool()
    }
}

/// Whether the player has let the app record — on iOS an interface's input
/// is a recording, and without this it is silent (and the session may list
/// no inputs at all). `AVAudioApplication` from iOS 17 (the session's own
/// `recordPermission` is deprecated there), the session's before.
pub(crate) fn input_access() -> crate::device::InputAccess {
    use crate::device::InputAccess;
    // The four-char codes both classes answer with.
    const GRANTED: usize = 0x6772_6e74; // 'grnt'
    const DENIED: usize = 0x6465_6e79; // 'deny'
    let code: usize = unsafe {
        match objc2::runtime::AnyClass::get(c"AVAudioApplication") {
            Some(app) => {
                let shared: *mut AnyObject = msg_send![app, sharedInstance];
                msg_send![shared, recordPermission]
            }
            None => msg_send![session(), recordPermission],
        }
    };
    match code {
        GRANTED => InputAccess::Granted,
        DENIED => InputAccess::Denied,
        _ => InputAccess::Undetermined,
    }
}
