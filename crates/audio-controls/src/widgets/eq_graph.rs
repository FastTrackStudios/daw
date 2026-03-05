//! Parametric EQ graph widget.
//!
//! A Dioxus component that renders a parametric EQ graph with Pro-Q style interactions:
//! - Frequency response curve visualization
//! - Draggable band control points (drag to adjust freq/gain)
//! - Mouse wheel to adjust Q while hovering/dragging
//! - Double-click on empty area to add new band
//! - Double-click on band to reset gain to 0 dB
//! - Drag band outside graph area to remove it
//! - Smart filter type selection based on click position
//!
//! Uses SVG rendering for cross-platform compatibility.

use crate::prelude::*;
use std::rc::Rc;

/// Get current timestamp in milliseconds (cross-platform).
#[cfg(target_arch = "wasm32")]
fn now_ms() -> f64 {
    js_sys::Date::now()
}

#[cfg(not(target_arch = "wasm32"))]
fn now_ms() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

/// Maximum number of EQ bands supported.
pub const MAX_BANDS: usize = 24;

/// Stereo placement mode for EQ bands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StereoMode {
    #[default]
    Stereo,
    Left,
    Right,
    Mid,
    Side,
}

impl StereoMode {
    /// Get display label for the stereo mode.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Stereo => "Stereo",
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Mid => "Mid",
            Self::Side => "Side",
        }
    }

    /// Get short label for the stereo mode.
    pub fn short_label(&self) -> &'static str {
        match self {
            Self::Stereo => "ST",
            Self::Left => "L",
            Self::Right => "R",
            Self::Mid => "M",
            Self::Side => "S",
        }
    }
}

/// EQ graph band data for rendering.
///
/// A simplified band representation for the EQ graph when
/// the full fts-dsp types aren't needed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EqBand {
    /// Band index (0-23).
    pub index: usize,
    /// Whether this band slot is in use.
    pub used: bool,
    /// Whether the band is enabled (bypassed when false).
    pub enabled: bool,
    /// Center frequency in Hz (10-30000).
    pub frequency: f32,
    /// Gain in dB (-30 to +30).
    pub gain: f32,
    /// Q factor (0.025 to 40). For cut filters, this represents slope order.
    pub q: f32,
    /// Filter shape (bell, shelf, cut, etc.).
    pub shape: EqBandShape,
    /// Whether this band is soloed (only this band audible).
    pub solo: bool,
    /// Stereo placement mode.
    pub stereo_mode: StereoMode,
}

/// EQ band filter shape.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EqBandShape {
    #[default]
    Bell,
    LowShelf,
    HighShelf,
    LowCut,
    HighCut,
    Notch,
    BandPass,
    TiltShelf,
    FlatTilt,
    AllPass,
}

impl EqBandShape {
    /// Get display label for the filter shape.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Bell => "Bell",
            Self::LowShelf => "Low Shelf",
            Self::HighShelf => "High Shelf",
            Self::LowCut => "Low Cut",
            Self::HighCut => "High Cut",
            Self::Notch => "Notch",
            Self::BandPass => "Band Pass",
            Self::TiltShelf => "Tilt Shelf",
            Self::FlatTilt => "Flat Tilt",
            Self::AllPass => "All Pass",
        }
    }

    /// Whether this filter type uses slope (dB/oct) instead of Q.
    pub fn uses_slope(&self) -> bool {
        matches!(self, Self::LowCut | Self::HighCut)
    }

    /// Whether this filter type uses gain.
    pub fn uses_gain(&self) -> bool {
        !matches!(
            self,
            Self::LowCut | Self::HighCut | Self::Notch | Self::AllPass
        )
    }

    /// All available filter shapes.
    pub fn all() -> &'static [EqBandShape] {
        &[
            Self::Bell,
            Self::LowShelf,
            Self::HighShelf,
            Self::LowCut,
            Self::HighCut,
            Self::Notch,
            Self::BandPass,
            Self::TiltShelf,
            Self::FlatTilt,
            Self::AllPass,
        ]
    }
}

/// Convert Q value to slope in dB/octave for cut filters.
pub fn q_to_slope_db(q: f32) -> f32 {
    // Q represents filter order: 0.5 = 6dB/oct, 1.0 = 12dB/oct, etc.
    (q * 2.0).round().max(1.0) * 6.0
}

/// Convert slope in dB/octave to Q value for cut filters.
pub fn slope_db_to_q(slope_db: f32) -> f32 {
    // 6dB/oct = 0.5, 12dB/oct = 1.0, etc.
    (slope_db / 6.0).round().max(1.0) / 2.0
}

/// Band colors matching Pro-Q / ZL Equalizer style.
/// Colors cycle through for bands 0-23, matching the screenshot reference.
const BAND_COLORS: &[&str] = &[
    "#4ade80", // 1: Green
    "#60a5fa", // 2: Blue
    "#c084fc", // 3: Purple
    "#f472b6", // 4: Pink
    "#fb7185", // 5: Red/Rose
    "#fb923c", // 6: Orange
    "#facc15", // 7: Yellow
    "#a3e635", // 8: Lime
    "#34d399", // 9: Emerald
    "#22d3d8", // 10: Cyan
    "#818cf8", // 11: Indigo
    "#e879f9", // 12: Fuchsia
    "#f87171", // 13: Red
    "#fdba74", // 14: Light Orange
    "#fde047", // 15: Light Yellow
    "#bef264", // 16: Light Lime
    "#6ee7b7", // 17: Light Emerald
    "#67e8f9", // 18: Light Cyan
    "#a5b4fc", // 19: Light Indigo
    "#f0abfc", // 20: Light Fuchsia
    "#fca5a5", // 21: Light Red
    "#fed7aa", // 22: Peach
    "#fef08a", // 23: Pale Yellow
    "#d9f99d", // 24: Pale Lime
];

/// Get the color for a band by index.
pub fn get_band_color(index: usize) -> &'static str {
    BAND_COLORS[index % BAND_COLORS.len()]
}

/// Get the fill color (semi-transparent) for a band by index.
pub fn get_band_fill_color(index: usize) -> String {
    let hex = get_band_color(index);
    // Convert hex to rgba with low opacity for fill
    if let (Ok(r), Ok(g), Ok(b)) = (
        u8::from_str_radix(&hex[1..3], 16),
        u8::from_str_radix(&hex[3..5], 16),
        u8::from_str_radix(&hex[5..7], 16),
    ) {
        format!("rgba({r}, {g}, {b}, 0.15)")
    } else {
        "rgba(100, 100, 100, 0.15)".to_string()
    }
}

