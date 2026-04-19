//! Bounded-context resolution for the cfdb workspace — shared library.
//!
//! This crate is the SINGLE canonical resolver of `(crate name) ->
//! bounded context`. Two layers:
//!
//! 1. Optional `.cfdb/concepts/<context>.toml` overrides read at the start of
//!    `extract_workspace` (or any other consumer). Each TOML file declares a
//!    context name, an optional `canonical_crate`, an optional `owning_rfc`,
//!    and an explicit `crates` list. The reverse map (crate name -> context
//!    name + metadata) WINS over the heuristic.
//! 2. A fallback crate-prefix heuristic for crates the overrides do not name.
//!    Well-known prefixes (`domain-`, `ports-`, `adapters-`, `application-`,
//!    `use-cases-`, `qbot-`) are stripped and the remainder becomes the
//!    bounded-context string. Crates with no known prefix return their full
//!    name unchanged so the self-workspace regression (`cfdb-core`,
//!    `cfdb-extractor`, ...) produces deterministic output.
//!
//! Determinism: everything here uses `BTreeMap` / sorted `Vec<PathBuf>` so
//! two runs on the same inputs emit byte-identical facts — RFC-029 §12.1 G1.
//!
//! # Origin
//!
//! Originally `cfdb-extractor/src/context.rs`; extracted into this dedicated
//! crate per Issue #3 (council-ratified #3841 doctrine). The extraction exists
//! because multiple consumers (`cfdb-extractor` and the future `cfdb-query`
//! DSL evaluator's `ContextMap` type) need the same loader, and a shared
//! crate is the Rust-level implementation of the Conformist pattern.
//!
//! Dependency discipline: zero heavy deps. No `syn`, no `cargo_metadata`,
//! no `ra-ap-hir`. Pure TOML + serde.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Ordered list of crate-name prefixes stripped by the heuristic. Order
/// matters: `use-cases-` must be checked before the empty string would
/// fall through. Longest distinctive prefix first is the safest rule of
/// thumb but alphabetical is fine here because none of the prefixes are
/// prefixes of each other.
const WELL_KNOWN_PREFIXES: &[&str] = &[
    "adapters-",
    "application-",
    "domain-",
    "ports-",
    "qbot-",
    "use-cases-",
];

/// The resolved context metadata for one bounded context. Emitted as a
/// `:Context` node; also used to look up the per-Item `bounded_context`
/// prop and the per-Crate `BELONGS_TO` edge target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextMeta {
    pub name: String,
    pub canonical_crate: Option<String>,
    pub owning_rfc: Option<String>,
}

/// The on-disk shape of a `.cfdb/concepts/<context>.toml` file.
#[derive(Debug, Deserialize)]
struct ConceptFile {
    name: String,
    #[serde(default)]
    crates: Vec<String>,
    #[serde(default)]
    canonical_crate: Option<String>,
    #[serde(default)]
    owning_rfc: Option<String>,
}

/// Loaded overrides: reverse map (crate name -> ContextMeta) so the
/// extractor can answer "which context does `crate X` belong to" in O(log n).
#[derive(Debug, Default)]
pub struct ConceptOverrides {
    /// Reverse map: crate name -> owning context metadata.
    by_crate: BTreeMap<String, ContextMeta>,
}

impl ConceptOverrides {
    /// Look up the context a crate is explicitly mapped to. Returns `None`
    /// if the crate is not covered by any override file — callers should
    /// fall back to the heuristic.
    pub fn lookup(&self, crate_name: &str) -> Option<&ContextMeta> {
        self.by_crate.get(crate_name)
    }

    /// All distinct contexts declared by overrides, keyed by context name.
    /// Used when emitting `:Context` nodes: contexts that only appear via
    /// the heuristic are synthesised separately.
    pub fn declared_contexts(&self) -> BTreeMap<String, ContextMeta> {
        let mut out: BTreeMap<String, ContextMeta> = BTreeMap::new();
        for meta in self.by_crate.values() {
            out.entry(meta.name.clone()).or_insert_with(|| meta.clone());
        }
        out
    }
}

