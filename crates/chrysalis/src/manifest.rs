//! `manifest.rs` — the package manifest as **`.ys`-as-data** (#67, Phase 1).
//!
//! A package's identity in the registry-category (the `pkg` agent's domain): its
//! `name`, `version`, `dependencies` (the morphisms to other packages), and
//! `exports`. The manifest is `project.ys` read as DATA through the chrysalis
//! parser — NOT a bespoke regex. (The legacy one-line `package <name>` directive
//! in [`crate::codegen`] is a DIFFERENT manifest: it names a *native Rust crate* to
//! link via codegen. The two converge in Phase 5, when the codegen path is rewritten
//! onto the canonical run-Core; until then they are read by different parsers and
//! describe different things — a `.ys`-package dependency graph here, a native-crate
//! link there.)
//!
//! Being real `.ys` data means `chrysalis add` (Phase 3) edits it through the same
//! correct-by-construction round-trip [`crate::coord::set_in_source`] uses for the
//! board — read → parse → set field → unparse → gate → write, never hand-written
//! syntax. The surface form is a single record binding:
//!
//! ```text
//! # project.ys
//! def package = {
//!   name: 'foo',
//!   version: '0.1.0',
//!   dependencies: { bar: { path: '.bar' } },
//!   exports: ['grow', 'Cell'],
//! }
//! ```

use std::path::{Path, PathBuf};

use indexmap::IndexMap;

use crate::ast::{Def, Expr, StringLit};
use crate::parse::parse_program;
use crate::version::{Version, VersionReq};

/// The binding that holds the manifest record (`def package = { … }`).
pub const MANIFEST_BINDING: &str = "package";
/// The manifest file name, walked-for up the directory tree.
pub const MANIFEST_FILE: &str = "project.ys";

/// Where a project's registry lives — the two backends behind the `Registry` trait. A
/// `registry: '…'` that parses as a URL is a remote backend (#67 Phase 4b); anything else
/// is a local dir (Phase 3). See [`Manifest::registry_location`].
#[derive(Debug, Clone, PartialEq)]
pub enum RegistryLocation {
    /// A local directory registry (`<dir>/<name>/<version>/`).
    Dir(PathBuf),
    /// A remote registry addressed by base URL (`http://host:port`).
    Url(String),
}

/// A structured package manifest — the package's identity + its dependency edges.
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    pub name: String,
    /// The package's own version (the object's identity in the registry poset).
    pub version: Option<Version>,
    /// The package's **OWN** native crate (the co-located *mixed* shape: this
    /// package's theory includes a Rust crate's `domain_core()` — `spatio-flux`'s
    /// `sf_core()` + its `ys/`, `prism-audio`'s `audio_core()`). The path is the crate
    /// dir, relative to `dir` (or absolute); `'.'` is the co-located case. Distinct
    /// from a native *dependency* (`dependencies.x.native`): the own crate supplies the
    /// package's native BASE — its `prelude::core()` **and** `prelude::modules()` (its
    /// full import surface, e.g. `from diffusion import …`). The legacy `package <name>
    /// [at <path>]` directive migrates here (#67 Phase 5c).
    pub native: Option<PathBuf>,
    /// The project's package **registry** location — where registry dependencies
    /// (`{ foo: { version: '^1' } }`) are resolved from. A local directory now (#67
    /// Phase 3, `registry: '<dir>'`, relative to `dir` or absolute); a URL / pluggable
    /// backend in Phase 4. `None` ⇒ the project uses no registry (path/native deps only).
    pub registry: Option<PathBuf>,
    /// The package's **output sink** (`docs/domain-libraries.md` §5) — the name of the
    /// part (a native-dep edge, or the own `native:` crate) whose `prelude::run_realtime`
    /// drives the engine to the world (an audio device; later a display / file / stream).
    /// `chrysalis run <x>.ys --play` routes a sink-declaring package through the codegen
    /// runner in play mode; `None` is no sink (offline JSON only). (#67 Phase 5c / §5.)
    pub sink: Option<String>,
    pub dependencies: Vec<Dependency>,
    pub exports: Vec<String>,
    /// The directory containing `project.ys`. Path dependencies resolve relative to
    /// it; the resolver loads sibling `.ys` files from here.
    pub dir: PathBuf,
}

