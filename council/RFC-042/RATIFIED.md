# RFC-042 — RATIFIED

**Date:** 2026-05-17
**Convener:** captain (a0 session)
**RFC document:** `docs/RFC-042-test-bench-entry-points.md`
**Originating issue:** [`yg/cfdb#378`](https://agency.lab:3000/yg/cfdb/issues/378)

---

## Council verdicts (final)

| Lens | R1 | R2 | Final |
|---|---|---|---|
| `clean-arch` | REQUEST CHANGES | RATIFY | **RATIFY** |
| `ddd-specialist` | REQUEST CHANGES | RATIFY | **RATIFY** |
| `solid-architect` | REQUEST CHANGES | RATIFY | **RATIFY** |
| `rust-systems` | REQUEST CHANGES | RATIFY | **RATIFY** |

**Status: 4/4 RATIFY.** Per cfdb CLAUDE.md §2.3, RFC-042 is ratified without author override.

---

## Audit trail

- `council/RFC-042/BRIEF.md` — convener's brief, R1.
- `council/RFC-042/verdicts/clean-arch.md` — R1 verdict + R2 verdict.
- `council/RFC-042/verdicts/ddd-specialist.md` — R1 verdict + R2 verdict.
- `council/RFC-042/verdicts/solid-architect.md` — R1 verdict + R2 verdict.
- `council/RFC-042/verdicts/rust-systems.md` — R1 verdict + R2 verdict.
- `council/RFC-042/SYNTHESIS-R1.md` — R1 synthesis with 8 consolidated EDITs and the cross-lens trait-surface resolution (Position B).
- `council/RFC-042/SYNTHESIS-R2.md` — R2 synthesis, 4/4 RATIFY confirmation.

Commits of record:
- `8ee73fc` — RFC-042 draft authored (PR #379, merged).
- `3cd0a80` — `.context/378.md` freshness snapshot (verdict=contested).
- `<this commit>` — RFC-042 R1 EDITs applied + R2 ratification.

---

## Ratification effects

Per CLAUDE.md §2.4, the RFC's §7 "Issue decomposition" now becomes the concrete backlog:

1. **Slice 042-A** — extractor `:EntryPoint{kind=test|bench}` emission + fixture + schema-descriptor edits — to be filed as a forge issue with the verbatim §7 `Tests:` 4-row block.
2. **Slice 042-B** — `cfdb scope --production-only` flag + `PetgraphStore`-internal dual-BFS + `classifier-unwired-production.cypher` + schema-descriptor edits — to be filed as a forge issue with the verbatim §7 `Tests:` 4-row block.
3. **Slice 042-C** — empirical close-out on `agency:yg/qbot-core` — re-scope of issue #378 itself (issue body to be edited to point at the empirical close-out instead of the implementation work).
4. **Companion follow-up** — PR against `agency:yg/graph-specs-rust` adding `.cfdb/queries/arch-test-only-reachable-production-items.cypher` after 042-A + 042-B land on cfdb `develop`. Plan is described in `council/RFC-042/SYNTHESIS-R1.md` §5.

---

## Implementation-time non-blocking notes

See `council/RFC-042/SYNTHESIS-R2.md` §2 for the five non-blocking residual notes. None changes the RFC text; all are recorded for implementer consultation.

---

End of ratification.