/// Parametric EQ graph component with Pro-Q style interactions.
///
/// # Interactions
///
/// - **Drag band node**: Adjust frequency (X) and gain (Y)
/// - **Shift+Drag**: Fine adjustment mode
/// - **Mouse wheel on band**: Adjust Q factor
/// - **Double-click empty area**: Add new band (filter type based on position)
/// - **Double-click band**: Reset gain to 0 dB
/// - **Drag band outside graph**: Remove band
///
/// # Filter Type Selection (on double-click)
///
/// - Left edge (< 30 Hz): High-pass filter
/// - Right edge (> 15 kHz): Low-pass filter
/// - Low frequencies (30-80 Hz): Low shelf
/// - High frequencies (8-15 kHz): High shelf
/// - Center area: Bell/Peak filter
///
/// # Example
///
/// ```ignore
/// use audio_controls::widgets::{EqGraph, EqBand, EqBandShape};
///
/// #[component]
/// fn MyEQ() -> Element {
///     let mut bands = use_signal(|| vec![
///         EqBand { used: true, enabled: true, frequency: 100.0, gain: 3.0, q: 1.0, shape: EqBandShape::Bell, ..Default::default() },
///     ]);
///
///     rsx! {
///         EqGraph {
///             bands: bands,
///             on_band_change: move |(idx, band): (usize, EqBand)| {
///                 bands.write()[idx] = band;
///             },
///             on_band_add: move |band: EqBand| {
///                 bands.write().push(band);
///             },
///             on_band_remove: move |idx: usize| {
///                 bands.write().remove(idx);
///             },
///         }
///     }
/// }
/// ```
#[component]
pub fn EqGraph(
    /// Signal containing the EQ bands.
    bands: Signal<Vec<EqBand>>,
    /// dB range (symmetric around 0).
    #[props(default = 24.0)]
    db_range: f64,
    /// Minimum frequency in Hz.
    #[props(default = 20.0)]
    min_freq: f64,
    /// Maximum frequency in Hz.
    #[props(default = 20000.0)]
    max_freq: f64,
    /// Sample rate for filter calculations.
    #[props(default = 48000.0)]
    sample_rate: f64,
    /// Show grid lines.
    #[props(default = true)]
    show_grid: bool,
    /// Show frequency labels.
    #[props(default = true)]
    show_freq_labels: bool,
    /// Show dB labels.
    #[props(default = true)]
    show_db_labels: bool,
    /// Fill under the curve.
    #[props(default = true)]
    fill_curve: bool,
    /// Callback when a band is changed via drag.
    #[props(default)]
    on_band_change: Option<EventHandler<(usize, EqBand)>>,
    /// Callback when a new band is added (double-click on empty area).
    #[props(default)]
    on_band_add: Option<EventHandler<EqBand>>,
    /// Callback when a band is removed (dragged outside graph area).
    #[props(default)]
    on_band_remove: Option<EventHandler<usize>>,
    /// Callback when band editing begins.
    #[props(default)]
    on_begin: Option<EventHandler<usize>>,
    /// Callback when band editing ends.
    #[props(default)]
    on_end: Option<EventHandler<usize>>,
    /// Additional CSS class.
    #[props(default)]
    class: String,
    /// Whether the control is disabled.
    #[props(default = false)]
    disabled: bool,
) -> Element {
    // Internal dimensions for the SVG viewBox (fixed coordinate system)
    let vb_width = 800.0;
    let vb_height = 350.0;
    let padding = 40.0;
    let graph_width = vb_width - padding * 2.0;
    let graph_height = vb_height - padding * 2.0;

    // Internal state
    let mut dragging_band = use_signal(|| None::<usize>);
    let mut hovered_band = use_signal(|| None::<usize>);
    // Focused band shows the info popup (only one at a time)
    let mut focused_band: Signal<Option<usize>> = use_signal(|| None);
    // Selected bands for multi-selection (can be multiple)
    let mut selected_bands: Signal<Vec<usize>> = use_signal(Vec::new);
    // Selection rectangle state: (start_x, start_y, current_x, current_y)
    let mut selection_rect: Signal<Option<(f64, f64, f64, f64)>> = use_signal(|| None);
    // Track element size for coordinate transformation
    let mut element_size = use_signal(|| (vb_width, vb_height));
    // Store mounted element for size updates
    let mut mounted_element: Signal<Option<Rc<MountedData>>> = use_signal(|| None);
    // Track last click for double-click detection on mousedown
    // (timestamp_ms, x, y) - allows creating node on second mousedown so user can drag immediately
    let mut last_click: Signal<Option<(f64, f64, f64)>> = use_signal(|| None);
    // Track drag start position for multi-selection movement
    let mut drag_start: Signal<Option<(f64, f64)>> = use_signal(|| None);
    // Track original band positions for proportional scaling during multi-drag
    let mut drag_start_bands: Signal<Vec<(usize, f32, f32)>> = use_signal(Vec::new); // (idx, freq, gain)
    // Dropdown states for the popup
    let mut show_shape_dropdown: Signal<bool> = use_signal(|| false);
    let mut show_more_dropdown: Signal<bool> = use_signal(|| false);
    // Track when mouse left the focused band area (for fade timeout)
    // Stores (timestamp_ms, band_idx) when mouse leaves focus area
    let mut focus_leave_time: Signal<Option<(f64, usize)>> = use_signal(|| None);
    // Popup fade timeout in milliseconds
    let popup_fade_timeout_ms = 300.0;
    // Double-click threshold in milliseconds
    let double_click_threshold_ms = 400.0;
    // Distance threshold for double-click (in viewBox coords)
    let double_click_distance = 20.0;
    // Focus detection radius (larger so popup is easier to interact with)
    let focus_radius = 50.0;

    // Coordinate conversions
    let log_min = min_freq.log10();
    let log_max = max_freq.log10();

    let freq_to_x = move |freq: f64| -> f64 {
        let normalized = (freq.log10() - log_min) / (log_max - log_min);
        padding + normalized * graph_width
    };

    let x_to_freq = move |x: f64| -> f64 {
        let normalized = (x - padding) / graph_width;
        10.0_f64.powf(log_min + normalized * (log_max - log_min))
    };

    let db_to_y = move |db: f64| -> f64 {
        let normalized = 0.5 - db / (2.0 * db_range);
        padding + normalized * graph_height
    };

    let y_to_db = move |y: f64| -> f64 {
        let normalized = (y - padding) / graph_height;
        (0.5 - normalized) * 2.0 * db_range
    };

    // Helper to update element size from mounted element
    let update_element_size = move || {
        if let Some(mounted) = mounted_element.read().as_ref() {
            // Spawn a task to get the bounding rect asynchronously
            let mounted_clone = mounted.clone();
            spawn(async move {
                if let Ok(rect) = mounted_clone.get_client_rect().await {
                    let w = rect.width();
                    let h = rect.height();
                    if w > 0.0 && h > 0.0 {
                        element_size.set((w, h));
                    }
                }
            });
        }
    };

    // Transform element coordinates to viewBox coordinates
    // The SVG scales to fit its container while preserving aspect ratio (xMidYMid meet)
    let transform_coords = move |elem_x: f64, elem_y: f64| -> (f64, f64) {
        let (elem_w, elem_h) = *element_size.read();

        // Calculate the scale factor (preserveAspectRatio: xMidYMid meet)
        let scale_x = elem_w / vb_width;
        let scale_y = elem_h / vb_height;
        let scale = scale_x.min(scale_y);

        // Calculate offset due to centering (xMidYMid)
        let scaled_width = vb_width * scale;
        let scaled_height = vb_height * scale;
        let offset_x = (elem_w - scaled_width) / 2.0;
        let offset_y = (elem_h - scaled_height) / 2.0;

        // Transform element coords to viewBox coords
        let vb_x = (elem_x - offset_x) / scale;
        let vb_y = (elem_y - offset_y) / scale;

        (vb_x, vb_y)
    };

    // Check if position is inside the graph area (in viewBox coords)
    let is_inside_graph = move |x: f64, y: f64| -> bool {
        x >= padding && x <= padding + graph_width && y >= padding && y <= padding + graph_height
    };

    // Determine filter type based on click position
    let get_filter_type_for_position = move |freq: f64, gain: f64| -> EqBandShape {
        let gain_near_zero = gain.abs() < db_range * 0.2;

        if freq < 30.0 && gain_near_zero {
            EqBandShape::LowCut // High-pass on left edge
        } else if freq > 15000.0 && gain_near_zero {
            EqBandShape::HighCut // Low-pass on right edge
        } else if freq < 80.0 {
            EqBandShape::LowShelf
        } else if freq > 8000.0 {
            EqBandShape::HighShelf
        } else {
            EqBandShape::Bell
        }
    };

    // Generate SVG paths for the EQ curves (combined + per-band)
    let curve_paths = use_memo(move || {
        let bands_vec = bands.read();
        generate_all_eq_curves(
            &bands_vec,
            sample_rate,
            min_freq,
            max_freq,
            db_range,
            padding,
            graph_width,
            graph_height,
            400,
        )
    });

    // Generate grid lines
    let grid_elements = use_memo(move || {
        if !show_grid {
            return Vec::new();
        }
        generate_grid_elements(
            padding,
            graph_width,
            graph_height,
            min_freq,
            max_freq,
            db_range,
        )
    });

    // Generate frequency labels
    let freq_labels = use_memo(move || {
        if !show_freq_labels {
            return Vec::new();
        }
        generate_freq_labels(padding, graph_width, vb_height, min_freq, max_freq)
    });

    // Generate dB labels
    let db_labels = use_memo(move || {
        if !show_db_labels {
            return Vec::new();
        }
        generate_db_labels(padding, graph_height, db_range)
    });

    // Colors - Pro-Q inspired dark theme
    let bg_color = "#0a0a0a"; // Very dark, almost black
    let grid_color = "rgba(60, 60, 65, 0.4)";
    let grid_major_color = "rgba(80, 80, 85, 0.5)";
    let curve_stroke = "#d4a932"; // Golden/yellow combined curve like Pro-Q
    let curve_fill = "rgba(212, 169, 50, 0.08)";
    let band_inactive_color = "#555555";
    let text_color = "#666666";
    let _remove_zone_color = "rgba(255, 60, 60, 0.12)";

    // Clone bands for the read in the iterator
    let bands_snapshot: Vec<EqBand> = bands.read().clone();

    rsx! {
        svg {
            // Use viewBox for proper scaling - SVG will scale to fit container
            view_box: "0 0 {vb_width} {vb_height}",
            // Make it responsive
            width: "100%",
            height: "100%",
            preserve_aspect_ratio: "xMidYMid meet",
            class: "eq-graph {class}",
            style: "background: {bg_color}; user-select: none; display: block;",

            // Store mounted element for size tracking
            onmounted: move |evt: MountedEvent| {
                mounted_element.set(Some(evt.data()));
                // Initial size update
                update_element_size();
            },

            // Handle mouse leave - end drag
            onmouseleave: move |_| {
                // Copy the value first to avoid borrow conflicts
                let band_idx_opt = { *dragging_band.read() };
                if let Some(band_idx) = band_idx_opt {
                    dragging_band.set(None);
                    if let Some(cb) = &on_end {
                        cb.call(band_idx);
                    }
                }
                hovered_band.set(None);
            },

            // Update element size on mouse enter for coordinate transformation
            onmouseenter: move |_| {
                update_element_size();
            },

            // Handle mouse move for dragging and focus detection
            onmousemove: move |evt: MouseEvent| {
                if disabled {
                    return;
                }

                // Get element-relative coordinates and transform to viewBox space
                let coords = evt.element_coordinates();
                let (x, y) = transform_coords(coords.x, coords.y);

                // Handle selection rectangle drawing - copy value first
                let sel_rect = { *selection_rect.read() };
                if let Some((start_x, start_y, _, _)) = sel_rect {
                    selection_rect.set(Some((start_x, start_y, x, y)));
                    return;
                }

                // Copy dragging band value first to avoid borrow conflicts
                let dragging_band_idx = { *dragging_band.read() };

                // Handle band dragging (single or multi-select)
                if let Some(band_idx) = dragging_band_idx {
                    // Copy all needed values upfront
                    let selected = { selected_bands.read().clone() };
                    let is_multi_drag = selected.len() > 1 && selected.contains(&band_idx);

                    if is_multi_drag {
                        // Multi-selection proportional scaling - copy values first
                        let drag_start_pos = { *drag_start.read() };
                        if let Some((_start_x, _start_y)) = drag_start_pos {
                            let start_bands = { drag_start_bands.read().clone() };

                            // Find the dragged band's original position
                            let dragged_orig = start_bands.iter().find(|(idx, _, _)| *idx == band_idx);
                            if let Some(&(_, orig_freq, orig_gain)) = dragged_orig {
                                // Calculate delta for the dragged band
                                let new_gain = y_to_db(y).clamp(-30.0, 30.0) as f32;
                                let gain_delta = new_gain - orig_gain;

                                // Calculate proportional scale factor
                                // If dragged band moves from +3 to 0, scale is 0/3 = 0
                                // If dragged band moves from +3 to +2, scale is 2/3
                                let scale = if orig_gain.abs() > 0.01 {
                                    new_gain / orig_gain
                                } else {
                                    1.0 + gain_delta / 10.0 // Fallback for bands near 0
                                };

                                // Update all selected bands with proportional scaling
                                let mut bands_vec = bands.write();
                                for &(idx, _, orig_g) in &start_bands {
                                    if idx < bands_vec.len() {
                                        // Scale gain proportionally relative to 0
                                        let scaled_gain = (orig_g * scale).clamp(-30.0, 30.0);
                                        bands_vec[idx].gain = scaled_gain;

                                        // Also move frequency if dragging horizontally
                                        let new_freq = x_to_freq(x).clamp(10.0, 30000.0) as f32;
                                        let freq_ratio = new_freq / orig_freq;
                                        if let Some(&(_, orig_f, _)) = start_bands.iter().find(|(i, _, _)| *i == idx) {
                                            bands_vec[idx].frequency = (orig_f * freq_ratio).clamp(10.0, 30000.0);
                                        }
                                    }
                                }
                                // Collect updated bands for notification before dropping write borrow
                                let updated_bands: Vec<(usize, EqBand)> = start_bands
                                    .iter()
                                    .filter_map(|&(idx, _, _)| {
                                        if idx < bands_vec.len() {
                                            Some((idx, bands_vec[idx].clone()))
                                        } else {
                                            None
                                        }
                                    })
                                    .collect();
                                drop(bands_vec);

                                // Notify about changes (borrow is now released)
                                if let Some(cb) = &on_band_change {
                                    for (idx, band) in updated_bands {
                                        cb.call((idx, band));
                                    }
                                }
                            }
                        }
                    } else {
                        // Single band drag
                        let new_freq = x_to_freq(x).clamp(10.0, 30000.0) as f32;
                        let new_gain = y_to_db(y).clamp(-30.0, 30.0) as f32;

                        let updated_band: Option<(usize, EqBand)> = {
                            let mut bands_vec = bands.write();
                            if band_idx < bands_vec.len() {
                                bands_vec[band_idx].frequency = new_freq;
                                bands_vec[band_idx].gain = new_gain;
                                Some((band_idx, bands_vec[band_idx].clone()))
                            } else {
                                None
                            }
                        };

                        // Call callback after borrow is released
                        if let (Some((idx, band)), Some(cb)) = (updated_band, &on_band_change) {
                            cb.call((idx, band));
                        }
                    }
                    return;
                }

                // Find closest band to mouse for focus (within threshold)
                let closest_band: Option<(usize, f64)> = {
                    let bands_vec = bands.read();
                    let mut closest: Option<(usize, f64)> = None;
                    for (idx, band) in bands_vec.iter().enumerate() {
                        if band.used {
                            let bx = freq_to_x(band.frequency as f64);
                            let by = db_to_y(band.gain as f64);
                            let dist = ((x - bx).powi(2) + (y - by).powi(2)).sqrt();
                            if dist < focus_radius
                                && (closest.is_none() || dist < closest.unwrap().1) {
                                    closest = Some((idx, dist));
                                }
                        }
                    }
                    closest
                };

                // Update focused band with timeout logic
                let new_focus = closest_band.map(|(idx, _)| idx);
                let current_focus = { *focused_band.read() };
                let leave_time = { *focus_leave_time.read() };
                let now = now_ms();

                match (current_focus, new_focus) {
                    (Some(old_idx), Some(new_idx)) if old_idx != new_idx => {
                        // Switching to a different band - instant switch, no fade
                        focused_band.set(Some(new_idx));
                        focus_leave_time.set(None);
                        show_shape_dropdown.set(false);
                        show_more_dropdown.set(false);
                    }
                    (Some(_), Some(_)) => {
                        // Still on the same band - clear any pending fade
                        focus_leave_time.set(None);
                    }
                    (None, Some(new_idx)) => {
                        // Newly focusing on a band
                        focused_band.set(Some(new_idx));
                        focus_leave_time.set(None);
                        show_shape_dropdown.set(false);
                        show_more_dropdown.set(false);
                    }
                    (Some(old_idx), None) => {
                        // Mouse left the focus area - start fade timeout
                        match leave_time {
                            None => {
                                // First time leaving - record the time
                                focus_leave_time.set(Some((now, old_idx)));
                            }
                            Some((leave_ts, leave_idx)) if leave_idx == old_idx => {
                                // Check if timeout has elapsed
                                if now - leave_ts > popup_fade_timeout_ms {
                                    // Timeout elapsed - clear focus
                                    focused_band.set(None);
                                    focus_leave_time.set(None);
                                    show_shape_dropdown.set(false);
                                    show_more_dropdown.set(false);
                                }
                                // Otherwise keep waiting
                            }
                            _ => {
                                // Different band was being tracked - reset
                                focus_leave_time.set(Some((now, old_idx)));
                            }
                        }
                    }
                    (None, None) => {
                        // No focus, nothing to do
                        focus_leave_time.set(None);
                    }
                }
            },

            // Handle mouse up - end drag, complete selection, potentially remove band
            onmouseup: move |evt: MouseEvent| {
                let coords = evt.element_coordinates();
                let (x, y) = transform_coords(coords.x, coords.y);

                // Complete selection rectangle - copy value first
                let sel_rect = { *selection_rect.read() };
                if let Some((start_x, start_y, _, _)) = sel_rect {
                    let min_x = start_x.min(x);
                    let max_x = start_x.max(x);
                    let min_y = start_y.min(y);
                    let max_y = start_y.max(y);

                    // Find all bands within the selection rectangle
                    let newly_selected: Vec<usize> = {
                        let bands_vec = bands.read();
                        bands_vec.iter().enumerate()
                            .filter(|(_, band)| {
                                if !band.used { return false; }
                                let bx = freq_to_x(band.frequency as f64);
                                let by = db_to_y(band.gain as f64);
                                bx >= min_x && bx <= max_x && by >= min_y && by <= max_y
                            })
                            .map(|(idx, _)| idx)
                            .collect()
                    };

                    selected_bands.set(newly_selected);
                    selection_rect.set(None);
                    return;
                }

                // End band drag - copy value first
                let band_idx_opt = { *dragging_band.read() };
                if let Some(band_idx) = band_idx_opt {
                    let outside = !is_inside_graph(x, y);

                    // If released outside graph area, remove the band
                    if outside
                        && let Some(cb) = &on_band_remove {
                            cb.call(band_idx);
                        }

                    dragging_band.set(None);
                    drag_start.set(None);
                    drag_start_bands.set(Vec::new());
                    if let Some(cb) = &on_end {
                        cb.call(band_idx);
                    }
                }
            },

            // Handle mouse wheel for Q adjustment
            onwheel: move |evt: WheelEvent| {
                // Always prevent default to stop page scrolling when over the EQ graph
                evt.prevent_default();

                if disabled {
                    return;
                }

                // Adjust Q for dragging, focused, or hovered band - copy values first
                let dragging = { *dragging_band.read() };
                let focused = { *focused_band.read() };
                let hovered = { *hovered_band.read() };
                let target_band = dragging.or(focused).or(hovered);

                if let Some(band_idx) = target_band {
                    // Get the Y delta from wheel event (strip units to get raw value)
                    let delta_vec = evt.delta().strip_units();
                    let delta = delta_vec.y;
                    // Wheel up = increase Q, wheel down = decrease Q
                    let q_multiplier = if delta < 0.0 { 1.15 } else { 0.87 };

                    let updated_band = {
                        let mut bands_vec = bands.write();
                        if band_idx < bands_vec.len() {
                            let new_q = (bands_vec[band_idx].q * q_multiplier).clamp(0.1, 18.0);
                            bands_vec[band_idx].q = new_q;
                            Some(bands_vec[band_idx].clone())
                        } else {
                            None
                        }
                    };

                    if let (Some(band), Some(cb)) = (updated_band, &on_band_change) {
                        cb.call((band_idx, band));
                    }
                }
            },

            // Handle mousedown on empty area - detect double-click to add new band
            // Using mousedown instead of ondoubleclick so user can immediately drag the new node
            onmousedown: move |evt: MouseEvent| {
                if disabled {
                    return;
                }

                let coords = evt.element_coordinates();
                let (x, y) = transform_coords(coords.x, coords.y);

                // Only handle if inside graph area
                if !is_inside_graph(x, y) {
                    last_click.set(None);
                    return;
                }

                // Check if clicking on an existing band (those have their own handlers)
                let clicking_on_band = {
                    let bands_vec = bands.read();
                    bands_vec.iter().any(|band| {
                        if !band.used { return false; }
                        let bx = freq_to_x(band.frequency as f64);
                        let by = db_to_y(band.gain as f64);
                        let dist = ((x - bx).powi(2) + (y - by).powi(2)).sqrt();
                        dist < 15.0
                    })
                };

                if clicking_on_band {
                    last_click.set(None);
                    return;
                }

                // Get current timestamp
                let now = now_ms();

                // Check if this is a double-click (second click within threshold)
                let last_click_val = { *last_click.read() };
                let is_double_click = if let Some((last_time, last_x, last_y)) = last_click_val {
                    let time_diff = now - last_time;
                    let distance = ((x - last_x).powi(2) + (y - last_y).powi(2)).sqrt();
                    time_diff < double_click_threshold_ms && distance < double_click_distance
                } else {
                    false
                };

                if is_double_click {
                    // Double-click detected! Create a new band and start dragging it
                    last_click.set(None);

                    // Find first unused band slot or use next index
                    let new_idx = {
                        let bands_vec = bands.read();
                        bands_vec.iter().position(|b| !b.used).unwrap_or(bands_vec.len())
                    };

                    if new_idx >= MAX_BANDS {
                        return; // Max bands reached
                    }

                    let freq = x_to_freq(x).clamp(20.0, 20000.0) as f32;
                    let gain = y_to_db(y).clamp(-db_range, db_range) as f32;
                    let shape = get_filter_type_for_position(freq as f64, gain as f64);

                    // For cuts, set gain to 0 (cuts use Q for slope, not gain)
                    let final_gain: f32 = match shape {
                        EqBandShape::LowCut | EqBandShape::HighCut => 0.0,
                        _ => gain,
                    };

                    let new_band = EqBand {
                        index: new_idx,
                        used: true,
                        enabled: true,
                        frequency: freq,
                        gain: final_gain,
                        q: 1.0,
                        shape,
                        solo: false,
                        stereo_mode: StereoMode::default(),
                    };

                    // Add the band
                    if let Some(cb) = &on_band_add {
                        cb.call(new_band);
                    }

                    // Start dragging the new band immediately
                    dragging_band.set(Some(new_idx));
                    if let Some(cb) = &on_begin {
                        cb.call(new_idx);
                    }

                    evt.stop_propagation();
                    evt.prevent_default();
                } else {
                    // First click - record it for potential double-click
                    last_click.set(Some((now, x, y)));

                    // Also start a potential selection rectangle (will be cancelled if double-click occurs)
                    // Clear any existing selection if not shift-clicking
                    if !evt.modifiers().shift() {
                        selected_bands.set(Vec::new());
                    }
                    // Start selection rectangle from this point
                    selection_rect.set(Some((x, y, x, y)));
                    drag_start.set(Some((x, y)));
                }
            },

            // Grid lines
            for (i, line) in grid_elements.read().iter().enumerate() {
                line {
                    key: "{i}",
                    x1: "{line.0}",
                    y1: "{line.1}",
                    x2: "{line.2}",
                    y2: "{line.3}",
                    stroke: if line.4 { grid_major_color } else { grid_color },
                    stroke_width: if line.4 { "1" } else { "0.5" },
                }
            }

            // Frequency labels
            for (i, label) in freq_labels.read().iter().enumerate() {
                text {
                    key: "{i}",
                    x: "{label.0}",
                    y: "{label.1}",
                    fill: text_color,
                    font_size: "11",
                    text_anchor: "middle",
                    dominant_baseline: "hanging",
                    "{label.2}"
                }
            }

            // dB labels
            for (i, label) in db_labels.read().iter().enumerate() {
                text {
                    key: "{i}",
                    x: "{label.0}",
                    y: "{label.1}",
                    fill: text_color,
                    font_size: "11",
                    text_anchor: "end",
                    dominant_baseline: "middle",
                    "{label.2}"
                }
            }

            // Per-band influence curves (rendered behind main curve)
            {
                let curves = curve_paths.read();
                rsx! {
                    for (band_idx, (stroke_path, fill_path)) in curves.band_curves.iter().enumerate() {
                        // Band fill
                        if fill_curve {
                            path {
                                key: "band-fill-{band_idx}",
                                d: "{fill_path}",
                                fill: "{get_band_fill_color(band_idx)}",
                                stroke: "none",
                            }
                        }
                        // Band stroke
                        path {
                            d: "{stroke_path}",
                            fill: "none",
                            stroke: "{get_band_color(band_idx)}",
                            stroke_width: "1.5",
                            stroke_linecap: "round",
                            stroke_linejoin: "round",
                            opacity: "0.6",
                        }
                    }
                }
            }

            // Main combined curve fill
            if fill_curve {
                path {
                    d: "{curve_paths.read().combined_fill}",
                    fill: curve_fill,
                    stroke: "none",
                }
            }

            // Main combined curve stroke
            path {
                d: "{curve_paths.read().combined_stroke}",
                fill: "none",
                stroke: curve_stroke,
                stroke_width: "2",
                stroke_linecap: "round",
                stroke_linejoin: "round",
            }

            // SVG filter definitions for glow effect
            defs {
                // Subtle white glow filter for band nodes
                filter {
                    id: "glow",
                    x: "-50%",
                    y: "-50%",
                    width: "200%",
                    height: "200%",
                    // Gaussian blur for glow
                    feGaussianBlur {
                        _in: "SourceGraphic",
                        std_deviation: "2",
                        result: "blur",
                    }
                    // Merge blur with original
                    feMerge {
                        feMergeNode {
                            _in: "blur",
                        }
                        feMergeNode {
                            _in: "SourceGraphic",
                        }
                    }
                }
            }

            // Band control points - main circles with glow
            for (idx, band) in bands_snapshot.iter().enumerate() {
                if band.used {
                    {
                        let x = freq_to_x(band.frequency as f64);
                        let y = db_to_y(band.gain as f64);
                        let is_dragging = *dragging_band.read() == Some(idx);
                        let is_hovered = *hovered_band.read() == Some(idx);

                        let band_color_str = get_band_color(idx);

                        // Solid fill with the band color
                        let fill_color = if band.enabled {
                            band_color_str.to_string()
                        } else {
                            band_inactive_color.to_string()
                        };

                        // Subtle white glow outline - more visible when hovered/dragging
                        let glow_opacity = if !band.enabled {
                            0.0
                        } else if is_dragging {
                            0.9
                        } else if is_hovered {
                            0.7
                        } else {
                            0.4 // Subtle glow at rest
                        };

                        let radius = if is_dragging { 10 } else if is_hovered { 9 } else { 7 };

                        rsx! {
                            // Outer glow ring (subtle white)
                            circle {
                                key: "{idx}",
                                cx: "{x}",
                                cy: "{y}",
                                r: "{radius + 3}",
                                fill: "none",
                                stroke: "rgba(255, 255, 255, {glow_opacity * 0.3})",
                                stroke_width: "2",
                                pointer_events: "none",
                                filter: "url(#glow)",
                            }
                            // Main filled circle
                            circle {
                                cx: "{x}",
                                cy: "{y}",
                                r: "{radius}",
                                fill: "{fill_color}",
                                stroke: "rgba(255, 255, 255, {glow_opacity})",
                                stroke_width: "1.5",
                                style: if disabled { "cursor: not-allowed;" } else { "cursor: grab;" },

                                onmouseenter: move |_| {
                                    if dragging_band.read().is_none() {
                                        hovered_band.set(Some(idx));
                                    }
                                },
                                onmouseleave: move |_| {
                                    if *hovered_band.read() == Some(idx) {
                                        hovered_band.set(None);
                                    }
                                },

                                onmousedown: {
                                    let on_begin = on_begin;
                                    move |evt: MouseEvent| {
                                        if disabled {
                                            return;
                                        }
                                        evt.stop_propagation();
                                        evt.prevent_default();

                                        // Handle shift-click for multi-selection
                                        let is_shift = evt.modifiers().shift();

                                        // Read current selection first, then drop the borrow
                                        let current_selected = {
                                            let sel = selected_bands.read();
                                            sel.clone()
                                        };

                                        let new_selected = if is_shift {
                                            // Toggle selection
                                            let mut sel = current_selected.clone();
                                            if sel.contains(&idx) {
                                                sel.retain(|&i| i != idx);
                                            } else {
                                                sel.push(idx);
                                            }
                                            sel
                                        } else if !current_selected.contains(&idx) {
                                            // Click without shift on unselected band - select only this one
                                            vec![idx]
                                        } else {
                                            // Already selected, keep selection
                                            current_selected.clone()
                                        };

                                        selected_bands.set(new_selected.clone());

                                        // Store drag start position for multi-drag
                                        let coords = evt.element_coordinates();
                                        let (vx, vy) = transform_coords(coords.x, coords.y);
                                        drag_start.set(Some((vx, vy)));

                                        // Store original band positions for proportional scaling
                                        let start_bands: Vec<(usize, f32, f32)> = {
                                            let bands_vec = bands.read();
                                            new_selected
                                                .iter()
                                                .filter_map(|&i| {
                                                    bands_vec.get(i).map(|b| (i, b.frequency, b.gain))
                                                })
                                                .collect()
                                        };
                                        drag_start_bands.set(start_bands);

                                        // Clear selection rectangle if any
                                        selection_rect.set(None);

                                        dragging_band.set(Some(idx));
                                        if let Some(cb) = &on_begin {
                                            cb.call(idx);
                                        }
                                    }
                                },

                                // Double-click on band resets gain to 0
                                ondoubleclick: {
                                    let on_band_change = on_band_change;
                                    move |evt: MouseEvent| {
                                        if disabled {
                                            return;
                                        }
                                        evt.stop_propagation();

                                        let updated_band = {
                                            let mut bands_vec = bands.write();
                                            if idx < bands_vec.len() {
                                                bands_vec[idx].gain = 0.0;
                                                Some(bands_vec[idx].clone())
                                            } else {
                                                None
                                            }
                                        };

                                        if let (Some(band), Some(cb)) = (updated_band, &on_band_change) {
                                            cb.call((idx, band));
                                        }
                                    }
                                },
                            }
                        }
                    }
                }
            }

            // Band number labels (dark text for contrast on bright colored nodes)
            for (idx, band) in bands_snapshot.iter().enumerate() {
                if band.used {
                    {
                        let x = freq_to_x(band.frequency as f64);
                        let y = db_to_y(band.gain as f64);

                        // Use dark text for readability on colored background
                        let text_fill = if band.enabled {
                            "rgba(0, 0, 0, 0.7)"
                        } else {
                            "rgba(255, 255, 255, 0.5)"
                        };

                        rsx! {
                            text {
                                key: "{idx}",
                                x: "{x}",
                                y: "{y + 0.5}",
                                fill: text_fill,
                                font_size: "9",
                                font_weight: "600",
                                text_anchor: "middle",
                                dominant_baseline: "middle",
                                pointer_events: "none",
                                "{idx + 1}"
                            }
                        }
                    }
                }
            }

            // Selection rectangle (when dragging to select multiple bands)
            if let Some((start_x, start_y, curr_x, curr_y)) = *selection_rect.read() {
                {
                    let min_x = start_x.min(curr_x);
                    let min_y = start_y.min(curr_y);
                    let width = (curr_x - start_x).abs();
                    let height = (curr_y - start_y).abs();

                    // Only show if rectangle is large enough (avoids flicker on click)
                    if width > 5.0 || height > 5.0 {
                        rsx! {
                            rect {
                                x: "{min_x}",
                                y: "{min_y}",
                                width: "{width}",
                                height: "{height}",
                                fill: "rgba(100, 150, 255, 0.15)",
                                stroke: "rgba(100, 150, 255, 0.6)",
                                stroke_width: "1",
                                stroke_dasharray: "4,2",
                            }
                        }
                    } else {
                        rsx! {}
                    }
                }
            }

            // Unified compact overlay for focused/dragging/hovering band
            // Shows above the node with frequency, gain, Q info and action buttons
            {
                // Determine which band to show overlay for (priority: dragging > focused > hovered)
                let dragging = *dragging_band.read();
                let focused = *focused_band.read();
                let hovered = *hovered_band.read();
                let overlay_band_idx = dragging.or(focused).or(hovered);

                if let Some(band_idx) = overlay_band_idx {
                    let bands_vec = bands.read();
                    if let Some(band) = bands_vec.get(band_idx) {
                        let bx = freq_to_x(band.frequency as f64);
                        let by = db_to_y(band.gain as f64);

                        // Compact popup dimensions
                        let popup_width = 110.0;
                        let popup_height = 50.0;

                        // Position popup above the band node, centered horizontally
                        let popup_x = (bx - popup_width / 2.0).clamp(padding, padding + graph_width - popup_width);
                        let popup_y = if by - popup_height - 18.0 >= padding {
                            by - popup_height - 18.0
                        } else {
                            by + 18.0
                        }.clamp(padding, padding + graph_height - popup_height);

                        // Format values
                        let freq_str = if band.frequency >= 1000.0 {
                            format!("{:.1}k", band.frequency / 1000.0)
                        } else {
                            format!("{:.0}", band.frequency)
                        };

                        let q_or_slope_str = if band.shape.uses_slope() {
                            let slope = q_to_slope_db(band.q);
                            format!("{:.0}dB", slope)
                        } else {
                            format!("{:.1}", band.q)
                        };

                        let band_color = get_band_color(band_idx);
                        let is_interactive = dragging.is_none(); // Only show buttons when not dragging

                        // Clone band data we need for rendering
                        let band_enabled = band.enabled;
                        let band_solo = band.solo;
                        let band_gain = band.gain;
                        let band_shape = band.shape;
                        let _band_stereo_mode = band.stereo_mode;
                        drop(bands_vec);

                        rsx! {
                            // Popup background
                            rect {
                                x: "{popup_x}",
                                y: "{popup_y}",
                                width: "{popup_width}",
                                height: "{popup_height}",
                                rx: "4",
                                fill: "rgba(0, 0, 0, 0.9)",
                                stroke: "{band_color}",
                                stroke_width: "1",
                                stroke_opacity: "0.4",
                            }

                            // Top row: values (freq | gain | Q)
                            text {
                                x: "{popup_x + popup_width / 2.0}",
                                y: "{popup_y + 14.0}",
                                fill: "#ffffff",
                                font_size: "10",
                                text_anchor: "middle",
                                dominant_baseline: "middle",
                                "{freq_str}Hz  {band_gain:+.1}dB  Q{q_or_slope_str}"
                            }

                            // Bottom row: action buttons (only when not dragging)
                            if is_interactive {
                                // Bypass button
                                g {
                                    onclick: {
                                        let on_band_change = on_band_change;
                                        move |evt: MouseEvent| {
                                            evt.stop_propagation();
                                            let updated = {
                                                let mut bv = bands.write();
                                                if band_idx < bv.len() {
                                                    bv[band_idx].enabled = !bv[band_idx].enabled;
                                                    Some(bv[band_idx].clone())
                                                } else { None }
                                            };
                                            if let (Some(b), Some(cb)) = (updated, &on_band_change) {
                                                cb.call((band_idx, b));
                                            }
                                        }
                                    },
                                    style: "cursor: pointer;",
                                    circle {
                                        cx: "{popup_x + 15.0}",
                                        cy: "{popup_y + 35.0}",
                                        r: "8",
                                        fill: if band_enabled { "transparent" } else { "rgba(255,80,80,0.3)" },
                                        stroke: if band_enabled { "#666" } else { "#f66" },
                                        stroke_width: "1",
                                    }
                                    path {
                                        d: "M{popup_x + 15.0} {popup_y + 31.0} v3",
                                        stroke: if band_enabled { "#666" } else { "#f66" },
                                        stroke_width: "1",
                                        fill: "none",
                                    }
                                }

                                // Solo button
                                g {
                                    onclick: {
                                        let on_band_change = on_band_change;
                                        move |evt: MouseEvent| {
                                            evt.stop_propagation();
                                            let updated = {
                                                let mut bv = bands.write();
                                                if band_idx < bv.len() {
                                                    bv[band_idx].solo = !bv[band_idx].solo;
                                                    Some(bv[band_idx].clone())
                                                } else { None }
                                            };
                                            if let (Some(b), Some(cb)) = (updated, &on_band_change) {
                                                cb.call((band_idx, b));
                                            }
                                        }
                                    },
                                    style: "cursor: pointer;",
                                    circle {
                                        cx: "{popup_x + 37.0}",
                                        cy: "{popup_y + 35.0}",
                                        r: "8",
                                        fill: if band_solo { "rgba(255,200,50,0.3)" } else { "transparent" },
                                        stroke: if band_solo { "#fc0" } else { "#666" },
                                        stroke_width: "1",
                                    }
                                    text {
                                        x: "{popup_x + 37.0}",
                                        y: "{popup_y + 35.5}",
                                        fill: if band_solo { "#fc0" } else { "#666" },
                                        font_size: "8",
                                        text_anchor: "middle",
                                        dominant_baseline: "middle",
                                        "S"
                                    }
                                }

                                // Shape button
                                g {
                                    onclick: move |evt: MouseEvent| {
                                        evt.stop_propagation();
                                        let current = *show_shape_dropdown.read();
                                        show_shape_dropdown.set(!current);
                                        show_more_dropdown.set(false);
                                    },
                                    style: "cursor: pointer;",
                                    rect {
                                        x: "{popup_x + 50.0}",
                                        y: "{popup_y + 28.0}",
                                        width: "28",
                                        height: "14",
                                        rx: "2",
                                        fill: "rgba(60,60,65,0.8)",
                                    }
                                    text {
                                        x: "{popup_x + 64.0}",
                                        y: "{popup_y + 35.0}",
                                        fill: "#999",
                                        font_size: "7",
                                        text_anchor: "middle",
                                        dominant_baseline: "middle",
                                        "{band_shape.label()}"
                                    }
                                }

                                // Delete button
                                g {
                                    onclick: {
                                        let on_band_remove = on_band_remove;
                                        move |evt: MouseEvent| {
                                            evt.stop_propagation();
                                            if let Some(cb) = &on_band_remove {
                                                cb.call(band_idx);
                                            }
                                            focused_band.set(None);
                                        }
                                    },
                                    style: "cursor: pointer;",
                                    circle {
                                        cx: "{popup_x + popup_width - 15.0}",
                                        cy: "{popup_y + 35.0}",
                                        r: "8",
                                        fill: "transparent",
                                        stroke: "#666",
                                        stroke_width: "1",
                                    }
                                    path {
                                        d: "M{popup_x + popup_width - 18.0} {popup_y + 32.0} l6,6 M{popup_x + popup_width - 12.0} {popup_y + 32.0} l-6,6",
                                        stroke: "#666",
                                        stroke_width: "1",
                                        fill: "none",
                                    }
                                }
                            }

                            // Shape dropdown (if open and interactive)
                            if is_interactive && *show_shape_dropdown.read() {
                                {
                                    let dropdown_y = popup_y + popup_height + 2.0;
                                    let shapes = EqBandShape::all();

                                    rsx! {
                                        rect {
                                            x: "{popup_x}",
                                            y: "{dropdown_y}",
                                            width: "{popup_width}",
                                            height: "{shapes.len() as f64 * 16.0 + 6.0}",
                                            rx: "3",
                                            fill: "rgba(20, 20, 22, 0.98)",
                                            stroke: "rgba(80, 80, 85, 0.5)",
                                            stroke_width: "1",
                                        }
                                        for (i, shape) in shapes.iter().enumerate() {
                                            {
                                                let item_y = dropdown_y + 3.0 + i as f64 * 16.0;
                                                let is_selected = *shape == band_shape;
                                                let shape_clone = *shape;

                                                rsx! {
                                                    g {
                                                        onclick: {
                                                            let on_band_change = on_band_change;
                                                            move |evt: MouseEvent| {
                                                                evt.stop_propagation();
                                                                let updated = {
                                                                    let mut bv = bands.write();
                                                                    if band_idx < bv.len() {
                                                                        bv[band_idx].shape = shape_clone;
                                                                        if shape_clone.uses_slope() {
                                                                            bv[band_idx].q = 1.0;
                                                                        }
                                                                        Some(bv[band_idx].clone())
                                                                    } else { None }
                                                                };
                                                                if let (Some(b), Some(cb)) = (updated, &on_band_change) {
                                                                    cb.call((band_idx, b));
                                                                }
                                                                show_shape_dropdown.set(false);
                                                            }
                                                        },
                                                        style: "cursor: pointer;",
                                                        rect {
                                                            x: "{popup_x + 3.0}",
                                                            y: "{item_y}",
                                                            width: "{popup_width - 6.0}",
                                                            height: "14",
                                                            rx: "2",
                                                            fill: if is_selected { "rgba(100,150,255,0.3)" } else { "transparent" },
                                                        }
                                                        text {
                                                            x: "{popup_x + 8.0}",
                                                            y: "{item_y + 7.0}",
                                                            fill: if is_selected { "#fff" } else { "#ccc" },
                                                            font_size: "8",
                                                            dominant_baseline: "middle",
                                                            "{shape.label()}"
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        rsx! {}
                    }
                } else {
                    rsx! {}
                }
            }
        }
    }
}

/// All EQ curve paths (combined and per-band).
#[derive(Clone, Default, PartialEq)]
struct AllEqCurves {
    /// Combined curve stroke path.
    combined_stroke: String,
    /// Combined curve fill path.
    combined_fill: String,
    /// Per-band curves: Vec of (stroke_path, fill_path) for each active band.
    band_curves: Vec<(String, String)>,
}

/// Generate all EQ curves (combined and per-band).
#[allow(clippy::too_many_arguments)]
fn generate_all_eq_curves(
    bands: &[EqBand],
    sample_rate: f64,
    min_freq: f64,
    max_freq: f64,
    db_range: f64,
    padding: f64,
    graph_width: f64,
    graph_height: f64,
    num_points: usize,
) -> AllEqCurves {
    let log_min = min_freq.log10();
    let log_max = max_freq.log10();

    let frequencies: Vec<f64> = (0..num_points)
        .map(|i| {
            let t = i as f64 / (num_points - 1) as f64;
            10.0_f64.powf(log_min + t * (log_max - log_min))
        })
        .collect();

    let freq_to_x = |freq: f64| -> f64 {
        let normalized = (freq.log10() - log_min) / (log_max - log_min);
        padding + normalized * graph_width
    };

    let db_to_y = |db: f64| -> f64 {
        let clamped = db.clamp(-db_range, db_range);
        let normalized = 0.5 - clamped / (2.0 * db_range);
        padding + normalized * graph_height
    };

    let zero_y = db_to_y(0.0);

    // Generate combined response
    let combined_response: Vec<f64> = frequencies
        .iter()
        .map(|&freq| calculate_combined_response(bands, freq, sample_rate))
        .collect();

    let (combined_stroke, combined_fill) =
        build_curve_paths(&frequencies, &combined_response, freq_to_x, db_to_y, zero_y);

    // Generate per-band curves
    let mut band_curves = Vec::new();
    for band in bands {
        if !band.used || !band.enabled {
            continue;
        }

        let band_response: Vec<f64> = frequencies
            .iter()
            .map(|&freq| calculate_band_response(band, freq, sample_rate))
            .collect();

        let (stroke, fill) =
            build_curve_paths(&frequencies, &band_response, freq_to_x, db_to_y, zero_y);
        band_curves.push((stroke, fill));
    }

    AllEqCurves {
        combined_stroke,
        combined_fill,
        band_curves,
    }
}

/// Build stroke and fill paths from frequency/response data.
fn build_curve_paths<F, G>(
    frequencies: &[f64],
    response_db: &[f64],
    freq_to_x: F,
    db_to_y: G,
    zero_y: f64,
) -> (String, String)
where
    F: Fn(f64) -> f64,
    G: Fn(f64) -> f64,
{
    let mut stroke_path = String::new();
    for (i, (&freq, &db)) in frequencies.iter().zip(response_db.iter()).enumerate() {
        let x = freq_to_x(freq);
        let y = db_to_y(db);
        if i == 0 {
            stroke_path.push_str(&format!("M{x:.2} {y:.2}"));
        } else {
            stroke_path.push_str(&format!("L{x:.2} {y:.2}"));
        }
    }

    let mut fill_path = String::new();
    let first_x = freq_to_x(frequencies[0]);
    fill_path.push_str(&format!("M{first_x:.2} {zero_y:.2}"));

    for (&freq, &db) in frequencies.iter().zip(response_db.iter()) {
        let x = freq_to_x(freq);
        let y = db_to_y(db);
        fill_path.push_str(&format!("L{x:.2} {y:.2}"));
    }

    let last_x = freq_to_x(*frequencies.last().unwrap());
    fill_path.push_str(&format!("L{last_x:.2} {zero_y:.2}Z"));

    (stroke_path, fill_path)
}

/// Generate the SVG path for the EQ curve.
///
/// Returns (stroke_path, fill_path)
#[allow(dead_code, clippy::too_many_arguments)]
fn generate_eq_curve_path(
    bands: &[EqBand],
    sample_rate: f64,
    min_freq: f64,
    max_freq: f64,
    db_range: f64,
    padding: f64,
    graph_width: f64,
    graph_height: f64,
    num_points: usize,
) -> (String, String) {
    let log_min = min_freq.log10();
    let log_max = max_freq.log10();

    let frequencies: Vec<f64> = (0..num_points)
        .map(|i| {
            let t = i as f64 / (num_points - 1) as f64;
            10.0_f64.powf(log_min + t * (log_max - log_min))
        })
        .collect();

    let response_db: Vec<f64> = frequencies
        .iter()
        .map(|&freq| calculate_combined_response(bands, freq, sample_rate))
        .collect();

    let freq_to_x = |freq: f64| -> f64 {
        let normalized = (freq.log10() - log_min) / (log_max - log_min);
        padding + normalized * graph_width
    };

    let db_to_y = |db: f64| -> f64 {
        let clamped = db.clamp(-db_range, db_range);
        let normalized = 0.5 - clamped / (2.0 * db_range);
        padding + normalized * graph_height
    };

    // Build stroke path
    let mut stroke_path = String::new();
    for (i, (&freq, &db)) in frequencies.iter().zip(response_db.iter()).enumerate() {
        let x = freq_to_x(freq);
        let y = db_to_y(db);
        if i == 0 {
            stroke_path.push_str(&format!("M{x:.2} {y:.2}"));
        } else {
            stroke_path.push_str(&format!("L{x:.2} {y:.2}"));
        }
    }

    // Build fill path (closed area from 0dB line)
    let mut fill_path = String::new();
    let zero_y = db_to_y(0.0);

    let first_x = freq_to_x(frequencies[0]);
    fill_path.push_str(&format!("M{first_x:.2} {zero_y:.2}"));

    for (&freq, &db) in frequencies.iter().zip(response_db.iter()) {
        let x = freq_to_x(freq);
        let y = db_to_y(db);
        fill_path.push_str(&format!("L{x:.2} {y:.2}"));
    }

    let last_x = freq_to_x(*frequencies.last().unwrap());
    fill_path.push_str(&format!("L{last_x:.2} {zero_y:.2}Z"));

    (stroke_path, fill_path)
}

fn calculate_combined_response(bands: &[EqBand], freq: f64, sample_rate: f64) -> f64 {
    let mut total_db = 0.0;

    for band in bands {
        if !band.used || !band.enabled {
            continue;
        }
        total_db += calculate_band_response(band, freq, sample_rate);
    }

    total_db
}

/// Calculate analog second-order filter magnitude squared.
///
/// This implements |H(jw)|² for a biquad filter with transfer function:
/// H(s) = (b0 + b1*s + b2*s²) / (a0 + a1*s + a2*s²)
///
/// Coefficients array: [a0, a1, a2, b0, b1, b2]
fn biquad_magnitude_squared(coeff: &[f64; 6], w: f64) -> f64 {
    let w2 = w * w;
    // Denominator: |a0 + a1*jw + a2*(jw)²|² = |a0 - a2*w²|² + |a1*w|²
    let denom_real = coeff[0] - coeff[2] * w2;
    let denom_imag = coeff[1] * w;
    let denominator = denom_real * denom_real + denom_imag * denom_imag;

    // Numerator: |b0 + b1*jw + b2*(jw)²|² = |b0 - b2*w²|² + |b1*w|²
    let numer_real = coeff[3] - coeff[5] * w2;
    let numer_imag = coeff[4] * w;
    let numerator = numer_real * numer_real + numer_imag * numer_imag;

    if denominator > 1e-30 {
        numerator / denominator
    } else {
        1.0
    }
}

/// Get coefficients for a second-order low-pass filter.
/// Returns [a0, a1, a2, b0, b1, b2] for H(s) = w0² / (s² + (w0/Q)*s + w0²)
fn lowpass_coeffs(w0: f64, q: f64) -> [f64; 6] {
    let w02 = w0 * w0;
    [1.0, w0 / q, w02, w02, 0.0, 0.0]
}

/// Get coefficients for a second-order high-pass filter.
/// Returns [a0, a1, a2, b0, b1, b2] for H(s) = s² / (s² + (w0/Q)*s + w0²)
fn highpass_coeffs(w0: f64, q: f64) -> [f64; 6] {
    let w02 = w0 * w0;
    [1.0, w0 / q, w02, 0.0, 0.0, 1.0]
}

/// Get coefficients for a second-order low-shelf filter.
/// Returns [a0, a1, a2, b0, b1, b2]
///
/// Low shelf boosts/cuts frequencies below w0.
/// H(s) = G * (s² + sqrt(G)*w0/Q*s + w0²) / (s² + sqrt(G)*w0/Q*s + G*w0²)  for boost
/// The response is G at DC (s=0) and 1 at high frequencies.
fn lowshelf_coeffs(w0: f64, gain_linear: f64, q: f64) -> [f64; 6] {
    let w02 = w0 * w0;
    let sqrt_g = gain_linear.sqrt();
    let g4 = gain_linear.sqrt().sqrt(); // Fourth root for smoother transition

    // For low shelf: at DC (w=0), we want gain_linear. At high freq, we want 1.
    // Denominator: a0 + a1*s + a2*s²
    // Numerator: b0 + b1*s + b2*s²
    // At s=0: H = b0/a0 = gain_linear
    // At s=inf: H = b2/a2 = 1
    [
        w02,
        w0 * g4 / q,
        1.0,
        gain_linear * w02,
        w0 * sqrt_g * g4 / q,
        1.0,
    ]
}

/// Get coefficients for a second-order high-shelf filter.
/// Returns [a0, a1, a2, b0, b1, b2]
///
/// High shelf boosts/cuts frequencies above w0.
/// The response is 1 at DC and G at high frequencies.
fn highshelf_coeffs(w0: f64, gain_linear: f64, q: f64) -> [f64; 6] {
    let w02 = w0 * w0;
    let sqrt_g = gain_linear.sqrt();
    let g4 = gain_linear.sqrt().sqrt(); // Fourth root for smoother transition

    // For high shelf: at DC (w=0), we want 1. At high freq, we want gain_linear.
    // At s=0: H = b0/a0 = 1
    // At s=inf: H = b2/a2 = gain_linear
    [
        w02,
        w0 * g4 / q,
        1.0,
        w02,
        w0 * sqrt_g * g4 / q,
        gain_linear,
    ]
}

/// Get coefficients for a second-order peaking/bell filter.
/// Returns [a0, a1, a2, b0, b1, b2]
///
/// The analog transfer function for a peak filter is:
/// H(s) = (s² + s*(w0/Q)*A + w0²) / (s² + s*(w0/Q)/A + w0²)
/// where A = sqrt(gain_linear) for boost, A = 1/sqrt(gain_linear) for cut
///
/// At w = w0, for boost: magnitude = A * A = gain_linear
/// At w = w0, for cut: magnitude = 1/A * 1/A = gain_linear
fn peak_coeffs(w0: f64, gain_linear: f64, q: f64) -> [f64; 6] {
    let w02 = w0 * w0;
    let a = gain_linear.sqrt();

    // Coefficients: [a0, a1, a2, b0, b1, b2]
    // For the transfer function H(s) = (b0 + b1*s + b2*s²) / (a0 + a1*s + a2*s²)
    // We use: H(s) = (w0² + (w0*A/Q)*s + s²) / (w0² + (w0/(A*Q))*s + s²)
    //
    // At s = jw0: numerator = w0² + jw0²*A/Q - w0² = jw0²*A/Q
    //             denominator = w0² + jw0²/(A*Q) - w0² = jw0²/(A*Q)
    //             |H(jw0)| = |A/Q| / |1/(A*Q)| = A²  = gain_linear
    [w02, w0 / (a * q), 1.0, w02, w0 * a / q, 1.0]
}

/// Get coefficients for a second-order notch filter.
/// Returns [a0, a1, a2, b0, b1, b2]
fn notch_coeffs(w0: f64, q: f64) -> [f64; 6] {
    let w02 = w0 * w0;
    [1.0, w0 / q, w02, w02, 0.0, 1.0]
}

/// Get coefficients for a second-order band-pass filter.
/// Returns [a0, a1, a2, b0, b1, b2]
fn bandpass_coeffs(w0: f64, q: f64) -> [f64; 6] {
    let w02 = w0 * w0;
    [1.0, w0 / q, w02, 0.0, w0 / q, 0.0]
}

/// Calculate cascaded filter response for higher orders.
/// For a filter of order N, we cascade N/2 second-order sections with Butterworth Q distribution.
fn cascaded_magnitude_db(freq: f64, f0: f64, order: usize, filter_type: &EqBandShape) -> f64 {
    if order == 0 {
        return 0.0;
    }

    // Angular frequency
    let w = 2.0 * std::f64::consts::PI * freq;
    let w0 = 2.0 * std::f64::consts::PI * f0;

    // For first-order (order 1), use a simple one-pole response
    if order == 1 {
        let _ratio = freq / f0;
        let mag_sq = match filter_type {
            EqBandShape::LowCut | EqBandShape::HighCut => {
                // First-order high-pass: H(s) = s / (s + w0)
                // |H(jw)|² = w² / (w² + w0²)
                if matches!(filter_type, EqBandShape::LowCut) {
                    let w2 = w * w;
                    let w02 = w0 * w0;
                    w2 / (w2 + w02)
                } else {
                    // First-order low-pass: H(s) = w0 / (s + w0)
                    // |H(jw)|² = w0² / (w² + w0²)
                    let w2 = w * w;
                    let w02 = w0 * w0;
                    w02 / (w2 + w02)
                }
            }
            _ => 1.0,
        };
        return 10.0 * mag_sq.max(1e-30).log10();
    }

    // For higher orders, cascade second-order sections
    let num_sections = order / 2;
    let has_first_order = order % 2 == 1;

    let mut total_mag_sq = 1.0;

    // Add first-order section if odd order
    if has_first_order {
        let first_order_mag = match filter_type {
            EqBandShape::LowCut => {
                let w2 = w * w;
                let w02 = w0 * w0;
                w2 / (w2 + w02)
            }
            EqBandShape::HighCut => {
                let w2 = w * w;
                let w02 = w0 * w0;
                w02 / (w2 + w02)
            }
            _ => 1.0,
        };
        total_mag_sq *= first_order_mag;
    }

    // Add second-order sections with Butterworth pole distribution
    for i in 0..num_sections {
        // Butterworth Q for each section: Q = 1 / (2 * cos(theta))
        // where theta = π * (2k + 1) / (2n) for k = 0..n/2-1
        let theta = std::f64::consts::PI * (2 * i + 1) as f64 / (2 * order) as f64;
        let section_q = 1.0 / (2.0 * theta.cos());

        let coeffs = match filter_type {
            EqBandShape::LowCut => highpass_coeffs(w0, section_q),
            EqBandShape::HighCut => lowpass_coeffs(w0, section_q),
            _ => [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], // Unity
        };

        total_mag_sq *= biquad_magnitude_squared(&coeffs, w);
    }

    // Convert to dB
    10.0 * total_mag_sq.max(1e-30).log10()
}

fn calculate_band_response(band: &EqBand, freq: f64, _sample_rate: f64) -> f64 {
    let f0 = band.frequency as f64;
    let gain = band.gain as f64;
    let q = band.q as f64;

    // Angular frequencies
    let w = 2.0 * std::f64::consts::PI * freq;
    let w0 = 2.0 * std::f64::consts::PI * f0;

    match band.shape {
        EqBandShape::Bell => {
            // Peaking/parametric EQ filter using proper biquad response
            let gain_linear = 10.0_f64.powf(gain / 20.0);
            let coeffs = peak_coeffs(w0, gain_linear, q);
            let mag_sq = biquad_magnitude_squared(&coeffs, w);
            10.0 * mag_sq.max(1e-30).log10()
        }
        EqBandShape::LowShelf => {
            // Low shelf filter
            let gain_linear = 10.0_f64.powf(gain / 20.0);
            let coeffs = lowshelf_coeffs(w0, gain_linear, q.max(0.5));
            let mag_sq = biquad_magnitude_squared(&coeffs, w);
            10.0 * mag_sq.max(1e-30).log10()
        }
        EqBandShape::HighShelf => {
            // High shelf filter
            let gain_linear = 10.0_f64.powf(gain / 20.0);
            let coeffs = highshelf_coeffs(w0, gain_linear, q.max(0.5));
            let mag_sq = biquad_magnitude_squared(&coeffs, w);
            10.0 * mag_sq.max(1e-30).log10()
        }
        EqBandShape::LowCut => {
            // High-pass filter (cuts low frequencies)
            // Q represents slope: 1 = 6dB/oct (1st order), 2 = 12dB/oct (2nd order), etc.
            let order = (q * 2.0).round().max(1.0) as usize;
            cascaded_magnitude_db(freq, f0, order, &EqBandShape::LowCut)
        }
        EqBandShape::HighCut => {
            // Low-pass filter (cuts high frequencies)
            // Q represents slope: 1 = 6dB/oct (1st order), 2 = 12dB/oct (2nd order), etc.
            let order = (q * 2.0).round().max(1.0) as usize;
            cascaded_magnitude_db(freq, f0, order, &EqBandShape::HighCut)
        }
        EqBandShape::Notch => {
            // Notch filter
            let coeffs = notch_coeffs(w0, q.max(0.5));
            let mag_sq = biquad_magnitude_squared(&coeffs, w);
            10.0 * mag_sq.max(1e-30).log10()
        }
        EqBandShape::BandPass => {
            // Band-pass filter
            let coeffs = bandpass_coeffs(w0, q.max(0.5));
            let mag_sq = biquad_magnitude_squared(&coeffs, w);
            // Normalize so peak is at 0dB, then add gain
            let peak_mag_sq = biquad_magnitude_squared(&coeffs, w0);
            let normalized = mag_sq / peak_mag_sq.max(1e-30);
            gain + 10.0 * normalized.max(1e-30).log10()
        }
        EqBandShape::TiltShelf | EqBandShape::FlatTilt => {
            // Tilt filter - gradual slope across spectrum
            // Approximated as a slope in dB per octave
            let octaves = (freq / f0).log2();
            let slope_db_per_oct = gain / 3.0;
            octaves * slope_db_per_oct
        }
        EqBandShape::AllPass => 0.0, // All-pass doesn't change magnitude
    }
}

fn generate_grid_elements(
    padding: f64,
    graph_width: f64,
    graph_height: f64,
    min_freq: f64,
    max_freq: f64,
    db_range: f64,
) -> Vec<(f64, f64, f64, f64, bool)> {
    let mut lines = Vec::new();
    let log_min = min_freq.log10();
    let log_max = max_freq.log10();

    let freq_to_x = |freq: f64| -> f64 {
        let normalized = (freq.log10() - log_min) / (log_max - log_min);
        padding + normalized * graph_width
    };

    let db_to_y = |db: f64| -> f64 {
        let normalized = 0.5 - db / (2.0 * db_range);
        padding + normalized * graph_height
    };

    // Vertical frequency lines
    let major_freqs = [100.0, 1000.0, 10000.0];
    let minor_freqs = [20.0, 50.0, 200.0, 500.0, 2000.0, 5000.0, 20000.0];

    for freq in major_freqs {
        if freq >= min_freq && freq <= max_freq {
            let x = freq_to_x(freq);
            lines.push((x, padding, x, padding + graph_height, true));
        }
    }

    for freq in minor_freqs {
        if freq >= min_freq && freq <= max_freq {
            let x = freq_to_x(freq);
            lines.push((x, padding, x, padding + graph_height, false));
        }
    }

    // Horizontal dB lines
    let y_zero = db_to_y(0.0);
    lines.push((padding, y_zero, padding + graph_width, y_zero, true));

    let db_step = 6.0;
    let mut db = db_step;
    while db <= db_range {
        let y_pos = db_to_y(db);
        let y_neg = db_to_y(-db);
        lines.push((padding, y_pos, padding + graph_width, y_pos, false));
        lines.push((padding, y_neg, padding + graph_width, y_neg, false));
        db += db_step;
    }

    lines
}

fn generate_freq_labels(
    padding: f64,
    graph_width: f64,
    height: f64,
    min_freq: f64,
    max_freq: f64,
) -> Vec<(f64, f64, String)> {
    let mut labels = Vec::new();
    let log_min = min_freq.log10();
    let log_max = max_freq.log10();
    let y = height - padding + 15.0;

    let freq_to_x = |freq: f64| -> f64 {
        let normalized = (freq.log10() - log_min) / (log_max - log_min);
        padding + normalized * graph_width
    };

    let freq_labels = [
        (20.0, "20"),
        (50.0, "50"),
        (100.0, "100"),
        (200.0, "200"),
        (500.0, "500"),
        (1000.0, "1k"),
        (2000.0, "2k"),
        (5000.0, "5k"),
        (10000.0, "10k"),
        (20000.0, "20k"),
    ];

    for (freq, label) in freq_labels {
        if freq >= min_freq && freq <= max_freq {
            labels.push((freq_to_x(freq), y, label.to_string()));
        }
    }

    labels
}

fn generate_db_labels(padding: f64, graph_height: f64, db_range: f64) -> Vec<(f64, f64, String)> {
    let mut labels = Vec::new();
    let x = padding - 10.0;

    let db_to_y = |db: f64| -> f64 {
        let normalized = 0.5 - db / (2.0 * db_range);
        padding + normalized * graph_height
    };

    labels.push((x, db_to_y(0.0), "0".to_string()));

    let db_step = 6.0;
    let mut db = db_step;
    while db <= db_range {
        labels.push((x, db_to_y(db), format!("+{}", db as i32)));
        labels.push((x, db_to_y(-db), format!("{}", -(db as i32))));
        db += db_step;
    }

    labels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eq_band_default() {
        let band = EqBand::default();
        assert!(!band.used);
        assert!(!band.enabled);
        assert_eq!(band.gain, 0.0);
    }

    #[test]
    fn test_band_response_bell() {
        let band = EqBand {
            used: true,
            enabled: true,
            frequency: 1000.0,
            gain: 6.0,
            q: 1.0,
            shape: EqBandShape::Bell,
            ..Default::default()
        };

        let response_at_center = calculate_band_response(&band, 1000.0, 48000.0);
        // Bell filter should produce gain at center frequency
        // Allow some tolerance for numerical precision
        assert!(
            (response_at_center - 6.0).abs() < 0.5,
            "Expected ~6.0 dB at center, got {response_at_center}"
        );

        let response_far = calculate_band_response(&band, 100.0, 48000.0);
        // Far from center should be close to 0 dB
        assert!(
            response_far.abs() < 2.0,
            "Expected near 0 dB far from center, got {response_far}"
        );
    }

    #[test]
    fn test_combined_response() {
        let bands = vec![
            EqBand {
                used: true,
                enabled: true,
                frequency: 100.0,
                gain: 3.0,
                q: 1.0,
                shape: EqBandShape::Bell,
                ..Default::default()
            },
            EqBand {
                used: true,
                enabled: true,
                frequency: 10000.0,
                gain: -3.0,
                q: 1.0,
                shape: EqBandShape::Bell,
                ..Default::default()
            },
        ];

        let mid_response = calculate_combined_response(&bands, 1000.0, 48000.0);
        assert!(mid_response.abs() < 1.0);
    }

    #[test]
    fn test_grid_generation() {
        let grid = generate_grid_elements(40.0, 720.0, 220.0, 20.0, 20000.0, 24.0);
        assert!(!grid.is_empty());

        let major_count = grid.iter().filter(|l| l.4).count();
        let minor_count = grid.iter().filter(|l| !l.4).count();
        assert!(major_count > 0);
        assert!(minor_count > 0);
    }

    #[test]
    fn test_freq_labels_generation() {
        let labels = generate_freq_labels(40.0, 720.0, 300.0, 20.0, 20000.0);
        assert!(!labels.is_empty());

        let label_texts: Vec<&str> = labels.iter().map(|l| l.2.as_str()).collect();
        assert!(label_texts.contains(&"100"));
        assert!(label_texts.contains(&"1k"));
        assert!(label_texts.contains(&"10k"));
    }

    #[test]
    fn test_band_response_bell_negative_gain() {
        let band = EqBand {
            used: true,
            enabled: true,
            frequency: 1000.0,
            gain: -6.0, // Negative gain (cut)
            q: 1.0,
            shape: EqBandShape::Bell,
            ..Default::default()
        };

        let response_at_center = calculate_band_response(&band, 1000.0, 48000.0);
        // Bell filter with negative gain should produce cut at center frequency
        assert!(
            (response_at_center - (-6.0)).abs() < 0.5,
            "Expected ~-6.0 dB at center for negative gain, got {response_at_center}"
        );

        let response_far = calculate_band_response(&band, 100.0, 48000.0);
        // Far from center should be close to 0 dB
        assert!(
            response_far.abs() < 2.0,
            "Expected near 0 dB far from center, got {response_far}"
        );
    }

    #[test]
    fn test_band_response_low_shelf() {
        let band = EqBand {
            used: true,
            enabled: true,
            frequency: 100.0,
            gain: 6.0,
            q: 0.7,
            shape: EqBandShape::LowShelf,
            ..Default::default()
        };

        // Low shelf should boost below cutoff frequency
        let response_low = calculate_band_response(&band, 20.0, 48000.0);
        assert!(
            response_low > 3.0,
            "Expected boost below cutoff for low shelf, got {response_low}"
        );

        // Should be close to 0 dB well above cutoff
        let response_high = calculate_band_response(&band, 1000.0, 48000.0);
        assert!(
            response_high.abs() < 2.0,
            "Expected ~0 dB above cutoff for low shelf, got {response_high}"
        );
    }

    #[test]
    fn test_band_response_high_shelf() {
        let band = EqBand {
            used: true,
            enabled: true,
            frequency: 8000.0,
            gain: 6.0,
            q: 0.7,
            shape: EqBandShape::HighShelf,
            ..Default::default()
        };

        // High shelf should boost above cutoff frequency
        let response_high = calculate_band_response(&band, 16000.0, 48000.0);
        assert!(
            response_high > 3.0,
            "Expected boost above cutoff for high shelf, got {response_high}"
        );

        // Should be close to 0 dB well below cutoff
        let response_low = calculate_band_response(&band, 1000.0, 48000.0);
        assert!(
            response_low.abs() < 2.0,
            "Expected ~0 dB below cutoff for high shelf, got {response_low}"
        );
    }

    #[test]
    fn test_db_labels_generation() {
        let labels = generate_db_labels(40.0, 220.0, 24.0);
        assert!(!labels.is_empty());

        let label_texts: Vec<&str> = labels.iter().map(|l| l.2.as_str()).collect();
        assert!(label_texts.contains(&"0"));
        assert!(label_texts.contains(&"+6"));
        assert!(label_texts.contains(&"-6"));
    }
}
