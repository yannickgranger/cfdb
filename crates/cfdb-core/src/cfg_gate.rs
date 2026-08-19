use std::fmt;
use std::str::FromStr;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CfgGate {
    Feature(String),
    All(Vec<CfgGate>),
    Any(Vec<CfgGate>),
    Not(Box<CfgGate>),
}

impl CfgGate {
    pub fn evaluate(&self, enabled: &[&str]) -> bool {
        match self {
            CfgGate::Feature(name) => enabled.contains(&name.as_str()),
            CfgGate::All(xs) => xs.iter().all(|x| x.evaluate(enabled)),
            CfgGate::Any(xs) => xs.iter().any(|x| x.evaluate(enabled)),
            CfgGate::Not(x) => !x.evaluate(enabled),
        }
    }

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
        let gate =
            parse(r#"all(feature = "async", any(feature = "tokio", not(feature = "legacy")))"#);
        assert!(gate.evaluate(&["async", "tokio"]));
        assert!(gate.evaluate(&["async"]));
        assert!(!gate.evaluate(&["async", "legacy"]));
        assert!(!gate.evaluate(&["tokio"]));
    }

    #[test]
    fn parser_rejects_malformed_inputs() {
        assert!(CfgGate::from_str("feature").is_err());
        assert!(CfgGate::from_str(r#"feature = async"#).is_err());
        assert!(CfgGate::from_str("all()").is_err());
        assert!(CfgGate::from_str("any()").is_err());
        assert!(CfgGate::from_str("target_os = \"linux\"").is_err());
        assert!(CfgGate::from_str(r#"feature = "x" extra"#).is_err());
    }
}