/// Load every `.cfdb/concepts/*.toml` file under `workspace_root` into a
/// reverse map. Missing directory is NOT an error — returns an empty
/// `ConceptOverrides`. Parse errors are surfaced as `LoadError` so the
/// caller can fail loudly rather than silently fall back to the heuristic.
///
/// # Errors
///
/// Returns [`LoadError::Io`] on filesystem access failures (directory read,
/// file read) and [`LoadError::Toml`] on malformed TOML content.
pub fn load_concept_overrides(workspace_root: &Path) -> Result<ConceptOverrides, LoadError> {
    let dir = workspace_root.join(".cfdb").join("concepts");
    if !dir.exists() {
        return Ok(ConceptOverrides::default());
    }

    // Sort directory entries for determinism — `read_dir` order is OS-dependent.
    let mut entries: Vec<PathBuf> = Vec::new();
    let rd = fs::read_dir(&dir).map_err(|source| LoadError::Io {
        path: dir.clone(),
        source,
    })?;
    for entry in rd {
        let entry = entry.map_err(|source| LoadError::Io {
            path: dir.clone(),
            source,
        })?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("toml") {
            entries.push(path);
        }
    }
    entries.sort();

    let mut by_crate: BTreeMap<String, ContextMeta> = BTreeMap::new();
    for path in entries {
        let text = fs::read_to_string(&path).map_err(|source| LoadError::Io {
            path: path.clone(),
            source,
        })?;
        let parsed: ConceptFile = toml::from_str(&text).map_err(|source| LoadError::Toml {
            path: path.clone(),
            source: Box::new(source),
        })?;
        let meta = ContextMeta {
            name: parsed.name.clone(),
            canonical_crate: parsed.canonical_crate.clone(),
            owning_rfc: parsed.owning_rfc.clone(),
        };
        for crate_name in parsed.crates {
            by_crate.insert(crate_name, meta.clone());
        }
    }

    Ok(ConceptOverrides { by_crate })
}

/// Resolve the bounded context for a single crate name.
///
/// Override wins over the heuristic. The heuristic strips one well-known
/// crate-prefix (from [`WELL_KNOWN_PREFIXES`]) and returns the remainder;
/// crates with no known prefix return their full name unchanged.
#[must_use]
pub fn compute_bounded_context(package_name: &str, overrides: &ConceptOverrides) -> String {
    if let Some(meta) = overrides.lookup(package_name) {
        return meta.name.clone();
    }
    for prefix in WELL_KNOWN_PREFIXES {
        if let Some(rest) = package_name.strip_prefix(prefix) {
            if !rest.is_empty() {
                return rest.to_string();
            }
        }
    }
    package_name.to_string()
}

