//! clap value parsers for `cfdb` CLI flags that bind to domain enums.
//!
//! Split out of `main.rs` as part of the #128 god-file split. These two
//! parsers delegate to the canonical `FromStr` impls on their respective
//! domain types so the CLI surface is bound to the council-ratified
//! vocabularies — no hardcoded string lists (global CLAUDE.md §7 MCP/CLI
//! boundary fix AC).

use cfdb_cli::{TriggerId, UnknownTriggerId};
use cfdb_core::{ItemKind, UnknownItemKind};

/// clap value parser for a single `--kinds` entry. Delegates to
/// [`ItemKind::from_str`] so the CLI surface is bound to the council-ratified
/// vocabulary; unknown values exit with code 2 (clap default for value
/// parser errors).
pub(crate) fn parse_item_kind(s: &str) -> Result<ItemKind, UnknownItemKind> {
    s.parse::<ItemKind>()
}

/// clap value parser for `--trigger`. Delegates to
/// [`TriggerId::from_str`] so the valid-values enumeration in the
/// error message is derived from the domain enum itself — no
/// hardcoded string list (global CLAUDE.md §7 MCP/CLI boundary fix AC).
pub(crate) fn parse_trigger_id(s: &str) -> Result<TriggerId, UnknownTriggerId> {
    s.parse::<TriggerId>()
}
