# `docs/cross-fixture-bump.md` — cross-dogfood orchestration runbook

**Scope.** This document is the **canonical home** for the cross-dogfood orchestration vocabulary shared between `yg/cfdb` and `yg/graph-specs-rust`. Neither tool's per-crate RFC owns these concepts; this runbook does. The graph-specs-rust repository carries a byte-identical copy at the same path (modulo repo-specific examples).

**Canonical source.** Both copies are kept in lockstep by convention, not by CI: the author of the first repo to land a change sends the same change to the other within the same calendar day. If the two copies diverge, the older timestamp wins until they are re-aligned in an explicit reconciliation PR.

---

## 1. Vocabulary

Terms used below are defined *here*. If a new cross-repo RFC wants to reuse any of them, reference this runbook — do not redefine.

### 1.1 Companion repo

The paired tool's repository. For cfdb, the companion is `yg/graph-specs-rust`; for graph-specs-rust, the companion is `yg/cfdb`. Reserved strictly for this cross-tool relationship.

**Not** `sibling` — that word is reserved for RFC-001-style DDD context-sibling relationships inside a single repo.

### 1.2 Cross-fixture

The file `.cfdb/cross-fixture.toml` in each repo. Pins the companion at a known-good commit SHA. Schema is frozen at the shape specified in cfdb RFC-033 §3.1 / graph-specs RFC-002 §3.1:

```toml
[companion]
repo      = "yg/<companion>"
branch    = "develop"               # documentation only; sha is authoritative
sha       = "<40 lowercase hex>"
bumped_at = "<ISO-8601 UTC>"
bumped_by = "<actor — cron name, PR number, or human>"
```

Parsed exclusively via `ci/read-cross-fixture-sha.sh` (SOLID RC1). Do not add a second parser.

### 1.3 Cross-drift issue

An issue opened automatically by a scheduled cron when cross-dogfood fails. Naming convention: **`cross-drift-YYYY-WW`** where `YYYY-WW` is the ISO week. One issue per repo per week, de-duped by title-prefix match. Body includes:

- failing invocation (command line + exit code)
- companion repo name and SHA tested
- `ci/cross-dogfood.sh` exit code (one of 10 / 20 / 30)
- the last ~50 lines of the failing step's log, attached verbatim

### 1.4 Exit-code contract

`ci/cross-dogfood.sh` on both repos exits with exactly one of these codes:

| Exit | Meaning | Blocks merge? | Who fixes? |
|---|---|---|---|
| **0** | Pass — companion tree is clean under the local tool | No | — |
| **10** | Clone / checkout failed (infra) | Yes, as a re-run | CI infra owner |
| **20** | Tool failed to START on companion tree — most often a `SchemaVersion` lockstep window (cfdb flavour) or a graph-specs reader panic on a new dialect feature | Yes | Author of the lockstep / dialect change; see §4 |
| **30** | Tool ran and **reported findings** on companion tree | Yes | Author of the finding-producing rule or of the companion-side code that triggered the rule; see §5 |

Contract is frozen — do not add a 40 for a new case without a protocol amendment PR in BOTH repos.

### 1.5 Self-dogfood vs cross-dogfood vs target-dogfood

Three distinct gates. Order matters when diagnosing a failure:

1. **Self-dogfood.** Tool run against its own tree.
2. **Cross-dogfood.** Tool run against the **companion** tree at the pinned SHA. Defined by this runbook. Mechanised by `ci/cross-dogfood.sh`.
3. **Target-dogfood.** Tool run against **qbot-core** (the rescue target) at a per-PR pinned SHA. Prescribed per-issue when the PR claims rescue-payload value. Not workspace-wide.

A rule that passes self-dogfood but fails cross-dogfood is a **rule-shape problem** (too loose). A rule that passes both and fails target-dogfood is **new signal on the rescue target** — that is the payload, not a bug.

---

## 2. The four cron schedules

All times UTC. Distinct per-repo offsets prevent issue-tracker noise collision (rust-systems C3 in both RFCs).

| Job | cfdb | graph-specs-rust |
|---|---|---|
| Weekly bump (pin advance) | **Monday 06:00** | **Monday 06:30** |
| Closed-loop housekeeping (pin-free drift check at HEAD) | **Tuesday 06:00** | **Tuesday 06:30** |

Bump jobs attempt an automated pin advance. Closed-loop jobs check for drift at companion HEAD — they **never** bump the pin; they only file a cross-drift issue if HEAD diverges from pinned.

---

## 3. Manual bump protocol

Use when you know the companion has landed a change you need to consume *before* the next Monday cron. Common triggers:

- companion landed a new fact kind / dialect feature you want to test against
- companion fixed a false-positive you are currently pinning around
- you are coordinating a `SchemaVersion` lockstep (§4)

Procedure:

1. Open a PR on your repo with a single change to `.cfdb/cross-fixture.toml` — new `sha`, new `bumped_at` (current UTC, second-precision), `bumped_by = "manual — <your handle> — <one-line rationale>"`.
2. CI runs `ci/cross-dogfood.sh` against the new pin. It MUST exit 0.
3. If it exits 30, the fix is either:
    - land a fix on the companion first and bump to a SHA that includes it, or
    - narrow the local rule shape to exclude the false-positive — see §5 escape hatch rules.
4. Merge. Do not `--force` a bump that exits 30 even once. No allowlists.

---

## 4. `SchemaVersion` lockstep

cfdb's `cfdb_core::SchemaVersion` is the wire contract for every on-disk keyspace. When cfdb bumps it:

1. The cfdb PR bumping `SchemaVersion` **must** be accompanied by a draft PR on `yg/graph-specs-rust` that bumps `.cfdb/cross-fixture.toml` to the cfdb PR's HEAD SHA.
2. Merge order: cfdb first, then the graph-specs fixture bump **within minutes** (not hours).
3. During the lockstep window, graph-specs' PR-time cross-dogfood step may return exit 20 briefly — this is the documented reason for the 20 code.
4. If no matching graph-specs PR is open within the cfdb PR's review window, the cfdb reviewer **blocks** the cfdb PR. Lockstep is author discipline; enforcement is human.
5. The next weekly cron after a missed lockstep will file a `cross-drift-YYYY-WW` issue automatically.

cfdb's own `SchemaVersion` bump PRs must carry `Tests: Schema lockstep — draft graph-specs fixture bump PR <number> open` in the body.

---

## 5. The zero-false-positive invariant (escape-hatch shape)

Both `cfdb` and `graph-specs-rust` commit to the same invariant:

> A new `arch-ban-*.cypher` rule (cfdb) or a new equivalence level / violation variant (graph-specs) MUST produce zero findings against **both** repos' pinned cross-fixture trees before it ships.

When a new rule or level would fire on the companion:

- **Permitted escape A:** land a fix on the companion repo first, then bump `.cfdb/cross-fixture.toml` to consume the fix. Both PRs merge in that order.
- **Permitted escape B:** narrow the rule / level shape so it no longer matches the companion-side pattern. The pattern was not the intent; the narrower rule is.
- **Not permitted:** any allowlist file (`.cfdb-allowlist`, `.graph-specs-exemptions`, per-rule `--ignore`). Consistent with global no-metric-ratchets rule (CLAUDE.md §6 rule 8).

No `--update-baseline`, no ceiling constant, no transitional waiver. Every rule / level is zero-tolerance from its first CI run.

---

## 6. Weekly closed-loop housekeeping

On its Tuesday slot, each repo runs `ci/cross-dogfood.sh` against the companion at **`develop` HEAD**, not the pinned SHA. Purpose:

- detect the window between "companion landed a change" and "our next manual or automated pin bump"
- catch silent drift where the companion has moved ahead but the pin has not
- surface any rule that would *start* firing on fresh companion code

Failure opens a `cross-drift-YYYY-WW` issue (§1.3). Until the issue is resolved, **the next PR to merge in the failing repo is blocked**. Resolution paths:

1. companion-side fix + bump (§3) — preferred
2. rule narrowing (§5 escape B) — acceptable when the companion pattern is legitimate
3. pin-hold + documented acceptance — only if (1) is in flight as an open PR and the hold is time-boxed (days, not weeks)

---

## 7. Runbook-linked CLAUDE.md sections

Both repos link this runbook from their `CLAUDE.md`. The link lives in:

- **cfdb:** §3 dogfood enforcement table — add a row "Cross-dogfood | `ci/cross-dogfood.sh` against companion at pinned SHA | see `docs/cross-fixture-bump.md`".
- **graph-specs-rust:** §3 dual-control table — same row.

Changes to this runbook that affect the vocabulary (§1), exit-code contract (§1.4), or escape-hatch rules (§5) require an amendment PR in **both** repos. Single-repo amendments to other sections are acceptable but the mirror must be updated within the same calendar day.

---

## 8. Change history

| Date | Section | Change | PR |
|---|---|---|---|
| 2026-04-19 | all | Initial authoring per RFC-033 §7 C2 and RFC-002 §7 C2 mirror | cfdb #68 / graph-specs #35 |
