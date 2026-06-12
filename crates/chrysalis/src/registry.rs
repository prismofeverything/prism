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

use prism_bigraph::protocols::registry as transport;
use prism_schema::{Key, StateMap, Value};

use crate::manifest::Manifest;
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

/// **Publish** the package at `package_dir` INTO the local registry rooted at
/// `registry_root` (#67 Phase 4): read its manifest (name + version) and copy its source
/// to `<registry_root>/<name>/<version>/`. A published version is **immutable** — refuses
/// to overwrite an existing one unless `force` (a registry's reproducibility guarantee).
/// Returns the published `(version, destination dir)`. (The REMOTE backend in P4b uploads
/// over the mesh transport instead of copying; same shape, pluggable.)
pub fn publish(
    package_dir: &Path,
    registry_root: &Path,
    force: bool,
) -> Result<(Version, PathBuf), String> {
    let manifest = Manifest::load(package_dir)?;
    let version = manifest.version.ok_or_else(|| {
        format!(
            "{}/{} has no `version` — a published package must declare one",
            package_dir.display(),
            crate::manifest::MANIFEST_FILE
        )
    })?;
    let dest = registry_root.join(&manifest.name).join(version.to_string());
    if dest.exists() {
        if !force {
            return Err(format!(
                "`{}@{}` is already published (a registry version is immutable; use --force to \
                 overwrite)",
                manifest.name, version
            ));
        }
        std::fs::remove_dir_all(&dest).map_err(|e| format!("clear {}: {e}", dest.display()))?;
    }
    copy_package(package_dir, &dest)?;
    Ok((version, dest))
}

/// The package-source exclusion policy — ONE place, shared by [`copy_package`] (the local
/// publish) and [`pack_package`] (the remote publish): `project.lock` (the consumer
/// regenerates it), `target/` (build artifacts), and hidden files are NOT package source.
fn is_excluded(name: &str) -> bool {
    name == "project.lock" || name == "target" || name.starts_with('.')
}

