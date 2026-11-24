//! The packs a host has installed, and which theme it shows.
//!
//! Kept as one JSON file in the host's config (Studio's is
//! `<config>/fts/studio-themes.json`): each pack's link, the commit it is
//! pinned to and its folder, and the theme chosen for every view.

use std::path::{Path, PathBuf};

use facet::Facet;

use crate::pack::Pack;
use crate::source::Installed;

/// One installed pack.
#[derive(Facet, Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    /// What it was installed from: a link, or a folder.
    pub link: String,
    #[facet(default)]
    pub commit: Option<String>,
    pub path: String,
}

/// The theme every view shows.
#[derive(Facet, Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub pack: String,
    #[facet(default)]
    pub theme: Option<String>,
}

/// The installed packs and the choice.
#[derive(Facet, Clone, Debug, Default, PartialEq, Eq)]
pub struct Library {
    #[facet(default)]
    pub packs: Vec<Entry>,
    #[facet(default)]
    pub chosen: Option<Choice>,
}

/// Why the library could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}: {1}")]
    Io(PathBuf, std::io::Error),
    /// It does not parse (or, writing, does not serialize).
    #[error("{0}: {1}")]
    Parse(PathBuf, String),
}

impl Library {
    /// The library in `file`; an empty one if there is no file yet.
    ///
    /// # Errors
    /// It exists and cannot be read or parsed.
    pub fn load(file: &Path) -> Result<Self, Error> {
        match std::fs::read(file) {
            Ok(bytes) => facet_json::from_slice(&bytes)
                .map_err(|e| Error::Parse(file.to_owned(), e.to_string())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::Io(file.to_owned(), e)),
        }
    }

    /// Write the library to `file`.
    ///
    /// # Errors
    /// It cannot be written.
    pub fn save(&self, file: &Path) -> Result<(), Error> {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| Error::Io(dir.to_owned(), e))?;
        }
        let json = facet_json::to_string(self)
            .map_err(|e| Error::Parse(file.to_owned(), e.to_string()))?;
        std::fs::write(file, json).map_err(|e| Error::Io(file.to_owned(), e))
    }

    /// Keep `installed` (from `link`), replacing a pack of the same name.
    pub fn add(&mut self, link: &str, installed: &Installed) {
        let entry = Entry {
            name: installed.pack.name.clone(),
            link: link.to_owned(),
            commit: installed.commit.clone(),
            path: installed.pack.root.to_string_lossy().into_owned(),
        };
        match self.packs.iter_mut().find(|e| e.name == entry.name) {
            Some(at) => *at = entry,
            None => self.packs.push(entry),
        }
    }

    /// The installed pack named `name`, read from its folder.
    #[must_use]
    pub fn pack(&self, name: &str) -> Option<Pack> {
        let e = self.packs.iter().find(|e| e.name == name)?;
        let root = Path::new(&e.path);
        let folder = root.file_name()?.to_string_lossy().into_owned();
        // A cached checkout's folder is its commit: name it after itself.
        Pack::open_as(root, if e.commit.is_some() { &e.name } else { &folder })
            .ok()
            .map(|mut p| {
                p.name.clone_from(&e.name);
                p
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::tests::{Scratch, TOKENS};
    use crate::source::{Source, install};

    #[test]
    fn a_library_keeps_its_packs_and_choice_across_a_save() {
        let s = Scratch::new("library");
        s.write("studio-flat-theme/themes/flat.frame", TOKENS);
        let link = s.0.join("studio-flat-theme").to_string_lossy().into_owned();
        let installed = install(&Source::parse(&link).expect("link"), &s.0).expect("installs");
        let mut lib = Library::default();
        lib.add(&link, &installed);
        lib.add(&link, &installed);
        lib.chosen = Some(Choice {
            pack: "flat".into(),
            theme: Some("flat".into()),
        });
        let file = s.0.join("cfg/themes.json");
        lib.save(&file).expect("saves");
        let back = Library::load(&file).expect("loads");
        assert_eq!(back, lib);
        assert_eq!(back.packs.len(), 1);
        assert_eq!(back.pack("flat").map(|p| p.themes.len()), Some(1));
        assert_eq!(
            Library::load(&s.0.join("none.json")).expect("empty"),
            Library::default()
        );
    }
}
