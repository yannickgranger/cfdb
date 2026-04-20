//! `SkillRoutingTable` — external DebtClass → skill mapping (§A2.3).
//!
//! The classifier emits `:Finding` rows carrying only the abstract
//! `DebtClass`. Skill routing — the mapping from class to concrete
//! Claude skill — is a policy concern that lives OUTSIDE the graph
//! schema per council BLOCK-1 + solid-architect verdicts (RFC-cfdb-v0.2-
//! addendum-draft.md §A2.3).
//!
//! This module loads `.cfdb/skill-routing.toml` (or an equivalent TOML
//! buffer) into a strongly-typed table exposing one lookup: `route(class)
//! -> Option<&SkillRoute>`. Consumers (`/operate-module`, `/boy-scout
//! --from-inventory`, future orchestrators) read the routing decision
//! at invocation time.
//!
//! # DIP invariant
//!
//! The `Finding` struct ([`crate::Finding`]) MUST NOT carry a
//! `fix_skill` field. Any attempt to do so is a split-brain with this
//! table and is blocked by the architecture test
//! `finding_no_skill_field`.
//!
//! # File shape
//!
//! ```toml
//! schema_version = 1
//!
//! [classes.duplicated_feature]
//! skill = "sweep-epic"
//! council_required = false
//! notes = "..."
//!
//! [classes.context_homonym]
//! skill = "operate-module"
//! council_required = true
//! # ...
//! ```
//!
//! All six `DebtClass` variants are expected to carry a row. Missing
//! rows are surfaced via [`SkillRoutingTable::missing_classes`] so
//! consumers can decide whether absence is an error or a degradation.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::inventory::DebtClass;

/// Parsed `.cfdb/skill-routing.toml` content. One entry per
/// `DebtClass`, keyed by the class's `snake_case` spelling
/// ([`DebtClass::as_str`]).
///
/// The wire format uses a `BTreeMap<String, SkillRoute>` indirected by
/// the top-level `[classes]` table so that serde TOML preserves
/// key-by-class naming without requiring a custom deserializer for
/// the `DebtClass` enum itself (which has stricter FromStr rules).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillRoutingTable {
    /// Schema version of the routing table format. Bump when adding
    /// required fields on [`SkillRoute`]. v0.1 = 1.
    pub schema_version: u32,
    /// One entry per class (keyed by `DebtClass::as_str`).
    pub classes: BTreeMap<String, SkillRoute>,
}

/// One routing decision: class → skill + metadata.
///
/// # Fields
///
/// - `skill` — concrete Claude skill name (e.g. `"sweep-epic"`,
///   `"operate-module"`, `"boy-scout"`). String-typed so the table can
///   reference skills the cfdb crate does not know about.
/// - `council_required` — when `true`, the orchestrator MUST route
///   through a council deliberation before invoking the skill. Load-
///   bearing for `context_homonym` — mechanical dedup on a homonym is
///   a bounded-context regression (DDD Q3).
/// - `mode` — optional skill-specific variant flag (e.g.
///   `/sweep-epic --mode=port` for unfinished refactors).
/// - `notes` — free-form rationale shown in CLI / tool output. Not
///   load-bearing; purely explanatory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillRoute {
    pub skill: String,
    #[serde(default)]
    pub council_required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// Errors that can arise loading a [`SkillRoutingTable`] from a path.
#[derive(Debug)]
pub enum SkillRoutingLoadError {
    /// Filesystem-level error reading the TOML file.
    Io(std::io::Error),
    /// TOML parse error — malformed file contents.
    Toml(String),
}

impl std::fmt::Display for SkillRoutingLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkillRoutingLoadError::Io(e) => write!(f, "read skill-routing.toml: {e}"),
            SkillRoutingLoadError::Toml(msg) => write!(f, "parse skill-routing.toml: {msg}"),
        }
    }
}

impl std::error::Error for SkillRoutingLoadError {}

impl SkillRoutingTable {
    /// Load the routing table from a TOML file on disk.
    ///
    /// Pure I/O — reads the file once, parses, returns. No caching; the
    /// caller owns cache lifetime. Determinism: identical file contents
    /// produce identical tables by `PartialEq`.
    pub fn from_path(path: &Path) -> Result<Self, SkillRoutingLoadError> {
        let bytes = std::fs::read_to_string(path).map_err(SkillRoutingLoadError::Io)?;
        Self::from_toml_str(&bytes)
    }

