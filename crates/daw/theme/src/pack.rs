//! A pack: a folder of themes, each a list of Frame layers in a mode.
//!
//! A pack is a folder (a git checkout of a `studio-<name>-theme` repo)
//! with its themes under `themes/`. It may say what its themes are in a
//! `pack.styx` at its root:
//!
//! ```text
//! name ableton
//! title "Ableton-style"
//! spec 1
//! themes {
//!     ableton-dark  { title "Dark",  layers (themes/ableton), mode dark }
//!     ableton-light { title "Light", layers (themes/ableton), mode light }
//! }
//! ```
//!
//! Each theme's `layers` are files or folders of `.frame` files, base
//! first, each replacing what the ones before it name
//! (`frame::theme::runtime::layer`); `mode` is one of the documents'
//! modes. A pack with no `pack.styx` has a theme per entry of `themes/`:
//! a folder or a `.frame` file, named after it, in its first mode.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use facet::Facet;
use frame::theme::runtime::Theme;

/// The manifest's name.
pub const MANIFEST: &str = "pack.styx";

/// Where a pack keeps its themes.
pub const THEMES: &str = "themes";

/// Why a pack could not be read or a theme compiled.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("{0}: {1}")]
    Manifest(PathBuf, String),
    #[error("{path}: written for spec {wants}, this is spec {VERSION}", VERSION = crate::spec::VERSION)]
    Spec { path: PathBuf, wants: u32 },
    #[error("no theme `{0}` in the pack (it has: {1})")]
    NoTheme(String, String),
    #[error("no theme files in {0}")]
    Empty(String),
    #[error("{0}")]
    Compile(String),
}

#[derive(Facet, Debug)]
struct Manifest {
    name: Option<String>,
    title: Option<String>,
    spec: Option<u32>,
    themes: Option<HashMap<String, Entry>>,
}

#[derive(Facet, Debug)]
struct Entry {
    title: Option<String>,
    layers: Vec<String>,
    mode: Option<String>,
}

/// One theme of a pack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeDef {
    pub name: String,
    pub title: String,
    /// Files or folders, base first.
    pub layers: Vec<PathBuf>,
    pub mode: Option<String>,
}

/// A pack, read from its folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pack {
    pub name: String,
    pub title: String,
    pub root: PathBuf,
    /// By name.
    pub themes: Vec<ThemeDef>,
}

impl Pack {
    /// The pack in `root`.
    ///
    /// # Errors
    /// Its manifest does not parse or asks for a newer spec, or it has no
    /// `themes/` to read.
    pub fn open(root: &Path) -> Result<Self, Error> {
        let folder = root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self::open_as(root, &folder)
    }

    /// The pack in `root`, called after `folder` (`studio-<name>-theme`)
    /// unless its manifest names it: a checkout whose folder is a commit.
    ///
    /// # Errors
    /// As [`Pack::open`].
    pub fn open_as(root: &Path, folder: &str) -> Result<Self, Error> {
        let default_name = folder
            .strip_prefix("studio-")
            .and_then(|n| n.strip_suffix("-theme"))
            .unwrap_or(folder)
            .to_owned();
        let manifest_path = root.join(MANIFEST);
        let manifest = match std::fs::read_to_string(&manifest_path) {
            Ok(src) => Some(
                facet_styx::from_str::<Manifest>(&src)
                    .map_err(|e| Error::Manifest(manifest_path.clone(), e.to_string()))?,
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(Error::Io(manifest_path, e)),
        };
        if let Some(wants) = manifest.as_ref().and_then(|m| m.spec)
            && wants > crate::spec::VERSION
        {
            return Err(Error::Spec {
                path: manifest_path,
                wants,
            });
        }
        let (name, title, listed) = match manifest {
            Some(m) => {
                let name = m.name.unwrap_or(default_name);
                let title = m.title.unwrap_or_else(|| name.clone());
                (name, title, m.themes)
            }
            None => (default_name.clone(), default_name, None),
        };
        let mut themes = match listed {
            Some(listed) => listed
                .into_iter()
                .map(|(theme, e)| ThemeDef {
                    title: e.title.unwrap_or_else(|| theme.clone()),
                    name: theme,
                    layers: e.layers.iter().map(|l| root.join(l)).collect(),
                    mode: e.mode,
                })
                .collect(),
            None => discover(root)?,
        };
        themes.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Self {
            name,
            title,
            root: root.to_owned(),
            themes,
        })
    }

