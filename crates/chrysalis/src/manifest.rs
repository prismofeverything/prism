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

use crate::ast::{Def, Expr};
use crate::parse::parse_program;
use crate::version::{Version, VersionReq};

/// The binding that holds the manifest record (`def package = { … }`).
pub const MANIFEST_BINDING: &str = "package";
/// The manifest file name, walked-for up the directory tree.
pub const MANIFEST_FILE: &str = "project.ys";

/// A structured package manifest — the package's identity + its dependency edges.
#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    pub name: String,
    /// The package's own version (the object's identity in the registry poset).
    pub version: Option<Version>,
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
}

impl DependencySource {
    /// The declared path (a `.ys` package dir for `Path`, a crate dir for `Native`).
    pub fn path(&self) -> &Path {
        match self {
            DependencySource::Path(p) | DependencySource::Native(p) => p,
        }
    }

    /// Is this a native (Rust crate) dependency?
    pub fn is_native(&self) -> bool {
        matches!(self, DependencySource::Native(_))
    }
}

impl Dependency {
    /// The dependency's directory, resolved against `manifest_dir` (a `.ys` package
    /// dir for a path dep, a crate dir for a native dep).
    pub fn resolved_dir(&self, manifest_dir: &Path) -> PathBuf {
        let p = self.source.path();
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            manifest_dir.join(p)
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
            dependencies,
            exports,
            dir: dir.as_ref().to_path_buf(),
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
            return Err(format!(
                "dependency `{name}` needs a `path: '...'` (a `.ys` package) or \
                 `native: '...'` (a Rust crate)"
            ));
        };
        deps.push(Dependency {
            name: name.clone(),
            source,
            req,
        });
    }
    Ok(deps)
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
    fn a_dependency_without_a_source_is_an_error() {
        // A dep needs a `path:` (a `.ys` package) or `native:` (a Rust crate).
        let err = Manifest::parse(
            "def package = { name: 'p', dependencies: { d: { version: '1.0.0' } } }",
            "/p",
        )
        .unwrap_err();
        assert!(err.contains("path") && err.contains("native"), "got: {err}");
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
}
