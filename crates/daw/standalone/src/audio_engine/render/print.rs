//! A snapshot's fingerprint: one number that moves whenever anything a
//! render of the tracks ahead reads does ([`ahead_print`]).
//!
//! The render ahead keeps up to its depth of audio rendered before it is
//! heard. When a fader, a plugin's parameter or an item under the
//! playhead changes, what it rendered is stale, and the render ahead
//! starts again a little past the playhead (see `ahead`). A project's
//! revision moves on any change — a selection, a name — so it cannot say
//! when; this can, built once per snapshot, off the audio thread.

use std::hash::Hasher;

use daw_proto::automation::EnvelopePoint;

use super::envelope::Automated;
use super::graph::Pass;
use super::snapshot::{
    ItemSnapshot, MidiNoteSnapshot, MidiOtherSnapshot, NoteExpressionSnapshot, SendSnapshot,
    TempoMap, TrackSnapshot,
};

/// What goes into a fingerprint.
pub(crate) trait Print {
    fn print(&self, h: &mut Fx);
}

/// FxHash's mixing: a fingerprint of a whole song costs a pass over its
/// numbers and no more (SipHash, the std default, is several times that).
#[derive(Default)]
pub(crate) struct Fx(u64);

impl Hasher for Fx {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            self.write_u64(u64::from_le_bytes(word));
        }
    }

    fn write_u64(&mut self, n: u64) {
        const K: u64 = 0x51_7c_c1_b7_27_22_0a_95;
        self.0 = (self.0.rotate_left(5) ^ n).wrapping_mul(K);
    }
}

macro_rules! print_as_u64 {
    ($($t:ty),*) => {$(
        impl Print for $t {
            fn print(&self, h: &mut Fx) {
                h.write_u64(*self as u64);
            }
        }
    )*};
}
print_as_u64!(bool, u8, u32, u64, usize);

impl Print for u128 {
    fn print(&self, h: &mut Fx) {
        h.write_u64(*self as u64);
        h.write_u64((*self >> 64) as u64);
    }
}

impl Print for f64 {
    fn print(&self, h: &mut Fx) {
        h.write_u64(self.to_bits());
    }
}

impl Print for str {
    fn print(&self, h: &mut Fx) {
        self.len().print(h);
        h.write(self.as_bytes());
    }
}

impl Print for String {
    fn print(&self, h: &mut Fx) {
        self.as_str().print(h);
    }
}

impl<T: Print> Print for Option<T> {
    fn print(&self, h: &mut Fx) {
        match self {
            None => h.write_u64(0),
            Some(v) => {
                h.write_u64(1);
                v.print(h);
            }
        }
    }
}

impl<T: Print> Print for [T] {
    fn print(&self, h: &mut Fx) {
        self.len().print(h);
        for v in self {
            v.print(h);
        }
    }
}

impl<T: Print> Print for Vec<T> {
    fn print(&self, h: &mut Fx) {
        self.as_slice().print(h);
    }
}

impl<A: Print, B: Print> Print for (A, B) {
    fn print(&self, h: &mut Fx) {
        self.0.print(h);
        self.1.print(h);
    }
}

impl Print for EnvelopePoint {
    fn print(&self, h: &mut Fx) {
        self.time.as_seconds().print(h);
        self.value.print(h);
        (self.shape as u8).print(h);
        self.tension.print(h);
    }
}

impl Print for daw_proto::StretchMarker {
    fn print(&self, h: &mut Fx) {
        self.position.print(h);
        self.source_position.print(h);
        self.slope.print(h);
    }
}

impl Print for Automated {
    fn print(&self, h: &mut Fx) {
        self.id.print(h);
        self.min.print(h);
        self.max.print(h);
        self.points.print(h);
    }
}

impl Print for MidiNoteSnapshot {
    fn print(&self, h: &mut Fx) {
        self.start_seconds.print(h);
        self.length_seconds.print(h);
        self.channel.print(h);
        self.pitch.print(h);
        self.velocity.print(h);
    }
}

impl Print for MidiOtherSnapshot {
    fn print(&self, h: &mut Fx) {
        self.time_seconds.print(h);
        let mut bytes = [0u8; 64];
        match self.message.encode(&mut bytes) {
            Some(n) => h.write(&bytes[..n]),
            // Longer than a fingerprint needs (a sysex): its length says
            // enough that a change of it is seen.
            None => h.write_u64(u64::MAX),
        }
    }
}

impl Print for NoteExpressionSnapshot {
    fn print(&self, h: &mut Fx) {
        self.time_seconds.print(h);
        self.channel.print(h);
        self.note.print(h);
        (self.dimension as u8).print(h);
        self.value.print(h);
    }
}

