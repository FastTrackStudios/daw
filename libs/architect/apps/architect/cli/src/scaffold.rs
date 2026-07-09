//! `architect feature new <name>` — scaffold a feature crate family.
//!
//! Writes the canonical local-first layout:
//!
//!     features/<name>/
//!       <name>-proto/        wire contract (architect::Entity + placeholder)
//!       <name>-crdt/         Loro-backed source of truth (marker +
//!                              EntityCrdt impl + <Name>RepoLoro newtype)
//!       <name>-db/           SeaORM persistence + optional projections
//!                              (the snapshot/update tables live in
//!                              features/crdt/crdt-seaorm; this crate re-exports
//!                              the Migrator + holds any feature-specific
//!                              projection tables)
//!       <name>/              facade (vox + server + server-axum)
//!       spec/<name>.md       tracey rule stub
//!       tests/native/        cargo test against the CRDT backend
//!                              (ephemeral CrdtDoc, no DB needed)
//!
//! Then updates:
//!
//!   * Cargo.toml — members, default-members, workspace.dependencies
//!   * .config/tracey/config.styx — append a new spec block

use std::fs;
use std::path::{Path, PathBuf};

use heck::{ToPascalCase, ToSnakeCase};
use owo_colors::OwoColorize;
use toml_edit::{Array, DocumentMut, Item, Table, value};

pub fn feature_new(repo_root: &Path, name: &str, force: bool) -> eyre::Result<()> {
    validate_name(name)?;
    let names = Names::from_kebab(name);

    eprintln!(
        "{} scaffolding feature {} at features/{name}/",
        "❯❯".cyan().bold(),
        names.kebab.bold()
    );

    let feature_dir = repo_root.join("features").join(&names.kebab);
    if feature_dir.exists() && !force {
        let occupied = feature_dir
            .read_dir()
            .map(|d| d.count() > 0)
            .unwrap_or(false);
        if occupied {
            eyre::bail!(
                "features/{} exists and is non-empty; re-run with --force to overwrite",
                names.kebab
            );
        }
    }

    write_proto(&feature_dir, &names)?;
    write_crdt(&feature_dir, &names)?;
    write_db(&feature_dir, &names)?;
    write_facade(&feature_dir, &names)?;
    write_spec(&feature_dir, &names)?;
    write_tests_native(&feature_dir, &names)?;

    update_workspace_cargo(repo_root, &names)?;
    update_tracey_config(repo_root, &names)?;

    eprintln!(
        "{} feature {} scaffolded.

Next steps:
  1. Rename the placeholder `{pascal}` entity in features/{kebab}/{kebab}-proto/src/lib.rs
     to fit your domain (e.g. `Channel`, `Track`, `Clip`). Update the
     EntityCrdt impl in features/{kebab}/{kebab}-crdt/src/lib.rs to match.
  2. If this feature needs SQL-shaped queries, add projection tables
     under features/{kebab}/{kebab}-db/src/migrations.rs.
  3. Write rules at features/{kebab}/spec/{kebab}.md and verify with
     `cargo xtask tracey-validate`.
  4. Add a binary that depends on `{kebab}` with the features you need.
",
        "✓".green().bold(),
        names.kebab.bold(),
        pascal = names.pascal,
        kebab = names.kebab,
    );

    Ok(())
}

// ── Name conversions ──────────────────────────────────────────────────

struct Names {
    kebab: String,  // mixing
    snake: String,  // mixing
    pascal: String, // Mixing
}

impl Names {
    fn from_kebab(s: &str) -> Self {
        Self {
            kebab: s.to_string(),
            snake: s.to_snake_case(),
            pascal: s.to_pascal_case(),
        }
    }
}

fn validate_name(name: &str) -> eyre::Result<()> {
    if name.is_empty() {
        eyre::bail!("feature name is empty");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        eyre::bail!(
            "feature name `{}` must be kebab-case ([a-z0-9-]+); cargo package names accept only these",
            name
        );
    }
    if !name.chars().next().unwrap().is_ascii_lowercase() {
        eyre::bail!("feature name `{}` must start with a letter", name);
    }
    Ok(())
}

// ── Crate writers ─────────────────────────────────────────────────────

