//! The **real** Light Guide transport for MK3: a raw USB **bulk** write to the
//! device's vendor interface (interface 4, endpoint 4).
//!
//! This mirrors tillt/KompleteSynthesia (the only open MK3 implementation).
//! See `docs/protocol.md` for the byte-level derivation and citations.
//!
//! ## Confirmed vs. unconfirmed
//! - CONFIRMED (from tillt, high confidence): MK3 lights keys via USB bulk on
//!   interface 4 / endpoint 4; frame = `u32 len + 15-byte prefix +
//!   128×{0x92,key,color}`; the color byte is the shared MkII/MK3 palette index.
//! - The S88 MK3 USB **product id `0x2120`** — tillt's source lists it only as
//!   a placeholder guess, but it is CONFIRMED by enumeration on this rig
//!   (probe `list`, 2026-07-11). Still UNCONFIRMED: the exact MK3 **init**
//!   sequence and whether interface-4 bulk writes actually light LEDs (needs a
//!   run of `examples/probe.rs usb` at the machine).

use std::time::Duration;

use eyre::{eyre, Result};
use rusb::{Context, DeviceHandle, UsbContext};

use crate::LightColor;

/// Native Instruments USB vendor id.
pub const NI_VID: u16 = 0x17cc;

/// USB product ids for the Komplete Kontrol MK3 keyboards.
/// Only `S61_MK3` is confirmed in the reference source; the others are the
/// placeholders tillt/KompleteSynthesia guessed — verify by enumeration.
pub const PID_S49_MK3: u16 = 0x2100; // UNCONFIRMED (placeholder)
pub const PID_S61_MK3: u16 = 0x2110; // confirmed (tillt)
pub const PID_S88_MK3: u16 = 0x2120; // CONFIRMED by enumeration on this rig (2026-07-11)

/// Every MK3 product id we will try to open, most-likely-for-this-machine first.
pub const MK3_PIDS: &[u16] = &[PID_S88_MK3, PID_S61_MK3, PID_S49_MK3];

/// The vendor interface the Light Guide bulk endpoint lives on.
const MK3_INTERFACE: u8 = 0x04;
/// The bulk OUT endpoint address (endpoint number 4, OUT direction).
const MK3_ENDPOINT_OUT: u8 = 0x04;

/// 15-byte Light Guide frame prefix (tillt `kKompleteKontrolLightGuidePrefixMK3`).
const MK3_PREFIX: [u8; 15] = [
    0x93, 0x02, 0xCD, 0x01, 0x16, 0x92, 0xCD, 0x01, 0x51, 0x81, 0xCC, 0xFC, 0xDC, 0x00, 0x80,
];
/// Per-key command byte inside the frame.
const KEY_CMD: u8 = 0x92;
/// The frame addresses 128 keys (3 bytes each).
const NUM_KEYS: usize = 128;
/// Total frame size: 4 (len) + 15 (prefix) + 128*3 (keys) = 403.
const MSG_LEN: usize = 4 + MK3_PREFIX.len() + NUM_KEYS * 3;

/// Best-effort MK3 init report (tillt `kKompleteKontrolInitMK3`; the author
/// notes this is likely incomplete — KK software resends it on an ~8s timer).
const MK3_INIT: [u8; 10] = [0x06, 0x00, 0x00, 0x00, 0x93, 0x02, 0xcd, 0x01, 0x2c, 0x90];

/// An open USB connection to an MK3 keyboard's Light Guide.
pub struct KontrolUsb {
    handle: DeviceHandle<Context>,
    /// The color byte per key index (0..128). Mutated by [`Self::set_key`],
    /// pushed to the device by [`Self::flush`].
    keys: [u8; NUM_KEYS],
    /// The product id we actually opened (for logs / to report which model).
    pub pid: u16,
    /// Whether interface 4 was successfully claimed (bulk writes need it).
    claimed: bool,
}

impl KontrolUsb {
    /// Enumerate and open the first attached Komplete Kontrol MK3, claim its
    /// Light Guide interface, and run best-effort init.
    ///
    /// Requires permission to access the USB device (a udev rule granting your
    /// user access to VID 0x17cc, or running as root — see README/report).
    pub fn open() -> Result<Self> {
        let ctx = Context::new().map_err(|e| eyre!("usb context: {e}"))?;
        for dev in ctx.devices().map_err(|e| eyre!("list usb: {e}"))?.iter() {
            let desc = match dev.device_descriptor() {
                Ok(d) => d,
                Err(_) => continue,
            };
            if desc.vendor_id() != NI_VID || !MK3_PIDS.contains(&desc.product_id()) {
                continue;
            }
            let pid = desc.product_id();
            let handle = dev.open().map_err(|e| {
                eyre!("open NI device {pid:#06x} (permissions? need udev rule for 0x17cc): {e}")
            })?;
            // Interface 4 may be bound to a kernel driver; detach it.
            let _ = handle.set_auto_detach_kernel_driver(true);
            let claimed = match handle.claim_interface(MK3_INTERFACE) {
                Ok(()) => true,
                Err(e) => {
                    tracing::warn!("could not claim interface {MK3_INTERFACE}: {e}");
                    false
                }
            };
            let mut me = Self { handle, keys: [0; NUM_KEYS], pid, claimed };
            me.init();
            tracing::info!(pid = format!("{pid:#06x}"), claimed, "kontrol: opened MK3 over USB");
            return Ok(me);
        }
        Err(eyre!(
            "no Komplete Kontrol MK3 found on USB (looked for VID {NI_VID:#06x}, PIDs {MK3_PIDS:#06x?})"
        ))
    }

