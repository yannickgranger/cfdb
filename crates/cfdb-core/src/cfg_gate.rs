//! `CfgGate` — `#[cfg(feature = "…")]` expression tree captured on `:Item`.
//!
//! Added in SchemaVersion v0.1.2 per Issue #36. Represents the subset of
//! Rust's `cfg()` attribute language that is relevant to rescue signal:
//! feature predicates and their logical combinators.
//!
//! Recognised shapes:
//! - `feature = "x"`           → [`CfgGate::Feature`]
//! - `all(a, b, …)`            → [`CfgGate::All`]
//! - `any(a, b, …)`            → [`CfgGate::Any`]
//! - `not(a)`                  → [`CfgGate::Not`]
//!
//! **Scope limitation.** Non-feature cfg predicates (`cfg(test)`,
//! `cfg(target_os = "linux")`, `cfg(unix)`, …) are NOT captured. The
//! extractor's `extract_cfg_feature_gate` helper returns `None` for any
//! item whose cfg expression contains a non-feature leaf — the
//! all-or-nothing policy keeps the wire vocabulary closed (mixing
//! partial-capture with full-capture items would force every consumer to
//! decide whether missing-leaf means "feature-active" or "unknown").
//!
//! Multiple `#[cfg(...)]` attributes on the same item conjoin (Rust
//! semantics). The extractor combines them into a single `CfgGate::All`
//! before emission.
//!
//! Wire form: the `Display` output of the inner expression, stored as
//! `PropValue::Str` on `:Item.cfg_gate`. The outer `cfg(...)` wrap is
//! implicit — it's redundant given the property name and would force
//! consumers to strip it on every read.

use std::fmt;
use std::str::FromStr;

/// A feature-only `cfg()` expression captured on `:Item.cfg_gate`.
///
/// See module-level docs for the wire shape and scope limitations.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CfgGate {
    /// `feature = "x"` — leaf predicate.
    Feature(String),
    /// `all(a, b, …)` — logical AND. At least one child.
    All(Vec<CfgGate>),
    /// `any(a, b, …)` — logical OR. At least one child.
    Any(Vec<CfgGate>),
    /// `not(a)` — logical NOT.
    Not(Box<CfgGate>),
}

impl CfgGate {
    /// Evaluate the gate against an explicit enabled-feature set. Returns
    /// `true` when the item is compiled in under that feature selection.
    ///
    /// Consumers implement the "default features on / off / custom" hook
    /// by passing the appropriate slice — the gate itself is
    /// feature-selection-agnostic.
    pub fn evaluate(&self, enabled: &[&str]) -> bool {
        match self {
            CfgGate::Feature(name) => enabled.contains(&name.as_str()),
            CfgGate::All(xs) => xs.iter().all(|x| x.evaluate(enabled)),
            CfgGate::Any(xs) => xs.iter().any(|x| x.evaluate(enabled)),
            CfgGate::Not(x) => !x.evaluate(enabled),
        }
    }

    /// The canonical wire string — same as `Display`. Separate accessor so
    /// callers that want `String` (e.g. `PropValue::Str`) can skip the
    /// `format!` allocation wrapper.
    pub fn as_wire_str(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for CfgGate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CfgGate::Feature(name) => write!(f, "feature = {name:?}"),
            CfgGate::All(xs) => write_list(f, "all", xs),
            CfgGate::Any(xs) => write_list(f, "any", xs),
            CfgGate::Not(x) => write!(f, "not({x})"),
        }
    }
}

fn write_list(f: &mut fmt::Formatter<'_>, tag: &str, xs: &[CfgGate]) -> fmt::Result {
    write!(f, "{tag}(")?;
    for (i, x) in xs.iter().enumerate() {
        if i > 0 {
            f.write_str(", ")?;
        }
        write!(f, "{x}")?;
    }
    f.write_str(")")
}

