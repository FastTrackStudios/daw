//! Geometry for an analog VU meter — portable, no Dioxus, unit-testable.
//!
//! Both hardware faces put a VU movement at their centre, so the needle math
//! lives here once. The scale is the standard VU arc: -20 VU at the left stop,
//! 0 VU at roughly three-quarters across, +3 at the right stop, with the
//! familiar non-linear crowding at the bottom of the range.

/// Design-space width of the meter face.
pub const VU_W: f64 = 100.0;
/// Design-space height of the meter face.
pub const VU_H: f64 = 58.0;

/// Needle pivot, well below the visible face, and a long needle from it.
///
/// This is what sets how much of the card the scale occupies, and it is the
/// whole character of the movement: a real VU's scale runs nearly the full
/// width of its card, in a shallow arc, with the needle emerging from a slot
/// at the bottom. A short needle on a close pivot draws a steep little arc
/// down the middle and crowds every number into it — which is what this was
/// doing.
pub const PIVOT_X: f64 = VU_W * 0.5;
pub const PIVOT_Y: f64 = VU_H * 1.52;
/// Needle length from the pivot.
pub const NEEDLE_LEN: f64 = VU_H * 1.40;

/// Half-angle of the sweep, in degrees either side of vertical.
///
/// With the pivot and length above, ±34° puts the stops within about 5% of
/// each edge — the scale uses the card, rather than sitting in the middle of
/// it. [`the_scale_uses_the_width_of_the_card`] is the guard.
pub const SWEEP_DEG: f64 = 34.0;

/// The labelled ticks on a VU face, as (VU value, label, is_major).
pub const VU_TICKS: &[(f64, &str, bool)] = &[
    (-20.0, "20", true),
    (-10.0, "10", true),
    (-7.0, "7", false),
    (-5.0, "5", true),
    (-3.0, "3", false),
    (-2.0, "2", false),
    (-1.0, "1", false),
    (0.0, "0", true),
    (1.0, "1", false),
    (2.0, "2", false),
    (3.0, "3", true),
];

/// Map a VU value to its fraction along the scale (0 = left stop, 1 = right).
///
/// Real VU faces are not linear in dB: the bottom of the range is compressed
/// so the -20..-10 stretch takes far less arc than -3..+3. This reproduces
/// that with a power curve on the normalized dB position, which is what makes
/// a drawn face read as a VU rather than as a generic gauge.
pub fn vu_to_fraction(vu: f64) -> f64 {
    const MIN_VU: f64 = -20.0;
    const MAX_VU: f64 = 3.0;
    let t = ((vu.clamp(MIN_VU, MAX_VU) - MIN_VU) / (MAX_VU - MIN_VU)).clamp(0.0, 1.0);
    // Exponent > 1 pushes the low end together and opens up the top.
    t.powf(1.9)
}

/// Needle angle in degrees for a VU value; 0° is straight up.
pub fn vu_to_angle(vu: f64) -> f64 {
    (vu_to_fraction(vu) * 2.0 - 1.0) * SWEEP_DEG
}

/// Tip of the needle for a VU value, in design space.
pub fn needle_tip(vu: f64) -> (f64, f64) {
    let rad = vu_to_angle(vu).to_radians();
    (
        PIVOT_X + NEEDLE_LEN * rad.sin(),
        PIVOT_Y - NEEDLE_LEN * rad.cos(),
    )
}

/// Horizontal extent of the printed scale, as a fraction of the card's width.
pub fn scale_width_fraction() -> f64 {
    let (left, _) = tick_point(-20.0, 0.0);
    let (right, _) = tick_point(3.0, 0.0);
    (right - left) / VU_W
}

/// A point on the tick arc for a VU value, at `inset` from the needle length.
pub fn tick_point(vu: f64, inset: f64) -> (f64, f64) {
    let rad = vu_to_angle(vu).to_radians();
    let r = NEEDLE_LEN - inset;
    (PIVOT_X + r * rad.sin(), PIVOT_Y - r * rad.cos())
}

/// The scale arc as an SVG path, at `inset` from the needle length.
pub fn scale_arc_path(inset: f64) -> String {
    let (x0, y0) = tick_point(-20.0, inset);
    let (x1, y1) = tick_point(3.0, inset);
    let r = NEEDLE_LEN - inset;
    // Small sweep, so a single arc segment is exact enough.
    format!("M {x0:.2} {y0:.2} A {r:.2} {r:.2} 0 0 1 {x1:.2} {y1:.2}")
}

