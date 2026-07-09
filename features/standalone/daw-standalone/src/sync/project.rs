//! `impl Projects for Standalone` — stub. Per-project handle pattern
//! retired with the architect::rpc port; all methods take
//! `ProjectContext` now. Full impl pending — currently todo!() to keep
//! the workspace compiling.

use daw_proto::{DawResult, ProjectContext, ProjectInfo, Projects, UndoScope};

use super::daw::Standalone;

/// Thin per-project handle for `Daw::current_project()` / `project()`.
/// Post-port `Projects` methods all take `ProjectContext` so this
/// wrapper just forwards to `Standalone`.
pub struct StandaloneProject<'a> {
    daw: &'a Standalone,
    #[allow(dead_code)]
    guid: String,
}

impl<'a> StandaloneProject<'a> {
    pub(crate) fn new(daw: &'a Standalone, guid: String) -> Self {
        Self { daw, guid }
    }
}

impl<'a> Projects for StandaloneProject<'a> {
    fn info(&self, project: ProjectContext) -> DawResult<ProjectInfo> {
        self.daw.info(project)
    }
    fn current(&self) -> Option<ProjectInfo> {
        self.daw.current()
    }
    fn get(&self, id: &str) -> Option<ProjectInfo> {
        self.daw.get(id)
    }
    fn list(&self) -> Vec<ProjectInfo> {
        self.daw.list()
    }
    fn get_by_slot(&self, slot: u32) -> Option<ProjectInfo> {
        self.daw.get_by_slot(slot)
    }
    fn select(&self, id: &str) -> bool {
        self.daw.select(id)
    }
    fn open(&self, path: &str) -> Option<ProjectInfo> {
        self.daw.open(path)
    }
    fn create(&self) -> Option<ProjectInfo> {
        self.daw.create()
    }
    fn close(&self, id: &str) -> bool {
        self.daw.close(id)
    }
    fn begin_undo_block(&self, project: ProjectContext, label: &str) {
        self.daw.begin_undo_block(project, label)
    }
    fn end_undo_block(&self, project: ProjectContext, label: &str, scope: Option<UndoScope>) {
        self.daw.end_undo_block(project, label, scope)
    }
    fn undo(&self, project: ProjectContext) -> bool {
        self.daw.undo(project)
    }
    fn redo(&self, project: ProjectContext) -> bool {
        self.daw.redo(project)
    }
    fn last_undo_label(&self, project: ProjectContext) -> Option<String> {
        self.daw.last_undo_label(project)
    }
    fn last_redo_label(&self, project: ProjectContext) -> Option<String> {
        self.daw.last_redo_label(project)
    }
    fn run_command(&self, project: ProjectContext, command: &str) -> bool {
        self.daw.run_command(project, command)
    }
    fn save(&self, project: ProjectContext) {
        self.daw.save(project)
    }
    fn save_all(&self) {
        self.daw.save_all()
    }
    fn get_project_info_string(&self, project: ProjectContext, key: &str) -> String {
        self.daw.get_project_info_string(project, key)
    }
    fn set_project_info_string(&self, project: ProjectContext, key: &str, value: &str) {
        self.daw.set_project_info_string(project, key, value)
    }
    fn get_project_info(&self, project: ProjectContext, key: &str) -> f64 {
        self.daw.get_project_info(project, key)
    }
    fn set_project_info(&self, project: ProjectContext, key: &str, value: f64) {
        self.daw.set_project_info(project, key, value)
    }
    fn get_project_config(&self, project: ProjectContext, key: &str) -> Option<f64> {
        self.daw.get_project_config(project, key)
    }
    fn set_project_config(&self, project: ProjectContext, key: &str, value: f64) -> bool {
        self.daw.set_project_config(project, key, value)
    }
    fn set_ruler_lane_name(&self, project: ProjectContext, lane_index: u32, name: &str) {
        self.daw.set_ruler_lane_name(project, lane_index, name)
    }
    fn get_ruler_lane_name(&self, project: ProjectContext, lane_index: u32) -> String {
        self.daw.get_ruler_lane_name(project, lane_index)
    }
    fn ruler_lane_count(&self, project: ProjectContext) -> u32 {
        self.daw.ruler_lane_count(project)
    }
}

/// Resolve a `ProjectContext::Current` to the live current-project
/// guid, or pass through an explicit `Project(guid)`. `None` if no
/// project is current and the context was `Current`.
fn resolve_ctx_guid(daw: &Standalone, ctx: &ProjectContext) -> Option<String> {
    match ctx {
        ProjectContext::Project(g) => Some(g.clone()),
        ProjectContext::Current => daw.state.lock().ok()?.current_project_guid.clone(),
    }
}

