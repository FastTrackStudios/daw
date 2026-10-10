//! A plugin parameter's automation plays: at each block the engine hands the
//! plugin the value its envelope has there (an FX-param envelope, Read).

#![cfg(feature = "decode")]

use std::sync::Arc;
use std::sync::Mutex;

use daw_proto::automation::{AddPointParams, EnvelopeLocation, EnvelopeRef, EnvelopeShape};
use daw_proto::fx::{Effects, FxChainContext, InstalledFx};
use daw_proto::primitives::PositionInSeconds;
use daw_proto::project::ProjectContext;
use daw_proto::{Automation, ProjectInfo, TrackRef, Tracks};
use daw_standalone::audio_engine::render::ProjectRenderer;
use daw_standalone::plugin::{
    FxFactory, PluginDescriptor, PluginError, PluginEvents, PluginFormat, PluginInstance,
    PluginParamInfo,
};
use daw_standalone::sync::Standalone;

/// A plugin that keeps, each block, the value it is given for parameter 0.
struct Counting(Arc<Mutex<Vec<f64>>>);

impl PluginInstance for Counting {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor {
            id: "counting".into(),
            name: "Counting".into(),
            vendor: "test".into(),
            version: "0".into(),
            format: PluginFormat::Synthetic,
        }
    }
    fn params(&mut self) -> Vec<PluginParamInfo> {
        Vec::new()
    }
    fn param_value(&mut self, _id: u32) -> Option<f64> {
        None
    }
    fn value_to_text(&mut self, _id: u32, _value: f64) -> Option<String> {
        None
    }
    fn text_to_value(&mut self, _id: u32, _text: &str) -> Option<f64> {
        None
    }
    fn latency(&mut self) -> u32 {
        0
    }
    fn prepare(&mut self, _sample_rate: f64, _block_size: u32) -> Result<(), PluginError> {
        Ok(())
    }
    fn is_prepared(&self) -> bool {
        true
    }
    fn process_block(
        &mut self,
        in_l: &[f32],
        in_r: &[f32],
        out_l: &mut [f32],
        out_r: &mut [f32],
        events: &PluginEvents<'_>,
    ) -> Result<(), PluginError> {
        if let Some((_, value)) = events.params.iter().find(|(id, _)| *id == 0) {
            self.0.lock().expect("the log").push(*value);
        }
        out_l.copy_from_slice(in_l);
        out_r.copy_from_slice(in_r);
        Ok(())
    }
    fn deactivate(&mut self) {}
}

struct Factory(Arc<Mutex<Vec<f64>>>);

impl FxFactory for Factory {
    fn installed(&self) -> Vec<InstalledFx> {
        Vec::new()
    }
    fn provides(&self, name: &str) -> bool {
        name == "Counting"
    }
    fn create(&self, name: &str, _sample_rate: f64) -> Option<Box<dyn PluginInstance>> {
        self.provides(name)
            .then(|| Box::new(Counting(self.0.clone())) as Box<dyn PluginInstance>)
    }
}

#[test]
fn a_parameters_automation_plays() {
    let runtime = tokio::runtime::Runtime::new().expect("a runtime");
    let _inside = runtime.enter();
    let told = Arc::new(Mutex::new(Vec::new()));
    let daw = Standalone::new();
    daw.set_fx_factory(Arc::new(Factory(told.clone())));
    let guid = daw.seed_project(ProjectInfo {
        guid: "p".into(),
        name: "p".into(),
        path: String::new(),
    });
    let ctx = ProjectContext::Project(guid.clone());
    let track = Tracks::add(&daw, ctx.clone(), "Verb", None).expect("a track");
    let fx = Effects::add(
        &daw,
        ctx.clone(),
        FxChainContext::Track(track.clone()),
        "Counting",
    )
    .expect("the fx");
    let at = EnvelopeLocation::new(
        TrackRef::Guid(track),
        EnvelopeRef::FxParam {
            fx_guid: fx,
            param_index: 0,
        },
    );
    for (time, value) in [(0.0, 0.0), (2.0, 1.0)] {
        let _ = Automation::add_point(
            &daw,
            ctx.clone(),
            at.clone(),
            AddPointParams::new(
                PositionInSeconds::from_seconds(time),
                value,
                EnvelopeShape::Linear,
            ),
        );
    }
    let renderer = ProjectRenderer::new(&daw, &guid, 48_000);
    let _ = renderer.render_block(0, 512);
    let _ = renderer.render_block(48_000, 512);
    let told = told.lock().expect("the log");
    assert_eq!(told.len(), 2, "told each block: {told:?}");
    assert!(told[0].abs() < 1e-6, "0 at the start: {told:?}");
    assert!((told[1] - 0.5).abs() < 1e-6, "halfway at 1 s: {told:?}");
}