fn write_proto(feature_dir: &Path, n: &Names) -> eyre::Result<()> {
    let dir = feature_dir.join(format!("{}-proto", n.kebab));
    fs::create_dir_all(dir.join("src"))?;

    write(
        dir.join("Cargo.toml"),
        format!(
            r#"[package]
name = "{kebab}-proto"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true

# Wire contract for the `{kebab}` feature. Pull this crate (or the
# `{kebab}` facade) from any consumer — wasm-clean by default.

[dependencies]
architect.workspace = true
chrono.workspace = true
uuid = {{ workspace = true, features = ["js"] }}
facet.workspace = true
thiserror.workspace = true

vox = {{ workspace = true, optional = true }}
sea-orm = {{ workspace = true, optional = true }}
fake = {{ workspace = true, optional = true }}

[features]
default = ["vox"]
vox = ["dep:vox", "architect/vox"]
server = ["architect/server-seaorm", "dep:sea-orm"]
fake = ["dep:fake", "architect/fake"]
full = ["vox", "server", "fake"]
"#,
            kebab = n.kebab,
        ),
    )?;

    write(
        dir.join("src").join("lib.rs"),
        format!(
            r#"//! `{kebab}-proto` — wire contract for the `{kebab}` feature.
//!
//! Rename the `{pascal}` placeholder below to whatever the canonical
//! entity for this feature actually is (`Channel`, `Track`, `Clip`, …).

pub use architect;

use architect::Entity;
use chrono::{{DateTime, Utc}};
use uuid::Uuid;

#[cfg_attr(feature = "fake", derive(::fake::Dummy))]
#[derive(Entity, ::facet::Facet, Clone, Debug, PartialEq)]
#[architect(table_name = "{snake}_items", repo)]
pub struct {pascal} {{
    #[architect(primary_key, auto_increment = false, on_create = Uuid::new_v4())]
    pub id: Uuid,

    #[architect(filterable, sortable, fulltext)]
    pub name: String,

    #[architect(exclude(create, update), on_create = Utc::now())]
    pub created_at: DateTime<Utc>,

    #[architect(exclude(create, update), on_create = Utc::now(), on_update = Utc::now())]
    pub updated_at: DateTime<Utc>,
}}

// Hand-written service trait. Architect doesn't touch this; replace
// the placeholder methods with your real domain operations.

#[derive(Debug, Clone, PartialEq, Eq, ::facet::Facet, thiserror::Error)]
#[repr(u8)]
pub enum {pascal}ServiceError {{
    #[error("not found")]
    NotFound,
    #[error("invalid input: {{0}}")]
    InvalidInput(String),
    #[error("internal error: {{0}}")]
    Internal(String),
}}

#[cfg_attr(feature = "vox", vox::service)]
pub trait {pascal}Service {{
    /// TODO: replace with a real domain method.
    async fn ping(&self) -> Result<String, {pascal}ServiceError>;
}}
"#,
            kebab = n.kebab,
            snake = n.snake,
            pascal = n.pascal,
        ),
    )?;
    Ok(())
}

