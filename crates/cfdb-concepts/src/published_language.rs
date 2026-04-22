//! Published-language crates loader — `.cfdb/published-language-crates.toml`.
//!
//! DDD Published Language marker loader per RFC-cfdb-v0.2-addendum §A1.8
//! (issue #100). Declares which crates publish a "Published Language"
//! intentionally consumed across bounded contexts. The `:Finding`
//! classifier (issue #48) reads the materialised `:Crate.published_language`
//! prop to suppress false Context-Homonym positives for crates that live
//! cross-context by design (e.g. `qbot-prelude`, `qbot-types`).
//!
//! File shape (single file, not a directory per-context like the concepts
//! layout):
//!
//! ```toml
//! [[crate]]
//! name = "qbot-prelude"
//! language = "prelude"
//! owning_context = "core"
//! consumers = ["trading", "portfolio", "strategy"]
//!
//! [[crate]]
//! name = "qbot-types"
//! language = "types"
//! owning_context = "core"
//! consumers = ["*"]
//! ```
//!
//! The loader returns the same [`LoadError`](crate::LoadError) as
//! [`load_concept_overrides`](crate::load_concept_overrides) — `Io` for
//! filesystem access failures, `Toml` for malformed TOML, and a reused
//! `Io { InvalidData }` for duplicate `name` entries (a forbidden
//! last-wins would hide a configuration bug).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::LoadError;

/// Public metadata for one Published-Language crate. Stored as the value
/// type of [`PublishedLanguageCrates::by_crate`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishedLanguageEntry {
    /// Short identifier for the published language (e.g., `"prelude"`).
    pub language: String,
    /// Name of the bounded context that owns the language. Classifier
    /// uses this to route edits through the owning context's authority.
    pub owning_context: String,
    /// Contexts expected to consume this language. A single `"*"` entry
    /// means wildcard — any context may consume. The loader does NOT
    /// special-case `"*"`; classifier decides wildcard semantics.
    pub consumers: Vec<String>,
}

/// Loaded published-language marker list — reverse map from crate name
/// to [`PublishedLanguageEntry`] for O(log n) lookups by the extractor
/// while emitting `:Crate` nodes.
#[derive(Debug, Default)]
pub struct PublishedLanguageCrates {
    by_crate: BTreeMap<String, PublishedLanguageEntry>,
}

impl PublishedLanguageCrates {
    /// `true` iff `crate_name` is declared as a Published Language crate
    /// in `.cfdb/published-language-crates.toml`.
    #[must_use]
    pub fn is_published_language(&self, crate_name: &str) -> bool {
        self.by_crate.contains_key(crate_name)
    }

    /// The owning context for a Published Language crate, or `None` if
    /// the crate is not declared. Returned as `&str` to avoid allocating
    /// at the lookup site.
    #[must_use]
    pub fn owning_context(&self, crate_name: &str) -> Option<&str> {
        self.by_crate
            .get(crate_name)
            .map(|e| e.owning_context.as_str())
    }

    /// The declared list of consumer contexts for a Published Language
    /// crate, or `None` if the crate is not declared. A single `"*"`
    /// entry in the returned slice is wildcard — any context may
    /// consume. The loader passes `"*"` through verbatim; classifier
    /// interprets it.
    #[must_use]
    pub fn allowed_consumers(&self, crate_name: &str) -> Option<&[String]> {
        self.by_crate
            .get(crate_name)
            .map(|e| e.consumers.as_slice())
    }
}

/// On-disk shape of `.cfdb/published-language-crates.toml`.
///
/// Private to the module — callers interact with
/// [`PublishedLanguageCrates`] + [`PublishedLanguageEntry`] only.
#[derive(Debug, Deserialize)]
struct PublishedLanguageFile {
    #[serde(rename = "crate", default)]
    crates: Vec<PublishedLanguageCrateEntry>,
}

#[derive(Debug, Deserialize)]
struct PublishedLanguageCrateEntry {
    name: String,
    language: String,
    owning_context: String,
    #[serde(default)]
    consumers: Vec<String>,
}

/// Load `.cfdb/published-language-crates.toml` under `workspace_root` into
/// a reverse map. Missing file is NOT an error — returns an empty
/// [`PublishedLanguageCrates`] mirroring the `load_concept_overrides`
/// contract (per AC-3). Parse errors surface as [`LoadError::Toml`]
/// (AC-4); duplicate `name` entries surface as [`LoadError::Io`] with
/// `ErrorKind::InvalidData` to avoid silent last-wins.
///
/// # Errors
///
/// - [`LoadError::Io`] on filesystem access failures or duplicate-name
///   rejection.
/// - [`LoadError::Toml`] on malformed TOML content.
pub fn load_published_language_crates(
    workspace_root: &Path,
) -> Result<PublishedLanguageCrates, LoadError> {
    let path = workspace_root
        .join(".cfdb")
        .join("published-language-crates.toml");

    if !path.exists() {
        return Ok(PublishedLanguageCrates::default());
    }

    let text = fs::read_to_string(&path).map_err(|source| LoadError::Io {
        path: path.clone(),
        source,
    })?;

    let parsed: PublishedLanguageFile =
        toml::from_str(&text).map_err(|source| LoadError::Toml {
            path: path.clone(),
            source: Box::new(source),
        })?;

    if let Some(dup) = first_duplicate_name(&parsed.crates) {
        return Err(LoadError::Io {
            path,
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "duplicate crate name `{dup}` in published-language-crates.toml — \
                     silent last-wins is forbidden; remove the duplicate entry"
                ),
            ),
        });
    }

    let mut by_crate: BTreeMap<String, PublishedLanguageEntry> = BTreeMap::new();
    for entry in parsed.crates {
        by_crate.insert(
            entry.name,
            PublishedLanguageEntry {
                language: entry.language,
                owning_context: entry.owning_context,
                consumers: entry.consumers,
            },
        );
    }

    Ok(PublishedLanguageCrates { by_crate })
}

