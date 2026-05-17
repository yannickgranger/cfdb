# RFC-042 council synthesis — Round 2

**Date:** 2026-05-17
**Convener:** captain (a0 session)
**Status:** 4/4 RATIFY — proceeding to ratification.

---

## 1. Verdict roll-up

| Lens | R1 | R2 | Resolved by |
|---|---|---|---|
| `clean-arch` | REQUEST CHANGES (2) | **RATIFY** | EDIT 1 (Position B) — `mod enrich` is crate-private; `ReachabilityFilter` cannot leak. Composition root unambiguously `PetgraphStore::enrich_reachability`. |
| `ddd-specialist` | REQUEST CHANGES (2) | **RATIFY** | EDIT 2 — homonym disambiguation sentence + two new `:Item` attribute descriptors with `Provenance::EnrichReachability`. Tests tightenings (≥ grep count, JupiterCryptoBroker spot-check, ≥5 reclassified spot-audit) confirmed in §7. |
| `solid-architect` | REQUEST CHANGES (2) | **RATIFY** | EDIT 3 (test_bench.rs sibling file with CCP rationale in module-doc) + EDIT 1 (trait surface unchanged). EDIT 8 sibling .cypher hygiene MUST language is binding. |
| `rust-systems` | REQUEST CHANGES (1 blocking + 3 non-blocking) | **RATIFY** | EDIT 1 "Trait surface impact" subsection answers RS-1 precisely (signature preserved verbatim; `pub(crate)` enum; `attrs_written` sums). EDITs 4/5/6 cover RS-3/5/6. |

**All four R2 verdicts are unconditional RATIFY.** No further iteration required.

---

## 2. Non-blocking residual notes (implementation-time, not RFC text)

These were flagged at R2 as informational notes for the 042-A and 042-B implementers. None requires RFC text change.

1. **(solid-architect)** `ReachabilityFilter` must not appear in any `pub` re-export. Verify during 042-B implementation that the enum's visibility stays `pub(crate)` and no module re-exports it.

2. **(solid-architect)** `has_test_attr` / `has_bench_attr` unit tests should be co-located in `test_bench.rs #[cfg(test)] mod tests`, not a separate file. (RFC §7 042-A unit prescription is consistent with this — it names the file as the test site.)

3. **(rust-systems)** `ReachabilityFilter::ProductionOnly` filter implementation reads `node.props.get("kind")` as `PropValue::Str` and excludes `"test"` and `"bench"`. The implementer derives this from the schema vocabulary; the RFC correctly leaves it as an implementation detail.

4. **(ddd-specialist)** Companion follow-up PR caveat: if the initial extract of graph-specs-rust against the new cfdb HEAD produces non-zero rows on `arch-test-only-reachable-production-items.cypher`, the PR MUST either fix findings or explicitly switch to cleanup-driving with a filed issue. The synthesis §5 "Follow-up PR plan" bullet 2 covers this, but the explicit operator decision is non-silent.

5. **(clean-arch)** RFC §3.3 "MUST sum per-pass counters" rule is enforced by the determinism unit test in the 042-B prescription. No additional RFC text needed.

These notes are recorded here so the 042-A / 042-B implementers can consult the synthesis if they hit ambiguity.

---

## 3. Next action

Convener proceeds to ratification (`council/RFC-042/RATIFIED.md`), records each lens verdict in RFC-042 §5.1-5.4, updates the RFC status line to `RATIFIED`, files slice issues 042-A / 042-B / 042-C against `agency:yg/cfdb`, files the companion follow-up PR plan, and gracefully shuts down the four lens teammates.

End of synthesis.
