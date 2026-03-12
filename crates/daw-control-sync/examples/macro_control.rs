//! Example: Using DawSync for real-time macro parameter control
//!
//! This demonstrates how a macro plugin would use the synchronous DAW API
//! to control FX parameters in real-time without blocking the audio loop.

use daw_control_sync::DawSync;

/// Simulate a macro mapping
struct MacroMapping {
    source_macro: u8,
    target_track: u32,
    target_fx: u32,
    target_param: u32,
    min_value: f32,
    max_value: f32,
}

impl MacroMapping {
    /// Apply a scale range transformation
    fn transform(&self, macro_value: f32) -> f32 {
        let clamped = macro_value.clamp(0.0, 1.0);
        self.min_value + clamped * (self.max_value - self.min_value)
    }
}

/// Simulate an audio processing loop
fn process_macro_mappings(
    daw: &DawSync,
    macro_values: &[f32; 8],
    mappings: &[MacroMapping],
) -> eyre::Result<()> {
    // In real audio processing, this runs in the real-time loop
    for mapping in mappings {
        let macro_value = macro_values[mapping.source_macro as usize];
        let transformed = mapping.transform(macro_value);

        // Queue the parameter change (non-blocking!)
        daw.queue_set_param(
            mapping.target_track,
            mapping.target_fx,
            mapping.target_param,
            transformed,
        )?;
    }

    Ok(())
}

fn main() -> eyre::Result<()> {
    println!("DawSync Macro Control Example");
    println!("=============================\n");

    // In a real plugin, this connection would come from the host
    // For this example, we just show the API usage
    println!("Example mappings configuration:");
    println!("  Macro 0 → Track 0, FX 1, Param 2 (0.0-1.0 scale)");
    println!("  Macro 1 → Track 1, FX 0, Param 3 (0.5-8.0 scale)");
    println!("  Macro 2 → Track 2, FX 2, Param 1 (0.0-0.3 scale)\n");

    // Show what the API looks like
    println!("Pseudo-code usage in audio loop:");
    println!("```rust");
    println!("fn process(buffer: &mut Buffer, daw: &DawSync) {{");
    println!("    let macro_values = read_macro_parameters();");
    println!("    let mappings = load_macro_mappings();");
    println!("");
    println!("    for mapping in mappings {{");
    println!("        let transformed = mapping.transform(macro_values[mapping.source]);");
    println!("        daw.queue_set_param(");
    println!("            mapping.target_track,");
    println!("            mapping.target_fx,");
    println!("            mapping.target_param,");
    println!("            transformed,");
    println!("        )?;");
    println!("    }}");
    println!("}}");
    println!("```\n");

    // Define example mappings
    let mappings = vec![
        MacroMapping {
            source_macro: 0,
            target_track: 0,
            target_fx: 1,
            target_param: 2,
            min_value: 0.0,
            max_value: 1.0,
        },
        MacroMapping {
            source_macro: 1,
            target_track: 1,
            target_fx: 0,
            target_param: 3,
            min_value: 0.5,
            max_value: 8.0,
        },
        MacroMapping {
            source_macro: 2,
            target_track: 2,
            target_fx: 2,
            target_param: 1,
            min_value: 0.0,
            max_value: 0.3,
        },
    ];

    // Simulate macro parameter values from automation
    let macro_values = [0.0, 0.5, 1.0, 0.25, 0.75, 0.1, 0.9, 0.5];

    println!("Simulated macro values: {:?}\n", macro_values);

    println!("Resulting parameter changes (if DawSync were connected):");
    for mapping in &mappings {
        let macro_value = macro_values[mapping.source_macro as usize];
        let transformed = mapping.transform(macro_value);
        println!(
            "  Macro {} (value={:.2}) → Track {}, FX {}, Param {} = {:.4}",
            mapping.source_macro,
            macro_value,
            mapping.target_track,
            mapping.target_fx,
            mapping.target_param,
            transformed
        );
    }

    println!("\nKey benefits of DawSync:");
    println!("  ✓ Non-blocking: queue_set_param never blocks");
    println!("  ✓ Real-time safe: can be called from audio processing loop");
    println!("  ✓ DAW-agnostic: works with any DAW service");
    println!("  ✓ Async underneath: background runtime handles actual DAW calls");

    Ok(())
}
