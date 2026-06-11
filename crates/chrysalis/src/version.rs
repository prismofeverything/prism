//! `version.rs` — a small, owned **semver** for the package layer (#67, Phase 2).
//!
//! Dependency-light by design (no `semver` crate): a package manager's version logic
//! is load-bearing, so the package layer owns a focused, fully-tested subset —
//! `major.minor.patch` versions + the common requirement operators. **Cargo
//! semantics:** a bare `1.2.3` requirement means `^1.2.3` (caret). If the full semver
//! spec (pre-release tags, build metadata, comma-AND ranges) is ever needed, swap to
//! the `semver` crate behind this same `Version` / `VersionReq` interface.
//!
//! Semver is what makes the registry a **poset over `name × version`** (#67): the
//! resolver picks one version per name so the dependency colimit is well-defined, and
//! a `VersionReq` is the **compatibility functor** — patch/minor preserve a theory's
//! morphisms, a major may break them.

use std::fmt;
use std::str::FromStr;

/// A `major.minor.patch` version. The derived order is the correct semver precedence
/// for the release subset (no pre-release tags).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    pub fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self { major, minor, patch }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl FromStr for Version {
    type Err = String;
    /// Parse `1`, `1.2`, or `1.2.3` (missing trailing components are 0).
    fn from_str(s: &str) -> Result<Self, String> {
        let (v, _components) = parse_version_with_components(s.trim())?;
        Ok(v)
    }
}

/// Parse a version, returning it plus how many components were written (1/2/3) — the
/// component count drives caret/tilde upper bounds.
fn parse_version_with_components(s: &str) -> Result<(Version, u8), String> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.is_empty() || parts.len() > 3 || parts.iter().any(|p| p.is_empty()) {
        return Err(format!(
            "invalid version `{s}` (expected `major[.minor[.patch]]`)"
        ));
    }
    let component = |p: &str| -> Result<u64, String> {
        p.parse::<u64>()
            .map_err(|_| format!("invalid version `{s}`: `{p}` is not a number"))
    };
    let major = component(parts[0])?;
    let minor = parts.get(1).map(|p| component(p)).transpose()?.unwrap_or(0);
    let patch = parts.get(2).map(|p| component(p)).transpose()?.unwrap_or(0);
    Ok((Version { major, minor, patch }, parts.len() as u8))
}

/// The requirement operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    /// `*` — any version.
    Any,
    /// `=x.y.z` — exactly this version.
    Exact,
    /// `^x.y.z` (and bare `x.y.z`) — compatible: up to the next breaking change.
    Caret,
    /// `~x.y.z` — patch-level changes within the written minor.
    Tilde,
    Gte,
    Gt,
    Lte,
    Lt,
}

/// A version **requirement** — the compatibility functor over the version poset.
/// A single operator (Cargo's comma-AND of multiple comparators is a deliberate
/// later extension; one requirement covers the package-dependency cases).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionReq {
    op: Op,
    version: Version,
    /// Components written in the requirement's version (1/2/3) — needed for tilde.
    components: u8,
    /// The original text, for display + the lockfile.
    raw: String,
}

impl VersionReq {
    /// `*` — matches any version (the default when a dependency declares no version).
    pub fn any() -> Self {
        VersionReq {
            op: Op::Any,
            version: Version::new(0, 0, 0),
            components: 3,
            raw: "*".to_string(),
        }
    }

    /// Does `version` satisfy this requirement?
    pub fn matches(&self, version: &Version) -> bool {
        match self.op {
            Op::Any => true,
            Op::Exact => *version == self.version,
            Op::Gte => *version >= self.version,
            Op::Gt => *version > self.version,
            Op::Lte => *version <= self.version,
            Op::Lt => *version < self.version,
            Op::Caret => *version >= self.version && *version < self.caret_upper(),
            Op::Tilde => *version >= self.version && *version < self.tilde_upper(),
        }
    }

    /// The exclusive upper bound of a caret requirement — the next version that may
    /// break compatibility. Driven by the FIRST non-zero component (standard caret):
    /// `^1.2.3 → <2.0.0`, `^0.2.3 → <0.3.0`, `^0.0.3 → <0.0.4`.
    fn caret_upper(&self) -> Version {
        let v = self.version;
        if v.major > 0 {
            Version::new(v.major + 1, 0, 0)
        } else if v.minor > 0 {
            Version::new(0, v.minor + 1, 0)
        } else {
            Version::new(0, 0, v.patch + 1)
        }
    }

    /// The exclusive upper bound of a tilde requirement — patch-level within the
    /// written minor: `~1.2.3 → <1.3.0`, `~1.2 → <1.3.0`, `~1 → <2.0.0`.
    fn tilde_upper(&self) -> Version {
        let v = self.version;
        if self.components >= 2 {
            Version::new(v.major, v.minor + 1, 0)
        } else {
            Version::new(v.major + 1, 0, 0)
        }
    }

    /// The original requirement text (for display + the lockfile).
    pub fn as_str(&self) -> &str {
        &self.raw
    }
}

impl fmt::Display for VersionReq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