fn write_crdt(feature_dir: &Path, n: &Names) -> eyre::Result<()> {
    let dir = feature_dir.join(format!("{}-crdt", n.kebab));
    fs::create_dir_all(dir.join("src"))?;

    write(
        dir.join("Cargo.toml"),
        format!(
            r#"[package]
name = "{kebab}-crdt"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true

# Loro-backed source-of-truth for the `{kebab}` feature.
#
# Owns the marker type that makes Rust's orphan rules cooperate
# (foreign trait + foreign type), the `EntityCrdt` impl (codec +
# id/timestamp policy + sort lookup), and the `{pascal}RepoLoro`
# newtype that implements the architect-emitted `{pascal}Repo`.
#
# All real work happens inside `LoroRepo<{pascal}Entity>` from
# `features/crdt/crdt`; this crate is intentionally thin so the architect
# derive can emit it from field attributes in a future revision.

[dependencies]
{kebab}-proto.workspace = true
crdt.workspace = true
architect.workspace = true
loro.workspace = true
chrono.workspace = true
uuid.workspace = true
"#,
            kebab = n.kebab,
            pascal = n.pascal,
        ),
    )?;

    write(
        dir.join("src").join("lib.rs"),
        format!(
            r#"//! Loro-backed `{pascal}Repo`. Source of truth lives in a LoroDoc;
//! persistence is the concern of `{kebab}-db`.

use architect::{{Page, RepoError, SortOrder}};
use chrono::Utc;
use crdt::EntityCrdt;
use crdt::codec::{{read_dt, read_str, read_uuid, write_dt, write_str, write_uuid}};
use {snake}_proto::{{
    {pascal}, {pascal}Create, {pascal}List, {pascal}Repo, {pascal}Update,
}};
use loro::LoroMap;
use uuid::Uuid;

pub use crdt::{{CrdtDoc, LoroRepo}};

/// Zero-sized marker for the `EntityCrdt` impl. Lets us implement
/// the foreign `EntityCrdt` trait without owning the wire struct.
pub struct {pascal}Entity;

/// Newtype-wrapped repo, implements `{pascal}Repo`. Construct from a
/// `CrdtDoc` — multiple repos over different entity types can share
/// one doc, mutations commit together, exports cover the whole doc.
#[derive(Clone)]
pub struct {pascal}RepoLoro {{
    inner: LoroRepo<{pascal}Entity>,
}}

impl {pascal}RepoLoro {{
    pub fn new(doc: &CrdtDoc) -> Self {{
        Self {{ inner: doc.repo() }}
    }}

    pub fn inner(&self) -> &LoroRepo<{pascal}Entity> {{
        &self.inner
    }}

    pub fn doc(&self) -> &loro::LoroDoc {{
        self.inner.doc()
    }}
}}

// ── EntityCrdt impl ───────────────────────────────────────────────────
//
// Container layout: a top-level "{snake}_items" LoroMap keyed by
// uuid string; each entry is a sub-LoroMap with the wire fields.
// For features with hierarchy or ordering, swap this for a LoroTree
// or LoroMovableList — the trait surface stays the same.

impl EntityCrdt for {pascal}Entity {{
    type Wire = {pascal};
    type Create = {pascal}Create;
    type Update = {pascal}Update;
    type List = {pascal}List;

    const ROOT: &'static str = "{snake}_items";

    fn id(wire: &{pascal}) -> Uuid {{
        wire.id
    }}

    fn from_create(input: {pascal}Create) -> {pascal} {{
        let now = Utc::now();
        {pascal} {{
            id: Uuid::new_v4(),
            name: input.name,
            created_at: now,
            updated_at: now,
        }}
    }}

    fn encode_into(m: &LoroMap, e: &{pascal}) -> Result<(), RepoError> {{
        write_uuid(m, "id", e.id)?;
        write_str(m, "name", &e.name)?;
        write_dt(m, "created_at", e.created_at)?;
        write_dt(m, "updated_at", e.updated_at)?;
        Ok(())
    }}

    fn decode_from(m: &LoroMap) -> Result<{pascal}, RepoError> {{
        Ok({pascal} {{
            id: read_uuid(m, "id")?,
            name: read_str(m, "name")?,
            created_at: read_dt(m, "created_at")?,
            updated_at: read_dt(m, "updated_at")?,
        }})
    }}

    fn apply_update(m: &LoroMap, input: {pascal}Update) -> Result<(), RepoError> {{
        if let Some(name) = input.name {{
            write_str(m, "name", &name)?;
        }}
        write_dt(m, "updated_at", Utc::now())?;
        Ok(())
    }}

    fn sort_items(items: &mut [{pascal}], field: &str, order: SortOrder) -> Result<(), RepoError> {{
        match field {{
            "name" => items.sort_by(|a, b| a.name.cmp(&b.name)),
            "created_at" => items.sort_by(|a, b| a.created_at.cmp(&b.created_at)),
            other => {{
                return Err(RepoError::InvalidInput(format!(
                    "unsortable field: {{other}}"
                )));
            }}
        }}
        if matches!(order, SortOrder::Desc) {{
            items.reverse();
        }}
        Ok(())
    }}

    fn build_list(items: Vec<{pascal}>, total: u32, page: Page) -> {pascal}List {{
        {pascal}List {{ items, total, page }}
    }}
}}

// ── {pascal}Repo forwarders ────────────────────────────────────────────

impl {pascal}Repo for {pascal}RepoLoro {{
    async fn get(&self, id: Uuid) -> Result<{pascal}, RepoError> {{
        self.inner.get(id).await
    }}

    async fn list(
        &self,
        page: architect::Page,
        sort: Option<architect::Sort>,
        filter: Option<architect::Filter>,
    ) -> Result<{pascal}List, RepoError> {{
        self.inner.list(page, sort, filter).await
    }}

    async fn create(&self, input: {pascal}Create) -> Result<{pascal}, RepoError> {{
        self.inner.create(input).await
    }}

    async fn update(&self, id: Uuid, input: {pascal}Update) -> Result<{pascal}, RepoError> {{
        self.inner.update(id, input).await
    }}

    async fn delete(&self, id: Uuid) -> Result<(), RepoError> {{
        self.inner.delete(id).await
    }}
}}

"#,
            kebab = n.kebab,
            snake = n.snake,
            pascal = n.pascal,
        ),
    )?;
    Ok(())
}

