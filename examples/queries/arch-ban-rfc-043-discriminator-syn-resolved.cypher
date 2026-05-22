// arch-ban-rfc-043-discriminator-syn-resolved.cypher
//   — RFC-044 §3.8 slice 044-H (issue #427); encodes the
//     Label::CALL_SITE discriminator contract (RFC-029 §A1.2 / RFC-043 §4).
//
// # Invariant encoded (CallSite discriminator contract)
//
// `cfdb_core::schema::Label::CALL_SITE` publishes a two-property
// discriminator contract (SchemaVersion v0.1.3+, `labels.rs:27-41`):
//
//   - `resolver`         ∈ {"syn", "hir"} — which extractor produced the node
//   - `callee_resolved`  bool — `true` ONLY when method dispatch / re-export /
//                         trait impl was resolved via HIR.
//
// Because `callee_resolved = true` means "resolved via HIR", the implication
//
//     callee_resolved = true  ⟹  resolver = "hir"
//
// is part of the contract. A node carrying `resolver = "syn"` (the
// unresolved name-based `cfdb-extractor`) together with
// `callee_resolved = true` is a contract violation: the syn extractor
// cannot resolve dispatch, so it must never claim a resolved callee. This
// is the homonym-mitigation invariant RFC-043 §4 holds the dual-extractor
// pipeline to; before this rule it was reviewer-only.
//
// # Inversion (positive invariant → its negation)
//
// The positive invariant is the implication above. Its negation —
// `callee_resolved = true AND resolver <> "hir"` — is exactly the set of
// violating rows. A clean tree yields none; every returned row is a
// CallSite whose discriminator pair is internally inconsistent.
//
// # File location — documented deviation from RFC §3.8
//
// RFC §3.8 literally says `.cfdb/queries/`. This rule ships in
// `examples/queries/` because the self-dogfood loop
// (`.gitea/workflows/ci.yml` ~line 201) and cross-dogfood loop
// (`ci/cross-dogfood.sh` ~line 86) already glob
// `examples/queries/arch-ban-*.cypher`. Zero CI-workflow edits required.
//
// # Usage
//   cfdb violations --db <dir> --keyspace <ks> \
//     --rule examples/queries/arch-ban-rfc-043-discriminator-syn-resolved.cypher
//
// Expected: empty on a clean tree. Any row is a CallSite whose
// (resolver, callee_resolved) pair violates the discriminator contract.

MATCH (c:CallSite)
WHERE c.callee_resolved = true
  AND c.resolver <> 'hir'
RETURN c.caller_qname AS caller_qname,
       c.callee_path AS callee_path,
       c.resolver AS resolver,
       c.file AS file,
       c.line AS line
ORDER BY file ASC, line ASC
