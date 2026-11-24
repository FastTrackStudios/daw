//! Where a pack comes from, and installing one from a link.
//!
//! A pack is named by a link: `owner/repo`, `owner/repo@rev`, a GitHub URL
//! (`https://github.com/owner/repo`, `…/tree/<rev>`, either with `@rev`),
//! or a folder on this machine. [`install`] fetches a linked pack with
//! `git` into a cache, one checkout per commit, so a pack is pinned: the
//! same link and rev always give the same files, and a new rev is a new
//! folder beside the old one.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::pack::{self, Pack};

/// Where a pack is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// A folder on this machine, used as it is.
    Folder(PathBuf),
    /// A git repository: `url`, at `rev` (its default branch's head when
    /// `None`), cached under `owner/repo`.
    Git {
        url: String,
        owner: String,
        repo: String,
        rev: Option<String>,
    },
}

/// Why a link could not be installed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("`{0}` is not a pack link (owner/repo[@rev], a GitHub URL or a folder)")]
    Link(String),
    #[error("git {0}: {1}")]
    Git(String, String),
    #[error("{0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error(transparent)]
    Pack(#[from] pack::Error),
}

impl Source {
    /// Read a link.
    ///
    /// # Errors
    /// It is none of the forms a link takes.
    pub fn parse(link: &str) -> Result<Self, Error> {
        let link = link.trim();
        let bad = || Error::Link(link.to_owned());
        if link.starts_with(['/', '.', '~']) || Path::new(link).is_dir() {
            return Ok(Self::Folder(expand_home(link)));
        }
        let (body, rev) = match link.rsplit_once('@') {
            Some((b, r)) if !r.is_empty() && !r.contains('/') => (b, Some(r.to_owned())),
            _ => (link, None),
        };
        let path = body
            .strip_prefix("https://github.com/")
            .or_else(|| body.strip_prefix("http://github.com/"))
            .or_else(|| body.strip_prefix("github.com/"))
            .or_else(|| body.strip_prefix("github:"))
            .unwrap_or(body);
        let mut parts = path.trim_end_matches('/').split('/');
        let (Some(owner), Some(repo)) = (parts.next(), parts.next()) else {
            return Err(bad());
        };
        let repo = repo.trim_end_matches(".git");
        // `…/tree/<rev>` names the rev in the path.
        let rev = match (parts.next(), parts.next()) {
            (Some("tree"), Some(r)) if rev.is_none() => Some(r.to_owned()),
            (None, _) => rev,
            _ => return Err(bad()),
        };
        let ok = |s: &str| {
            !s.is_empty()
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        };
        if !ok(owner) || !ok(repo) {
            return Err(bad());
        }
        Ok(Self::Git {
            url: format!("https://github.com/{owner}/{repo}.git"),
            owner: owner.to_owned(),
            repo: repo.to_owned(),
            rev,
        })
    }
}

/// `~/x` as a path under the home folder.
fn expand_home(link: &str) -> PathBuf {
    match (link.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(link),
    }
}

/// A pack installed: where it came from, the commit it is at, its folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    pub source: Source,
    /// The commit checked out; `None` for a folder.
    pub commit: Option<String>,
    pub pack: Pack,
}

