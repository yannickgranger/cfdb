# Pattern C — canonical bypass (generalized)

RFC reference: [`RFC-cfdb-v0.2-addendum-draft.md`](RFC-cfdb-v0.2-addendum-draft.md) §A1.4.

Pattern C identifies wiring bugs in which a canonical resolver for a
concept exists in the graph (declared via
`(:Item)-[:CANONICAL_FOR]->(:Concept)` edges emitted by
`enrich_concepts`) but some call sites resolve the concept by calling a
non-canonical wire-level form instead. The generalized v0.2 rule
supersedes the v0.1 `ledger-canonical-bypass.cypher` specialized
instance (commit `349b153d6`) with parameterization on concept name,
bypass method name, and caller scope.

The v0.2 rule does not fit in a single `.cypher` file because the
cfdb-query parser does not support `UNION` or `CASE WHEN`. The four
verdicts split into four files:

| Rule file | Verdict | Action hint |
|---|---|---|
| `canonical-bypass-caller.cypher` | `CANONICAL_CALLER` | OK — healthy wiring, no action |
| `canonical-bypass-reachable.cypher` | `BYPASS_REACHABLE` | Rewire — replace bypass with canonical call |
| `canonical-bypass-dead.cypher` | `BYPASS_DEAD` | Delete — bypass exists but no entry point reaches it |
| `canonical-unreachable.cypher` | `CANONICAL_UNREACHABLE` | Wire bypass callers through, or delete the canonical |

All four rules live at [`examples/queries/`](../examples/queries/).

## Pipeline prerequisites

Every Pattern C rule requires three enrichment steps before it reads
correctly:

```
cfdb extract --workspace <dir> --db <db> --keyspace <ks> --hir
cfdb enrich-concepts     --db <db> --keyspace <ks> --workspace <dir>
cfdb enrich-reachability --db <db> --keyspace <ks>
```

- `--hir` populates `:EntryPoint` nodes + `CALLS` / `INVOKES_AT` edges.
  Without it, `enrich_reachability` cannot seed BFS and returns
  `ran: false`, which causes every reachability-gated rule to return
  an empty result (silent no-op — see the per-file docs).
- `enrich-concepts` reads `.cfdb/concepts/<name>.toml` and emits
  `:Concept` + `LABELED_AS` + `CANONICAL_FOR`. Without a TOML declaration,
  there is no canonical anchor and Pattern C has nothing to check.
- `enrich-reachability` BFS's from every `:EntryPoint` over `CALLS*`,
  writing `:Item.reachable_from_entry` (bool) + `:Item.reachable_entry_count`
  (i64). The Pattern C rules filter on these attrs.

## Worked examples

The rest of this page walks one row per verdict against the synthetic
fixture at
[`examples/queries/fixtures/canonical-bypass/`](../examples/queries/fixtures/canonical-bypass/).
The fixture layout:

- `ledger/` — a canonical crate declaring `LedgerRepository::append`
  (non-canonical) and `LedgerRepository::append_idempotent` (canonical),
  plus four `LedgerService` methods exercising every verdict
- `cli/` — a `#[derive(Parser)]` struct (the `:EntryPoint` anchor)
  that fans out to two of the service methods
- `.cfdb/concepts/ledger.toml` — declares `ledger` as the concept and
  `ledger` as the canonical crate

### 1. `CANONICAL_CALLER` — healthy wiring

```
cfdb query --params '{
  "concept": "ledger",
  "canonical_callee_name": "append_idempotent",
  "caller_regex": ".*::LedgerService::.*"
}' "$(cat examples/queries/canonical-bypass-caller.cypher)"
```

Expected row (excerpted):

```json
{
  "concept":   "ledger",
  "call_site": "append_idempotent",
  "caller":    "ledger::LedgerService::record_trade_safe",
  "verdict":   "CANONICAL_CALLER",
  "evidence":  "ledger/src/lib.rs"
}
```

**When to act:** never. These are the call sites that already resolve
the concept through the canonical wire. Keep them. This rule exists to
give the triage story its baseline — if a bypass rule fires, the
`CANONICAL_CALLER` rule on the same fixture confirms the canonical form
IS in use elsewhere (so the bypass is a migration gap, not a total
miss).

### 2. `BYPASS_REACHABLE` — a live wiring bug

```
cfdb query --params '{
  "concept": "ledger",
  "bypass_callee_name": "append",
  "caller_regex": ".*::LedgerService::.*"
}' "$(cat examples/queries/canonical-bypass-reachable.cypher)"
```

Expected row:

```json
{
  "concept":   "ledger",
  "call_site": "append",
  "caller":    "ledger::LedgerService::record_trade",
  "verdict":   "BYPASS_REACHABLE",
  "evidence":  "ledger/src/lib.rs"
}
```

