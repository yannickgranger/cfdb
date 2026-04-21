// Params: $type_pattern (regex:<pattern>), $fin_precision_crates (list:<a,b,c>)
// Returns: (qname, line, reason) — canonical three-column violation format.
//
// Detect a public `fn` :Item whose rendered signature matches a type-regex
// AND whose `.crate` is in a caller-supplied list. Canonical use:
// "public fn returns Decimal in a crate matching financial-precision-crates.toml"
// where the caller binds $fin_precision_crates from a pre-loaded list of
// crate names (Slice 1 resolver at cfdb-cli::param_resolver — #145).
//
// All composition is at the top-level WHERE (no subquery) so the v0.1
// evaluator limitations on inner-scope bindings do not apply.
MATCH (i:Item)
WHERE i.kind = 'fn'
  AND i.visibility = 'pub'
  AND i.signature =~ $type_pattern
  AND i.crate IN $fin_precision_crates
RETURN i.qname AS qname, i.line AS line, 'public fn signature matches type-pattern in precision-crate set' AS reason
ORDER BY qname