/// Minimal recursive parser for the canonical wire form emitted by
/// `Display`. Accepts the exact shapes defined at the module level and
/// rejects anything else. Intended for round-trip tests and for downstream
/// tools that want to consume the string form without pulling `syn`.
impl FromStr for CfgGate {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (gate, rest) = parse_expr(s.trim_start())?;
        if !rest.trim_start().is_empty() {
            return Err(format!("trailing input after cfg expression: {rest:?}"));
        }
        Ok(gate)
    }
}

fn parse_expr(input: &str) -> Result<(CfgGate, &str), String> {
    let s = input.trim_start();
    if let Some(rest) = s.strip_prefix("all(") {
        parse_all_expr(rest)
    } else if let Some(rest) = s.strip_prefix("any(") {
        parse_any_expr(rest)
    } else if let Some(rest) = s.strip_prefix("not(") {
        parse_not_expr(rest)
    } else if let Some(rest) = s.strip_prefix("feature") {
        parse_feature_expr(rest)
    } else {
        Err(format!("unrecognised cfg expression prefix: {s:?}"))
    }
}

fn parse_all_expr(rest: &str) -> Result<(CfgGate, &str), String> {
    let (children, rest) = parse_list(rest)?;
    if children.is_empty() {
        return Err("all(...) with no children".into());
    }
    Ok((CfgGate::All(children), rest))
}

fn parse_any_expr(rest: &str) -> Result<(CfgGate, &str), String> {
    let (children, rest) = parse_list(rest)?;
    if children.is_empty() {
        return Err("any(...) with no children".into());
    }
    Ok((CfgGate::Any(children), rest))
}

fn parse_not_expr(rest: &str) -> Result<(CfgGate, &str), String> {
    let (inner, rest) = parse_expr(rest)?;
    let rest = rest.trim_start();
    let rest = rest
        .strip_prefix(')')
        .ok_or_else(|| format!("expected ')' after not(...): {rest:?}"))?;
    Ok((CfgGate::Not(Box::new(inner)), rest))
}

fn parse_feature_expr(rest: &str) -> Result<(CfgGate, &str), String> {
    let rest = rest.trim_start();
    let rest = rest
        .strip_prefix('=')
        .ok_or_else(|| format!("expected '=' after feature: {rest:?}"))?;
    let rest = rest.trim_start();
    let rest = rest
        .strip_prefix('"')
        .ok_or_else(|| format!("expected '\"' for feature name: {rest:?}"))?;
    let end = rest
        .find('"')
        .ok_or_else(|| "unterminated feature-name string".to_string())?;
    let name = &rest[..end];
    Ok((CfgGate::Feature(name.to_string()), &rest[end + 1..]))
}