/// One dependency edge: a name bound to where the package is found, plus the version
/// requirement (the compatibility functor) the resolved package must satisfy.
#[derive(Debug, Clone, PartialEq)]
pub struct Dependency {
    pub name: String,
    pub source: DependencySource,
    /// The version requirement on this edge (`VersionReq::any()` if unspecified).
    pub req: VersionReq,
}

/// Where a dependency's Core comes from. Native vs `.ys` is just *where the part's
/// Core is sourced* — `Core::colimit` links them uniformly. Registry sources (a
/// version requirement resolved against an index) arrive in Phase 3.
#[derive(Debug, Clone, PartialEq)]
pub enum DependencySource {
    /// A local **path** dependency: another `.ys` package on disk, relative to the
    /// depending manifest's `dir` (or absolute). Its Core is compiled from `.ys`.
    Path(PathBuf),
    /// A **native** dependency: a Rust crate (the directory holding its `Cargo.toml`)
    /// exposing a `domain_core()` via the `prelude::core()`/`modules()` convention.
    /// chrysalis (a fixed binary) can't link it in-process, so a native dep routes the
    /// build through codegen — a generated runner links the crate and supplies its
    /// Core to [`crate::resolver::resolve`] (#67 Phase 5).
    Native(PathBuf),
    /// A **registry** dependency: no declared path — resolved by `name` + the edge's
    /// [`VersionReq`] against the project's registry ([`Manifest::registry`]). The
    /// `{ foo: { version: '^1.0' } }` shape `chrysalis add foo` writes (#67 Phase 3).
    Registry,
}

impl DependencySource {
    /// The declared on-disk path, if this source has one (`Path`/`Native`); `None` for
    /// a registry dependency (its source comes from the registry, not the manifest).
    pub fn path(&self) -> Option<&Path> {
        match self {
            DependencySource::Path(p) | DependencySource::Native(p) => Some(p),
            DependencySource::Registry => None,
        }
    }

    /// Is this a native (Rust crate) dependency?
    pub fn is_native(&self) -> bool {
        matches!(self, DependencySource::Native(_))
    }

    /// Is this a registry dependency (resolved by name + version, no path)?
    pub fn is_registry(&self) -> bool {
        matches!(self, DependencySource::Registry)
    }
}

impl Dependency {
    /// The dependency's directory for a path/native source, resolved against
    /// `manifest_dir`. A registry dependency has no path here — it is resolved via the
    /// registry, never through this method — so it falls back to `manifest_dir` (a
    /// harmless placeholder the resolver never reads for a registry dep).
    pub fn resolved_dir(&self, manifest_dir: &Path) -> PathBuf {
        match self.source.path() {
            Some(p) if p.is_absolute() => p.to_path_buf(),
            Some(p) => manifest_dir.join(p),
            None => manifest_dir.to_path_buf(),
        }
    }
}

impl Manifest {
    /// Parse `project.ys` SOURCE as `.ys`-data. `dir` is the manifest's directory
    /// (path dependencies resolve relative to it).
    pub fn parse(src: &str, dir: impl AsRef<Path>) -> Result<Manifest, String> {
        let program = parse_program(src).map_err(|e| format!("parse {MANIFEST_FILE}: {e}"))?;
        let fields = program
            .defs
            .iter()
            .find_map(|d| match d {
                Def::Binding {
                    name,
                    value: Expr::Record(fields),
                    ..
                } if name == MANIFEST_BINDING => Some(fields),
                _ => None,
            })
            .ok_or_else(|| {
                format!("{MANIFEST_FILE} has no `def {MANIFEST_BINDING} = {{ … }}` manifest record")
            })?;

        let name = string_field(fields, "name")
            .ok_or_else(|| format!("{MANIFEST_FILE}: the manifest needs a `name: '...'`"))?;
        let version = match string_field(fields, "version") {
            Some(s) => Some(
                s.parse::<Version>()
                    .map_err(|e| format!("{MANIFEST_FILE}: {e}"))?,
            ),
            None => None,
        };
        let native = string_field(fields, "native").map(PathBuf::from);
        let registry = string_field(fields, "registry").map(PathBuf::from);
        let sink = string_field(fields, "sink");
        let exports = match fields.get("exports") {
            Some(e) => string_list(e)
                .ok_or_else(|| "`exports` must be a list of strings".to_string())?,
            None => Vec::new(),
        };
        let dependencies = match fields.get("dependencies") {
            Some(e) => parse_dependencies(e)?,
            None => Vec::new(),
        };

        Ok(Manifest {
            name,
            version,
            native,
            registry,
            sink,
            dependencies,
            exports,
            dir: dir.as_ref().to_path_buf(),
        })
    }

