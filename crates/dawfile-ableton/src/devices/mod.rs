//! Typed parameter models for Ableton built-in devices.
//!
//! Each sub-module contains parameter structs, a parser, and a writer for one
//! category of devices. The [`BuiltinParams`] enum wraps all variants so a
//! [`Device`](crate::types::Device) can carry its recognised parameters.

pub mod delay_reverb;
pub mod dynamics;
pub mod eq;
pub mod modulation;
pub mod utility;

use crate::write::xml_writer::AbletonXmlWriter;
use roxmltree::Node;
use std::io::{self, Write};

/// Typed parameters for known Ableton built-in devices.
/// Enables cross-DAW parameter translation (e.g., Ableton EQ Eight -> REAPER ReaEQ).
#[derive(Debug, Clone)]
pub enum BuiltinParams {
    Eq8(eq::Eq8Params),
    Compressor(dynamics::CompressorParams),
    GlueCompressor(dynamics::GlueCompressorParams),
    Gate(dynamics::GateParams),
    Limiter(dynamics::LimiterParams),
    MultibandDynamics(dynamics::MultibandDynamicsParams),
    Reverb(delay_reverb::ReverbParams),
    Delay(delay_reverb::DelayParams),
    Echo(delay_reverb::EchoParams),
    AutoFilter(modulation::AutoFilterParams),
    Chorus(modulation::ChorusParams),
    Phaser(modulation::PhaserParams),
    Flanger(modulation::FlangerParams),
    Saturator(modulation::SaturatorParams),
    Utility(utility::UtilityParams),
    Tuner(utility::TunerParams),
    Cabinet(utility::CabinetParams),
    Erosion(utility::ErosionParams),
    Redux(utility::ReduxParams),
    Vinyl(utility::VinylParams),
}

/// Try to parse typed builtin parameters from a device XML node.
///
/// Returns `None` if `tag` is not a recognised built-in device.
pub fn parse_builtin_params(tag: &str, node: Node<'_, '_>) -> Option<BuiltinParams> {
    match tag {
        "Eq8" => Some(BuiltinParams::Eq8(eq::parse(node))),
        "Compressor2" => Some(BuiltinParams::Compressor(dynamics::parse_compressor(node))),
        "GlueCompressor" => Some(BuiltinParams::GlueCompressor(
            dynamics::parse_glue_compressor(node),
        )),
        "Gate" => Some(BuiltinParams::Gate(dynamics::parse_gate(node))),
        "Limiter" => Some(BuiltinParams::Limiter(dynamics::parse_limiter(node))),
        "MultibandDynamics" => Some(BuiltinParams::MultibandDynamics(
            dynamics::parse_multiband_dynamics(node),
        )),
        "Reverb" => Some(BuiltinParams::Reverb(delay_reverb::parse_reverb(node))),
        "Delay" => Some(BuiltinParams::Delay(delay_reverb::parse_delay(node))),
        "Echo" => Some(BuiltinParams::Echo(delay_reverb::parse_echo(node))),
        "AutoFilter" => Some(BuiltinParams::AutoFilter(modulation::parse_auto_filter(
            node,
        ))),
        "Chorus2" | "ChorusEnsemble" => Some(BuiltinParams::Chorus(modulation::parse_chorus(node))),
        "Phaser" | "PhaserNew" => Some(BuiltinParams::Phaser(modulation::parse_phaser(node))),
        "Flanger" => Some(BuiltinParams::Flanger(modulation::parse_flanger(node))),
        "Saturator" => Some(BuiltinParams::Saturator(modulation::parse_saturator(node))),
        "StereoGain" => Some(BuiltinParams::Utility(utility::parse_utility(node))),
        "Tuner" => Some(BuiltinParams::Tuner(utility::parse_tuner(node))),
        "Cabinet" => Some(BuiltinParams::Cabinet(utility::parse_cabinet(node))),
        "Erosion" => Some(BuiltinParams::Erosion(utility::parse_erosion(node))),
        "Redux" | "Redux2" => Some(BuiltinParams::Redux(utility::parse_redux(node))),
        "Vinyl" => Some(BuiltinParams::Vinyl(utility::parse_vinyl(node))),
        _ => None,
    }
}

/// Write the typed builtin parameters into an already-opened device element.
pub fn write_builtin_params<W: Write>(
    w: &mut AbletonXmlWriter<W>,
    params: &BuiltinParams,
) -> io::Result<()> {
    match params {
        BuiltinParams::Eq8(p) => eq::write(w, p),
        BuiltinParams::Compressor(p) => dynamics::write_compressor(w, p),
        BuiltinParams::GlueCompressor(p) => dynamics::write_glue_compressor(w, p),
        BuiltinParams::Gate(p) => dynamics::write_gate(w, p),
        BuiltinParams::Limiter(p) => dynamics::write_limiter(w, p),
        BuiltinParams::MultibandDynamics(p) => dynamics::write_multiband_dynamics(w, p),
        BuiltinParams::Reverb(p) => delay_reverb::write_reverb(w, p),
        BuiltinParams::Delay(p) => delay_reverb::write_delay(w, p),
        BuiltinParams::Echo(p) => delay_reverb::write_echo(w, p),
        BuiltinParams::AutoFilter(p) => modulation::write_auto_filter(w, p),
        BuiltinParams::Chorus(p) => modulation::write_chorus(w, p),
        BuiltinParams::Phaser(p) => modulation::write_phaser(w, p),
        BuiltinParams::Flanger(p) => modulation::write_flanger(w, p),
        BuiltinParams::Saturator(p) => modulation::write_saturator(w, p),
        BuiltinParams::Utility(p) => utility::write_utility(w, p),
        BuiltinParams::Tuner(p) => utility::write_tuner(w, p),
        BuiltinParams::Cabinet(p) => utility::write_cabinet(w, p),
        BuiltinParams::Erosion(p) => utility::write_erosion(w, p),
        BuiltinParams::Redux(p) => utility::write_redux(w, p),
        BuiltinParams::Vinyl(p) => utility::write_vinyl(w, p),
    }
}
