//! `impl ResourcePaths for Reaper` — sync trait + REAPER's resource-path C API.
//!
//! Mounting goes through `daw_proto::resource::serve(Reaper)`. The
//! architect::rpc bridge hops calls onto REAPER's main thread via
//! `HasDispatcher`. All bodies assume main-thread execution.

use std::path::PathBuf;

use daw_proto::ResourcePaths;
use reaper_high::Reaper as ReaperHigh;

use crate::safe_wrappers::cstring;

impl ResourcePaths for crate::Reaper {
    fn resource_path(&self) -> PathBuf {
        let reaper = ReaperHigh::get();
        let medium = reaper.medium_reaper();
        let utf8_path = medium.get_resource_path(|p| p.to_path_buf());
        utf8_path.into_std_path_buf()
    }

    fn ini_file_path(&self) -> PathBuf {
        let reaper = ReaperHigh::get();
        let medium = reaper.medium_reaper();
        let utf8_path = medium.get_ini_file(|p| p.to_path_buf());
        utf8_path.into_std_path_buf()
    }

    fn color_theme_path(&self) -> Option<PathBuf> {
        let reaper = ReaperHigh::get();
        let low = reaper.medium_reaper().low();
        // GetLastColorThemeFile returns a C string pointer
        let ptr = low.GetLastColorThemeFile();
        cstring::read_cstr(ptr).map(PathBuf::from)
    }
}