/// Gain reduction (dB, positive) shown on a GR-mode VU.
///
/// Hardware GR meters read backwards: no reduction rests at 0 on the right,
/// and the needle swings left as the compressor works. 20 dB of reduction is
/// the left stop.
pub fn gr_to_vu(gr_db: f64) -> f64 {
    -gr_db.clamp(0.0, 20.0)
}

/// Level (dBFS) shown on a level-mode VU, referenced so that -18 dBFS reads
/// 0 VU — the usual digital alignment.
pub fn db_to_vu(db: f64) -> f64 {
    (db + 18.0).clamp(-20.0, 3.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scale_runs_left_to_right_across_its_range() {
        assert_eq!(vu_to_fraction(-20.0), 0.0);
        assert_eq!(vu_to_fraction(3.0), 1.0);
        assert!(vu_to_fraction(-20.0) < vu_to_fraction(0.0));
        assert!(vu_to_fraction(0.0) < vu_to_fraction(3.0));
    }

    #[test]
    fn the_scale_is_crowded_at_the_bottom_like_a_real_vu() {
        // The -20..-10 half of the dB range must occupy less arc than -10..0,
        // which is what gives a VU face its characteristic look.
        let low = vu_to_fraction(-10.0) - vu_to_fraction(-20.0);
        let high = vu_to_fraction(0.0) - vu_to_fraction(-10.0);
        assert!(low < high, "low span {low} should be tighter than high {high}");
    }

    #[test]
    fn out_of_range_values_pin_to_the_stops() {
        assert_eq!(vu_to_fraction(-99.0), 0.0);
        assert_eq!(vu_to_fraction(99.0), 1.0);
    }

    #[test]
    fn the_scale_uses_the_width_of_the_card() {
        // A VU's scale runs nearly edge to edge. Anything much less and the
        // numbers crowd into the middle, which is what a short needle on a
        // close pivot produces.
        let fraction = scale_width_fraction();
        assert!(
            fraction > 0.85,
            "the scale spans only {:.0}% of the card",
            fraction * 100.0
        );
        assert!(fraction < 1.0, "the scale must stay on the card");
    }

    #[test]
    fn the_arc_is_shallow_rather_than_steep() {
        // The stops sit lower than the centre, but by a fifth of the card at
        // most — a VU is a wide shallow sweep, not a rainbow.
        let (_, mid) = tick_point(-4.0, 0.0);
        let (_, edge) = tick_point(-20.0, 0.0);
        let drop = (edge - mid) / VU_H;
        assert!(drop > 0.1 && drop < 0.35, "arc depth is {drop:.2} of the card");
    }

    #[test]
    fn the_needle_reaches_the_top_of_the_card() {
        let (_, tip) = needle_tip(-4.0);
        assert!(tip > 0.0 && tip < VU_H * 0.2, "needle tip at y={tip}");
    }

    #[test]
    fn the_needle_sweeps_symmetrically_about_vertical() {
        assert!((vu_to_angle(-20.0) + SWEEP_DEG).abs() < 1e-9);
        assert!((vu_to_angle(3.0) - SWEEP_DEG).abs() < 1e-9);
        // Straight up is somewhere inside the range, not at either stop.
        let mid = vu_to_angle(-4.0);
        assert!(mid.abs() < SWEEP_DEG);
    }

    #[test]
    fn the_needle_tip_stays_above_the_pivot_and_moves_right_with_level() {
        let (lx, ly) = needle_tip(-20.0);
        let (rx, ry) = needle_tip(3.0);
        assert!(lx < PIVOT_X, "left stop should sit left of the pivot");
        assert!(rx > PIVOT_X, "right stop should sit right of the pivot");
        assert!(ly < PIVOT_Y && ry < PIVOT_Y, "needle points upward");
    }

    #[test]
    fn gain_reduction_reads_backwards_from_zero() {
        assert_eq!(gr_to_vu(0.0), 0.0, "no reduction rests at 0");
        assert!(gr_to_vu(6.0) < 0.0, "reduction swings the needle left");
        assert_eq!(gr_to_vu(50.0), -20.0, "clamps at the left stop");
    }

    #[test]
    fn level_mode_aligns_minus_18_dbfs_to_zero_vu() {
        assert_eq!(db_to_vu(-18.0), 0.0);
        assert!(db_to_vu(-30.0) < 0.0);
        assert_eq!(db_to_vu(0.0), 3.0, "hot signals pin at the top of the scale");
    }

    #[test]
    fn the_scale_arc_is_a_single_well_formed_segment() {
        let d = scale_arc_path(6.0);
        assert!(d.starts_with("M "));
        assert!(d.contains(" A "));
    }
}