fn write_db(feature_dir: &Path, n: &Names) -> eyre::Result<()> {
    let dir = feature_dir.join(format!("{}-db", n.kebab));
    fs::create_dir_all(dir.join("src"))?;

    write(
        dir.join("Cargo.toml"),
        format!(
            r#"[package]
name = "{kebab}-db"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true

# SeaORM-side concerns for the `{kebab}` feature.
#
# The CRDT snapshot/update tables live in `features/crdt/crdt-seaorm` and are
# generic across every feature. This crate owns just the feature-
# specific persistence bits:
#
# - Re-exports `SeaOrmPersistence` so the app wires one type per
#   feature for readability.
# - Provides a `{pascal}Migrator` that delegates to crdt-seaorm's
#   migrator by default. Add projection tables here when this feature
#   needs SQL-shaped queries.

[dependencies]
{kebab}-proto.workspace = true
crdt-seaorm.workspace = true
sea-orm.workspace = true
sea-orm-migration.workspace = true
async-trait = "0.1"
"#,
            kebab = n.kebab,
            pascal = n.pascal,
        ),
    )?;

    write(
        dir.join("src").join("lib.rs"),
        format!(
            r#"//! SeaORM persistence for the `{kebab}` feature.

pub use crdt_seaorm::SeaOrmPersistence;

pub mod migrations;

pub use migrations::{pascal}Migrator;
"#,
            kebab = n.kebab,
            pascal = n.pascal,
        ),
    )?;

    write(
        dir.join("src").join("migrations.rs"),
        format!(
            r#"//! Migrator for the `{kebab}` feature. Runs crdt-seaorm's generic
//! migrations (the `crdt_doc` + `crdt_update` tables) by default;
//! add projection tables below when SQL-shaped queries are needed.

use sea_orm_migration::prelude::*;

pub struct {pascal}Migrator;

#[async_trait::async_trait]
impl MigratorTrait for {pascal}Migrator {{
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {{
        // Start with the generic CRDT-persistence migrations; append
        // feature-specific projection migrations after them when the
        // feature needs SQL-shaped queries.
        crdt_seaorm::Migrator::migrations()
    }}
}}
"#,
            kebab = n.kebab,
            pascal = n.pascal,
        ),
    )?;
    Ok(())
}

fn write_facade(feature_dir: &Path, n: &Names) -> eyre::Result<()> {
    let dir = feature_dir.join(&n.kebab);
    fs::create_dir_all(dir.join("src"))?;

    write(
        dir.join("Cargo.toml"),
        format!(
            r#"[package]
name = "{kebab}"
version.workspace = true
edition.workspace = true

# Facade — single import root for the `{kebab}` feature.
#
#   default        wire types only (wasm-clean client baseline)
#   vox            #[vox::service] surface + dispatcher/client
#   server         CRDT source of truth + SeaORM persistence (pulls
#                  {kebab}-crdt + {kebab}-db together — they're paired)
#   server-axum    architect::axum_ws helpers, re-exported here
#   fake           #[derive(Dummy)] on every emitted wire struct
#   full           everything above

[dependencies]
{kebab}-proto.workspace = true
{kebab}-crdt = {{ workspace = true, optional = true }}
{kebab}-db = {{ workspace = true, optional = true }}
crdt = {{ workspace = true, optional = true }}
architect = {{ workspace = true, optional = true }}

[features]
default = []
vox = ["{kebab}-proto/vox"]
server = ["dep:{kebab}-crdt", "dep:{kebab}-db", "dep:crdt"]
server-axum = ["dep:architect", "architect/server-axum"]
fake = ["{kebab}-proto/fake"]
full = [
    "vox",
    "server",
    "server-axum",
    "fake",
]
"#,
            kebab = n.kebab,
        ),
    )?;

    write(
        dir.join("src").join("lib.rs"),
        format!(
            r#"//! Facade for the `{kebab}` feature. Wire types are always
//! re-exported; backends + transport adapters are feature-gated.

pub use {snake}_proto::*;

/// CRDT source of truth + SeaORM persistence. Construct one
/// `CrdtDoc` per collaboration boundary and hand a `{pascal}RepoLoro`
/// to the vox dispatcher.
#[cfg(feature = "server")]
pub mod server {{
    pub use crdt::{{CrdtDoc, Persistence}};
    pub use {snake}_crdt::{{{pascal}Entity, {pascal}RepoLoro}};
    pub use {snake}_db::{{{pascal}Migrator, SeaOrmPersistence}};
}}

#[cfg(feature = "server-axum")]
pub use architect::axum_ws;
"#,
            kebab = n.kebab,
            snake = n.snake,
            pascal = n.pascal,
        ),
    )?;
    Ok(())
}