    /// The theme named `name`, or the pack's only one when `name` is
    /// `None`.
    ///
    /// # Errors
    /// The pack has no such theme, or several and none was named.
    pub fn theme(&self, name: Option<&str>) -> Result<&ThemeDef, Error> {
        let found = match name {
            Some(n) => self.themes.iter().find(|t| t.name == n),
            None if self.themes.len() == 1 => self.themes.first(),
            None => None,
        };
        found.ok_or_else(|| {
            Error::NoTheme(
                name.unwrap_or("").to_owned(),
                self.themes
                    .iter()
                    .map(|t| t.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            )
        })
    }
}

/// A theme per folder or `.frame` file in `root/themes`.
fn discover(root: &Path) -> Result<Vec<ThemeDef>, Error> {
    let dir = root.join(THEMES);
    let entries = std::fs::read_dir(&dir).map_err(|e| Error::Io(dir.clone(), e))?;
    Ok(entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir() || is_theme_file(p))
        .map(|p| {
            let name = p
                .file_stem()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            ThemeDef {
                title: name.clone(),
                name,
                layers: vec![p],
                mode: None,
            }
        })
        .collect())
}

/// Whether `path` is a Frame document: `.frame`, or the older `.fm`.
#[must_use]
pub fn is_theme_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e == "frame" || e == "fm")
}

/// The files `layers` come to, in layering order: a folder's by name.
#[must_use]
pub fn files(layers: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for path in layers {
        if path.is_dir() {
            let mut inside: Vec<PathBuf> = std::fs::read_dir(path)
                .into_iter()
                .flatten()
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| is_theme_file(p))
                .collect();
            inside.sort();
            out.extend(inside);
        } else {
            out.push(path.clone());
        }
    }
    out
}

/// The theme `files` layer into, in `mode`, against the spec's fields.
///
/// # Errors
/// A file cannot be read, or the documents do not compile: every problem,
/// `; `-joined.
pub fn compile(files: &[PathBuf], mode: Option<&str>) -> Result<Theme, Error> {
    if files.is_empty() {
        return Err(Error::Empty(String::new()));
    }
    let mut sources = Vec::with_capacity(files.len());
    for path in files {
        let src = std::fs::read_to_string(path).map_err(|e| Error::Io(path.clone(), e))?;
        sources.push((path.display().to_string(), src));
    }
    let sources: Vec<(&str, &str)> = sources
        .iter()
        .map(|(l, s)| (l.as_str(), s.as_str()))
        .collect();
    Theme::from_sources(&sources, crate::spec::schema(), mode).map_err(|errors| {
        Error::Compile(
            errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; "),
        )
    })
}

impl ThemeDef {
    /// This theme, compiled.
    ///
    /// # Errors
    /// As [`compile`].
    pub fn compile(&self) -> Result<Theme, Error> {
        let files = files(&self.layers);
        if files.is_empty() {
            return Err(Error::Empty(self.name.clone()));
        }
        compile(&files, self.mode.as_deref())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A scratch folder, removed when dropped.
    pub struct Scratch(pub PathBuf);

    impl Scratch {
        pub fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "daw-theme-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("scratch");
            Self(dir)
        }

        pub fn write(&self, rel: &str, body: &str) {
            let p = self.0.join(rel);
            std::fs::create_dir_all(p.parent().expect("parent")).expect("dirs");
            std::fs::write(p, body).expect("write");
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    pub const TOKENS: &str = r##"<Document><Variables modes="dark light"><Var name="text" dark="#ffffff" light="#000000" /></Variables></Document>"##;

    #[test]
    fn a_pack_without_a_manifest_has_a_theme_per_entry() {
        let s = Scratch::new("discover");
        s.write("themes/console/tokens.frame", TOKENS);
        s.write("themes/flat.frame", TOKENS);
        let pack = Pack::open(&s.0).expect("opens");
        let names: Vec<_> = pack.themes.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["console", "flat"]);
        assert!(
            pack.theme(Some("console"))
                .expect("theme")
                .compile()
                .is_ok()
        );
        assert!(pack.theme(None).is_err(), "two themes and none named");
    }

    #[test]
    fn a_manifest_names_themes_layers_and_modes() {
        let s = Scratch::new("manifest");
        s.write("themes/base/tokens.frame", TOKENS);
        s.write(
            MANIFEST,
            "name demo\ntitle \"Demo\"\nspec 1\nthemes {\n    demo-light { title Light, layers (themes/base), mode light }\n}\n",
        );
        let pack = Pack::open(&s.0).expect("opens");
        assert_eq!((pack.name.as_str(), pack.title.as_str()), ("demo", "Demo"));
        let light = pack.theme(None).expect("one theme");
        assert_eq!(light.mode.as_deref(), Some("light"));
        let theme = light.compile().expect("compiles");
        assert_eq!(theme.mode(), "light");
    }

    #[test]
    fn a_pack_for_a_newer_spec_is_refused() {
        let s = Scratch::new("newer");
        s.write("themes/a.frame", TOKENS);
        s.write(MANIFEST, "spec 99\n");
        assert!(matches!(
            Pack::open(&s.0),
            Err(Error::Spec { wants: 99, .. })
        ));
    }
}
