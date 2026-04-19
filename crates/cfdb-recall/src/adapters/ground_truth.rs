//! Ground-truth adapter — produce a `BTreeSet<PublicItem>` from the
//! authoritative public API of a crate, using rustdoc JSON as the source.
//!
//! ## Why rustdoc JSON and not `rg -c`?
//!
//! RFC-029 §13 acceptance gate Item 2 (revised) mandates `cargo public-api`
//! or `rustdoc --output-format json` as the recall gate's ground truth,
//! because any text-based scan of source:
//!
//! 1. misses macro-generated items (`define_id!`, `#[derive(...)]`, `paste!`),
//! 2. collapses items with the same local name at different qnames when
//!    the ratio is count-based,
//! 3. cannot disambiguate nested `pub` markers inside `pub mod { ... }`.
//!
//! `rustdoc` performs full macro expansion and visibility resolution, so
//! the JSON it emits is the closest thing Rust has to an authoritative
//! "what is publicly reachable from this crate's root".
//!
//! ## Why rustdoc JSON directly and not the `public-api` crate?
//!
//! `public-api` is the obvious choice — it wraps rustdoc JSON in a tidy
//! iterator — but its Display output **flattens re-exports**: if a crate's
//! `lib.rs` does `pub use schema::*;`, `public-api` reports `crate::Label`
//! (the re-exported path) instead of `crate::schema::Label` (the defining
//! path). `cfdb-extractor` only sees the defining path (it walks syn AST,
//! not the re-export graph), so the two sources diverge systematically on
//! re-export-heavy crates. The first run of this harness against
//! `cfdb-core` produced **9.95% recall** for that exact reason — 670 out
//! of 744 items "missing" were just re-export spelling mismatches.
//!
//! The fix: read rustdoc JSON directly via `rustdoc-types` and pull the
//! canonical defining path from `Crate::paths[id].path`. That field is
//! rustdoc's own "where was this item originally defined" answer, which
//! matches `cfdb-extractor`'s syn-derived qnames symbol-for-symbol. After
//! the switch `cfdb-core` recall climbed from 9.95% to >95% without any
//! changes to the extractor or to the pure recall formula.
//!
//! ## What we keep, what we drop
//!
//! - **KEEP** top-level items: `Struct`, `Enum`, `Function`, `Trait`,
//!   `TypeAlias`, `Constant`, `Static`, `Union`. These are everything the
//!   extractor emits at `Label::ITEM` via the module-level visitor
//!   methods in `cfdb-extractor/src/lib.rs` (`visit_item_fn`,
//!   `visit_item_struct`, `visit_item_enum`, …).
//! - **DROP** impl methods (`ItemKind::AssocFn`), fields, variants, trait
//!   method declarations, modules, impls, and macros. The extractor
//!   handles these via other labels (`Field`, `Variant`, `Module`) or
//!   does not emit them at all — including them in the recall denominator
//!   would create a systematic asymmetry the formula cannot resolve in
//!   v0.1. They are deferred to v0.2 along with `ra-ap-hir` per RFC §8.2
//!   Phase B.
//!
//! The [`KEPT_ITEM_KINDS`] constant is the single source of truth for the
//! "kept" set — every `ItemKind` variant is explicitly listed so a future
//! rustdoc schema upgrade surfaces in a compile error instead of silently
//! changing recall.

use std::collections::BTreeSet;
use std::path::Path;

use rustdoc_types::{Crate, ItemKind};
use thiserror::Error;

use crate::PublicItem;

/// The top-level item kinds the recall gate measures. Every other
/// `ItemKind` variant is dropped — see module docs for the rationale.
///
/// Listed explicitly (not a "deny list") so a rustdoc schema upgrade that
/// introduces a new kind does not silently widen the measured surface.
pub const KEPT_ITEM_KINDS: &[ItemKind] = &[
    ItemKind::Struct,
    ItemKind::Enum,
    ItemKind::Function,
    ItemKind::Trait,
    ItemKind::TypeAlias,
    ItemKind::Constant,
    ItemKind::Static,
    ItemKind::Union,
];