fn write_spec(feature_dir: &Path, n: &Names) -> eyre::Result<()> {
    let dir = feature_dir.join("spec");
    fs::create_dir_all(&dir)?;

    write(
        dir.join(format!("{}.md", n.kebab)),
        format!(
            r#"+++
title = "{pascal} contract"
description = "Tracey-tracked rules the {pascal}Repo implementation must hold."
weight = 100
+++

Rules are linked to source via `r[impl <id>]` and `r[verify <id>]`
annotations. Run `cargo xtask tracey-validate` to confirm coverage.

r[{snake}.placeholder]
TODO: write a real rule for this feature. Tracey enforces
lowercase-dot-separated IDs and that each rule has at least one impl
+ verify reference.
"#,
            pascal = n.pascal,
            snake = n.snake,
        ),
    )?;
    Ok(())
}

fn write_tests_native(feature_dir: &Path, n: &Names) -> eyre::Result<()> {
    let dir = feature_dir.join("tests").join("native");
    fs::create_dir_all(dir.join("src"))?;

    write(
        dir.join("Cargo.toml"),
        format!(
            r#"[package]
name = "{kebab}-tests-native"
version.workspace = true
edition.workspace = true
authors.workspace = true
license.workspace = true
publish = false

# Drives the `{kebab}-proto` contract against the Loro-backed
# `{kebab}-crdt` using an ephemeral `CrdtDoc` (no persistence). Fast,
# no DB, no WebSocket. Use fake-rs Dummy derives via the `fake`
# feature on `{kebab}-proto` to seed repositories deterministically.

[dependencies]
{kebab}-proto = {{ workspace = true, features = ["fake"] }}
{kebab}-crdt.workspace = true
crdt.workspace = true
loro.workspace = true
architect = {{ workspace = true, features = ["fake"] }}
fake.workspace = true
uuid.workspace = true
tokio = {{ workspace = true, features = ["macros", "rt"] }}
"#,
            kebab = n.kebab,
        ),
    )?;

    write(
        dir.join("src").join("lib.rs"),
        format!(
            r#"//! Native integration tests for the `{kebab}` feature. Drives
//! the wire contract against the Loro-backed implementation through
//! an ephemeral `CrdtDoc`.

#![cfg(test)]

use {snake}_crdt::{{CrdtDoc, {pascal}RepoLoro}};
use {snake}_proto::{{{pascal}Create, {pascal}Repo}};

fn repo() -> {pascal}RepoLoro {{
    {pascal}RepoLoro::new(&CrdtDoc::ephemeral())
}}

#[tokio::test]
async fn create_then_get_round_trip() {{
    let r = repo();
    let created = r
        .create({pascal}Create {{ name: "alpha".into() }})
        .await
        .unwrap();
    let got = r.get(created.id).await.unwrap();
    assert_eq!(got.id, created.id);
    assert_eq!(got.name, "alpha");
}}

// Concurrent-replica merge — two repos backed by independent docs
// exchange their bytes and converge. This is the test that says
// "yes, this is actually a CRDT."
#[tokio::test]
async fn two_replicas_converge_after_sync() {{
    use loro::ExportMode;
    let a = repo();
    let b = repo();
    a.create({pascal}Create {{ name: "from-a".into() }}).await.unwrap();
    b.create({pascal}Create {{ name: "from-b".into() }}).await.unwrap();
    let ab = a.doc().export(ExportMode::all_updates()).unwrap();
    let bb = b.doc().export(ExportMode::all_updates()).unwrap();
    b.doc().import(&ab).unwrap();
    a.doc().import(&bb).unwrap();
    let a_list = a.list(architect::Page {{ index: 0, size: 100 }}, None, None).await.unwrap();
    let b_list = b.list(architect::Page {{ index: 0, size: 100 }}, None, None).await.unwrap();
    assert_eq!(a_list.total, 2);
    assert_eq!(b_list.total, 2);
}}
"#,
            kebab = n.kebab,
            snake = n.snake,
            pascal = n.pascal,
        ),
    )?;
    Ok(())
}

