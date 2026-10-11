//! Per-frame envelope evaluation for the render hot path.

use daw_proto::automation::EnvelopePoint;

/// Stateful per-frame envelope cursor. `eval_at(t)` advances through
/// the points list without rescanning from the start each call.
/// Construct fresh per render block.
///
/// Time *must* be monotonically non-decreasing across `eval_at` calls
/// — meets the audio callback contract where frames march forward.
pub(crate) struct EnvelopeCursor<'a> {
    points: &'a [EnvelopePoint],
    /// Index of the next point ahead of the cursor. Once
    /// `points[next].time > t`, the active segment is
    /// `points[next-1]..points[next]`.
    next: usize,
}

impl<'a> EnvelopeCursor<'a> {
    pub(crate) fn new(points: &'a [EnvelopePoint]) -> Self {
        Self { points, next: 1 }
    }

    /// Evaluate the envelope at `time_seconds`. Returns `None` only
    /// if the envelope is empty.
    pub(crate) fn eval_at(&mut self, time_seconds: f64) -> Option<f64> {
        if self.points.is_empty() {
            return None;
        }
        // Clamp left of first point.
        let first = &self.points[0];
        if time_seconds < first.time.as_seconds() {
            return Some(first.value);
        }
        // Clamp right of last point.
        let last = self.points.last().unwrap();
        if time_seconds >= last.time.as_seconds() {
            return Some(last.value);
        }
        // Advance `next` past every point at or before `t`: where two
        // points share a time the later holds from there (a jump).
        while self.next < self.points.len()
            && self.points[self.next].time.as_seconds() <= time_seconds
        {
            self.next += 1;
        }
        let a = &self.points[self.next - 1];
        let b = &self.points[self.next];
        let span = b.time.as_seconds() - a.time.as_seconds();
        if span <= 0.0 {
            return Some(b.value);
        }
        let f = (time_seconds - a.time.as_seconds()) / span;
        Some((b.value - a.value).mul_add(a.shape.ease(f, a.tension), a.value))
    }
}

/// An FX's parameters for a block starting `at` seconds: its stored values
/// (`fixed`), each automated one replaced by its envelope's value there.
/// Into `out`, cleared first and reused across blocks, so a block does not
/// allocate once it has grown to the chain's size.
pub(crate) fn params_at(
    fixed: &[(u32, f64)],
    envelopes: &[(u32, Vec<EnvelopePoint>)],
    at: f64,
    out: &mut Vec<(u32, f64)>,
) {
    out.clear();
    out.extend_from_slice(fixed);
    for (index, points) in envelopes {
        let Some(value) = EnvelopeCursor::new(points).eval_at(at) else {
            continue;
        };
        let value = value.clamp(0.0, 1.0);
        match out.iter_mut().find(|(i, _)| i == index) {
            Some(slot) => slot.1 = value,
            None => out.push((*index, value)),
        }
    }
}

#[cfg(test)]
mod params_tests {
    use super::*;

    fn point(time: f64, value: f64) -> EnvelopePoint {
        EnvelopePoint {
            time: daw_proto::primitives::PositionInSeconds::from_seconds(time),
            value,
            ..EnvelopePoint::default()
        }
    }

    /// An automated parameter takes its envelope's value at the block;
    /// the rest keep theirs.
    #[test]
    fn an_automated_parameter_follows_its_envelope() {
        let fixed = [(0, 0.2), (1, 0.7)];
        let envelopes = vec![(1, vec![point(0.0, 0.0), point(2.0, 1.0)])];
        let mut out = Vec::new();
        params_at(&fixed, &envelopes, 1.0, &mut out);
        assert_eq!(out[0], (0, 0.2));
        assert!((out[1].1 - 0.5).abs() < 1e-9, "{out:?}");
        params_at(&fixed, &envelopes, 3.0, &mut out);
        assert!((out[1].1 - 1.0).abs() < 1e-9, "held after the last point");
    }
}
