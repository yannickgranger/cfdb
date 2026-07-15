// arch-ban-rfc-053-syn-visibility-split-resolution.cypher
//   — RFC-053 §3.5 / §3.6 slice 53-C (issue #512); the first LIVE
//     split-resolution-point fence. Instantiates the external-type fence
//     template (`matchsite-external-type-fence.cypher`) for `syn::Visibility`.
//
// # Invariant encoded (single-owner AST → Visibility mapping)
//
// Per RFC-053 §1, the conversion `syn::Visibility` → `cfdb_core::Visibility`
// is a split-resolution-point risk: it is ONE logical mapping that must stay
// single-owner. It had a real duplication once (`parse_syn_visibility`
// bypassing `Visibility::FromStr`), collapsed by boy-scout #107
// (commit 2aedd013, 2026-04-20). Today the mapping lives at exactly one
// canonical site — `crates/cfdb-extractor/src/item_visitor.rs`
// (`parse_syn_visibility`), which self-documents as "the canonical (and
// only) AST → Visibility mapping". Before RFC-053 that canonicality was
// enforced by a doc-comment alone; nothing structural caught a second site
// reappearing. This rule is that structural guard.
//
// The fence: no `match` expression OUTSIDE the canonical module may dispatch
// on the external `syn::Visibility` type.
//
// # Why the fully-qualified `^syn::Visibility$` anchor (homonym separation)
//
// `matched_path` is the name-level arm-pattern prefix AS WRITTEN, never a
// resolved type (RFC-053 §3.1). Two same-named types collide at this level:
//   - the EXTERNAL `syn::Visibility` — the canonical site writes it fully
//     qualified, so its prefix is `syn::Visibility`;
//   - the WORKSPACE `cfdb_core::Visibility` — matched unqualified in
//     `crates/cfdb-core/src/visibility.rs` (`as_wire_str`), prefix
//     `Visibility`.
// A broad `(^|::)Visibility$` regex would fire on the workspace enum's
// `as_wire_str` site (a legitimate self-dispatch), a false positive. The
// principled complement — "exclude sites that carry a MATCHES_ON edge to a
// workspace enum" — is not realized here: NOT EXISTS in the v0.1 subset does not
// bind an outer-scope node inside a `NOT EXISTS { ... }` subquery
// (verified, `vertical-split-brain-drop.cypher:67-73`), so the resolved-edge
// absence cannot be a correlated anti-join on `m` via NOT EXISTS. (A
// correlated `OPTIONAL MATCH` + null-fill WHERE test remains uninvestigated
// as an alternative; this fence ships the proven anchored form.) The
// homonym is instead
// separated at the syntax level: an external type is only reachable by its
// qualified path `syn::Visibility`, so anchoring `matched_path` to
// `^syn::Visibility$` selects the external-type sites and none of the
// workspace ones. The workspace-enum companion form
// (`matchsite-workspace-enum-fence.cypher`) is homonym-proof the other way,
// via the resolved edge — see `docs/split-resolution-fences.md`.
//
// # Canonical-site designation (the one guardrail NOT-clause)
//
// `NOT m.file =~ '.*crates/cfdb-extractor/src/item_visitor.*'` exempts the one
// designated owner (RFC-053 §3.6: a fence REQUIRES a canonical-site
// designation). Per the guardrail there is at most ONE canonical-site
// NOT-clause in this file, and it never grows into an exception list — a
// rule file that accretes NOT-clauses is a de facto allowlist and is
// rejected on sight (§3.6 / project CLAUDE.md §3).
//
// # File location — documented deviation from RFC/issue `.cfdb/queries/`
//
// RFC-053 §7 and issue #512 name `.cfdb/queries/` for the live fence. This
// rule ships in `examples/queries/` instead, because that is where the
// ENFORCING globs live: the PR-time self-audit (`.gitea/workflows/ci.yml`
// ~line 249), the nightly HIR self-audit
// (`ci-hir-self-audit-nightly.yml` ~line 105), and cross-dogfood
// (`ci/cross-dogfood.sh` ~line 86) all iterate
// `examples/queries/arch-ban-*.cypher`. `.cfdb/queries/*.cypher` is only
// SMOKE-executed (row count ignored, ci.yml ~line 283) — never run through
// `cfdb violations` as a merge gate. A fence placed there would be inert.
// Same resolution as the sibling `arch-ban-rfc-043-*.cypher` rules; zero
// CI-workflow edits required.
//
// # PR-time viability (syn keyspace, not deferred to nightly)
//
// `:MatchSite` and its `matched_path` are emitted by the SYN extractor
// (53-A), so this fence evaluates on the fast PR-time `cfdb-self` syn
// keyspace. The self-audit HIR-defer filter keys on the HIR-only labels
// (entry-point nodes, the export edge, the resolved-call edge) — none of
// which this rule names — so it is NOT deferred; it blocks at PR time.
// (This header itself avoids spelling those labels literally, so the
// filter's own grep does not misclassify this file.)
//
// # Expected — zero rows on develop; cross-dogfood clean
//
// cfdb-self: the only `^syn::Visibility$` site is the canonical one, exempt
// by the NOT-clause ⇒ 0 rows. graph-specs-rust (pinned companion): its only
// `Visibility` matches are `matches!(...)` MACRO invocations, a named v0
// exclusion that emits no `:MatchSite` (§6); it has no fully-qualified
// `syn::Visibility::` match arm ⇒ 0 rows. qbot-core: no `syn` dependency in
// domain code ⇒ 0 rows.
//
// # Usage
//   cfdb violations --db <dir> --keyspace <ks> \
//     --rule examples/queries/arch-ban-rfc-053-syn-visibility-split-resolution.cypher
//
// Any row is a `match` on `syn::Visibility` outside the canonical module —
// a re-opened split resolution point. Route it back through
// `parse_syn_visibility`.

//
// Known name-level evasion (shared with the RFC's own broad-regex sketch,
// accepted §6 envelope): an import alias (`use syn::Visibility as V;`)
// records prefix `V`, which this rule does not match. The HIR tier (§6)
// closes alias-level resolution.
MATCH (m:MatchSite)
WHERE m.matched_path =~ '^syn::Visibility$'
  AND m.is_test = false
  AND NOT m.file =~ '.*crates/cfdb-extractor/src/item_visitor.*'
RETURN m.file AS file,
       m.line AS line,
       m.matched_path AS matched_path
ORDER BY file ASC, line ASC
