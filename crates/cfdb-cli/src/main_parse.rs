#[cfg(feature = "classify")]
use cfdb_classify::{TriggerId, UnknownTriggerId};
use cfdb_core::{ItemKind, UnknownItemKind};

pub(crate) fn parse_item_kind(s: &str) -> Result<ItemKind, UnknownItemKind> {
    s.parse::<ItemKind>()
}

#[cfg(feature = "classify")]
pub(crate) fn parse_trigger_id(s: &str) -> Result<TriggerId, UnknownTriggerId> {
    s.parse::<TriggerId>()
}