/// Parse a `,`-separated list of expressions up to the matching `)`. The
/// caller has already consumed the opening `(`.
fn parse_list(input: &str) -> Result<(Vec<CfgGate>, &str), String> {
    let mut children = Vec::new();
    let mut rest = input.trim_start();
    if let Some(after_paren) = rest.strip_prefix(')') {
        return Ok((children, after_paren));
    }
    loop {
        let (child, tail) = parse_expr(rest)?;
        children.push(child);
        let tail = tail.trim_start();
        if let Some(after_paren) = tail.strip_prefix(')') {
            return Ok((children, after_paren));
        }
        rest = tail
            .strip_prefix(',')
            .ok_or_else(|| format!("expected ',' or ')' in list: {tail:?}"))?;
        rest = rest.trim_start();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> CfgGate {
        CfgGate::from_str(s).expect("test fixture parses")
    }

    #[test]
    fn display_feature_quotes_the_name() {
        assert_eq!(
            CfgGate::Feature("async".into()).to_string(),
            r#"feature = "async""#
        );
    }

    #[test]
    fn display_all_and_any_comma_separated() {
        let gate = CfgGate::All(vec![
            CfgGate::Feature("a".into()),
            CfgGate::Feature("b".into()),
        ]);
        assert_eq!(gate.to_string(), r#"all(feature = "a", feature = "b")"#);

        let gate = CfgGate::Any(vec![
            CfgGate::Feature("x".into()),
            CfgGate::Feature("y".into()),
        ]);
        assert_eq!(gate.to_string(), r#"any(feature = "x", feature = "y")"#);
    }

    #[test]
    fn display_not_wraps_once() {
        let gate = CfgGate::Not(Box::new(CfgGate::Feature("legacy".into())));
        assert_eq!(gate.to_string(), r#"not(feature = "legacy")"#);
    }

    #[test]
    fn round_trip_through_wire_string() {
        let gates = [
            CfgGate::Feature("async".into()),
            CfgGate::All(vec![
                CfgGate::Feature("a".into()),
                CfgGate::Feature("b".into()),
            ]),
            CfgGate::Any(vec![
                CfgGate::Feature("x".into()),
                CfgGate::Not(Box::new(CfgGate::Feature("legacy".into()))),
            ]),
            CfgGate::Not(Box::new(CfgGate::All(vec![
                CfgGate::Feature("a".into()),
                CfgGate::Any(vec![
                    CfgGate::Feature("b".into()),
                    CfgGate::Feature("c".into()),
                ]),
            ]))),
        ];
        for g in gates {
            let wire = g.to_string();
            let back = parse(&wire);
            assert_eq!(g, back, "failed round-trip for {wire}");
        }
    }

    #[test]
    fn evaluate_feature_matches_enabled_set() {
        let gate = CfgGate::Feature("async".into());
        assert!(gate.evaluate(&["async"]));
        assert!(gate.evaluate(&["async", "tokio"]));
        assert!(!gate.evaluate(&["tokio"]));
        assert!(!gate.evaluate(&[]));
    }

    #[test]
    fn evaluate_all_requires_every_child() {
        let gate = parse(r#"all(feature = "a", feature = "b")"#);
        assert!(gate.evaluate(&["a", "b"]));
        assert!(!gate.evaluate(&["a"]));
        assert!(!gate.evaluate(&["b"]));
        assert!(!gate.evaluate(&[]));
    }

    #[test]
    fn evaluate_any_requires_at_least_one_child() {
        let gate = parse(r#"any(feature = "a", feature = "b")"#);
        assert!(gate.evaluate(&["a"]));
        assert!(gate.evaluate(&["b"]));
        assert!(gate.evaluate(&["a", "b", "c"]));
        assert!(!gate.evaluate(&["c"]));
        assert!(!gate.evaluate(&[]));
    }

    #[test]
    fn evaluate_not_negates_child() {
        let gate = parse(r#"not(feature = "legacy")"#);
        assert!(gate.evaluate(&[]));
        assert!(gate.evaluate(&["modern"]));
        assert!(!gate.evaluate(&["legacy"]));
        assert!(!gate.evaluate(&["legacy", "modern"]));
    }

    #[test]
    fn evaluate_nested_expression() {
        // all(feature = "async", any(feature = "tokio", not(feature = "legacy")))
        let gate =
            parse(r#"all(feature = "async", any(feature = "tokio", not(feature = "legacy")))"#);
        assert!(gate.evaluate(&["async", "tokio"]));
        assert!(gate.evaluate(&["async"])); // tokio missing but legacy also missing → not(legacy) true
        assert!(!gate.evaluate(&["async", "legacy"])); // tokio missing and legacy on
        assert!(!gate.evaluate(&["tokio"])); // async missing
    }

    #[test]
    fn parser_rejects_malformed_inputs() {
        assert!(CfgGate::from_str("feature").is_err());
        assert!(CfgGate::from_str(r#"feature = async"#).is_err()); // unquoted name
        assert!(CfgGate::from_str("all()").is_err()); // empty list
        assert!(CfgGate::from_str("any()").is_err());
        assert!(CfgGate::from_str("target_os = \"linux\"").is_err()); // not a feature
        assert!(CfgGate::from_str(r#"feature = "x" extra"#).is_err()); // trailing
    }
}