**When to act:** always. The caller is reached from the CLI
`:EntryPoint`, so a user action can trigger this code path. Replace
`.append(...)` with `.append_idempotent(external_ref, ...)`. This
row is the #3525 class of bug — a user-triggerable write path that
bypasses an idempotency barrier.

### 3. `BYPASS_DEAD` — dead wiring

```
cfdb query --params '{
  "concept": "ledger",
  "bypass_callee_name": "append",
  "caller_regex": ".*::LedgerService::.*"
}' "$(cat examples/queries/canonical-bypass-dead.cypher)"
```

Expected row:

```json
{
  "concept":   "ledger",
  "call_site": "append",
  "caller":    "ledger::LedgerService::record_orphan",
  "verdict":   "BYPASS_DEAD",
  "evidence":  "ledger/src/lib.rs"
}
```

**When to act:** delete. The bypass call site exists but no entry point
reaches it. This is the #3544/#3545/#3546 class of bug: a resolver path
scatters, leaving one branch stranded behind an unwired helper. Dead
code is cheaper to delete than to rewire — unless the unreachability is
itself the bug (in which case it will also surface under
`CANONICAL_UNREACHABLE`).

### 4. `CANONICAL_UNREACHABLE` — orphaned safety

```
cfdb query --params '{"concept":"ledger"}' \
  "$(cat examples/queries/canonical-unreachable.cypher)"
```

Expected row (one of several — every `:Item` in the canonical crate
that is unreached surfaces):

```json
{
  "concept":        "ledger",
  "canonical_item": "ledger::LedgerService::record_isolated",
  "call_site":      "(no call site — canonical impl is unreachable)",
  "caller":         "ledger::LedgerService::record_isolated",
  "verdict":        "CANONICAL_UNREACHABLE",
  "evidence":       "ledger/src/lib.rs"
}
```

**When to act:** wire or delete. The canonical impl exists but no
`:EntryPoint` reaches it, so either callers need to migrate onto it
(the common case — they're using the bypass) or the canonical impl is
orphaned and should be removed. This is the #1526 class of bug: a
safety envelope (`LiveTradingService`) declared canonical but wired
around instead of through.

## Triage workflow

For a given concept, the expected reading order is:

1. Run `canonical-bypass-reachable.cypher` — any row here is a LIVE bug.
   Fix immediately (rewire).
2. Run `canonical-unreachable.cypher` — any row here is a SAFETY gap.
   Determine whether the canonical impl should be wired in or deleted.
3. Run `canonical-bypass-dead.cypher` — any row here is dead code. Delete.
4. Run `canonical-bypass-caller.cypher` — confirms the healthy wiring
   baseline exists. If this is EMPTY while the others have rows, the
   canonical form is unused everywhere — a larger architectural issue
   than the Pattern C rule alone can characterize.

## Known limitations

- **No UNION / CASE** in the cfdb-query parser → four files instead of
  one. When the parser gains these constructs, the four rules can
  collapse into a single `canonical-bypass.cypher` with a `CASE`-derived
  `verdict` column.
- **`$canonical_callee_name` / `$bypass_callee_name` are manual** — the
  concepts TOML declares which CRATE is canonical, but not which METHOD
  NAME on that crate is the canonical form. A concepts TOML extension
  adding `canonical_method_patterns = ["append_idempotent"]` would let
  the rule derive both params automatically; deferred to a follow-up.
- **`CANONICAL_FOR` is crate-wide** in the current `enrich_concepts`
  emission rules. `canonical-unreachable.cypher` therefore can surface
  helper items (`build_entries`, trait decls) that share a crate with
  the canonical method but aren't themselves the canonical resolver.
  The rule treats this as acceptable noise — the row's
  `canonical_item` column makes the shape obvious, and narrowing via
  `canonical_item_patterns` is the same concepts-TOML extension noted
  above.
- **`cfdb violations` has no `--params` flag** in v0.2. Use `cfdb query`
  with the rule content piped in via `"$(cat <rule>)"` until
  `violations --params` ships.

## Motivating qbot-core backlog

| Issue | Shape | Verdict in fixture |
|---|---|---|
| #3525 | `LedgerService::record_trade` calls `.append()` not `.append_idempotent()` | BYPASS_REACHABLE (via `record_trade`) |
| #3544 / #3545 / #3546 | `parse_params` / `build_resolved_config` scatter — bypass stranded behind unwired helper | BYPASS_DEAD (via `record_orphan`) |
| #1526 | Capital.com `LiveTradingService` safety envelope declared canonical but wired around | CANONICAL_UNREACHABLE (via `record_isolated`) |