/// Errors surfaced by [`load_concept_overrides`] — file-system access and
/// TOML parse failures. I/O errors carry the offending path so callers can
/// pinpoint which concept file is malformed.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("io error reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("toml parse error in {path}: {source}")]
    Toml {
        path: PathBuf,
        #[source]
        source: Box<toml::de::Error>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty() -> ConceptOverrides {
        ConceptOverrides::default()
    }

    #[test]
    fn heuristic_strips_domain_prefix() {
        assert_eq!(
            compute_bounded_context("domain-trading", &empty()),
            "trading"
        );
    }

    #[test]
    fn heuristic_strips_ports_prefix() {
        assert_eq!(
            compute_bounded_context("ports-trading", &empty()),
            "trading"
        );
    }

    #[test]
    fn heuristic_strips_adapters_prefix() {
        // `adapters-postgres-trading` -> `postgres-trading` (only the first
        // well-known prefix is stripped — deeper semantics live in the
        // override TOML).
        assert_eq!(
            compute_bounded_context("adapters-postgres-trading", &empty()),
            "postgres-trading"
        );
    }

    #[test]
    fn heuristic_strips_application_prefix() {
        assert_eq!(
            compute_bounded_context("application-live-trading", &empty()),
            "live-trading"
        );
    }

    #[test]
    fn heuristic_strips_use_cases_prefix() {
        assert_eq!(
            compute_bounded_context("use-cases-backtest", &empty()),
            "backtest"
        );
    }

    #[test]
    fn heuristic_strips_qbot_prefix() {
        assert_eq!(compute_bounded_context("qbot-mcp", &empty()), "mcp");
    }

    #[test]
    fn heuristic_returns_full_name_when_no_prefix() {
        assert_eq!(compute_bounded_context("cfdb-core", &empty()), "cfdb-core");
        assert_eq!(
            compute_bounded_context("cfdb-extractor", &empty()),
            "cfdb-extractor"
        );
    }

    #[test]
    fn heuristic_returns_full_name_for_bare_prefix() {
        // `domain-` with nothing after it should not collapse to empty string.
        assert_eq!(compute_bounded_context("domain-", &empty()), "domain-");
    }

    #[test]
    fn override_wins_over_heuristic() {
        let mut by_crate = BTreeMap::new();
        by_crate.insert(
            "domain-trading".to_string(),
            ContextMeta {
                name: "portfolio".to_string(),
                canonical_crate: Some("domain-portfolio".to_string()),
                owning_rfc: Some("RFC-007".to_string()),
            },
        );
        let overrides = ConceptOverrides { by_crate };
        // The heuristic would strip to "trading"; the override forces "portfolio".
        assert_eq!(
            compute_bounded_context("domain-trading", &overrides),
            "portfolio"
        );
    }

    #[test]
    fn override_applies_to_unknown_prefix() {
        let mut by_crate = BTreeMap::new();
        by_crate.insert(
            "messenger".to_string(),
            ContextMeta {
                name: "cross-cutting".to_string(),
                canonical_crate: None,
                owning_rfc: None,
            },
        );
        let overrides = ConceptOverrides { by_crate };
        assert_eq!(
            compute_bounded_context("messenger", &overrides),
            "cross-cutting"
        );
    }

    #[test]
    fn load_concept_overrides_missing_directory_returns_empty() {
        let tmp = tempfile::tempdir().expect("tempdir");
        // Do NOT create .cfdb/concepts/ — load must succeed with empty map.
        let overrides = load_concept_overrides(tmp.path()).expect("load");
        assert!(overrides.lookup("domain-trading").is_none());
        assert!(overrides.declared_contexts().is_empty());
    }

    #[test]
    fn load_concept_overrides_parses_single_toml_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let concepts = tmp.path().join(".cfdb").join("concepts");
        fs::create_dir_all(&concepts).expect("mkdir");
        fs::write(
            concepts.join("cfdb.toml"),
            r#"
name = "cfdb"
crates = ["cfdb-core", "cfdb-extractor", "cfdb-cli"]
canonical_crate = "cfdb-core"
owning_rfc = "RFC-029"
"#,
        )
        .expect("write toml");

        let overrides = load_concept_overrides(tmp.path()).expect("load");
        let meta = overrides.lookup("cfdb-core").expect("cfdb-core mapped");
        assert_eq!(meta.name, "cfdb");
        assert_eq!(meta.canonical_crate.as_deref(), Some("cfdb-core"));
        assert_eq!(meta.owning_rfc.as_deref(), Some("RFC-029"));
        assert!(overrides.lookup("cfdb-extractor").is_some());
        assert!(overrides.lookup("cfdb-cli").is_some());
        assert!(overrides.lookup("unknown-crate").is_none());

        let declared = overrides.declared_contexts();
        assert_eq!(declared.len(), 1);
        assert!(declared.contains_key("cfdb"));
    }

    #[test]
    fn load_concept_overrides_sorts_entries_deterministically() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let concepts = tmp.path().join(".cfdb").join("concepts");
        fs::create_dir_all(&concepts).expect("mkdir");
        // Create two files whose names would sort in opposite orders on
        // different OSes if we relied on raw `read_dir` iteration.
        fs::write(
            concepts.join("b.toml"),
            "name = \"b\"\ncrates = [\"crate-b\"]\n",
        )
        .expect("write b");
        fs::write(
            concepts.join("a.toml"),
            "name = \"a\"\ncrates = [\"crate-a\"]\n",
        )
        .expect("write a");

        // Two loads must produce identical reverse maps.
        let first = load_concept_overrides(tmp.path()).expect("load 1");
        let second = load_concept_overrides(tmp.path()).expect("load 2");
        assert_eq!(
            first.declared_contexts(),
            second.declared_contexts(),
            "two loads must agree",
        );
    }

    #[test]
    fn load_concept_overrides_rejects_malformed_toml() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let concepts = tmp.path().join(".cfdb").join("concepts");
        fs::create_dir_all(&concepts).expect("mkdir");
        fs::write(concepts.join("bad.toml"), "name = this is not toml").expect("write bad");
        let err = load_concept_overrides(tmp.path()).expect_err("must fail");
        assert!(matches!(err, LoadError::Toml { .. }));
    }
}