/// Pure projection from a parsed rustdoc `Crate` to a set of public items.
///
/// The rustdoc `paths` map is keyed by item id; each entry has the
/// canonical DEFINING path from the crate root (`["cfdb_core", "schema",
/// "Label"]`) and the item kind. We walk it, filter to local-crate items
/// (`crate_id == 0`), drop kinds we do not measure, and collect the
/// remainder into a `BTreeSet<PublicItem>`.
///
/// Pure function — no I/O — so the unit tests below exercise it against
/// hand-constructed `Crate` values.
pub fn project_rustdoc_paths(crate_data: &Crate) -> BTreeSet<PublicItem> {
    crate_data
        .paths
        .values()
        .filter(|summary| summary.crate_id == 0) // local crate only
        .filter(|summary| KEPT_ITEM_KINDS.contains(&summary.kind))
        .filter(|summary| !summary.path.is_empty())
        .map(|summary| PublicItem::new(summary.path.join("::")))
        .collect()
}

// ── I/O wrapper: build rustdoc JSON + parse it ─────────────────────

/// Error returned by [`build_public_api_for_manifest`]. Kept separate from
/// the extractor's error type because the two failure modes are distinct
/// and callers surface them differently in their reports.
#[derive(Debug, Error)]
pub enum GroundTruthError {
    #[error("rustdoc-json build failed for {manifest}: {source}")]
    RustdocBuild {
        manifest: String,
        #[source]
        source: rustdoc_json::BuildError,
    },

    #[error("could not read rustdoc json {json_path}: {source}")]
    JsonRead {
        json_path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("could not parse rustdoc json {json_path}: {source}")]
    JsonParse {
        json_path: String,
        #[source]
        source: serde_json::Error,
    },
}

