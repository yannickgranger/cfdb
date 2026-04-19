//! Debt-class taxonomy and structured scope inventory
//! (RFC-cfdb-v0.2-addendum-draft.md §A2 / §A3.3).
//!
//! `DebtClass` is the 6-variant canonical taxonomy used by the `cfdb scope`
//! verb. `ScopeInventory` is the JSON envelope returned to consumer skills
//! (`/operate-module`, etc.). All types here are pure data — no workflow hints,
//! no computation.

use serde::{Deserialize, Serialize};

/// Canonical debt-class taxonomy for the `cfdb scope` verb
/// (RFC-cfdb-v0.2-addendum-draft.md §A2.1). The 6 variants are the exact
/// classes used by `ScopeInventory::findings_by_class` JSON buckets.
///
/// Serde variant naming is `snake_case` so the JSON keys match the addendum
/// §A2.1 names verbatim: `duplicated_feature, context_homonym,
/// unfinished_refactor, random_scattering, canonical_bypass, unwired`.
///
/// The enum derives `Ord` so `BTreeMap<DebtClass, _>` serializes
/// deterministically (AC G1 — deterministic output across runs).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DebtClass {
    DuplicatedFeature,
    ContextHomonym,
    UnfinishedRefactor,
    RandomScattering,
    CanonicalBypass,
    Unwired,
}

impl DebtClass {
    /// Canonical list of the 6 classes, addendum §A2.1 order.
    pub fn variants() -> &'static [DebtClass] {
        &[
            DebtClass::DuplicatedFeature,
            DebtClass::ContextHomonym,
            DebtClass::UnfinishedRefactor,
            DebtClass::RandomScattering,
            DebtClass::CanonicalBypass,
            DebtClass::Unwired,
        ]
    }

    /// Snake-case class name used in JSON output (matches the addendum §A2.1
    /// taxonomy string verbatim).
    pub fn as_str(self) -> &'static str {
        match self {
            DebtClass::DuplicatedFeature => "duplicated_feature",
            DebtClass::ContextHomonym => "context_homonym",
            DebtClass::UnfinishedRefactor => "unfinished_refactor",
            DebtClass::RandomScattering => "random_scattering",
            DebtClass::CanonicalBypass => "canonical_bypass",
            DebtClass::Unwired => "unwired",
        }
    }
}

impl std::fmt::Display for DebtClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for DebtClass {
    type Err = UnknownDebtClass;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "duplicated_feature" => Ok(DebtClass::DuplicatedFeature),
            "context_homonym" => Ok(DebtClass::ContextHomonym),
            "unfinished_refactor" => Ok(DebtClass::UnfinishedRefactor),
            "random_scattering" => Ok(DebtClass::RandomScattering),
            "canonical_bypass" => Ok(DebtClass::CanonicalBypass),
            "unwired" => Ok(DebtClass::Unwired),
            other => Err(UnknownDebtClass(other.to_string())),
        }
    }
}

/// Parse error for [`DebtClass`]'s `FromStr`. Carries the rejected input so
/// the caller can format a message that enumerates valid values from
/// [`DebtClass::variants`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownDebtClass(pub String);

impl std::fmt::Display for UnknownDebtClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "unknown DebtClass `{}` — valid values: {}",
            self.0,
            DebtClass::variants()
                .iter()
                .map(|c| c.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

impl std::error::Error for UnknownDebtClass {}

/// A single finding row in a `ScopeInventory::findings_by_class` bucket.
///
/// Mirrors the `:Item` attributes surfaced by `list_items_matching` plus a
/// deterministic `id` (the `item:<qname>` key). No workflow strings —
/// purely structural data.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Finding {
    pub qname: String,
    pub name: String,
    pub kind: String,
    #[serde(rename = "crate")]
    pub crate_name: String,
    pub file: String,
    pub line: u64,
    pub bounded_context: String,
}

/// A candidate for a canonical "single source of truth" resolution. Sourced
/// from the `hsb-by-name` rule's enriched row shape (same `name`+`kind`
/// defined in N crates). Consumers (e.g. `/operate-module`) compare against
/// Pattern I results to pick portage targets.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CanonicalCandidate {
    pub name: String,
    pub kind: String,
    pub crates: Vec<String>,
    pub qnames: Vec<String>,
    pub files: Vec<String>,
}

/// A per-item reachability entry. `None` at the outer `ScopeInventory` level
/// in v0.1 (HIR-blocked per RFC-cfdb §10 + addendum §A1.2). This struct is
/// defined for forward-compatible JSON schema and deserialization by
/// consumer skills — no v0.1 code populates it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReachabilityEntry {
    pub reachable_from_entry_point: bool,
    pub depth: Option<u32>,
    pub entry_qname: Option<String>,
}