    /// Which model string best matches the opened PID.
    pub fn model(&self) -> &'static str {
        match self.pid {
            PID_S49_MK3 => "S49 MK3 (PID unconfirmed)",
            PID_S61_MK3 => "S61 MK3",
            PID_S88_MK3 => "S88 MK3 (PID unconfirmed)",
            _ => "unknown MK3",
        }
    }

    /// Best-effort init. Sends the (uncertain) MK3 init report as a HID
    /// SET_REPORT control transfer. Non-fatal on failure — TODO: confirm the
    /// full init handshake for MK3 (tillt's is self-described as incomplete).
    fn init(&mut self) {
        // HID SET_REPORT: bmRequestType=0x21 (class|interface|host->device),
        // bRequest=0x09 (SET_REPORT), wValue=(0x02 output <<8)|reportId 0.
        let res = self.handle.write_control(
            0x21,
            0x09,
            0x0200,
            MK3_INTERFACE as u16,
            &MK3_INIT,
            Duration::from_millis(200),
        );
        match res {
            Ok(n) => tracing::info!("kontrol: sent MK3 init ({n} bytes) — CONFIRM this is sufficient"),
            Err(e) => tracing::warn!("kontrol: MK3 init control transfer failed (may be fine): {e}"),
        }
    }

    /// Set one key's color in the local map. Call [`Self::flush`] to push.
    /// `key` is the raw device key index (0..128); see [`crate::note_to_key`].
    pub fn set_key(&mut self, key: u8, color: LightColor) {
        if let Some(slot) = self.keys.get_mut(key as usize) {
            *slot = color.byte();
        }
    }

    /// Clear the local map (all keys off). Call [`Self::flush`] to push.
    pub fn clear(&mut self) {
        self.keys = [0; NUM_KEYS];
    }

    /// Serialize the current key map into the 403-byte frame.
    fn frame(&self) -> [u8; MSG_LEN] {
        let mut buf = [0u8; MSG_LEN];
        let payload_len = (MSG_LEN - 4) as u32;
        buf[0..4].copy_from_slice(&payload_len.to_le_bytes());
        buf[4..19].copy_from_slice(&MK3_PREFIX);
        for (k, &color) in self.keys.iter().enumerate() {
            let off = 19 + k * 3;
            buf[off] = KEY_CMD;
            buf[off + 1] = k as u8;
            buf[off + 2] = color;
        }
        buf
    }

    /// Push the current key map to the device via a USB **bulk** write.
    pub fn flush(&mut self) -> Result<()> {
        if !self.claimed {
            return Err(eyre!(
                "interface {MK3_INTERFACE} not claimed — cannot bulk-write (permissions / driver?)"
            ));
        }
        let buf = self.frame();
        let n = self
            .handle
            .write_bulk(MK3_ENDPOINT_OUT, &buf, Duration::from_millis(200))
            .map_err(|e| eyre!("bulk write to endpoint {MK3_ENDPOINT_OUT:#04x}: {e}"))?;
        tracing::debug!("kontrol: bulk-wrote {n}/{MSG_LEN} bytes to Light Guide");
        Ok(())
    }

    /// Convenience: set one key and flush immediately.
    pub fn light(&mut self, key: u8, color: LightColor) -> Result<()> {
        self.set_key(key, color);
        self.flush()
    }
}

impl Drop for KontrolUsb {
    fn drop(&mut self) {
        if self.claimed {
            let _ = self.handle.release_interface(MK3_INTERFACE);
        }
    }
}

/// List attached NI USB devices (VID 0x17cc) as `(pid, "model")` for diagnostics.
pub fn list_ni_devices() -> Vec<(u16, String)> {
    let Ok(ctx) = Context::new() else { return Vec::new() };
    let Ok(devs) = ctx.devices() else { return Vec::new() };
    devs.iter()
        .filter_map(|d| d.device_descriptor().ok())
        .filter(|desc| desc.vendor_id() == NI_VID)
        .map(|desc| {
            let pid = desc.product_id();
            let model = match pid {
                PID_S49_MK3 => "S49 MK3?",
                PID_S61_MK3 => "S61 MK3",
                PID_S88_MK3 => "S88 MK3?",
                _ => "other NI device",
            };
            (pid, model.to_string())
        })
        .collect()
}