    /// Parse a routing table from a TOML string. Factored out of
    /// [`Self::from_path`] so unit tests can exercise parsing without
    /// touching the filesystem.
    pub fn from_toml_str(s: &str) -> Result<Self, SkillRoutingLoadError> {
        toml::from_str(s).map_err(|e| SkillRoutingLoadError::Toml(e.to_string()))
    }

    /// Look up the routing decision for a class. Returns `None` when
    /// the table does not declare a row for this class — the caller
    /// decides how to degrade (log a warning, default to a hardcoded
    /// skill, refuse to route).
    pub fn route(&self, class: DebtClass) -> Option<&SkillRoute> {
        self.classes.get(class.as_str())
    }

    /// Enumerate any `DebtClass` variant that is not declared in the
    /// table. A routing table missing rows is a config bug — the CLI
    /// uses this to emit a startup diagnostic.
    pub fn missing_classes(&self) -> Vec<DebtClass> {
        DebtClass::variants()
            .iter()
            .copied()
            .filter(|c| !self.classes.contains_key(c.as_str()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL_TABLE: &str = r#"
schema_version = 1

[classes.duplicated_feature]
skill = "sweep-epic"
council_required = false

[classes.context_homonym]
skill = "operate-module"
council_required = true
notes = "Context Mapping decision"

[classes.unfinished_refactor]
skill = "sweep-epic"
mode = "port"
council_required = false

[classes.random_scattering]
skill = "boy-scout"
council_required = false

[classes.canonical_bypass]
skill = "sweep-epic"
council_required = false

[classes.unwired]
skill = "boy-scout"
council_required = false
"#;

    #[test]
    fn from_toml_str_parses_every_class() {
        let t = SkillRoutingTable::from_toml_str(FULL_TABLE).expect("parse");
        assert_eq!(t.schema_version, 1);
        assert_eq!(t.classes.len(), 6);
        assert!(t.missing_classes().is_empty());
    }

    #[test]
    fn route_looks_up_by_class() {
        let t = SkillRoutingTable::from_toml_str(FULL_TABLE).expect("parse");
        let r = t.route(DebtClass::ContextHomonym).expect("row present");
        assert_eq!(r.skill, "operate-module");
        assert!(r.council_required);
        assert_eq!(r.mode, None);
        assert_eq!(r.notes.as_deref(), Some("Context Mapping decision"));
    }

    #[test]
    fn route_carries_mode_flag_for_unfinished_refactor() {
        let t = SkillRoutingTable::from_toml_str(FULL_TABLE).expect("parse");
        let r = t
            .route(DebtClass::UnfinishedRefactor)
            .expect("row present");
        assert_eq!(r.skill, "sweep-epic");
        assert_eq!(r.mode.as_deref(), Some("port"));
        assert!(!r.council_required);
    }

    #[test]
    fn missing_classes_reports_every_absent_variant() {
        let partial = r#"
schema_version = 1

[classes.duplicated_feature]
skill = "sweep-epic"
"#;
        let t = SkillRoutingTable::from_toml_str(partial).expect("parse");
        let missing = t.missing_classes();
        // Five of six variants absent — only `duplicated_feature` present.
        assert_eq!(missing.len(), 5);
        assert!(!missing.contains(&DebtClass::DuplicatedFeature));
        assert!(missing.contains(&DebtClass::ContextHomonym));
        assert!(missing.contains(&DebtClass::UnfinishedRefactor));
        assert!(missing.contains(&DebtClass::RandomScattering));
        assert!(missing.contains(&DebtClass::CanonicalBypass));
        assert!(missing.contains(&DebtClass::Unwired));
    }

    #[test]
    fn route_returns_none_for_absent_class() {
        let partial = r#"
schema_version = 1

[classes.duplicated_feature]
skill = "sweep-epic"
"#;
        let t = SkillRoutingTable::from_toml_str(partial).expect("parse");
        assert!(t.route(DebtClass::ContextHomonym).is_none());
    }

    #[test]
    fn from_toml_str_rejects_malformed_input() {
        let err = SkillRoutingTable::from_toml_str("not-valid-toml = {").expect_err("bad toml");
        let msg = err.to_string();
        assert!(msg.contains("parse skill-routing.toml"));
    }

    #[test]
    fn parse_is_deterministic() {
        // BTreeMap ordering guarantees identical round-trips across runs.
        let a = SkillRoutingTable::from_toml_str(FULL_TABLE).expect("parse a");
        let b = SkillRoutingTable::from_toml_str(FULL_TABLE).expect("parse b");
        assert_eq!(a, b);
    }
}
