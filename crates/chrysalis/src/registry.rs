//! `registry.rs` — a package **registry** (#67, Phase 3): resolve a name + a version
//! requirement to a concrete package.
//!
//! The registry is the **poset over `name × version`** the categorical model names
//! (`docs/packages-ecosystem.md`): a `Registry::resolve` picks the highest version
//! satisfying a [`VersionReq`] (the compatibility functor), so the dependency colimit
//! is well-defined. It is a **pluggable backend** — a LOCAL dir-based index now
//! (Phase 3), a remote HTTP backend later (Phase 4, the mesh-transport generalization)
//! — behind one trait, so `chrysalis add` / the resolver never care which.

use std::path::{Path, PathBuf};

use crate::version::{Version, VersionReq};

/// A package registry. `resolve` (the version solver) is provided in terms of the two
/// primitives a backend supplies: the versions of a name, and the source of a specific
/// `name@version`.
pub trait Registry {
    /// All versions available for `name` (unordered; empty ⇒ the registry has no such
    /// package).
    fn versions(&self, name: &str) -> Result<Vec<Version>, String>;

    /// The source directory of a SPECIFIC `name@version` — a package dir the resolver
    /// loads exactly like a path dependency (`project.ys` + `lib.ys`).
    fn source(&self, name: &str, version: &Version) -> Result<PathBuf, String>;

    /// Resolve `name` + `req` to the HIGHEST available version satisfying `req`, with
    /// its source dir. The version solver against this registry's poset (picking the
    /// max keeps resolution deterministic and prefers the newest compatible theory).
    fn resolve(&self, name: &str, req: &VersionReq) -> Result<(Version, PathBuf), String> {
        let mut versions = self.versions(name)?;
        if versions.is_empty() {
            return Err(format!("package `{name}` not found in the registry"));
        }
        versions.sort();
        let chosen = versions
            .into_iter()
            .rev() // highest first
            .find(|v| req.matches(v))
            .ok_or_else(|| format!("no version of `{name}` satisfies `{req}`"))?;
        let source = self.source(name, &chosen)?;
        Ok((chosen, source))
    }
}

/// A LOCAL dir-based registry: `<root>/<name>/<version>/` is a package (holding its
/// `project.ys` + `lib.ys`); the versions of a name are the `<version>` subdir names.
/// The simplest backend — no index file to maintain, the directory structure IS the
/// `name → versions` index.
pub struct LocalRegistry {
    root: PathBuf,
}

impl LocalRegistry {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        LocalRegistry { root: root.into() }
    }

    /// The registry root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl Registry for LocalRegistry {
    fn versions(&self, name: &str) -> Result<Vec<Version>, String> {
        let dir = self.root.join(name);
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(format!("read registry dir {}: {e}", dir.display())),
        };
        let mut versions = Vec::new();
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                // A subdir whose name parses as a version is a published version;
                // anything else (a stray file/dir) is ignored.
                if let Some(parsed) = entry
                    .file_name()
                    .to_str()
                    .and_then(|n| n.parse::<Version>().ok())
                {
                    versions.push(parsed);
                }
            }
        }
        Ok(versions)
    }

    fn source(&self, name: &str, version: &Version) -> Result<PathBuf, String> {
        let dir = self.root.join(name).join(version.to_string());
        if dir.join(crate::manifest::MANIFEST_FILE).is_file() {
            Ok(dir)
        } else {
            Err(format!(
                "`{name}@{version}` is not in the registry (no {}/{name}/{version}/{})",
                self.root.display(),
                crate::manifest::MANIFEST_FILE
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a fixture registry with `foo` at 1.0.0, 1.2.0, 2.0.0 (each a minimal
    /// package dir with a `project.ys`).
    fn fixture_registry(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("reg-test-{tag}-{}", std::process::id()));
        for v in ["1.0.0", "1.2.0", "2.0.0"] {
            let dir = root.join("foo").join(v);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join("project.ys"),
                format!("def package = {{ name: 'foo', version: '{v}' }}\n"),
            )
            .unwrap();
        }
        root
    }

    #[test]
    fn lists_a_packages_versions() {
        let root = fixture_registry("versions");
        let reg = LocalRegistry::new(&root);
        let mut versions = reg.versions("foo").unwrap();
        versions.sort();
        assert_eq!(
            versions,
            vec![Version::new(1, 0, 0), Version::new(1, 2, 0), Version::new(2, 0, 0)]
        );
        assert!(reg.versions("nope").unwrap().is_empty());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn resolve_picks_the_highest_satisfying_version() {
        let root = fixture_registry("resolve");
        let reg = LocalRegistry::new(&root);

        // `^1.0` → the highest 1.x (1.2.0, NOT 2.0.0).
        let (v, dir) = reg.resolve("foo", &"^1.0".parse().unwrap()).unwrap();
        assert_eq!(v, Version::new(1, 2, 0));
        assert_eq!(dir, root.join("foo").join("1.2.0"));

        // `*` → the highest overall (2.0.0).
        let (v, _) = reg.resolve("foo", &"*".parse().unwrap()).unwrap();
        assert_eq!(v, Version::new(2, 0, 0));

        // `=1.0.0` → exactly 1.0.0.
        let (v, _) = reg.resolve("foo", &"=1.0.0".parse().unwrap()).unwrap();
        assert_eq!(v, Version::new(1, 0, 0));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn resolve_errors_on_unknown_package_or_unsatisfiable_req() {
        let root = fixture_registry("err");
        let reg = LocalRegistry::new(&root);
        assert!(reg.resolve("ghost", &"*".parse().unwrap()).unwrap_err().contains("not found"));
        assert!(reg
            .resolve("foo", &"^3.0".parse().unwrap())
            .unwrap_err()
            .contains("satisfies"));
        std::fs::remove_dir_all(&root).ok();
    }
}