/// Make `source` a pack on this machine: a folder as it is; a repository
/// fetched into `cache/<owner>/<repo>/<commit>` unless that commit is
/// there already.
///
/// # Errors
/// `git` failed (no network, no such repo or rev), or what it fetched is
/// not a pack.
pub fn install(source: &Source, cache: &Path) -> Result<Installed, Error> {
    let Source::Git {
        url,
        owner,
        repo,
        rev,
    } = source
    else {
        let Source::Folder(dir) = source else {
            return Err(Error::Link(String::new()));
        };
        return Ok(Installed {
            source: source.clone(),
            commit: None,
            pack: Pack::open(dir)?,
        });
    };
    let home = cache.join(owner).join(repo);
    // A full commit already fetched needs no network.
    if let Some(r) = rev.as_deref().filter(|r| is_commit(r))
        && home.join(r).is_dir()
    {
        return Ok(Installed {
            source: source.clone(),
            commit: Some(r.to_owned()),
            pack: Pack::open_as(&home.join(r), repo)?,
        });
    }
    std::fs::create_dir_all(&home).map_err(|e| Error::Io(home.clone(), e))?;
    let staging = home.join(format!(".fetch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    git(
        &home,
        &[
            "clone",
            "--quiet",
            "--no-checkout",
            url,
            &staging.to_string_lossy(),
        ],
    )?;
    let fetched = checkout(&staging, rev.as_deref());
    let commit = match fetched {
        Ok(c) => c,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
    };
    let at = home.join(&commit);
    if at.is_dir() {
        let _ = std::fs::remove_dir_all(&staging);
    } else {
        std::fs::rename(&staging, &at).map_err(|e| Error::Io(at.clone(), e))?;
    }
    Ok(Installed {
        source: source.clone(),
        commit: Some(commit),
        pack: Pack::open_as(&at, repo)?,
    })
}

/// Check `rev` out in `dir` (the default branch's head when `None`); the
/// commit it is.
fn checkout(dir: &Path, rev: Option<&str>) -> Result<String, Error> {
    git(
        dir,
        &["checkout", "--quiet", "--detach", rev.unwrap_or("HEAD")],
    )?;
    git(dir, &["rev-parse", "HEAD"]).map(|out| out.trim().to_owned())
}

/// Whether `rev` is a whole commit id.
fn is_commit(rev: &str) -> bool {
    rev.len() == 40 && rev.chars().all(|c| c.is_ascii_hexdigit())
}

/// `git args` in `dir`; its output.
fn git(dir: &Path, args: &[&str]) -> Result<String, Error> {
    let what = args.first().copied().unwrap_or("").to_owned();
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        // Never wait on a password prompt nobody can see.
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| Error::Git(what.clone(), e.to_string()))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(Error::Git(
            what,
            String::from_utf8_lossy(&out.stderr).trim().to_owned(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::tests::{Scratch, TOKENS};

    fn gh(owner: &str, repo: &str, rev: Option<&str>) -> Source {
        Source::Git {
            url: format!("https://github.com/{owner}/{repo}.git"),
            owner: owner.into(),
            repo: repo.into(),
            rev: rev.map(Into::into),
        }
    }

    #[test]
    fn links_read_in_every_form() {
        let want = gh("FastTrackStudios", "studio-fts-theme", None);
        for link in [
            "FastTrackStudios/studio-fts-theme",
            "https://github.com/FastTrackStudios/studio-fts-theme",
            "https://github.com/FastTrackStudios/studio-fts-theme.git",
            "github.com/FastTrackStudios/studio-fts-theme/",
            "github:FastTrackStudios/studio-fts-theme",
        ] {
            assert_eq!(Source::parse(link).expect(link), want, "{link}");
        }
        let pinned = gh("a", "studio-x-theme", Some("v1.2"));
        assert_eq!(
            Source::parse("a/studio-x-theme@v1.2").expect("pinned"),
            pinned
        );
        assert_eq!(
            Source::parse("https://github.com/a/studio-x-theme/tree/v1.2").expect("tree"),
            pinned
        );
        assert!(Source::parse("just-a-word").is_err());
        assert!(Source::parse("a/b/c").is_err());
        assert!(Source::parse("a b/c").is_err());
    }

    /// A git repo holding a pack, as a link would fetch it.
    fn repo(s: &Scratch) -> PathBuf {
        let r = s.0.join("origin");
        std::fs::create_dir_all(r.join("themes/flat")).expect("dirs");
        std::fs::write(r.join("themes/flat/tokens.frame"), TOKENS).expect("write");
        for args in [
            &["init", "--quiet", "-b", "main"][..],
            &["add", "."],
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "--quiet",
                "-m",
                "one",
            ],
        ] {
            git(&r, args).expect("git");
        }
        r
    }

    #[test]
    fn a_repo_installs_pinned_to_its_commit() {
        let s = Scratch::new("install");
        let origin = repo(&s);
        let source = Source::Git {
            url: origin.to_string_lossy().into_owned(),
            owner: "o".into(),
            repo: "studio-flat-theme".into(),
            rev: None,
        };
        let cache = s.0.join("cache");
        let first = install(&source, &cache).expect("installs");
        let commit = first.commit.clone().expect("a commit");
        assert!(is_commit(&commit));
        assert_eq!(
            first.pack.root,
            cache.join("o/studio-flat-theme").join(&commit)
        );
        assert_eq!(
            first.pack.themes.first().map(|t| t.name.as_str()),
            Some("flat")
        );
        assert_eq!(first.pack.name, "flat");

        // A second commit upstream: the pinned rev still gives the first.
        std::fs::write(
            origin.join("themes/flat/more.frame"),
            TOKENS.replace("text", "accent"),
        )
        .expect("write");
        git(&origin, &["add", "."]).expect("add");
        git(
            &origin,
            &[
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "--quiet",
                "-m",
                "two",
            ],
        )
        .expect("commit");
        let pinned = Source::Git {
            url: origin.to_string_lossy().into_owned(),
            owner: "o".into(),
            repo: "studio-flat-theme".into(),
            rev: Some(commit.clone()),
        };
        let again = install(&pinned, &cache).expect("pinned");
        assert_eq!(again.commit.as_deref(), Some(commit.as_str()));
        assert!(!again.pack.root.join("themes/flat/more.frame").exists());
        let latest = install(&source, &cache).expect("latest");
        assert_ne!(latest.commit, again.commit);
        assert!(latest.pack.root.join("themes/flat/more.frame").exists());
    }

    #[test]
    fn a_folder_is_used_as_it_is() {
        let s = Scratch::new("folder");
        s.write("themes/flat.frame", TOKENS);
        let link = s.0.to_string_lossy().into_owned();
        let got =
            install(&Source::parse(&link).expect("folder"), &s.0.join("cache")).expect("installs");
        assert_eq!(got.commit, None);
        assert_eq!(got.pack.root, s.0);
    }
}