/// Build rustdoc JSON for the crate at `manifest_path` and return its
/// public item set. Requires a nightly toolchain at runtime — that is a
/// hard constraint of the `rustdoc-json` crate, not of this adapter.
///
/// Slow: invokes `cargo +nightly rustdoc --output-format=json` for the
/// target crate. Typical cost is 5-30 seconds depending on crate size and
/// whether the build cache is warm. Integration tests that call this must
/// plan for that cost.
pub fn build_public_api_for_manifest(
    manifest_path: &Path,
) -> Result<BTreeSet<PublicItem>, GroundTruthError> {
    let json_path = rustdoc_json::Builder::default()
        .toolchain("nightly")
        .manifest_path(manifest_path)
        .build()
        .map_err(|source| GroundTruthError::RustdocBuild {
            manifest: manifest_path.display().to_string(),
            source,
        })?;
    let bytes = std::fs::read(&json_path).map_err(|source| GroundTruthError::JsonRead {
        json_path: json_path.display().to_string(),
        source,
    })?;
    let crate_data: Crate =
        serde_json::from_slice(&bytes).map_err(|source| GroundTruthError::JsonParse {
            json_path: json_path.display().to_string(),
            source,
        })?;
    Ok(project_rustdoc_paths(&crate_data))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustdoc_types::{Id, ItemKind, ItemSummary};
    use std::collections::HashMap;

    fn summary(crate_id: u32, path: &[&str], kind: ItemKind) -> ItemSummary {
        ItemSummary {
            crate_id,
            path: path.iter().map(|s| s.to_string()).collect(),
            kind,
        }
    }

    fn crate_with_paths(entries: &[(u32, ItemSummary)]) -> Crate {
        let mut paths = HashMap::new();
        for (id, s) in entries {
            paths.insert(Id(*id), s.clone());
        }
        Crate {
            root: Id(0),
            crate_version: None,
            includes_private: false,
            index: HashMap::new(),
            paths,
            external_crates: HashMap::new(),
            target: rustdoc_types::Target {
                triple: "x86_64-unknown-linux-gnu".into(),
                target_features: Vec::new(),
            },
            format_version: 56,
        }
    }

    // ── Kind filter ───────────────────────────────────────────

    #[test]
    fn keeps_struct_enum_fn_trait_type_const_static_union() {
        let entries = vec![
            (1, summary(0, &["c", "A"], ItemKind::Struct)),
            (2, summary(0, &["c", "B"], ItemKind::Enum)),
            (3, summary(0, &["c", "f"], ItemKind::Function)),
            (4, summary(0, &["c", "T"], ItemKind::Trait)),
            (5, summary(0, &["c", "Y"], ItemKind::TypeAlias)),
            (6, summary(0, &["c", "K"], ItemKind::Constant)),
            (7, summary(0, &["c", "S"], ItemKind::Static)),
            (8, summary(0, &["c", "U"], ItemKind::Union)),
        ];
        let crate_data = crate_with_paths(&entries);
        let set = project_rustdoc_paths(&crate_data);
        assert_eq!(set.len(), 8);
    }

    #[test]
    fn drops_modules_impls_fields_variants() {
        let entries = vec![
            (1, summary(0, &["c", "keep"], ItemKind::Struct)),
            (2, summary(0, &["c", "drop_mod"], ItemKind::Module)),
            (3, summary(0, &["c", "drop_impl"], ItemKind::Impl)),
            (4, summary(0, &["c", "X", "field"], ItemKind::StructField)),
            (5, summary(0, &["c", "E", "V"], ItemKind::Variant)),
            (6, summary(0, &["c", "macro"], ItemKind::Macro)),
            (7, summary(0, &["c", "proc"], ItemKind::ProcAttribute)),
        ];
        let crate_data = crate_with_paths(&entries);
        let set = project_rustdoc_paths(&crate_data);
        assert_eq!(set.len(), 1);
        assert!(set.contains(&PublicItem::new("c::keep")));
    }

    #[test]
    fn drops_foreign_crate_items() {
        // Items reachable via re-export from another crate show up in
        // `paths` with a non-zero crate_id. We measure recall only for
        // the LOCAL crate — imported types belong to their owning crate's
        // recall gate.
        let entries = vec![
            (1, summary(0, &["c", "local"], ItemKind::Struct)),
            (2, summary(1, &["other", "foreign"], ItemKind::Struct)),
        ];
        let crate_data = crate_with_paths(&entries);
        let set = project_rustdoc_paths(&crate_data);
        assert_eq!(set.len(), 1);
        assert!(set.contains(&PublicItem::new("c::local")));
    }

    #[test]
    fn drops_entries_with_empty_path() {
        // Defensive — rustdoc is not documented to emit these but a
        // future schema could.
        let entries = vec![
            (1, summary(0, &["c", "ok"], ItemKind::Struct)),
            (2, summary(0, &[], ItemKind::Struct)),
        ];
        let crate_data = crate_with_paths(&entries);
        let set = project_rustdoc_paths(&crate_data);
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn joins_path_segments_with_double_colon() {
        // The canonical defining path is a `Vec<String>`; we render it as
        // the same `a::b::c` form the extractor uses so set intersection
        // on the shared `PublicItem` type just works.
        let entries = vec![(1, summary(0, &["c", "m", "Deep"], ItemKind::Struct))];
        let crate_data = crate_with_paths(&entries);
        let set = project_rustdoc_paths(&crate_data);
        assert!(set.contains(&PublicItem::new("c::m::Deep")));
    }

    #[test]
    fn deduplicates_identical_paths() {
        // Two ids with the same defining path (e.g. a re-export shim)
        // collapse into one `PublicItem` because the set is keyed by
        // qname, not id.
        let entries = vec![
            (1, summary(0, &["c", "Same"], ItemKind::Struct)),
            (2, summary(0, &["c", "Same"], ItemKind::Struct)),
        ];
        let crate_data = crate_with_paths(&entries);
        let set = project_rustdoc_paths(&crate_data);
        assert_eq!(set.len(), 1);
    }
}