// ── Workspace + tracey integration ────────────────────────────────────

fn update_workspace_cargo(repo_root: &Path, n: &Names) -> eyre::Result<()> {
    let path = repo_root.join("Cargo.toml");
    let contents = fs::read_to_string(&path)?;
    let mut doc: DocumentMut = contents.parse()?;

    // Members the cargo workspace builds.
    let members_to_add = [
        format!("features/{}/{}", n.kebab, n.kebab),
        format!("features/{}/{}-proto", n.kebab, n.kebab),
        format!("features/{}/{}-crdt", n.kebab, n.kebab),
        format!("features/{}/{}-db", n.kebab, n.kebab),
        format!("features/{}/tests/native", n.kebab),
    ];

    for key in ["members", "default-members"] {
        if let Some(arr) = doc
            .get_mut("workspace")
            .and_then(|w| w.get_mut(key))
            .and_then(|v| v.as_array_mut())
        {
            for m in &members_to_add {
                if !array_contains_str(arr, m) {
                    arr.push(m.as_str());
                }
            }
        }
    }

    // workspace.dependencies entries.
    let deps_to_add = [
        (n.kebab.clone(), format!("features/{}/{}", n.kebab, n.kebab)),
        (
            format!("{}-proto", n.kebab),
            format!("features/{}/{}-proto", n.kebab, n.kebab),
        ),
        (
            format!("{}-crdt", n.kebab),
            format!("features/{}/{}-crdt", n.kebab, n.kebab),
        ),
        (
            format!("{}-db", n.kebab),
            format!("features/{}/{}-db", n.kebab, n.kebab),
        ),
    ];

    if let Some(deps_table) = doc
        .get_mut("workspace")
        .and_then(|w| w.get_mut("dependencies"))
        .and_then(|v| v.as_table_mut())
    {
        for (name, path_rel) in &deps_to_add {
            if !deps_table.contains_key(name) {
                let mut t = Table::new();
                t.set_implicit(false);
                t["path"] = value(path_rel);
                deps_table.insert(
                    name,
                    Item::Value(toml_edit::Value::InlineTable(t.into_inline_table())),
                );
            }
        }
    }

    fs::write(&path, doc.to_string())?;
    eprintln!("  {} updated Cargo.toml", "✓".green());
    Ok(())
}

fn array_contains_str(arr: &Array, needle: &str) -> bool {
    arr.iter().filter_map(|v| v.as_str()).any(|s| s == needle)
}

fn update_tracey_config(repo_root: &Path, n: &Names) -> eyre::Result<()> {
    let path = repo_root.join(".config").join("tracey").join("config.styx");
    if !path.exists() {
        // No tracey config — skip silently.
        return Ok(());
    }
    let mut contents = fs::read_to_string(&path)?;
    let probe = format!("name {}", n.kebab);
    if contents.contains(&probe) {
        return Ok(());
    }

    // Find the closing `)` of the outer specs block. The styx file
    // shape is:
    //   @schema ...
    //
    //   specs (
    //       { ... }
    //       { ... }  <- we want to insert another block here
    //   )
    //
    // Append before the LAST closing `)` on its own line, indented or
    // not. Cheap heuristic: replace the final `\n)\n` with our block + `\n)\n`.
    // Default new features to a single `live` impl, scoped to the
    // db backend that the scaffold just created. Adding more
    // backends later means adding more impl blocks (e.g. `mock`,
    // `reaper`, `protools`) under the same spec.
    let block = format!(
        r#"
    {{
        name {kebab}
        include (features/{kebab}/spec/**/*.md)
        impls (
            {{
                name live
                include (
                    features/{kebab}/{kebab}-crdt/**/*.rs
                )
                exclude (
                    features/{kebab}/**/target/**
                )
                test_include (
                    features/{kebab}/tests/**/*.rs
                )
            }}
        )
    }}
"#,
        kebab = n.kebab,
    );

    // Insert just before the last `)` (the outer specs close).
    if let Some(idx) = contents.rfind(')') {
        contents.insert_str(idx, &block);
        fs::write(&path, contents)?;
        eprintln!("  {} updated .config/tracey/config.styx", "✓".green());
    }
    Ok(())
}

// ── File I/O helper ───────────────────────────────────────────────────

fn write(path: PathBuf, contents: impl AsRef<[u8]>) -> eyre::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, contents)?;
    Ok(())
}