    /// The project's registry root directory, resolved against `dir` (if declared).
    pub fn registry_dir(&self) -> Option<PathBuf> {
        self.registry.as_ref().map(|r| {
            if r.is_absolute() {
                r.clone()
            } else {
                self.dir.join(r)
            }
        })
    }

    /// Where the registry lives: a remote URL (`http://…`/`https://…`, the Phase-4b
    /// backend) or a local dir (Phase 3, resolved against `dir`). `None` ⇒ the project
    /// uses no registry. The seam `resolver::open_registry` branches on to pick a
    /// [`RemoteRegistry`](crate::registry::RemoteRegistry) vs a
    /// [`LocalRegistry`](crate::registry::LocalRegistry) — the pluggable backend.
    pub fn registry_location(&self) -> Option<RegistryLocation> {
        let raw = self.registry.as_ref()?;
        let s = raw.to_string_lossy();
        if s.starts_with("http://") || s.starts_with("https://") {
            Some(RegistryLocation::Url(s.into_owned()))
        } else {
            // `registry` is `Some`, so `registry_dir` is too.
            Some(RegistryLocation::Dir(self.registry_dir().unwrap()))
        }
    }

    /// The package's own native crate directory (if any), resolved against `dir`.
    pub fn native_dir(&self) -> Option<PathBuf> {
        self.native.as_ref().map(|p| {
            if p.is_absolute() {
                p.clone()
            } else {
                self.dir.join(p)
            }
        })
    }

    /// Read + parse the manifest at `<dir>/project.ys`.
    pub fn load(dir: impl AsRef<Path>) -> Result<Manifest, String> {
        let dir = dir.as_ref();
        let path = dir.join(MANIFEST_FILE);
        let src = std::fs::read_to_string(&path)
            .map_err(|e| format!("read {}: {e}", path.display()))?;
        Manifest::parse(&src, dir)
    }

    /// Walk UP from a `.ys` file (or a directory) to the nearest `project.ys` that
    /// holds a structured `def package` record, returning the parsed manifest.
    /// `None` ⇒ no structured manifest above (a std `.ys`, or a legacy native-crate
    /// directive `project.ys` that this parser deliberately does not claim).
    pub fn find(start: impl AsRef<Path>) -> Option<Manifest> {
        let start = start.as_ref().canonicalize().ok()?;
        let mut dir = if start.is_dir() {
            Some(start.as_path())
        } else {
            start.parent()
        };
        while let Some(d) = dir {
            if d.join(MANIFEST_FILE).is_file() {
                if let Ok(m) = Manifest::load(d) {
                    return Some(m);
                }
            }
            dir = d.parent();
        }
        None
    }
}

// ── field extractors: walk the parsed `.ys` record (homoiconic — the parser is the
//    one source of truth for the surface, no second grammar) ──

pub(crate) fn string_field(fields: &IndexMap<String, Expr>, key: &str) -> Option<String> {
    match fields.get(key)? {
        Expr::Str(s) => s.as_plain(),
        _ => None,
    }
}

