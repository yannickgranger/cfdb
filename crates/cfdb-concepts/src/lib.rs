use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use cfdb_core::ContextSource;
use serde::Deserialize;

mod published_language;

pub use published_language::{
    load_published_language_crates, PublishedLanguageCrates, PublishedLanguageEntry,
};

const WELL_KNOWN_PREFIXES: &[&str] = &[
    "adapters-",
    "application-",
    "domain-",
    "ports-",
    "qbot-",
    "use-cases-",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextMeta {
    pub name: String,
    pub canonical_crate: Option<String>,
    pub owning_rfc: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundedContext {
    pub name: String,
    pub source: ContextSource,
}

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

#[derive(Debug, Default)]
pub struct ConceptOverrides {
    by_crate: BTreeMap<String, ContextMeta>,
}

impl ConceptOverrides {
    pub fn lookup(&self, crate_name: &str) -> Option<&ContextMeta> {
        self.by_crate.get(crate_name)
    }

    pub fn declared_contexts(&self) -> BTreeMap<String, ContextMeta> {
        let mut out: BTreeMap<String, ContextMeta> = BTreeMap::new();
        self.by_crate.values().for_each(|meta| {
            out.entry(meta.name.clone()).or_insert_with(|| meta.clone());
        });
        out
    }

    pub fn crate_assignments(&self) -> &BTreeMap<String, ContextMeta> {
        &self.by_crate
    }
}

pub fn load_concept_overrides(workspace_root: &Path) -> Result<ConceptOverrides, LoadError> {
    let dir = workspace_root.join(".cfdb").join("concepts");
    if !dir.exists() {
        return Ok(ConceptOverrides::default());
    }

    let entries = collect_toml_entries(&dir)?;

    let mut by_crate: BTreeMap<String, ContextMeta> = BTreeMap::new();
    for path in entries {
        load_single_concept_file(&path, &mut by_crate)?;
    }

    Ok(ConceptOverrides { by_crate })
}

fn collect_toml_entries(dir: &Path) -> Result<Vec<PathBuf>, LoadError> {
    let rd = fs::read_dir(dir).map_err(|source| LoadError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    let mut entries: Vec<PathBuf> = rd
        .map(|entry| {
            entry.map(|e| e.path()).map_err(|source| LoadError::Io {
                path: dir.to_path_buf(),
                source,
            })
        })
        .filter_map(|result| match result {
            Ok(path) if path.extension().and_then(|e| e.to_str()) == Some("toml") => Some(Ok(path)),
            Ok(_) => None,
            Err(e) => Some(Err(e)),
        })
        .collect::<Result<_, _>>()?;
    entries.sort();
    Ok(entries)
}

fn load_single_concept_file(
    path: &Path,
    by_crate: &mut BTreeMap<String, ContextMeta>,
) -> Result<(), LoadError> {
    let text = fs::read_to_string(path).map_err(|source| LoadError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let parsed: ConceptFile = toml::from_str(&text).map_err(|source| LoadError::Toml {
        path: path.to_path_buf(),
        source: Box::new(source),
    })?;
    let meta = ContextMeta {
        name: parsed.name,
        canonical_crate: parsed.canonical_crate,
        owning_rfc: parsed.owning_rfc,
    };
    by_crate.extend(parsed.crates.into_iter().map(|c| (c, meta.clone())));
    Ok(())
}

#[must_use]
pub fn compute_bounded_context(package_name: &str, overrides: &ConceptOverrides) -> BoundedContext {
    if let Some(meta) = overrides.lookup(package_name) {
        return BoundedContext {
            name: meta.name.clone(),
            source: ContextSource::Declared,
        };
    }
    for prefix in WELL_KNOWN_PREFIXES {
        if let Some(rest) = package_name.strip_prefix(prefix) {
            if !rest.is_empty() {
                return BoundedContext {
                    name: rest.to_string(),
                    source: ContextSource::Heuristic,
                };
            }
        }
    }
    BoundedContext {
        name: package_name.to_string(),
        source: ContextSource::Heuristic,
    }
}

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
            compute_bounded_context("domain-trading", &empty()).name,
            "trading"
        );
    }

    #[test]
    fn heuristic_strips_ports_prefix() {
        assert_eq!(
            compute_bounded_context("ports-trading", &empty()).name,
            "trading"
        );
    }

    #[test]
    fn heuristic_strips_adapters_prefix() {
        assert_eq!(
            compute_bounded_context("adapters-postgres-trading", &empty()).name,
            "postgres-trading"
        );
    }

    #[test]
    fn heuristic_strips_application_prefix() {
        assert_eq!(
            compute_bounded_context("application-live-trading", &empty()).name,
            "live-trading"
        );
    }

    #[test]
    fn heuristic_strips_use_cases_prefix() {
        assert_eq!(
            compute_bounded_context("use-cases-backtest", &empty()).name,
            "backtest"
        );
    }

    #[test]
    fn heuristic_strips_qbot_prefix() {
        assert_eq!(compute_bounded_context("qbot-mcp", &empty()).name, "mcp");
    }

    #[test]
    fn heuristic_returns_full_name_when_no_prefix() {
        assert_eq!(
            compute_bounded_context("cfdb-core", &empty()).name,
            "cfdb-core"
        );
        assert_eq!(
            compute_bounded_context("cfdb-extractor", &empty()).name,
            "cfdb-extractor"
        );
    }

    #[test]
    fn heuristic_returns_full_name_for_bare_prefix() {
        assert_eq!(compute_bounded_context("domain-", &empty()).name, "domain-");
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
        assert_eq!(
            compute_bounded_context("domain-trading", &overrides).name,
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
            compute_bounded_context("messenger", &overrides).name,
            "cross-cutting"
        );
    }

    #[test]
    fn declared_when_overridden() {
        let mut by_crate = BTreeMap::new();
        by_crate.insert(
            "domain-trading".to_string(),
            ContextMeta {
                name: "trading".to_string(),
                canonical_crate: None,
                owning_rfc: None,
            },
        );
        let overrides = ConceptOverrides { by_crate };
        let bc = compute_bounded_context("domain-trading", &overrides);
        assert_eq!(bc.name, "trading");
        assert_eq!(bc.source, ContextSource::Declared);
    }

    #[test]
    fn heuristic_when_prefix_stripped() {
        let bc = compute_bounded_context("domain-trading", &ConceptOverrides::default());
        assert_eq!(bc.name, "trading");
        assert_eq!(bc.source, ContextSource::Heuristic);
    }

    #[test]
    fn heuristic_when_no_prefix_match() {
        let bc = compute_bounded_context("cfdb-core", &ConceptOverrides::default());
        assert_eq!(bc.name, "cfdb-core");
        assert_eq!(bc.source, ContextSource::Heuristic);
    }

    #[test]
    fn load_concept_overrides_missing_directory_returns_empty() {
        let tmp = tempfile::tempdir().expect("tempdir");
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
