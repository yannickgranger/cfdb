# Raid plan schema — bucket convention

**RFC-036 §3.5 / §6.** Documents the external parameter contract the
five `examples/queries/raid/*.cypher` templates expect from a consumer
workspace. cfdb intentionally does NOT ship a raid-plan YAML parser
(RFC-036 CP7) — the consumer workspace owns parsing and hands
the bucket sets + source context to `cfdb query` as parameters.

## Required named sets

Each set is a list of strings passed as a `$name` list parameter to
the raid queries.

| Parameter name | Shape of members | Meaning |
|---|---|---|
| `$portage` | item qnames — `crate::module::Item` | Items moved as-is into the new bounded context. The raid's straight-move bucket. |
| `$rewrite` | concept names — strings that match `:Concept.name` | Concepts with new canonical implementations in the new context. The raid's redesign-at-concept-level bucket. Graph-side: these resolve through `:Item-[:LABELED_AS]->:Concept` membership, not through item qnames. |
| `$glue` | item qnames | Adapter / wiring items that are being rewritten wholesale (not the domain types — the infrastructure between them). |
| `$drop` | item qnames | Items discarded — no replacement. |

The four buckets are **disjoint** by convention: an item that appears
in two buckets is a plan-authoring error. The raid queries do not
enforce disjointness themselves; the consumer's YAML loader should.

## Required scalar

| Parameter name | Shape | Meaning |
|---|---|---|
| `$source_context` | String — crate name | The crate being raided. Passed explicitly rather than inferred from the plan, so completeness checks never conflate "omitted by author" with "fell outside source crate." |

## Optional scalars (template-specific)

| Parameter name | Shape | Template | Meaning |
|---|---|---|---|
| `$max_unwraps` | Int | `raid-signal-mismatch.cypher` | Threshold on `:Item.unwrap_count`. `0` means "any unwrap contradicts `clean`." |
| `$min_coverage` | Float in [0.0, 1.0] | `raid-signal-mismatch.cypher` | Threshold on `:Item.test_coverage`. `0.6` is a reasonable starting point — tune per project. |

## Template output contract

Each template returns rows that the consumer YAML tool interprets as
plan-validation findings. Empty result = query's invariant holds; any
row is a finding the author must address before executing the raid.

| Template | Finding meaning |
|---|---|
| `raid-completeness.cypher` | An item in `$source_context` not claimed by any **qname bucket** (`$portage` / `$glue` / `$drop`). v2 does not introspect `$rewrite` concept-level labels — items that are canonicals for a rewrite concept must be explicitly added to `$portage` or flagged here. |
| `raid-dangling-drop.cypher` | An item in `$drop` that `$portage` / `$glue` still calls — raid would leave a dangling reference. |
| `raid-hidden-callers.cypher` | An item in `$portage` with callers outside `$source_context` — moving the item breaks external code the author didn't account for. |
| `raid-missing-canonical.cypher` | A concept in `$rewrite` with no `:CANONICAL_FOR` target — TODO the author needs to resolve. |
| `raid-signal-mismatch.cypher` | An item in `$portage` whose `unwrap_count` / `test_coverage` contradicts the "clean" claim — caller triages. |

## Example consumer-side YAML shape (non-normative)

This is what a consumer's `plan.yaml` MIGHT look like. **cfdb does not
define or parse this file** — it's an illustration of what the consumer
loader feeds to the templates.

```yaml
# Illustrative — cfdb does not define this schema.
source_context: stop-engine

buckets:
  portage:
    - stop_engine::types::StopLoss
    - stop_engine::types::TrailingStop

  rewrite:
    - compound_stop    # concept name — gets a new canonical
    - risk_ratio

  glue:
    - stop_engine::mcp::handle_request
    - stop_engine::cli::RunStop

  drop:
    - stop_engine::legacy::LegacyStopBuilder
    - stop_engine::legacy::parse_stop_bps
```

The consumer loader reads this file, validates disjointness, and
invokes cfdb once per template with the buckets passed as list
parameters:

```bash
cfdb query \
    --cypher "$(cat examples/queries/raid/raid-completeness.cypher)" \
    --param source_context=stop-engine \
    --param-list portage=stop_engine::types::StopLoss,stop_engine::types::TrailingStop \
    --param-list rewrite=compound_stop,risk_ratio \
    --param-list glue=stop_engine::mcp::handle_request,stop_engine::cli::RunStop \
    --param-list drop=stop_engine::legacy::LegacyStopBuilder,stop_engine::legacy::parse_stop_bps
```

(The `--param-list` CLI flag shape is illustrative — the binding
surface ships with the consumer's runner, not with cfdb.)

## Versioning

This schema version is v1. Future changes follow the RFC-036 template
for schema additions: new optional scalars are additive; new required
parameters are a v2 break. Template files carry a comment header
naming their bucket dependencies so consumers can detect breakage at
bind time.