/// Single-pass duplicate detection. Returns the first repeated `name`
/// in TOML order, or `None` when every entry is unique. Hoisted out of
/// the insertion loop so the per-iteration body of [`load`] holds no
/// `.clone()` of the file path.
fn first_duplicate_name(crates: &[PublishedLanguageCrateEntry]) -> Option<&str> {
    let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for entry in crates {
        if !seen.insert(entry.name.as_str()) {
            return Some(entry.name.as_str());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_toml(dir: &Path, body: &str) {
        let cfdb_dir = dir.join(".cfdb");
        std::fs::create_dir_all(&cfdb_dir).expect("mkdir .cfdb");
        std::fs::write(cfdb_dir.join("published-language-crates.toml"), body)
            .expect("write published-language-crates.toml");
    }

    #[test]
    fn missing_file_returns_empty() {
        let tmp = tempfile::tempdir().expect("tempdir");
        // Do NOT create `.cfdb/published-language-crates.toml`.
        let loaded = load_published_language_crates(tmp.path()).expect("load ok");
        assert!(!loaded.is_published_language("anything"));
        assert_eq!(loaded.owning_context("anything"), None);
        assert_eq!(loaded.allowed_consumers("anything"), None);
    }

    #[test]
    fn empty_file_returns_empty() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_toml(tmp.path(), ""); // zero [[crate]] tables
        let loaded = load_published_language_crates(tmp.path()).expect("load ok");
        assert!(!loaded.is_published_language("anything"));
    }

    #[test]
    fn single_crate_parses() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_toml(
            tmp.path(),
            r#"
[[crate]]
name = "qbot-prelude"
language = "prelude"
owning_context = "core"
consumers = ["trading", "portfolio"]
"#,
        );
        let loaded = load_published_language_crates(tmp.path()).expect("load ok");
        assert!(loaded.is_published_language("qbot-prelude"));
        assert_eq!(loaded.owning_context("qbot-prelude"), Some("core"));
        let consumers = loaded
            .allowed_consumers("qbot-prelude")
            .expect("consumers present");
        assert_eq!(consumers, &["trading".to_string(), "portfolio".to_string()]);
        // Unmapped crate returns None/false.
        assert!(!loaded.is_published_language("cfdb-core"));
    }

    #[test]
    fn multiple_crates_parse() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_toml(
            tmp.path(),
            r#"
[[crate]]
name = "qbot-prelude"
language = "prelude"
owning_context = "core"
consumers = ["trading"]

[[crate]]
name = "qbot-types"
language = "types"
owning_context = "core"
consumers = ["*"]
"#,
        );
        let loaded = load_published_language_crates(tmp.path()).expect("load ok");
        assert!(loaded.is_published_language("qbot-prelude"));
        assert!(loaded.is_published_language("qbot-types"));
        assert!(!loaded.is_published_language("not-listed"));
    }

    #[test]
    fn wildcard_consumers_pass_through_verbatim() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_toml(
            tmp.path(),
            r#"
[[crate]]
name = "qbot-types"
language = "types"
owning_context = "core"
consumers = ["*"]
"#,
        );
        let loaded = load_published_language_crates(tmp.path()).expect("load ok");
        assert_eq!(
            loaded.allowed_consumers("qbot-types"),
            Some(["*".to_string()].as_slice())
        );
    }

    #[test]
    fn malformed_toml_returns_toml_error() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_toml(tmp.path(), "not = valid = toml");
        let err = load_published_language_crates(tmp.path()).expect_err("must fail");
        assert!(
            matches!(err, LoadError::Toml { .. }),
            "expected LoadError::Toml, got {err:?}"
        );
    }

    #[test]
    fn two_loads_are_deterministic() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_toml(
            tmp.path(),
            r#"
[[crate]]
name = "alpha"
language = "alpha-lang"
owning_context = "core"
consumers = ["b", "a"]

[[crate]]
name = "beta"
language = "beta-lang"
owning_context = "core"
consumers = ["*"]
"#,
        );
        let a = load_published_language_crates(tmp.path()).expect("a");
        let b = load_published_language_crates(tmp.path()).expect("b");
        assert_eq!(a.by_crate, b.by_crate);
    }

    #[test]
    fn duplicate_crate_name_is_rejected() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_toml(
            tmp.path(),
            r#"
[[crate]]
name = "qbot-prelude"
language = "first"
owning_context = "core"
consumers = []

[[crate]]
name = "qbot-prelude"
language = "second"
owning_context = "core"
consumers = []
"#,
        );
        let err = load_published_language_crates(tmp.path()).expect_err("must fail");
        match err {
            LoadError::Io { source, .. } => {
                assert_eq!(source.kind(), std::io::ErrorKind::InvalidData);
                assert!(
                    source.to_string().contains("duplicate crate name"),
                    "expected duplicate-name message, got: {source}"
                );
            }
            other => panic!("expected LoadError::Io for duplicate-name, got {other:?}"),
        }
    }
}