impl Projects for Standalone {
    fn info(&self, project: ProjectContext) -> DawResult<ProjectInfo> {
        let guid = resolve_ctx_guid(self, &project)
            .ok_or_else(|| daw_proto::DawError::not_found("project", "current"))?;
        let state = self
            .state
            .lock()
            .map_err(|_| daw_proto::DawError::internal("state poisoned"))?;
        state
            .projects
            .get(&guid)
            .map(|p| p.info.clone())
            .ok_or_else(|| daw_proto::DawError::not_found("project", &guid))
    }

    fn current(&self) -> Option<ProjectInfo> {
        let s = self.state.lock().ok()?;
        let guid = s.current_project_guid.as_ref()?;
        s.projects.get(guid).map(|p| p.info.clone())
    }

    fn get(&self, project_id: &str) -> Option<ProjectInfo> {
        let s = self.state.lock().ok()?;
        s.projects.get(project_id).map(|p| p.info.clone())
    }

    fn list(&self) -> Vec<ProjectInfo> {
        let Ok(s) = self.state.lock() else {
            return Vec::new();
        };
        let mut out: Vec<ProjectInfo> = s.projects.values().map(|p| p.info.clone()).collect();
        // Deterministic ordering by name so tests can rely on it.
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    fn get_by_slot(&self, slot: u32) -> Option<ProjectInfo> {
        self.list().into_iter().nth(slot as usize)
    }

    fn select(&self, project_id: &str) -> bool {
        let Ok(mut s) = self.state.lock() else {
            return false;
        };
        if s.projects.contains_key(project_id) {
            s.current_project_guid = Some(project_id.to_string());
            true
        } else {
            false
        }
    }

    /// Load a `.rpp` file from disk into a new project tab. Native-only
    /// — wasm callers should use `crate::project_loader::load_rpp_text`
    /// directly with a `BayFileResolver`.
    fn open(&self, path: &str) -> Option<ProjectInfo> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let text = std::fs::read_to_string(path).ok()?;
            let name = std::path::Path::new(path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("project")
                .to_string();
            #[cfg(any(feature = "rpp-project", feature = "rpp-project-wasm"))]
            {
                let summary =
                    crate::project_loader::load_rpp_text(self, &name, path, &text).ok()?;
                let s = self.state.lock().ok()?;
                s.projects
                    .get(&summary.project_guid)
                    .map(|p| p.info.clone())
            }
            #[cfg(not(any(feature = "rpp-project", feature = "rpp-project-wasm")))]
            {
                let _ = text;
                let _ = name;
                None
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            None
        }
    }

    /// Create a new empty project tab + make it current. Synthesises
    /// a fresh GUID + a default name.
    fn create(&self) -> Option<ProjectInfo> {
        let guid = uuid::Uuid::new_v4().to_string();
        let info = ProjectInfo {
            guid: guid.clone(),
            name: format!("Untitled-{}", &guid[..8]),
            path: String::new(),
        };
        self.seed_project(info.clone());
        Some(info)
    }

    fn close(&self, project_id: &str) -> bool {
        let Ok(mut s) = self.state.lock() else {
            return false;
        };
        if s.projects.remove(project_id).is_some() {
            if s.current_project_guid.as_deref() == Some(project_id) {
                // Fall back to any remaining project, else None.
                s.current_project_guid = s.projects.keys().next().cloned();
            }
            true
        } else {
            false
        }
    }

    fn begin_undo_block(&self, _project: ProjectContext, _label: &str) {
        // Undo is a planned subsystem — for now we accept calls so
        // tests don't have to skip the begin/end ceremony. The
        // `last_*_label` accessors return None so any client that
        // checks gets a consistent "no undo history" signal.
    }
    fn end_undo_block(&self, _project: ProjectContext, _label: &str, _scope: Option<UndoScope>) {}
    fn undo(&self, _project: ProjectContext) -> bool {
        false
    }
    fn redo(&self, _project: ProjectContext) -> bool {
        false
    }
    fn last_undo_label(&self, _project: ProjectContext) -> Option<String> {
        None
    }
    fn last_redo_label(&self, _project: ProjectContext) -> Option<String> {
        None
    }
    fn run_command(&self, _project: ProjectContext, _command: &str) -> bool {
        // No REAPER action registry on standalone; commands no-op
        // and return false (not found). Custom actions can be
        // routed through the `action_registry` service if needed.
        false
    }

    fn save(&self, project: ProjectContext) {
        // Standalone has no persistent backing — the project lives
        // in memory only. `save` accepts the call to keep the trait
        // surface usable. Writing back to an .rpp file would be a
        // dawfile-reaper serialize round-trip + std::fs::write; not
        // wired in yet because the standalone test surface focuses
        // on read-side parity.
        let _ = project;
    }
    fn save_all(&self) {}

    fn get_project_info_string(&self, project: ProjectContext, key: &str) -> String {
        let Some(guid) = resolve_ctx_guid(self, &project) else {
            return String::new();
        };
        let Ok(s) = self.state.lock() else {
            return String::new();
        };
        let Some(p) = s.projects.get(&guid) else {
            return String::new();
        };
        // Map a handful of canonical REAPER `GetSetProjectInfo_String`
        // keys to fields we model. Unknown keys return empty.
        match key {
            "PROJECT_NAME" => p.info.name.clone(),
            "PROJECT_PATH" => p.info.path.clone(),
            "PROJECT_NOTES" => p
                .project_ext_state
                .get(&(String::new(), "PROJECT_NOTES".into()))
                .cloned()
                .unwrap_or_default(),
            _ => p
                .project_ext_state
                .get(&(String::new(), key.to_string()))
                .cloned()
                .unwrap_or_default(),
        }
    }

    fn set_project_info_string(&self, project: ProjectContext, key: &str, value: &str) {
        let Some(guid) = resolve_ctx_guid(self, &project) else {
            return;
        };
        let _ = self.with_project_mut(&guid, |p| match key {
            "PROJECT_NAME" => p.info.name = value.to_string(),
            "PROJECT_PATH" => p.info.path = value.to_string(),
            _ => {
                p.project_ext_state
                    .insert((String::new(), key.to_string()), value.to_string());
            }
        });
    }

    fn get_project_info(&self, project: ProjectContext, key: &str) -> f64 {
        let Some(guid) = resolve_ctx_guid(self, &project) else {
            return 0.0;
        };
        let Ok(s) = self.state.lock() else { return 0.0 };
        let Some(p) = s.projects.get(&guid) else {
            return 0.0;
        };
        // Surface the most-used REAPER numeric keys. Unknown keys = 0.
        match key {
            "PROJECT_TIMESIG_NUM" => p.transport.time_signature.numerator as f64,
            "PROJECT_TIMESIG_DENOM" => p.transport.time_signature.denominator as f64,
            "PROJECT_BPM" => p.transport.tempo.bpm(),
            _ => 0.0,
        }
    }

    fn set_project_info(&self, project: ProjectContext, key: &str, value: f64) {
        let Some(guid) = resolve_ctx_guid(self, &project) else {
            return;
        };
        let _ = self.with_project_mut(&guid, |p| match key {
            "PROJECT_BPM" => {
                p.transport.tempo = daw_proto::primitives::Tempo::from_bpm(value.max(1.0));
            }
            "PROJECT_TIMESIG_NUM" => {
                let denom = p.transport.time_signature.denominator;
                p.transport.time_signature =
                    daw_proto::primitives::TimeSignature::new(value as u32, denom);
            }
            "PROJECT_TIMESIG_DENOM" => {
                let num = p.transport.time_signature.numerator;
                p.transport.time_signature =
                    daw_proto::primitives::TimeSignature::new(num, value as u32);
            }
            _ => {}
        });
    }

    fn get_project_config(&self, project: ProjectContext, key: &str) -> Option<f64> {
        let guid = resolve_ctx_guid(self, &project)?;
        let s = self.state.lock().ok()?;
        let value = s
            .projects
            .get(&guid)?
            .project_ext_state
            .get(&("daw-standalone:project_config".into(), key.to_string()))?
            .parse()
            .ok()?;
        Some(value)
    }

    fn set_project_config(&self, project: ProjectContext, key: &str, value: f64) -> bool {
        let Some(guid) = resolve_ctx_guid(self, &project) else {
            return false;
        };
        self.with_project_mut(&guid, |p| {
            p.project_ext_state.insert(
                ("daw-standalone:project_config".into(), key.to_string()),
                value.to_string(),
            );
        })
        .is_ok()
    }

    fn set_ruler_lane_name(&self, project: ProjectContext, lane_index: u32, name: &str) {
        // Store ruler-lane names in project_ext_state under a
        // synthetic section so they round-trip through get/set
        // without needing a dedicated field on ProjectState.
        let Some(guid) = resolve_ctx_guid(self, &project) else {
            return;
        };
        let _ = self.with_project_mut(&guid, |p| {
            p.project_ext_state.insert(
                ("daw-standalone:ruler_lanes".into(), format!("{lane_index}")),
                name.to_string(),
            );
        });
    }

    fn get_ruler_lane_name(&self, project: ProjectContext, lane_index: u32) -> String {
        let Some(guid) = resolve_ctx_guid(self, &project) else {
            return String::new();
        };
        let Ok(s) = self.state.lock() else {
            return String::new();
        };
        s.projects
            .get(&guid)
            .and_then(|p| {
                p.project_ext_state
                    .get(&("daw-standalone:ruler_lanes".into(), format!("{lane_index}")))
                    .cloned()
            })
            .unwrap_or_default()
    }

    fn ruler_lane_count(&self, project: ProjectContext) -> u32 {
        let Some(guid) = resolve_ctx_guid(self, &project) else {
            return 0;
        };
        let Ok(s) = self.state.lock() else { return 0 };
        s.projects
            .get(&guid)
            .map(|p| {
                p.project_ext_state
                    .keys()
                    .filter(|(section, _)| section == "daw-standalone:ruler_lanes")
                    .count() as u32
            })
            .unwrap_or(0)
    }
}
