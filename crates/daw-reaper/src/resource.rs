//! REAPER Resource Path Implementation

use daw_proto::resource::ResourceService;
use reaper_high::Reaper;
use std::path::PathBuf;

use crate::main_thread;
use crate::safe_wrappers::cstring;

/// REAPER resource path service implementation
#[derive(Clone)]
pub struct ReaperResource;

impl ReaperResource {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ReaperResource {
    fn default() -> Self {
        Self::new()
    }
}

impl ResourceService for ReaperResource {
    async fn get_resource_path(&self) -> PathBuf {
        main_thread::query(|| {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();
            let utf8_path = medium.get_resource_path(|p| p.to_path_buf());
            Some(utf8_path.into_std_path_buf())
        })
        .await
        .flatten()
        .unwrap_or_else(|| PathBuf::from("."))
    }

    async fn get_ini_file_path(&self) -> PathBuf {
        main_thread::query(|| {
            let reaper = Reaper::get();
            let medium = reaper.medium_reaper();
            let utf8_path = medium.get_ini_file(|p| p.to_path_buf());
            Some(utf8_path.into_std_path_buf())
        })
        .await
        .flatten()
        .unwrap_or_else(|| PathBuf::from("reaper.ini"))
    }

    async fn get_color_theme_path(&self) -> Option<PathBuf> {
        main_thread::query(|| {
            let reaper = Reaper::get();
            let low = reaper.medium_reaper().low();

            // GetLastColorThemeFile returns a C string pointer
            let ptr = low.GetLastColorThemeFile();
            cstring::read_cstr(ptr).map(PathBuf::from)
        })
        .await
        .flatten()
    }
}