impl FromStr for VersionReq {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let raw = s.trim().to_string();
        if raw == "*" {
            return Ok(VersionReq::any());
        }
        // Split a leading operator from the version body.
        let (op, body) = if let Some(rest) = raw.strip_prefix(">=") {
            (Op::Gte, rest)
        } else if let Some(rest) = raw.strip_prefix("<=") {
            (Op::Lte, rest)
        } else if let Some(rest) = raw.strip_prefix('>') {
            (Op::Gt, rest)
        } else if let Some(rest) = raw.strip_prefix('<') {
            (Op::Lt, rest)
        } else if let Some(rest) = raw.strip_prefix('^') {
            (Op::Caret, rest)
        } else if let Some(rest) = raw.strip_prefix('~') {
            (Op::Tilde, rest)
        } else if let Some(rest) = raw.strip_prefix('=') {
            (Op::Exact, rest)
        } else {
            // Bare `1.2.3` ⇒ caret (Cargo semantics).
            (Op::Caret, raw.as_str())
        };
        let (version, components) = parse_version_with_components(body.trim())?;
        Ok(VersionReq {
            op,
            version,
            components,
            raw,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        s.parse().unwrap()
    }
    fn req(s: &str) -> VersionReq {
        s.parse().unwrap()
    }

    #[test]
    fn parses_and_orders_versions() {
        assert_eq!(v("1.2.3"), Version::new(1, 2, 3));
        assert_eq!(v("1.2"), Version::new(1, 2, 0));
        assert_eq!(v("1"), Version::new(1, 0, 0));
        assert!(v("1.2.3") < v("1.2.4"));
        assert!(v("1.2.0") < v("1.3.0"));
        assert!(v("1.9.9") < v("2.0.0"));
        assert_eq!(v("1.2.3").to_string(), "1.2.3");
    }

    #[test]
    fn rejects_bad_versions() {
        assert!("1.2.3.4".parse::<Version>().is_err());
        assert!("1..2".parse::<Version>().is_err());
        assert!("1.x".parse::<Version>().is_err());
        assert!("".parse::<Version>().is_err());
    }

    #[test]
    fn caret_is_compatible_up_to_the_next_breaking() {
        let r = req("^1.2.3");
        assert!(r.matches(&v("1.2.3")));
        assert!(r.matches(&v("1.2.9")));
        assert!(r.matches(&v("1.9.0")));
        assert!(!r.matches(&v("1.2.2"))); // below lower bound
        assert!(!r.matches(&v("2.0.0"))); // breaking
    }

    #[test]
    fn caret_zero_versions_treat_the_first_nonzero_as_breaking() {
        // ^0.2.3 → >=0.2.3, <0.3.0
        let r = req("^0.2.3");
        assert!(r.matches(&v("0.2.3")));
        assert!(r.matches(&v("0.2.9")));
        assert!(!r.matches(&v("0.3.0")));
        // ^0.0.3 → >=0.0.3, <0.0.4
        let r = req("^0.0.3");
        assert!(r.matches(&v("0.0.3")));
        assert!(!r.matches(&v("0.0.4")));
    }

    #[test]
    fn bare_version_is_caret() {
        let bare = req("1.2.3");
        let caret = req("^1.2.3");
        assert!(bare.matches(&v("1.5.0")));
        assert!(!bare.matches(&v("2.0.0")));
        assert_eq!(bare.matches(&v("1.5.0")), caret.matches(&v("1.5.0")));
    }

    #[test]
    fn tilde_is_patch_level_within_the_written_minor() {
        // ~1.2.3 → >=1.2.3, <1.3.0
        let r = req("~1.2.3");
        assert!(r.matches(&v("1.2.3")));
        assert!(r.matches(&v("1.2.9")));
        assert!(!r.matches(&v("1.3.0")));
        // ~1 (major only) → >=1.0.0, <2.0.0
        let r = req("~1");
        assert!(r.matches(&v("1.9.9")));
        assert!(!r.matches(&v("2.0.0")));
    }

    #[test]
    fn exact_and_comparators() {
        assert!(req("=1.2.3").matches(&v("1.2.3")));
        assert!(!req("=1.2.3").matches(&v("1.2.4")));
        assert!(req(">=1.0.0").matches(&v("9.9.9")));
        assert!(!req(">=1.0.0").matches(&v("0.9.9")));
        assert!(req(">1.0.0").matches(&v("1.0.1")));
        assert!(!req(">1.0.0").matches(&v("1.0.0")));
        assert!(req("<2.0.0").matches(&v("1.9.9")));
        assert!(!req("<2.0.0").matches(&v("2.0.0")));
        assert!(req("<=1.2.3").matches(&v("1.2.3")));
    }

    #[test]
    fn any_matches_everything() {
        assert!(req("*").matches(&v("0.0.1")));
        assert!(VersionReq::any().matches(&v("99.0.0")));
    }

    #[test]
    fn requirement_round_trips_its_text() {
        assert_eq!(req("^1.2.3").as_str(), "^1.2.3");
        assert_eq!(req(">=1.0.0").to_string(), ">=1.0.0");
    }
}