impl Print for ItemSnapshot {
    fn print(&self, h: &mut Fx) {
        // A source by identity: a source is never changed in place, and a
        // stretched take's render is kept by its markers.
        self.audio
            .as_ref()
            .map(|a| std::sync::Arc::as_ptr(a) as usize)
            .print(h);
        self.fixed_lane.print(h);
        self.position_seconds.print(h);
        self.length_seconds.print(h);
        self.fade_in_seconds.print(h);
        self.fade_out_seconds.print(h);
        (self.fade_in_shape as u8).print(h);
        (self.fade_out_shape as u8).print(h);
        self.loop_source.print(h);
        self.muted.print(h);
        self.item_volume.print(h);
        self.take_volume.print(h);
        self.play_rate.print(h);
        self.start_offset_seconds.print(h);
        self.stretch_markers.print(h);
        self.channel_mode.print(h);
        self.take_pitch_semitones.print(h);
        self.take_volume_env.print(h);
        self.take_pan_env.print(h);
        self.take_mute_env.print(h);
        self.take_pitch_env.print(h);
        self.midi_notes.print(h);
        self.midi_other.print(h);
        self.note_expressions.print(h);
    }
}

impl Print for SendSnapshot {
    fn print(&self, h: &mut Fx) {
        self.dest_idx.print(h);
        self.to_sidechain.print(h);
        self.from_pair.print(h);
        self.volume.print(h);
        self.pan.print(h);
        self.muted.print(h);
        (self.mode as u8).print(h);
        self.volume_env.print(h);
        self.pan_env.print(h);
        self.mute_env.print(h);
    }
}

impl Print for TrackSnapshot {
    fn print(&self, h: &mut Fx) {
        self.guid.print(h);
        self.volume.print(h);
        self.pan.print(h);
        self.phase_inverted.print(h);
        self.muted.print(h);
        self.soloed.print(h);
        self.parent_send.print(h);
        self.lane_play_mask.print(h);
        self.vca_lead.print(h);
        self.vca_follow.print(h);
        self.hw_out.print(h);
        self.parent_idx.print(h);
        self.sends.print(h);
        self.items.print(h);
        self.fx_chain.print(h);
        self.fx_enabled.print(h);
        self.fx_params.print(h);
        self.fx_envs.print(h);
        self.volume_env.print(h);
        self.pan_env.print(h);
        self.mute_env.print(h);
        self.volume_prefx_env.print(h);
        self.pan_prefx_env.print(h);
        self.volume_replaces.print(h);
        self.pan_replaces.print(h);
    }
}

impl Print for Pass {
    fn print(&self, h: &mut Fx) {
        h.write_u64(match self {
            Pass::Off => 0,
            Pass::On => 1,
            Pass::Feed => 2,
            Pass::Monitor => 3,
        });
    }
}

impl Print for TempoMap {
    fn print(&self, h: &mut Fx) {
        self.print_segments(h);
    }
}

/// The fingerprint of what a render of the tracks ahead reads: every
/// track rendered ahead whole, where it sits, the solo routing and the
/// tempo map — and of a live track only what can reach a track ahead (a
/// VCA lead's fader and mute). A change to a live track alone leaves it
/// where it was: that track is rendered at the device's block anyway.
pub(crate) fn ahead_print(
    tracks: &[TrackSnapshot],
    live: &[bool],
    solo_pass: Option<&[Pass]>,
    tempo_map: &TempoMap,
) -> u64 {
    let mut h = Fx::default();
    tracks.len().print(&mut h);
    for (i, t) in tracks.iter().enumerate() {
        i.print(&mut h);
        if live.get(i).copied().unwrap_or(false) {
            h.write_u64(1);
            if t.vca_lead != 0 {
                t.volume.print(&mut h);
                t.muted.print(&mut h);
                t.vca_lead.print(&mut h);
            }
        } else {
            h.write_u64(0);
            t.print(&mut h);
        }
    }
    solo_pass.print(&mut h);
    tempo_map.print(&mut h);
    h.finish()
}

impl Print for &[Pass] {
    fn print(&self, h: &mut Fx) {
        (**self).print(h);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn print<T: Print + ?Sized>(v: &T) -> u64 {
        let mut h = Fx::default();
        v.print(&mut h);
        h.finish()
    }

    #[test]
    fn a_change_moves_the_print_and_nothing_else_does() {
        assert_eq!(print(&vec![1.0, 2.0]), print(&vec![1.0, 2.0]));
        assert_ne!(print(&vec![1.0, 2.0]), print(&vec![1.0, 2.5]));
        assert_ne!(print(&vec![1.0]), print(&vec![1.0, 0.0]), "length counts");
        assert_ne!(print(&Some(0.0)), print(&None::<f64>));
        assert_ne!(print("ab"), print("ba"));
    }
}
