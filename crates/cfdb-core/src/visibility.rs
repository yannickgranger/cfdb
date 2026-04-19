//! `Visibility` — Rust item visibility for `:Item` fact attributes.
//!
//! Added in SchemaVersion v0.1.1 per RFC-032 Group A / Issue #35. Captures
//! the five forms an item can carry in Rust source:
//!
//! - `Public`           — `pub`
//! - `CrateLocal`       — `pub(crate)`
//! - `Module`           — `pub(super)` or `pub(self)` (module-scope)
//! - `Private`          — no modifier (inherited)
//! - `Restricted(path)` — `pub(in some::module::path)` with arbitrary path
//!
//! This is a programmatic type. Items store visibility on the wire as a
//! `PropValue::Str` formatted by `Visibility::Display`; `Visibility::FromStr`
//! is the inverse. Canonical wire strings:
//!
//! | variant              | wire string        |
//! |----------------------|--------------------|
//! | `Public`             | `"pub"`            |
//! | `CrateLocal`         | `"pub(crate)"`     |
//! | `Module`             | `"pub(super)"`     |
//! | `Private`            | `"private"`        |
//! | `Restricted(p)`      | `"pub(in {p})"`    |
//!
//! The `Module` variant always renders as `pub(super)` even when the original
//! source was `pub(self)` — the two are semantically equivalent at this
//! granularity, and collapsing them keeps the wire vocabulary closed. If a
//! future consumer needs the distinction, it becomes a second variant and a
//! SchemaVersion bump.

use std::fmt;
use std::str::FromStr;

/// Visibility of a Rust item as it appears in `:Item.visibility` (RFC-033
/// §7 Group A1 / Issue #35).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Visibility {
    Public,
    CrateLocal,
    Module,
    Private,
    Restricted(String),
}

impl Visibility {
    /// Canonical wire string — stable across SchemaVersion bumps within the
    /// same major. Used to round-trip through `PropValue::Str`.
    pub fn as_wire_str(&self) -> String {
        match self {
            Visibility::Public => "pub".into(),
            Visibility::CrateLocal => "pub(crate)".into(),
            Visibility::Module => "pub(super)".into(),
            Visibility::Private => "private".into(),
            Visibility::Restricted(path) => format!("pub(in {path})"),
        }
    }
}

impl fmt::Display for Visibility {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.as_wire_str())
    }
}

impl FromStr for Visibility {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pub" => Ok(Visibility::Public),
            "pub(crate)" => Ok(Visibility::CrateLocal),
            "pub(super)" | "pub(self)" => Ok(Visibility::Module),
            "private" | "" => Ok(Visibility::Private),
            s if s.starts_with("pub(in ") && s.ends_with(')') => {
                let path = &s["pub(in ".len()..s.len() - 1];
                if path.is_empty() {
                    Err(format!("empty pub(in ...) path: {s:?}"))
                } else {
                    Ok(Visibility::Restricted(path.to_string()))
                }
            }
            _ => Err(format!("unrecognised visibility: {s:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_matches_wire_str() {
        for v in [
            Visibility::Public,
            Visibility::CrateLocal,
            Visibility::Module,
            Visibility::Private,
            Visibility::Restricted("a::b::c".into()),
        ] {
            assert_eq!(v.to_string(), v.as_wire_str());
        }
    }

    #[test]
    fn round_trip_through_wire_string() {
        for v in [
            Visibility::Public,
            Visibility::CrateLocal,
            Visibility::Module,
            Visibility::Private,
            Visibility::Restricted("a::b::c".into()),
        ] {
            let wire = v.as_wire_str();
            let back: Visibility = wire.parse().expect("wire form round-trips");
            assert_eq!(v, back);
        }
    }

    #[test]
    fn pub_self_collapses_to_module() {
        // `pub(self)` is semantically equivalent to `pub(super)` at this
        // granularity — both are "module scope, not crate-wide".
        let parsed: Visibility = "pub(self)".parse().expect("pub(self) is valid");
        assert_eq!(parsed, Visibility::Module);
        // Display always renders as pub(super) — the canonical form.
        assert_eq!(parsed.to_string(), "pub(super)");
    }

    #[test]
    fn empty_string_is_private() {
        let parsed: Visibility = "".parse().expect("empty visibility == private");
        assert_eq!(parsed, Visibility::Private);
    }

    #[test]
    fn restricted_with_nested_path() {
        let parsed: Visibility = "pub(in foo::bar::baz)"
            .parse()
            .expect("pub(in ...) is valid");
        assert_eq!(parsed, Visibility::Restricted("foo::bar::baz".into()));
        assert_eq!(parsed.to_string(), "pub(in foo::bar::baz)");
    }

    #[test]
    fn unknown_visibility_is_rejected() {
        assert!("pub(super::mod)".parse::<Visibility>().is_err());
        assert!("hidden".parse::<Visibility>().is_err());
        assert!("pub(in )".parse::<Visibility>().is_err());
    }
}
