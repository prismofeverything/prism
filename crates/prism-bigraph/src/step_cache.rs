//! Incremental step execution — a persistent cache of step OUTPUTS keyed by node
//! name, so a workflow's steps can be SKIPPED when their result is already on disk
//! and selectively re-run. This is "step triggers in general": the engine decides,
//! per step, whether to fire it or reload its prior output, and the step DAG
//! propagates staleness downstream (a stale step makes its dependents stale).
//! chrysalis surfaces it as `chrysalis run f.ys --steps a,b` (force) plus the
//! on-disk artifact model — "this file is on disk, don't regenerate."
//!
//! Increment 1 (this): persistence + presence-freshness (a cache entry older than
//! the workflow source is stale — the coarse, make-style rule) + explicit force +
//! DAG cascade. Increment 2 (planned, the "step triggers in general" revisit):
//! per-step content fingerprints for *granular* auto-invalidation — edit one step
//! and only it + its dependents recompute — replacing the coarse source-mtime rule.
//!
//! Serialization is `serde_json` over [`Value`] (its own lossless round-trip),
//! encapsulated here so it can later become schema-directed (`algebra::serialize`)
//! if type-exactness across units / int↔float ever needs the schema.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

use prism_schema::Value;

/// A per-workflow step-output cache rooted at `dir`. A step is FRESH (skippable)
/// when its cache file exists and is newer than `source_mtime` and it is not in
/// `forced`. Steps DOWNSTREAM of a stale/forced step are also stale (cascade) —
/// the engine computes that from the step DAG, not this struct.
#[derive(Debug, Clone)]
pub struct StepCache {
    dir: PathBuf,
    /// Modification time of the workflow source; a cache entry older than this is
    /// stale (the coarse, make-style invalidation of increment 1). `None` disables
    /// the time check (presence-only) — used by unit tests.
    source_mtime: Option<SystemTime>,
    /// Steps the caller forced to recompute (`--steps`). Their dependents cascade.
    forced: HashSet<String>,
}

impl StepCache {
    /// A cache rooted at `dir`. `source_mtime` (the workflow source's mtime, if
    /// any) invalidates entries older than it; `forced` are steps to recompute
    /// regardless (their dependents cascade, decided by the engine).
    pub fn new(
        dir: impl Into<PathBuf>,
        source_mtime: Option<SystemTime>,
        forced: HashSet<String>,
    ) -> Self {
        Self {
            dir: dir.into(),
            source_mtime,
            forced,
        }
    }

    /// The cache file for a step (its node name sanitised to a safe filename).
    fn entry_path(&self, step: &str) -> PathBuf {
        let safe: String = step
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.dir.join(format!("{safe}.json"))
    }

    /// Was this step explicitly forced to recompute (`--steps`)?
    pub fn is_forced(&self, step: &str) -> bool {
        self.forced.contains(step)
    }

    /// Is this step's cached output present and up to date (newer than the source)?
    /// Forcing is handled separately, by [`StepCache::is_forced`].
    pub fn is_fresh(&self, step: &str) -> bool {
        let Ok(meta) = fs::metadata(self.entry_path(step)) else {
            return false;
        };
        match self.source_mtime {
            None => true,
            Some(src) => meta.modified().map(|m| m >= src).unwrap_or(false),
        }
    }

    /// Load a step's cached output value, if present and decodable.
    pub fn load(&self, step: &str) -> Option<Value> {
        let bytes = fs::read(self.entry_path(step)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Parse a cache directive carried in a composite's `config.cache`:
    /// `{ dir: <path>, forced: [<step>, …] }`. Returns `None` when no `dir` is
    /// given (the composite simply runs without caching). This is how the cache
    /// crosses the composite boundary — as DATA in config, never a Rust handle —
    /// so each composite caches its OWN step network (and a remote composite gets
    /// the same directive over the wire). `source_mtime` is `None` here
    /// (presence-based freshness); granular per-step fingerprints are future work.
    pub fn from_config(value: &Value) -> Option<StepCache> {
        let map = value.as_map()?;
        let dir = map.get("dir").and_then(|v| v.as_str())?;
        let forced = match map.get("forced") {
            Some(Value::List(items)) => items
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
            _ => HashSet::new(),
        };
        Some(StepCache::new(dir, None, forced))
    }

    /// Persist a step's output value (creating the cache dir on first write).
    /// Best-effort: a write failure just means this step won't be cached.
    pub fn store(&self, step: &str, value: &Value) {
        if fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        if let Ok(bytes) = serde_json::to_vec_pretty(value) {
            let _ = fs::write(self.entry_path(step), bytes);
        }
    }
}
