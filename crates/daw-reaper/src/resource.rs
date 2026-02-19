//! REAPER Resource Path Implementation

use daw_proto::resource::ResourceService;
use reaper_high::Reaper;
use roam::Context;
use std::path::PathBuf;

use crate::main_thread;

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
    async fn get_resource_path(&self, _cx: &Context) -> PathBuf {
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

    async fn get_ini_file_path(&self, _cx: &Context) -> PathBuf {
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

    async fn get_color_theme_path(&self, _cx: &Context) -> Option<PathBuf> {
        main_thread::query(|| {
            let reaper = Reaper::get();
            let low = reaper.medium_reaper().low();

            // GetLastColorThemeFile returns a C string pointer
            let ptr = unsafe { low.GetLastColorThemeFile() };

            if ptr.is_null() {
                return None;
            }

            // Convert C string to Rust string
            let c_str = unsafe { std::ffi::CStr::from_ptr(ptr) };
            let path_str = c_str.to_str().ok()?;

            if path_str.is_empty() {
                None
            } else {
                Some(PathBuf::from(path_str))
            }
        })
        .await
        .flatten()
    }
}