/// Recursively copy a package's SOURCE into `dest`, excluding the build/lock artifacts a
/// consumer regenerates ([`is_excluded`]) — not package source.
fn copy_package(src: &Path, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| format!("create {}: {e}", dest.display()))?;
    let entries = std::fs::read_dir(src).map_err(|e| format!("read {}: {e}", src.display()))?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if is_excluded(&name_str) {
            continue;
        }
        let from = src.join(&name);
        let to = dest.join(&name);
        if from.is_dir() {
            copy_package(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)
                .map_err(|e| format!("copy {} → {}: {e}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

// ── remote registry (#67 Phase 4b) ───────────────────────────────────────────────
//
// The transport (`prism_bigraph::protocols::registry`, mesh's) moves a package as a
// wire `Value` — `map[relpath → file-string]`, the shape this packer/unpacker owns and
// the transport codecs + checksums. `RemoteRegistry` fetches that Value and writes it to
// a disk cache, returning a dir the resolver loads exactly like a path dep — so the
// `Registry` trait is UNCHANGED and a remote dep resolves identically to a local one.

/// **Pack** a package directory into the wire `Value` (`map[relpath → file-string]`):
/// every source file (excluding the build/lock artifacts of [`is_excluded`]) keyed by its
/// `/`-separated path relative to `dir`. The inverse of [`unpack_package`]; the payload
/// `publish_remote` uploads. Paths use `/` regardless of host so a package packed on one
/// OS unpacks faithfully on another.
pub fn pack_package(dir: &Path) -> Result<Value, String> {
    let mut files = StateMap::new();
    pack_into(dir, dir, &mut files)?;
    Ok(Value::Map(files))
}

fn pack_into(root: &Path, cur: &Path, out: &mut StateMap) -> Result<(), String> {
    let entries = std::fs::read_dir(cur).map_err(|e| format!("read {}: {e}", cur.display()))?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if is_excluded(&name.to_string_lossy()) {
            continue;
        }
        let path = cur.join(&name);
        if path.is_dir() {
            pack_into(root, &path, out)?;
        } else {
            let rel = path
                .strip_prefix(root)
                .map_err(|e| format!("relativize {}: {e}", path.display()))?
                .to_string_lossy()
                .replace('\\', "/");
            let content =
                std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
            out.insert(Key::from(rel.as_str()), Value::String(content));
        }
    }
    Ok(())
}

/// **Unpack** a package `Value` (`map[relpath → file-string]`) into `dest`, writing each
/// file and creating parent dirs. The inverse of [`pack_package`]; how a fetched remote
/// package lands in the resolver's cache so it loads like a path dep.
pub fn unpack_package(pkg: &Value, dest: &Path) -> Result<(), String> {
    let Value::Map(files) = pkg else {
        return Err(format!("a package must be a map[path → file-string], got {pkg:?}"));
    };
    for (rel, content) in files.iter() {
        let Value::String(text) = content else {
            return Err(format!(
                "package entry `{rel}` must be a string (file contents), got {content:?}"
            ));
        };
        let path = dest.join(rel.as_str());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create {}: {e}", parent.display()))?;
        }
        std::fs::write(&path, text).map_err(|e| format!("write {}: {e}", path.display()))?;
    }
    Ok(())
}

/// A **REMOTE** registry over mesh's HTTP transport (`prism_bigraph::protocols::registry`):
/// the same `Registry` trait, backed by the wire instead of a local dir. `versions` is a
/// GET; `source` fetches the package `Value` (checksum-verified on the wire by the
/// transport), unpacks it under `cache/<name>/<version>/`, and returns that dir — which the
/// resolver loads exactly like a path dependency. A fetched version is cached, so a diamond
/// or a re-resolve hits the disk, not the network.
pub struct RemoteRegistry {
    base_url: String,
    cache: PathBuf,
}

impl RemoteRegistry {
    pub fn new(base_url: impl Into<String>, cache: impl Into<PathBuf>) -> Self {
        RemoteRegistry { base_url: base_url.into(), cache: cache.into() }
    }
}

impl Registry for RemoteRegistry {
    fn versions(&self, name: &str) -> Result<Vec<Version>, String> {
        let strings = transport::versions(&self.base_url, name).map_err(|e| e.to_string())?;
        strings
            .iter()
            .map(|s| {
                s.parse::<Version>().map_err(|e| {
                    format!("registry returned an unparseable version `{s}` for `{name}`: {e}")
                })
            })
            .collect()
    }

    fn source(&self, name: &str, version: &Version) -> Result<PathBuf, String> {
        let dest = self.cache.join(name).join(version.to_string());
        // Cached already? A fetched package is immutable, so the cached dir is canonical.
        if dest.join(crate::manifest::MANIFEST_FILE).is_file() {
            return Ok(dest);
        }
        let pkg = transport::fetch(&self.base_url, name, &version.to_string())
            .map_err(|e| format!("fetching `{name}@{version}`: {e}"))?;
        unpack_package(&pkg, &dest)?;
        Ok(dest)
    }
}

/// **Publish** the package at `package_dir` to a REMOTE registry at `base_url` (#67 P4b):
/// pack its source ([`pack_package`]) and upload over mesh's transport. The remote dual of
/// [`publish`] — but a remote version is *always* immutable (the server refuses a re-publish;
/// there is no `--force`, so a published theory never silently changes under a consumer).
/// Returns the published version.
pub fn publish_remote(package_dir: &Path, base_url: &str) -> Result<Version, String> {
    let manifest = Manifest::load(package_dir)?;
    let version = manifest.version.ok_or_else(|| {
        format!(
            "{}/{} has no `version` — a published package must declare one",
            package_dir.display(),
            crate::manifest::MANIFEST_FILE
        )
    })?;
    let pkg = pack_package(package_dir)?;
    transport::publish(base_url, &manifest.name, &version.to_string(), &pkg)
        .map_err(|e| format!("publishing `{}@{}` to {base_url}: {e}", manifest.name, version))?;
    Ok(version)
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

    #[test]
    fn publish_copies_a_package_into_the_registry_immutably() {
        let root = std::env::temp_dir().join(format!("reg-publish-{}", std::process::id()));
        let pkg = root.join("mypkg");
        std::fs::create_dir_all(pkg.join("ys")).unwrap();
        std::fs::write(
            pkg.join("project.ys"),
            "def package = { name: 'mypkg', version: '0.3.0', exports: ['Foo'] }\n",
        )
        .unwrap();
        std::fs::write(pkg.join("lib.ys"), "process Foo ~{n :: Float} ->{n :: Float} (\n  {n: 1.0}\n)\n").unwrap();
        std::fs::write(pkg.join("ys").join("demo.ys"), "Foo\n").unwrap(); // a subdir source file
        std::fs::write(pkg.join("project.lock"), "def lock = {}\n").unwrap(); // must be EXCLUDED

        let reg = root.join("registry");
        let (version, dest) = publish(&pkg, &reg, false).expect("publish");
        assert_eq!(version, Version::new(0, 3, 0));
        assert_eq!(dest, reg.join("mypkg").join("0.3.0"));
        // Source copied (project.ys + lib.ys + ys/demo.ys); the lock is EXCLUDED.
        assert!(dest.join("project.ys").is_file());
        assert!(dest.join("lib.ys").is_file());
        assert!(dest.join("ys").join("demo.ys").is_file());
        assert!(!dest.join("project.lock").exists(), "the lock is excluded from publish");

        // The registry now resolves the published package.
        let registry = LocalRegistry::new(&reg);
        let (v, source) = registry.resolve("mypkg", &"^0.3".parse().unwrap()).unwrap();
        assert_eq!(v, Version::new(0, 3, 0));
        assert_eq!(source, dest);

        // A published version is IMMUTABLE — re-publishing is refused unless forced.
        assert!(publish(&pkg, &reg, false).unwrap_err().contains("immutable"));
        assert!(publish(&pkg, &reg, true).is_ok(), "--force overwrites");

        std::fs::remove_dir_all(&root).ok();
    }

    /// Build a minimal package dir (with a `ys/` subdir source file + a lock to exclude).
    fn fixture_package(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pkg-pack-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("ys")).unwrap();
        std::fs::write(
            dir.join("project.ys"),
            "def package = { name: 'greet', version: '1.0.0', exports: ['Greet'] }\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("lib.ys"),
            "process Greet ~{n :: Float} ->{n :: Float} (\n  {n: 1.0}\n)\n",
        )
        .unwrap();
        std::fs::write(dir.join("ys").join("demo.ys"), "Greet\n").unwrap();
        std::fs::write(dir.join("project.lock"), "def lock = {}\n").unwrap(); // EXCLUDED
        dir
    }

    #[test]
    fn pack_then_unpack_round_trips_a_package() {
        let src = fixture_package("rt");
        let pkg = pack_package(&src).expect("pack");

        // The wire shape: map[relpath → file-string], `/`-separated, lock EXCLUDED.
        let Value::Map(files) = &pkg else { panic!("a package is a map, got {pkg:?}") };
        assert!(files.contains_key(&Key::from("project.ys")));
        assert!(files.contains_key(&Key::from("lib.ys")));
        assert!(files.contains_key(&Key::from("ys/demo.ys")), "nested path is `/`-flattened");
        assert!(!files.contains_key(&Key::from("project.lock")), "the lock is not package source");

        // Unpack to a fresh dir → the source files reappear (the lock does not).
        let dest = std::env::temp_dir().join(format!("pkg-unpack-{}", std::process::id()));
        std::fs::remove_dir_all(&dest).ok();
        unpack_package(&pkg, &dest).expect("unpack");
        assert_eq!(
            std::fs::read_to_string(dest.join("project.ys")).unwrap(),
            std::fs::read_to_string(src.join("project.ys")).unwrap()
        );
        assert!(dest.join("ys").join("demo.ys").is_file(), "the subdir is recreated");
        assert!(!dest.join("project.lock").exists());

        std::fs::remove_dir_all(&src).ok();
        std::fs::remove_dir_all(&dest).ok();
    }

    #[test]
    fn remote_registry_publishes_and_fetches_over_the_transport() {
        use prism_bigraph::protocols::registry::RegistryServer;

        let server = RegistryServer::start().expect("start registry server");
        let base_url = server.base_url();
        let src = fixture_package("remote");

        // Publish the package over the wire, then resolve it back through a RemoteRegistry.
        let version = publish_remote(&src, &base_url).expect("publish_remote");
        assert_eq!(version, Version::new(1, 0, 0));

        let cache = std::env::temp_dir().join(format!("pkg-remote-cache-{}", std::process::id()));
        std::fs::remove_dir_all(&cache).ok();
        let reg = RemoteRegistry::new(&base_url, &cache);

        // versions() hits the transport; an unknown package is an EMPTY set (not an error),
        // matching LocalRegistry.
        assert_eq!(reg.versions("greet").unwrap(), vec![Version::new(1, 0, 0)]);
        assert!(reg.versions("ghost").unwrap().is_empty());

        // resolve() (the shared version solver) fetches + caches the source dir; it loads
        // like a path dep (its project.ys is present), and the cache is reused on re-fetch.
        let (v, dir) = reg.resolve("greet", &"^1.0".parse().unwrap()).expect("resolve");
        assert_eq!(v, Version::new(1, 0, 0));
        assert!(dir.join(crate::manifest::MANIFEST_FILE).is_file());
        assert!(dir.join("ys").join("demo.ys").is_file(), "the nested source survives the wire");
        assert!(reg.source("greet", &v).is_ok(), "a cached fetch is idempotent");

        // The remote registry is IMMUTABLE — re-publishing the same version is refused.
        assert!(publish_remote(&src, &base_url).is_err(), "a published version is immutable");

        std::fs::remove_dir_all(&src).ok();
        std::fs::remove_dir_all(&cache).ok();
    }
}
