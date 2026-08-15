//! `ItemKind` vocabulary — wire-form surface for the `list_items_matching`
//! verb. The variants map to the cfdb syn extractor's lowercase emission
//! strings via [`ItemKind::to_extractor_str`].

use serde::{Deserialize, Serialize};

/// Kind vocabulary for the `list_items_matching` verb. The variants are
/// the wire-form surface; they map to the extractor's emitted `:Item.kind`
/// strings via [`ItemKind::to_extractor_str`].
///
/// Variant order is canonical — consumers that iterate [`ItemKind::variants`]
/// get a stable order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ItemKind {
    Struct,
    Enum,
    Fn,
    Const,
    TypeAlias,
    ImplBlock,
    Trait,
    Static,
    Union,
}

impl ItemKind {
    /// Canonical list of all variants in stable order (callers can depend on it).
    pub fn variants() -> &'static [ItemKind] {
        &[
            ItemKind::Struct,
            ItemKind::Enum,
            ItemKind::Fn,
            ItemKind::Const,
            ItemKind::TypeAlias,
            ItemKind::ImplBlock,
            ItemKind::Trait,
            ItemKind::Static,
            ItemKind::Union,
        ]
    }

    /// Map a kind to the string the cfdb syn extractor emits as `:Item.kind`.
    /// The extractor's vocabulary is lowercase and diverges from the kind names
    /// in two spots: `TypeAlias → "type_alias"` and `ImplBlock → "impl_block"`.
    /// The extractor emits one `:Item { kind: "impl_block" }` per `impl ... {}`
    /// block alongside `IMPLEMENTS` + `IMPLEMENTS_FOR` edges. Older keyspaces
    /// may have zero `impl_block` items.
    pub fn to_extractor_str(self) -> &'static str {
        match self {
            ItemKind::Struct => "struct",
            ItemKind::Enum => "enum",
            ItemKind::Fn => "fn",
            ItemKind::Const => "const",
            ItemKind::TypeAlias => "type_alias",
            ItemKind::Trait => "trait",
            ItemKind::ImplBlock => "impl_block",
            ItemKind::Static => "static",
            ItemKind::Union => "union",
        }
    }

    /// Human-readable council-spelled name (the same spelling users type on
    /// the CLI).
    pub fn as_str(self) -> &'static str {
        match self {
            ItemKind::Struct => "Struct",
            ItemKind::Enum => "Enum",
            ItemKind::Fn => "Fn",
            ItemKind::Const => "Const",
            ItemKind::TypeAlias => "TypeAlias",
            ItemKind::ImplBlock => "ImplBlock",
            ItemKind::Trait => "Trait",
            ItemKind::Static => "Static",
            ItemKind::Union => "Union",
        }
    }
}

impl std::fmt::Display for ItemKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for ItemKind {
    type Err = UnknownItemKind;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Struct" => Ok(ItemKind::Struct),
            "Enum" => Ok(ItemKind::Enum),
            "Fn" => Ok(ItemKind::Fn),
            "Const" => Ok(ItemKind::Const),
            "TypeAlias" => Ok(ItemKind::TypeAlias),
            "ImplBlock" => Ok(ItemKind::ImplBlock),
            "Trait" => Ok(ItemKind::Trait),
            "Static" => Ok(ItemKind::Static),
            "Union" => Ok(ItemKind::Union),
            other => Err(UnknownItemKind(other.to_string())),
        }
    }
}

/// Parse error for [`ItemKind`]'s `FromStr`. Carries the rejected input so the
/// caller can format a user-facing message that enumerates valid values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownItemKind(pub String);

impl std::fmt::Display for UnknownItemKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "unknown Item kind `{}` — valid values: {}",
            self.0,
            ItemKind::variants()
                .iter()
                .map(|k| k.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

impl std::error::Error for UnknownItemKind {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_kind_variants_enumerates_all_nine_in_order() {
        let variants = ItemKind::variants();
        assert_eq!(
            variants,
            &[
                ItemKind::Struct,
                ItemKind::Enum,
                ItemKind::Fn,
                ItemKind::Const,
                ItemKind::TypeAlias,
                ItemKind::ImplBlock,
                ItemKind::Trait,
                ItemKind::Static,
                ItemKind::Union,
            ],
            "variants() must expose the 7 council-ratified kinds in order, \
             then the #479/#515 additions"
        );
    }

    #[test]
    fn item_kind_fromstr_display_roundtrips_every_variant() {
        use std::str::FromStr;
        for k in ItemKind::variants() {
            let spelled = k.to_string();
            let parsed = ItemKind::from_str(&spelled).expect("roundtrip of council-spelled name");
            assert_eq!(&parsed, k, "FromStr/Display roundtrip for {k:?}");
        }
    }

    #[test]
    fn item_kind_fromstr_rejects_unknown() {
        use std::str::FromStr;
        let err = ItemKind::from_str("impl").expect_err("lowercase rejected");
        assert_eq!(err.0, "impl");
        let err2 = ItemKind::from_str("NotAKind").expect_err("nonsense rejected");
        assert_eq!(err2.0, "NotAKind");
    }

    /// #479 red — `static` has been a wire value since the first
    /// extractor release and `union` sits in recall's KEPT_ITEM_KINDS,
    /// yet neither could be spelled on the CLI (`--kind Static` /
    /// `--kind Union` errored). String-level on purpose: compiles
    /// before the variants exist, so the red is honest.
    #[test]
    fn item_kind_fromstr_accepts_static_and_union() {
        use std::str::FromStr;
        let parsed = ItemKind::from_str("Static")
            .expect("`Static` must parse — the wire value `static` exists on every keyspace");
        assert_eq!(parsed.to_extractor_str(), "static");
        let parsed = ItemKind::from_str("Union")
            .expect("`Union` must parse — recall KEPT_ITEM_KINDS names the wire value");
        assert_eq!(parsed.to_extractor_str(), "union");
    }

    #[test]
    fn item_kind_to_extractor_str_maps_every_variant() {
        // Pins the AC vocabulary → extractor vocabulary mapping table.
        // Every variant emits a concrete `kind` string that appears on
        // real `:Item` nodes. `ImplBlock` mapped to `"<unemitted:impl_block>"`
        // pre-#42 because the extractor did not walk impl blocks;
        // post-#42 (SchemaVersion V0_2_2) every `impl ... {}` emits
        // `kind = "impl_block"` so the sentinel is no longer needed.
        assert_eq!(ItemKind::Struct.to_extractor_str(), "struct");
        assert_eq!(ItemKind::Enum.to_extractor_str(), "enum");
        assert_eq!(ItemKind::Fn.to_extractor_str(), "fn");
        assert_eq!(ItemKind::Const.to_extractor_str(), "const");
        assert_eq!(ItemKind::TypeAlias.to_extractor_str(), "type_alias");
        assert_eq!(ItemKind::Trait.to_extractor_str(), "trait");
        assert_eq!(ItemKind::ImplBlock.to_extractor_str(), "impl_block");
        assert_eq!(ItemKind::Static.to_extractor_str(), "static");
        assert_eq!(ItemKind::Union.to_extractor_str(), "union");
    }
}