pub(crate) fn string_list(e: &Expr) -> Option<Vec<String>> {
    match e {
        Expr::List(items) => items
            .iter()
            .map(|i| match i {
                Expr::Str(s) => s.as_plain(),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

fn parse_dependencies(e: &Expr) -> Result<Vec<Dependency>, String> {
    let fields = match e {
        Expr::Record(f) => f,
        _ => return Err("`dependencies` must be a record `{ name: { path: '...' } }`".into()),
    };
    let mut deps = Vec::new();
    for (name, spec) in fields {
        let spec_fields = match spec {
            Expr::Record(f) => f,
            _ => {
                return Err(format!(
                    "dependency `{name}` must be a record like `{{ path: '...' }}`"
                ))
            }
        };
        let req = match string_field(spec_fields, "version") {
            Some(s) => s
                .parse::<VersionReq>()
                .map_err(|e| format!("dependency `{name}`: {e}"))?,
            None => VersionReq::any(),
        };
        let source = if let Some(p) = string_field(spec_fields, "path") {
            DependencySource::Path(PathBuf::from(p))
        } else if let Some(p) = string_field(spec_fields, "native") {
            DependencySource::Native(PathBuf::from(p))
        } else {
            // Neither `path` nor `native` ⇒ a REGISTRY dependency, resolved by name +
            // the `version` requirement against the project's registry (#67 Phase 3).
            DependencySource::Registry
        };
        deps.push(Dependency {
            name: name.clone(),
            source,
            req,
        });
    }
    Ok(deps)
}

/// Add (or replace) a dependency `name` in a `project.ys` SOURCE, returning the edited
/// source. Reuses the **coord-set homoiconic round-trip** — parse →
/// [`set_path`](crate::coord::set_path) the `dependencies.<name>` field → unparse →
/// re-parse GATE — so the manifest only ever goes good → good and a stray brace/quote
/// can never wedge it (the same correct-by-construction edit the board heartbeats use).
/// `chrysalis add`'s core (#67 Phase 3).
pub fn add_dependency(
    src: &str,
    name: &str,
    source: &DependencySource,
    version: Option<&str>,
) -> Result<String, String> {
    let mut program = parse_program(src).map_err(|e| format!("parse {MANIFEST_FILE}: {e}"))?;
    let record = program
        .defs
        .iter_mut()
        .find_map(|d| match d {
            Def::Binding {
                name: binding,
                value: Expr::Record(fields),
                ..
            } if binding == MANIFEST_BINDING => Some(fields),
            _ => None,
        })
        .ok_or_else(|| {
            format!("{MANIFEST_FILE} has no `def {MANIFEST_BINDING} = {{ … }}` record to add to")
        })?;

    crate::coord::set_path(record, &["dependencies", name], dep_spec_expr(source, version));

    // Preserve the leading comment/banner block (the AST drops comments), then GATE.
    let header: String = src
        .lines()
        .take_while(|l| {
            let t = l.trim_start();
            t.is_empty() || t.starts_with('#')
        })
        .map(|l| format!("{l}\n"))
        .collect();
    let rendered = format!("{header}{}", crate::unparse::unparse(&program));
    parse_program(&rendered).map_err(|e| {
        format!(
            "`chrysalis add` ABORTED (nothing written): the edited {MANIFEST_FILE} would not \
             re-parse — {e}"
        )
    })?;
    Ok(rendered)
}

/// Remove dependency `name` from a `project.ys` SOURCE (the reverse of
/// [`add_dependency`], same gated round-trip). Errors if there is no such dependency.
pub fn remove_dependency(src: &str, name: &str) -> Result<String, String> {
    let mut program = parse_program(src).map_err(|e| format!("parse {MANIFEST_FILE}: {e}"))?;
    let record = program
        .defs
        .iter_mut()
        .find_map(|d| match d {
            Def::Binding {
                name: binding,
                value: Expr::Record(fields),
                ..
            } if binding == MANIFEST_BINDING => Some(fields),
            _ => None,
        })
        .ok_or_else(|| format!("{MANIFEST_FILE} has no `def {MANIFEST_BINDING} = {{ … }}` record"))?;

    let removed = match record.get_mut("dependencies") {
        Some(Expr::Record(deps)) => deps.shift_remove(name).is_some(),
        _ => false,
    };
    if !removed {
        return Err(format!("no dependency `{name}` in {MANIFEST_FILE}"));
    }

    let header: String = src
        .lines()
        .take_while(|l| {
            let t = l.trim_start();
            t.is_empty() || t.starts_with('#')
        })
        .map(|l| format!("{l}\n"))
        .collect();
    let rendered = format!("{header}{}", crate::unparse::unparse(&program));
    parse_program(&rendered)
        .map_err(|e| format!("`chrysalis remove` ABORTED (nothing written): would not re-parse — {e}"))?;
    Ok(rendered)
}

/// Read `<dir>/project.ys`, remove the dependency, write it back (gated).
pub fn remove_dependency_from_file(dir: &Path, name: &str) -> Result<(), String> {
    let path = dir.join(MANIFEST_FILE);
    let src =
        std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let rendered = remove_dependency(&src, name)?;
    std::fs::write(&path, &rendered).map_err(|e| format!("write {}: {e}", path.display()))
}

/// Read `<dir>/project.ys`, add the dependency, write it back (gated). The on-disk
/// form of [`add_dependency`].
pub fn add_dependency_to_file(
    dir: &Path,
    name: &str,
    source: &DependencySource,
    version: Option<&str>,
) -> Result<(), String> {
    let path = dir.join(MANIFEST_FILE);
    let src =
        std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let rendered = add_dependency(&src, name, source, version)?;
    std::fs::write(&path, &rendered).map_err(|e| format!("write {}: {e}", path.display()))
}

/// Build the dependency spec record — `{ path: '...', version: '...' }` or
/// `{ native: '...' }` — as an `Expr::Record` (bare keys, the form the manifest parser
/// reads back as a dependency).
fn dep_spec_expr(source: &DependencySource, version: Option<&str>) -> Expr {
    let mut rec: IndexMap<String, Expr> = IndexMap::new();
    match source {
        DependencySource::Path(p) => {
            rec.insert("path".to_string(), str_lit(&p.to_string_lossy()));
        }
        DependencySource::Native(p) => {
            rec.insert("native".to_string(), str_lit(&p.to_string_lossy()));
        }
        // A registry dependency is version-only — no `path`/`native` key.
        DependencySource::Registry => {}
    }
    if let Some(v) = version {
        rec.insert("version".to_string(), str_lit(v));
    }
    Expr::Record(rec)
}

fn str_lit(s: &str) -> Expr {
    Expr::Str(StringLit::plain(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_full_manifest() {
        let src = "\
def package = {
  name: 'foo',
  version: '0.2.1',
  dependencies: { bar: { path: '.bar', version: '^1.0' }, baz: { path: '../baz' } },
  exports: ['grow', 'Cell'],
}
";
        let m = Manifest::parse(src, "/proj").expect("parses");
        assert_eq!(m.name, "foo");
        assert_eq!(m.version, Some(Version::new(0, 2, 1)));
        assert_eq!(m.exports, vec!["grow", "Cell"]);
        assert_eq!(m.dependencies.len(), 2);
        assert_eq!(m.dependencies[0].name, "bar");
        assert_eq!(
            m.dependencies[0].source,
            DependencySource::Path(PathBuf::from(".bar"))
        );
        // The version requirement on the edge (the compatibility functor).
        assert!(m.dependencies[0].req.matches(&Version::new(1, 5, 0)));
        assert!(!m.dependencies[0].req.matches(&Version::new(2, 0, 0)));
        // An unspecified requirement defaults to "any".
        assert!(m.dependencies[1].req.matches(&Version::new(9, 9, 9)));
        assert_eq!(m.dependencies[1].name, "baz");
    }

    #[test]
    fn an_invalid_package_version_is_an_error() {
        let err = Manifest::parse("def package = { name: 'p', version: 'not-a-version' }", "/p")
            .unwrap_err();
        assert!(err.contains("version"), "got: {err}");
    }

    #[test]
    fn parses_a_minimal_manifest() {
        let m = Manifest::parse("def package = { name: 'solo' }", "/p").expect("parses");
        assert_eq!(m.name, "solo");
        assert_eq!(m.version, None);
        assert!(m.dependencies.is_empty());
        assert!(m.exports.is_empty());
        assert_eq!(m.sink, None, "no sink unless declared");
    }

    #[test]
    fn parses_a_sink_field() {
        // The output boundary (docs/domain-libraries.md §5): `sink: 'audio'` names the
        // native-dep edge whose device the codegen runner drives `--play` to.
        let m = Manifest::parse(
            "def package = { name: 'synth', sink: 'audio', dependencies: { audio: { native: '../audio' } } }",
            "/p",
        )
        .unwrap();
        assert_eq!(m.sink, Some("audio".to_string()));
    }

    #[test]
    fn a_path_dependency_resolves_relative_to_the_manifest_dir() {
        let m = Manifest::parse("def package = { name: 'p', dependencies: { d: { path: '../d' } } }", "/work/p")
            .unwrap();
        let dir = m.dependencies[0].resolved_dir(&m.dir);
        assert_eq!(dir, PathBuf::from("/work/p/../d"));
    }

    #[test]
    fn an_absolute_path_dependency_is_used_as_is() {
        let m = Manifest::parse(
            "def package = { name: 'p', dependencies: { d: { path: '/abs/d' } } }",
            "/work/p",
        )
        .unwrap();
        assert_eq!(
            m.dependencies[0].resolved_dir(&m.dir),
            PathBuf::from("/abs/d")
        );
    }

    #[test]
    fn missing_manifest_record_is_an_error() {
        // A `.ys` file with no `def package` record is not a structured manifest.
        let err = Manifest::parse("def other = { name: 'x' }", "/p").unwrap_err();
        assert!(err.contains("def package"), "got: {err}");
    }

    #[test]
    fn missing_name_is_an_error() {
        let err = Manifest::parse("def package = { version: '1.0.0' }", "/p").unwrap_err();
        assert!(err.contains("name"), "got: {err}");
    }

    #[test]
    fn a_version_only_dependency_is_a_registry_dependency() {
        // No `path`/`native` ⇒ a registry dependency, resolved by name + version.
        let m = Manifest::parse(
            "def package = { name: 'p', registry: '../reg', dependencies: { d: { version: '^1.0' } } }",
            "/p",
        )
        .unwrap();
        assert_eq!(m.dependencies.len(), 1);
        assert!(m.dependencies[0].source.is_registry());
        assert!(m.dependencies[0].req.matches(&Version::new(1, 5, 0)));
        assert_eq!(m.registry_dir(), Some(PathBuf::from("/p/../reg")));
    }

    #[test]
    fn parses_a_native_dependency() {
        let m = Manifest::parse(
            "def package = { name: 'app', dependencies: { audio: { native: '../crates/prism-audio' } } }",
            "/proj",
        )
        .unwrap();
        assert_eq!(m.dependencies.len(), 1);
        let dep = &m.dependencies[0];
        assert_eq!(dep.name, "audio");
        assert!(dep.source.is_native());
        assert_eq!(
            dep.source,
            DependencySource::Native(PathBuf::from("../crates/prism-audio"))
        );
    }

    #[test]
    fn parses_an_own_native_crate() {
        // The co-located mixed shape — the package IS a native crate + its `.ys`.
        let m = Manifest::parse(
            "def package = { name: 'spatio-flux', native: '.' }",
            "/work/spatio-flux",
        )
        .unwrap();
        assert_eq!(m.native, Some(PathBuf::from(".")));
        assert_eq!(m.native_dir(), Some(PathBuf::from("/work/spatio-flux/.")));
        // A package with no own native crate.
        let plain = Manifest::parse("def package = { name: 'p' }", "/p").unwrap();
        assert_eq!(plain.native, None);
        assert_eq!(plain.native_dir(), None);
    }

    #[test]
    fn add_dependency_edits_the_manifest_through_the_round_trip() {
        // Adding to a manifest with no `dependencies` yet — `set_path` creates it.
        let src = "def package = { name: 'app' }\n";
        let edited = add_dependency(
            src,
            "foo",
            &DependencySource::Path(PathBuf::from("../foo")),
            Some("^1.0"),
        )
        .unwrap();
        let m = Manifest::parse(&edited, "/app").unwrap();
        assert_eq!(m.dependencies.len(), 1);
        assert_eq!(m.dependencies[0].name, "foo");
        assert_eq!(
            m.dependencies[0].source,
            DependencySource::Path(PathBuf::from("../foo"))
        );
        assert!(m.dependencies[0].req.matches(&Version::new(1, 5, 0)));

        // Adding a SECOND dep (a native one) to the now-populated manifest.
        let edited2 = add_dependency(
            &edited,
            "bar",
            &DependencySource::Native(PathBuf::from("../bar")),
            None,
        )
        .unwrap();
        let m2 = Manifest::parse(&edited2, "/app").unwrap();
        assert_eq!(m2.dependencies.len(), 2);
        assert!(m2.dependencies.iter().find(|d| d.name == "bar").unwrap().source.is_native());
    }

    #[test]
    fn remove_dependency_round_trips() {
        let src = "def package = { name: 'app', dependencies: { foo: { path: '../foo' }, bar: { path: '../bar' } } }\n";
        let edited = remove_dependency(src, "foo").unwrap();
        let m = Manifest::parse(&edited, "/app").unwrap();
        assert_eq!(m.dependencies.len(), 1);
        assert_eq!(m.dependencies[0].name, "bar");
        // Removing a dependency that isn't there errors.
        assert!(remove_dependency(&edited, "ghost").is_err());
    }

    #[test]
    fn add_dependency_preserves_header_and_other_fields() {
        let src = "# my project\ndef package = { name: 'app', version: '0.1.0' }\n";
        let edited =
            add_dependency(src, "foo", &DependencySource::Path(PathBuf::from("../foo")), None)
                .unwrap();
        assert!(edited.contains("# my project"), "the header is preserved");
        let m = Manifest::parse(&edited, "/app").unwrap();
        assert_eq!(m.name, "app");
        assert_eq!(m.version, Some(Version::new(0, 1, 0)));
        assert_eq!(m.dependencies.len(), 1);
    }
}
