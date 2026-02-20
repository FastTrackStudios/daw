//! REAPER UI Service Implementation

use daw_proto::{UiService, UserInputResult};
use reaper_high::Reaper;
use roam::Context;
use std::path::PathBuf;

use crate::main_thread;

/// REAPER UI service implementation
#[derive(Clone)]
pub struct ReaperUi;

impl ReaperUi {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReaperUi {
    fn default() -> Self {
        Self::new()
    }
}

impl UiService for ReaperUi {
    async fn get_user_inputs(
        &self,
        _cx: &Context,
        title: String,
        prompts: Vec<String>,
        defaults: Vec<String>,
    ) -> Option<UserInputResult> {
        main_thread::query(move || {
            let medium = Reaper::get().medium_reaper();

            // REAPER's GetUserInputs takes up to 16 fields
            if prompts.len() > 16 || prompts.len() != defaults.len() {
                return None;
            }

            let num_inputs = prompts.len() as u32;

            // Build the caption string (CSV format: "field1,field2,field3")
            let captions = prompts.join(",");

            // Build initial values string (also CSV)
            let initial_csv = defaults.join(",");

            // Call GetUserInputs
            let result = medium.get_user_inputs(
                title,
                num_inputs,
                captions,
                initial_csv,
                512, // max buffer size per field
            );

            match result {
                Some(values_str) => {
                    // Parse the returned CSV
                    let values: Vec<String> = values_str
                        .to_str()
                        .split(',')
                        .map(|s| s.to_string())
                        .collect();

                    Some(UserInputResult { ok: true, values })
                }
                None => {
                    // User cancelled
                    Some(UserInputResult {
                        ok: false,
                        values: vec![],
                    })
                }
            }
        })
        .await
        .flatten()
    }

    async fn browse_for_file(
        &self,
        _cx: &Context,
        _title: String,
        _initial_dir: Option<PathBuf>,
        _filter: Option<String>,
    ) -> Option<PathBuf> {
        // TODO: Implement using REAPER's file browser API
        // The medium-level API doesn't expose these functions yet
        None
    }

    async fn browse_for_save_file(
        &self,
        _cx: &Context,
        _title: String,
        _initial_dir: Option<PathBuf>,
        _default_name: String,
        _filter: Option<String>,
    ) -> Option<PathBuf> {
        // TODO: Implement using REAPER's file browser API
        // The medium-level API doesn't expose these functions yet
        None
    }

    async fn browse_for_directory(
        &self,
        _cx: &Context,
        _title: String,
        _initial_dir: Option<PathBuf>,
    ) -> Option<PathBuf> {
        // TODO: Implement using REAPER's directory browser API
        // The medium-level API doesn't expose these functions yet
        None
    }

    async fn set_prevent_ui_refresh(&self, _cx: &Context, prevent: bool) {
        main_thread::run(move || {
            let low = Reaper::get().medium_reaper().low();
            low.PreventUIRefresh(if prevent { 1 } else { 0 });
        });
    }
}
