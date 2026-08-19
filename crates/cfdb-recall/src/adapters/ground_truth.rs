use std::collections::BTreeSet;
#[cfg(feature = "runner")]
use std::path::Path;

use rustdoc_types::{Crate, ItemKind};
#[cfg(feature = "runner")]
use thiserror::Error;

use crate::PublicItem;

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

pub fn project_rustdoc_paths(crate_data: &Crate) -> BTreeSet<PublicItem> {
    crate_data
        .paths
        .values()
        .filter(|summary| summary.crate_id == 0)
        .filter(|summary| KEPT_ITEM_KINDS.contains(&summary.kind))
        .filter(|summary| !summary.path.is_empty())
        .map(|summary| PublicItem::new(summary.path.join("::")))
        .collect()
}

#[cfg(feature = "runner")]
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

#[cfg(feature = "runner")]
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
        let entries = vec![(1, summary(0, &["c", "m", "Deep"], ItemKind::Struct))];
        let crate_data = crate_with_paths(&entries);
        let set = project_rustdoc_paths(&crate_data);
        assert!(set.contains(&PublicItem::new("c::m::Deep")));
    }

    #[test]
    fn deduplicates_identical_paths() {
        let entries = vec![
            (1, summary(0, &["c", "Same"], ItemKind::Struct)),
            (2, summary(0, &["c", "Same"], ItemKind::Struct)),
        ];
        let crate_data = crate_with_paths(&entries);
        let set = project_rustdoc_paths(&crate_data);
        assert_eq!(set.len(), 1);
    }
}