/// Structured infection inventory for a bounded context
/// (RFC-cfdb-v0.2-addendum-draft.md §A3.3). Returned by `cfdb scope
/// --context <name>`. Pure data aggregation — no workflow hints,
/// no raid-plan formatting; that is the consumer skill's concern
/// (`/operate-module` per §A3.4).
///
/// v0.1 populates only the fields whose classifier rules ship on develop.
/// `reachability_map` is `None` in v0.1 (HIR-blocked). The other 5 class
/// buckets carry `[]` + a `Warning` on the top-level `warnings` list when
/// their classifier is unavailable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScopeInventory {
    pub context: String,
    pub keyspace_sha: String,
    pub findings_by_class: std::collections::BTreeMap<DebtClass, Vec<Finding>>,
    pub canonical_candidates: Vec<CanonicalCandidate>,
    pub reachability_map: Option<std::collections::BTreeMap<String, ReachabilityEntry>>,
    pub loc_per_crate: std::collections::BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<crate::result::Warning>,
}

impl ScopeInventory {
    /// Construct an empty inventory for `context` pinned to `keyspace_sha`.
    /// All 6 class buckets are pre-seeded empty so consumers can iterate
    /// them without key-existence checks.
    pub fn new(context: impl Into<String>, keyspace_sha: impl Into<String>) -> Self {
        let mut findings_by_class = std::collections::BTreeMap::new();
        for class in DebtClass::variants() {
            findings_by_class.insert(*class, Vec::new());
        }
        Self {
            context: context.into(),
            keyspace_sha: keyspace_sha.into(),
            findings_by_class,
            canonical_candidates: Vec::new(),
            reachability_map: None,
            loc_per_crate: std::collections::BTreeMap::new(),
            warnings: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debt_class_variants_enumerates_taxonomy_in_a21_order() {
        assert_eq!(
            DebtClass::variants(),
            &[
                DebtClass::DuplicatedFeature,
                DebtClass::ContextHomonym,
                DebtClass::UnfinishedRefactor,
                DebtClass::RandomScattering,
                DebtClass::CanonicalBypass,
                DebtClass::Unwired,
            ]
        );
    }

    #[test]
    fn debt_class_json_keys_match_snake_case_taxonomy() {
        // The JSON wire form MUST use the exact §A2.1 spelling (snake_case,
        // no abbreviations). Consumer skills (operate-module, boy-scout)
        // depend on this contract.
        for class in DebtClass::variants() {
            let json = serde_json::to_string(class).expect("serialize");
            let expected = format!("\"{}\"", class.as_str());
            assert_eq!(json, expected, "JSON spelling for {class:?}");
        }
    }

    #[test]
    fn debt_class_fromstr_rejects_unknown_with_err_carrying_input() {
        use std::str::FromStr;
        let err = DebtClass::from_str("Duplicated").expect_err("titlecase rejected");
        assert_eq!(err.0, "Duplicated");
        let err = DebtClass::from_str("typo_scattering").expect_err("typo rejected");
        assert_eq!(err.0, "typo_scattering");
    }

    #[test]
    fn debt_class_roundtrips_fromstr_display_for_every_variant() {
        use std::str::FromStr;
        for class in DebtClass::variants() {
            let spelled = class.to_string();
            let parsed = DebtClass::from_str(&spelled).expect("roundtrip");
            assert_eq!(&parsed, class);
        }
    }

    #[test]
    fn scope_inventory_new_seeds_all_six_buckets_empty() {
        let inv = ScopeInventory::new("trading", "abc123");
        assert_eq!(inv.context, "trading");
        assert_eq!(inv.keyspace_sha, "abc123");
        assert_eq!(inv.findings_by_class.len(), 6);
        for class in DebtClass::variants() {
            assert_eq!(
                inv.findings_by_class.get(class).map(Vec::len),
                Some(0),
                "bucket for {class:?} missing or non-empty"
            );
        }
        assert!(inv.canonical_candidates.is_empty());
        assert!(inv.reachability_map.is_none());
        assert!(inv.loc_per_crate.is_empty());
        assert!(inv.warnings.is_empty());
    }

    #[test]
    fn scope_inventory_a33_envelope_serializes_with_required_keys() {
        // Pin the JSON envelope shape. Consumer skills parse these exact
        // top-level keys; renames break the §A3.3 contract.
        let inv = ScopeInventory::new("trading", "abc123");
        let json = serde_json::to_value(&inv).expect("serialize");
        let obj = json.as_object().expect("object");
        for key in [
            "context",
            "keyspace_sha",
            "findings_by_class",
            "canonical_candidates",
            "reachability_map",
            "loc_per_crate",
        ] {
            assert!(obj.contains_key(key), "missing top-level key `{key}`");
        }
        // reachability_map is null in v0.1 (HIR-blocked).
        assert!(obj["reachability_map"].is_null());
        // findings_by_class keys spelled in snake_case taxonomy.
        let classes = obj["findings_by_class"].as_object().expect("class map");
        for class in DebtClass::variants() {
            assert!(
                classes.contains_key(class.as_str()),
                "class bucket `{}` missing",
                class.as_str()
            );
        }
    }

    #[test]
    fn scope_inventory_deterministic_across_runs() {
        // Two fresh inventories with identical seed data must serialize
        // byte-identical (G1 — AC "Deterministic output given same
        // keyspace"). BTreeMap ordering is load-bearing.
        let a = ScopeInventory::new("trading", "abc123");
        let b = ScopeInventory::new("trading", "abc123");
        assert_eq!(a, b);
        assert_eq!(
            serde_json::to_string(&a).expect("ser a"),
            serde_json::to_string(&b).expect("ser b")
        );
    }
}
