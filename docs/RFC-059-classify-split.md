# RFC-059 — `cfdb-classify`: split the debt-classification layer (`scope` / `classify` / `check`, `DebtClass` taxonomy) out of `cfdb-query` and `cfdb-cli` (strangler-fig, completes #279's small-libs split)

- Status: **council 4/4 RATIFY (2026-08-18, R2)**. **RATIFIED on merge to doxa `develop` by the operator.**
- Refs: cfdb #279 (EPIC "v1.0 — split into small libs"; operator ruling 2026-07-31: cfdb stays a framework, the interest is the split), ledger cfdb #547 W3.3. Predecessors: `cfdb-056-enrich-port-split` (7 enrichment passes → `cfdb-enrich::EnrichEngine<S: GraphBackend>`, merged 2026-08-17), `cfdb-057-eval-port-split` (evaluator → `cfdb-eval::QueryEngine<'s, S: GraphBackend>`, merged 2026-08-18; `cfdb-petgraph` is storage-only since PR #606). Overturns one standing ruling: `cfdb-034-query-dsl` "`cfdb-query` responsibilities stay at 6: parser, builder, inventory, shape_lint, SkillRoutingTable loader, list_items_matching" (§2, §6). Preserves one: `cfdb#A2.3` (routing is external to the graph — the invariant survives, its dead loader does not, §3.2).
- Grounding: live-state archaeology 2026-08-18 on cfdb `develop` `33299c5` (Explore agent map, load-bearing claims re-verified by hand); council R1 re-verified every citation against `develop` `a863af5`. Every path below is cited against that SHA.

## 1. Problem

cfdb's README positions the project as a code-facts database. Its implementation carries four layers (cfdb #279 §"Problem"): **L1** code facts (`:Item`, `CALLS`, `IMPLEMENTS`, …), **L2** process facts about code (git history, `:RfcDoc`, `:Concept`, reachability), **L3** judgments over facts (`DebtClass` six-class taxonomy, `Finding`, `ScopeInventory`, the classifier Cypher rules, the `check` editorial-drift triggers), **L4** policy over judgments (`SkillRoutingTable`, `.cfdb/skill-routing.toml`). The operator's 2026-07-31 ruling accepted the framework and asked for the split into small libs so an adopter can stop at the facts core.

Two of the three moves are done and the pattern is proven: cfdb-056-enrich-port-split moved L2 production behind `GraphView` into `cfdb-enrich`; cfdb-057-eval-port-split moved the evaluator behind `GraphReader` into `cfdb-eval`. What is left is L3 + L4, and the archaeology shows #279's premise for it is stale:

- **The judgment types are not in `cfdb-core`.** `DebtClass` / `Finding` / `CanonicalCandidate` / `ReachabilityEntry` / `ScopeInventory` (`crates/cfdb-query/src/inventory.rs:22-153`, 297 LOC), `ClassifyEnvelope` / `DiffSourceMeta` / `CLASSIFY_ENVELOPE_SCHEMA_VERSION` (`cfdb-query/src/classify.rs:25-61`, 150 LOC) and `SkillRoutingTable` / `SkillRoute` / `SkillRoutingLoadError` (`cfdb-query/src/skill_routing.rs:57-140`, 256 LOC) all live in **`cfdb-query`** and are re-exported flat from `cfdb-query/src/lib.rs:25-38`. `cfdb-core` holds only a tombstone comment (`cfdb-core/src/query.rs:18-21`, `query/ast.rs:5-7`) from the earlier move (cfdb #25). So the parser crate — the thing every rule file and every downstream verb depends on — also carries the rescue taxonomy, and `cfdb-034 §` ratified that as "responsibilities stay at 6".
- **The judgment verbs are ~2.2k LOC of `cfdb-cli` that know the concrete store.** `scope` (`cfdb-cli/src/scope.rs` 452 + `scope/{classifier,helpers,explain_sink}.rs` 457), `classify` (`commands/classify.rs` 377 + `classify/sorted_jsonl.rs` 287), `check` T1/T3 (`check.rs` 305 + `check/{t1,t3,tests}.rs` 416). Seven of the eight moving functions are hard-wired to `&QueryEngine<'_, PetgraphStore>` (`scope/helpers.rs:9-10` `validate_context`, `scope/classifier.rs:58,119,142`, `scope.rs:173,204`, `scope/explain_sink.rs:50`; only `query_known_contexts`, `helpers.rs:31`, already takes `&dyn QueryBackend`); the seven classifier rules are `include_str!`-embedded at `scope.rs:28-52` from `examples/queries/classifier-*.cypher` + `hsb-by-name.cypher`. `check`'s `t1::run`/`t3::run` (`check/t1.rs:24`, `check/t3.rs:26`) take `(db: &Path, keyspace: &str)` and re-load the keyspace from disk through `commands::parse_and_execute` (`commands/rules.rs:75-92`) at four call sites (`t1.rs:153,184,199`, `t3.rs:27`), then print (`eprintln!("violations: …")`, `t1.rs:70`, `t3.rs:55`) and emit (`output::emit_json`, `t1.rs:72`, `t3.rs:57`) from inside the trigger. `check.rs:178` declares its own `pub(super) struct Finding` next to `cfdb_query::Finding` — a two-way homonym the file itself apologises for (`check.rs:230-234`, the `T3Row` doc naming "the two existing `Finding` types").
- **L4 is dead.** `SkillRoutingTable::from_path` has zero production callers: the only references outside `skill_routing.rs` are the crate-root re-export (`cfdb-query/src/lib.rs:38`), the architecture test `cfdb-query/tests/finding_no_skill_field.rs` (a doc-comment mention — the test itself never calls the loader), doc comments in `cfdb-query/src/classify.rs:7-9,133`, `cfdb-cli/src/commands/classify.rs:10` and `cfdb-cli/src/main_command/args.rs:277`, the standalone doc `docs/cfdb-classifier.md:103-142` ("SkillRoutingTable — the external policy", with example code), and `.cfdb/skill-routing.toml` (64 lines) which is read by no binary, script, CI job or test. `skill_routing.rs:126` is `cfdb-query`'s only `toml::` call site. It has been that way since the `cfdb#A2.3` ruling made routing external to the graph — the loader shipped, the consumer never did.
- **Co-change says where the seams are.** Since the cfdb #25 move, `inventory.rs`/`classify.rs`/`skill_routing.rs` were touched by 6 commits; `parser/`+`builder/`+`list_items.rs`+`impact.rs`+`shape_lint.rs` by 14; **2 shared**, both repo-wide sweeps (`3eb9421` comment strip, `60b1179` the move itself). The three rescue files co-changed with the `cfdb-cli` rescue verbs in 5 of their 6 commits (the sixth, `6cc5296`, is docs-only). Inside the `cfdb-cli` rescue set the same discipline splits it in two: `check.rs`+`check/` = 6 commits, `scope`+`classify` = 27, **intersection 2, both repo-wide sweeps** (`3eb9421` comment strip, `adbd847` "extract `output::emit_json`" across every CLI verb; `1ab2609` touches only `check/t1.rs` — solid's first count of 3 was corrected by ddd, verified) — `check`'s genuine feature-driven co-change with `scope`/`classify` is 0/6, and `grep` finds zero `DebtClass`/`ScopeInventory`/`CanonicalCandidate`/`ReachabilityEntry` in `check.rs`/`check/*.rs` and zero `crate::scope`↔`crate::check` imports either way (solid + ddd + clean-arch R1, three-way converged). `diff.rs` (3 commits) is split 1/1 and imports no judgment vocabulary. Same CCP evidence class cfdb-056-enrich-port-split#1 and cfdb-057-eval-port-split#1 recorded before their splits — and it says: one deployment unit, **two bounded contexts** (§2).
- **Downstream cannot notice.** `graph-specs-rust` links no cfdb crate (no `cfdb` entry in its `Cargo.toml`, zero `use cfdb_`); it pins a binary rev (`.cfdb/cfdb.rev`) and shells out to exactly `cfdb extract` and `cfdb violations` (its `.gitea/workflows/ci.yml:304` "cfdb checks that…" is prose, not the `check` verb — re-verified). Its six rules use `:Item`/`:Crate` structural props only. `qbot-core` consumes the binary too. Nothing in this RFC touches a byte those verbs emit.

The remaining question is therefore small and precise: move the judgment layer behind the same port discipline (`GraphBackend`) into its own crate, delete the dead policy layer, and let `cfdb-cli` compose the result — without touching the L2 vocabulary in `cfdb-core` (that is the option the operator declined on 2026-08-18; §6 records why and when to revisit).

## 2. Scope

Ships:

### 2.1 `cfdb-classify` — the crate and its two contexts

**`cfdb-classify`** — new workspace crate (root `Cargo.toml` `[workspace] members` gains `crates/cfdb-classify`; named explicitly per cfdb-057-eval-port-split#2's rust-systems fold). One deployment unit hosting **two bounded contexts that share an origin (`cfdb#Addendum B` "Rescue Mission Protocol", `cfdb.md:910`) but no model** (ddd + solid R1): (a) **debt classification** — the taxonomy (`DebtClass`, `Finding`, `CanonicalCandidate`, `ReachabilityEntry`, `ScopeInventory`, `UnknownDebtClass`, `ClassifyEnvelope`, `DiffSourceMeta`, `CLASSIFY_ENVELOPE_SCHEMA_VERSION`), the classifier execution (`scope`), the diff-restricted classification (`classify`) and the explain sink; (b) **editorial-drift triggers** — the closed T1/T3 registry (`check`, `TriggerId`, `UnknownTriggerId`, `T1Row`, `T3Row`, `ContextRow`, `CheckReport`). They live in one crate because both compute typed verdicts from Cypher via Rust logic (unlike `check-predicate`'s raw passthrough, §6), no external consumer differentiates them (§1 "downstream cannot notice"), and both are small; they are separated by a **module wall** (`cfdb_classify::{taxonomy, scope, classify, explain}` never import from `cfdb_classify::check` and vice versa — true today, made an invariant in §4 and a test in §7). Production deps: `cfdb-core`, `cfdb-query` (`parse`, `lint_shape`, `list_items_matching`, `diff::{DiffEnvelope, ENVELOPE_SCHEMA_VERSION}`), `cfdb-eval` (`QueryEngine` — the concrete `QueryBackend` implementor and the only source of `ExplainRow`; `execute_explained` is inherent on it by cfdb-035-persistent-inverted-indexes#4 / cfdb-057-eval-port-split#2 and stays there), `serde`, `serde_json`, `thiserror` (`ClassifyError` is a five-variant orchestrator error, the `CfdbCliError` shape at `cfdb-cli/src/error.rs:15-16`, and both things it wraps — `StoreError` `cfdb-core/src/store.rs:16,27`, `ParseError` `cfdb-query/src/parser/mod.rs:37-38` — are `thiserror`-derived; rust-systems R1). **Not** `cfdb-petgraph`, `petgraph`, `toml`, `cfdb-enrich`, any extractor, `cfdb-cli` (§4).

### 2.2 `ClassifyEngine<'s, S: GraphBackend>`

**`ClassifyEngine<'s, S: GraphBackend>`** in `cfdb-classify` (§3.1) — the third engine of the family (`EnrichEngine<S: GraphBackend>` cfdb-056-enrich-port-split#3.2, `QueryEngine<'s, S: GraphBackend>` cfdb-057-eval-port-split#3.2). Holds a `QueryEngine<'s, S>` by value (composition — no new port; the store is reached only through `GraphBackend`), exposes `scope` / `classify` / `check` as library methods returning typed payloads plus warnings. `cfdb-cli` keeps loading, formatting, writing and exit codes. **No `ClassifyBackend` trait** in `cfdb-core` (§6).

### 2.3 Same-PR composition-root cutover

**Same-PR composition-root cutover, no transient re-export shim** (cfdb-056-enrich-port-split#2's ratified rule, cfdb-057-eval-port-split#2 applied it to thirteen sites): the slice that moves a type or a function out of `cfdb-query`/`cfdb-cli` flips every consumer in the same PR — six `cfdb-cli/src` files (`scope.rs`, `scope/classifier.rs`, `scope/helpers.rs`, `commands/classify.rs`, `commands/classify/sorted_jsonl.rs`, `commands/diff.rs` for `DiffSourceMeta` only), two `cfdb-cli/tests` files (`classify_self_dogfood.rs`, `diff_handler.rs`), and `cfdb-query/src/lib.rs`'s re-exports (`:25-38`). No `pub use cfdb_classify::*` left behind in `cfdb-query`.

### 2.4 Delete the dead policy layer

**Delete the dead policy layer** (§3.2): `cfdb-query/src/skill_routing.rs`, its three re-exports (`lib.rs:38`), `.cfdb/skill-routing.toml`, the `SkillRoute` / `SkillRoutingTable` / `SkillRoutingLoadError` sections of `specs/concepts/cfdb-query.md`, the `docs/cfdb-classifier.md:103-142` "SkillRoutingTable — the external policy" section and its "Skill route" column framing (ddd R1), the doc-comment mentions (`cfdb-query/src/classify.rs:7-9,133`, `cfdb-cli/src/commands/classify.rs:10`, `cfdb-cli/src/main_command/args.rs:277`), and `toml` from `cfdb-query`'s `[dependencies]` (`skill_routing.rs:126` is its only user — unconditional, verified). `cfdb-query/tests/finding_no_skill_field.rs` **moves** with `Finding` into `cfdb-classify/tests/` — it pins the invariant that outlives the loader (`Finding` carries no `skill` / `fix_skill` / `class` / `debt_class` key; `cfdb#A2.3`), and it guards against *any* future re-coupling, not just the deleted loader.

### 2.5 The `classify` feature on `cfdb-cli`

**`cfdb-cli` composes the layer behind a default-on Cargo feature `classify`** (`[features] default = ["lang-rust", "classify"]`, `classify = ["dep:cfdb-classify"]`, §3.3): the `Scope` / `Classify` / `Check` / `FindCanonical` / `ListBypasses` `Command` variants (`main_command/args.rs:222,278,407,155,182`), their `main_dispatch.rs` arms (`:46,110,196`), the `TriggerId` imports and `parse_trigger_id` (`main_parse.rs:7,22-24`, `main_command/args.rs:7,11` — the combined `use crate::main_parse::{parse_item_kind, parse_trigger_id}` splits, `parse_item_kind` stays unconditional; rust-systems R1) and the thin handlers are `#[cfg(feature = "classify")]`. **This is new territory for this crate, owned as such** (clean-arch + rust-systems R1): today's `lang-*` features gate an optional dependency behind an always-present `extract` verb (runtime `NoProducerDetected`), and `hir` cfg-gates a `mod` + re-export pair (`lib.rs:25,43`) and an error variant (`error.rs:22-25`) — never a clap `Subcommand` variant. clap 4 supports `#[cfg]` on derive `Subcommand` variants; 059-C is the first in-repo proof — the slim build compiling with the variants cfg'd out **is** the compile proof, in CI (§3.3), not a test. Default build: identical binary surface, identical bytes on every verb.

### 2.6 The `Finding` homonym

**`Finding` homonym resolved by the move, not carried into it**: `check.rs:178`'s local `Finding` becomes **`T1Row`** — the sibling of the existing `T3Row` (`check.rs:236`); the two rows share zero fields and no trait, so there is no generic "trigger row" to name (ddd + rust-systems R1). `T3Row`'s doc comment (`check.rs:230-234`, premised on "the two existing `Finding` types") is rewritten in the same PR to state the rule only. `cfdb_classify::Finding` is the one `Finding`.

### 2.7 Dep rules

Dep rules: `.cfdb/workspace-dep-rules.toml` gains `[cfdb-classify]` (allowed: `cfdb-core`, `cfdb-query`, `cfdb-eval`, `serde`, `serde_json`, `thiserror`; forbidden: `cfdb-petgraph`, `petgraph`, `indexmap`, `toml`, `cfdb-enrich`, `cfdb-extractor*`, `cfdb-hir-*`, `cfdb-recall`, `cfdb-cli`, `cfdb-lang`, `cfdb-concepts`, `clap`) plus a `tests/architecture_dep_rule.rs` sibling (the **eighth** gated crate — `cfdb-core`, `cfdb-enrich`, `cfdb-eval`, `cfdb-extractor`, `cfdb-petgraph`, `cfdb-query`, `cfdb-recall` carry one today; the `[cfdb-eval]` section at `workspace-dep-rules.toml:162-177` is the structural precedent); `[cfdb-core]`, `[cfdb-query]`, `[cfdb-eval]`, `[cfdb-enrich]`, `[cfdb-petgraph]` `.forbidden += cfdb-classify` (no reverse edge, cfdb-057-eval-port-split#4 precedent), plus the crate-specific "`cfdb-classify` appears nowhere in `cfdb-query/Cargo.toml`, any section" assertion (the same shape as `cfdb_petgraph_never_links_cfdb_eval_in_any_section`).

### 2.8 Tripwires

Tripwires on **non-test** sources of `cfdb-classify` (cfdb-057-eval-port-split#3.4's `eval_port_seam.rs` shape, widened per clean-arch + solid + rust-systems R1): no `PetgraphStore` (imports **and** signatures), `cfdb_petgraph`, `KeyspaceState`, `.raw()`, `from_raw(`; no `println!` / `eprint` (the engine never does I/O); under `src/check/` no `load_store` / `parse_and_execute` (the per-trigger keyspace re-load is gone by assertion, not observation); and the module wall (`src/check/**` imports no `DebtClass`/`ScopeInventory`/`Finding`/`CanonicalCandidate`/`ReachabilityEntry`; `src/{scope,classify,taxonomy,explain}` import no `TriggerId`/`ContextRow`/`T1Row`/`T3Row`/`CheckReport`).

### 2.9 The spec files

`specs/concepts/cfdb-classify.md` (new: one section per pub type — `ClassifyEngine`, `ClassifyError`, `ScopeOptions`, `DebtClass`, `Finding`, `CanonicalCandidate`, `ReachabilityEntry`, `ScopeInventory`, `UnknownDebtClass`, `ClassifyEnvelope`, `DiffSourceMeta`, `ExplainSink`, `TriggerId`, `UnknownTriggerId`, `T1Row`, `T3Row`, `ContextRow`, `CheckReport` — final list per §7), `specs/concepts/cfdb-query.md` (rescue sections removed, prose says what stayed), `specs/concepts/cfdb-cli.md` (feature `classify`), `.cfdb/concepts/cfdb.toml` crate list, README crate table, `docs/` pointers.

### 2.10 Does not ship

Does **not** ship (§6): any change to the L2 vocabulary in `cfdb-core` (`:RfcDoc`, `:Concept`, `:Context`, the overlay attrs on `:Item`, `schema-describe` output); any `SchemaVersion` bump; any wire-format change; a move of `diff` (stays in `cfdb-query`, §6); a move of `check-predicate` (stays in `cfdb-cli`, L1 predicate runner); any change to a classifier rule's text or to the six-class taxonomy; any correctness change to `scope`/`classify`/`check` output; a `ClassifyBackend` trait. Acceptance bar for every slice: existing suite passes with zero assertion changes **and** the JSON of `cfdb scope`, `cfdb classify`, `cfdb check --trigger T1|T3` and `cfdb scope --explain` through the real `cfdb` binary on the same extract of cfdb's own tree is byte-identical to the 059-0 baseline (§4).

## 3. Design

### 3.1 `ClassifyEngine`

#### 3.1.1 `ClassifyEngine`

```rust
// crates/cfdb-classify/src/engine.rs
pub struct ClassifyEngine<'s, S: GraphBackend> {
    query: QueryEngine<'s, S>,   // cfdb-eval; the only way the engine reaches a keyspace
}

impl<'s, S: GraphBackend> ClassifyEngine<'s, S> {
    pub fn new(store: &'s S) -> Self;
```

#### 3.1.2 `scope`

```rust
    /// `cfdb scope` — the §A3.3 infection inventory for one bounded context.
    /// Validates the context (ClassifyError::UnknownContext { known }), runs the seven
    /// classifier rules + hsb-by-name, buckets findings by DebtClass, resolves canonical
    /// candidates and reachability rows. `explain` is the shared sink today's helpers
    /// already accept (`&ExplainSink`, interior mutability, `scope/explain_sink.rs:40-70`).
    pub fn scope(&self, keyspace: &Keyspace, context: &str, opts: &ScopeOptions,
                 explain: Option<&ExplainSink>) -> Result<(ScopeInventory, Vec<Warning>), ClassifyError>;
```

#### 3.1.3 `classify`

```rust
    /// `cfdb classify` — scope restricted to a DiffEnvelope's added/changed set, wrapped
    /// in the versioned ClassifyEnvelope; `ClassifyEnvelope::sorted_rows()` is the pure
    /// (DebtClass::Ord, qname) order sorted_jsonl pins today.
    pub fn classify(&self, keyspace: &Keyspace, context: &str, diff: &DiffEnvelope,
                    opts: &ScopeOptions) -> Result<(ClassifyEnvelope, Vec<Warning>), ClassifyError>;
```

#### 3.1.4 `check`

```rust
    /// `cfdb check --trigger` — the closed trigger registry (T1, T3), one report per run.
    pub fn check(&self, keyspace: &Keyspace, trigger: TriggerId) -> Result<CheckReport, ClassifyError>;
}
```

#### 3.1.5 `CheckReport`

```rust
/// What `cfdb check` prints today as one merged QueryResult (t1.rs:72 / t3.rs:57), typed:
/// the trigger's rows already projected to Row/RowValue (T1Row / T3Row → Row is the same
/// projection the two triggers perform now), the row count that drives exit 30, warnings.
pub struct CheckReport { pub trigger: TriggerId, pub rows: Vec<Row>, pub warnings: Vec<Warning> }
impl CheckReport { pub fn row_count(&self) -> usize; }
```

#### 3.1.6 One engine, three verbs, no new port

**One engine, three verbs, no new port.** The engine reaches the graph only through `QueryEngine<'s, S>` (`QueryBackend::execute` + inherent `execute_explained`, `cfdb-eval/src/engine.rs:29-53`) — the same `S: GraphBackend` bound `EnrichEngine` and `QueryEngine` carry, so `cfdb-cli` constructs it exactly like the other two (`compose::classify_engine(&store)` next to `compose::query_engine`, `compose.rs:51`). Held **by value, not `&dyn QueryBackend`**: `--explain` needs `execute_explained`, which is inherent on `QueryEngine` and deliberately off the `QueryBackend` trait (cfdb-035-persistent-inverted-indexes#4, cfdb-057-eval-port-split#2); a `dyn ExplainPort` would have one producer and one consumer (YAGNI — clean-arch, solid, rust-systems R1 converged). `S: GraphBackend` is zero-cost (`PetgraphStore` is the sole implementor, `cfdb-petgraph/src/graph_view_backend.rs:184`) and `Send + Sync` come free from `GraphBackend`'s bound (`cfdb-core/src/graph.rs:189`). No `dyn`, no `Box`, no second `GraphBackend` implementor. No `workspace_root` on the engine: nothing in the moving code reads it — T1's `canonical_crate`/`owning_rfc` come from `:Context` nodes already in the graph (`check/t1.rs:152-224`), `.cfdb/indexes.toml` is `compose.rs`'s business (cfdb-035-persistent-inverted-indexes#3.8), and `scope.rs:364`'s `.cfdb/concepts` mention is remediation help text (clean-arch R1).

#### 3.1.7 Boundary rule

**Boundary rule, restated from cfdb-056-enrich-port-split#3.2 / cfdb-057-eval-port-split#3.2 (solid R1):** the engine is dispatch + orchestration only; primitives stay in submodules. `build_scope_inventory` (`scope.rs:131-165`: two queries + `populate_findings_by_class` + warning attach) is orchestration and becomes the body of `ClassifyEngine::scope`; `run_classifier_rule` / `query_findings_in_context` / `query_canonical_candidates` (`scope/classifier.rs:58,119,142`) stay a `classifier` submodule the engine calls; `sorted_rows` is a pure function on `ClassifyEnvelope`; T1/T3 fetch-and-project logic stays in `check/{t1,t3}.rs`. Rule execution and Cypher construction never move into the `impl ClassifyEngine` block.

#### 3.1.8 What moves in verbatim, what changes shape

**What moves in verbatim, what changes shape** (rust-systems + clean-arch + solid R1 corrected the draft's "moves as-is"): only `query_known_contexts` (`scope/helpers.rs:31`) already takes `&dyn QueryBackend`. `validate_context` (`helpers.rs:9-23`; called first by both `scope.rs:119` and `commands/classify.rs:68`), `run_classifier_rule` / `query_findings_in_context` / `query_canonical_candidates` (`classifier.rs:58,119,142`), `populate_findings_by_class` / `populate_findings_by_class_restricted` (`scope.rs:173,204`) and `ExplainSink::run` (`explain_sink.rs:50`) take the concrete `&QueryEngine<'_, PetgraphStore>` — eight signatures across four files are genericised to `&QueryEngine<'_, S>` in 059-B1 (the tripwire's `PetgraphStore` = 0 covers imports and signatures). **`validate_context` moves into the engine** (the draft said "stays a `cfdb-cli` handler" and contradicted its own `ClassifyError::UnknownContext`; clean-arch R1) — it is the first step of both `scope` and `classify`, and its "known contexts:" message text is preserved verbatim by the handler's `ClassifyError → CfdbCliError` translation (`tests/scope.rs::scope_rejects_format_table_in_v01` and the "known contexts:" substring asserts pin it). `scope.rs:82-91`'s `pub fn scope(db, context, workspace, format, output, keyspace, explain, production_only)` splits along the seam it already has: loading (`compose::load_store_with_workspace`, keyspace resolution), output (`emit_scope_output`) and exit stay in `cfdb-cli`; classification (`build_scope_inventory` and below) becomes `ClassifyEngine::scope`. `commands/classify.rs`'s diff-restriction and envelope build become `ClassifyEngine::classify`; `sorted_jsonl.rs`'s ordering rule moves as `ClassifyEnvelope::sorted_rows()`, the JSONL writing stays in `cfdb-cli`.

#### 3.1.9 `check` is a rewrite, `CheckReport` a design decision

**`check` is a rewrite, not a relocation, and `CheckReport` is a design decision, not a move** (solid + clean-arch R1; the draft undersold it as "one behaviour-preserving change"). Decision: `CheckReport` carries the trigger id, the rows **already projected to `Row`/`RowValue`** exactly as `t1`/`t3` project them today before merging into one `QueryResult`, and the warnings — it does not unify `T1Row` and `T3Row` (zero shared fields, no natural union; they stay per-trigger internal projections in `check/{t1,t3}.rs`). 059-B2's issue body carries this as its own design item with a spec section, and the golden test (§7) pins the projection. Mechanics: `t1::run` / `t3::run` today take `(db: &Path, keyspace: &str)`, re-load the keyspace through `commands::parse_and_execute` at four sites (`t1.rs:153,184,199`, `t3.rs:27`), and end by printing `violations: N (rule: trigger T{1,3})` (`t1.rs:70`, `t3.rs:55`) and calling `output::emit_json` (`t1.rs:72`, `t3.rs:57`), returning `usize`. In `cfdb-classify` they become `t1::run(&QueryEngine<'_, S>, &Keyspace) -> Result<CheckReport, ClassifyError>` / `t3::run(…)`: same Cypher, same projection to `Row`s, same merged shape, **no I/O** — the `eprintln!` + `emit_json` pair is the `cfdb-cli` handler's, exactly the pure/impure split `scope.rs` already has (`build_scope_inventory` vs `emit_scope_output`). Same keyspace bytes, same rows — the self-dogfood diff proves it; the tripwires assert the shape.

#### 3.1.10 Warnings and exit codes

**Warnings and exit codes.** Every method returns the `Vec<Warning>` it collects (the cfdb-054-target-identity-namespace ingest-warning prepend arrives through `QueryEngine`, untouched; `CheckReport` carries them); `cfdb-cli` keeps `main_exit.rs`'s single-site `30`/`0`/`1`/`2` mapping — the engine returns row counts and typed errors, never exits.

#### 3.1.11 `ClassifyError`

**`ClassifyError`** — one `#[non_exhaustive]` `thiserror` enum (`Store(StoreError)`, `Parse(ParseError)`, `UnknownContext { known: Vec<String> }`, `UnknownTrigger(UnknownTriggerId)`, `Diff(String)`), converted into `CfdbCliError` by the handlers so the user-visible messages are byte-identical.

#### 3.1.12 Rules stay where they are

**Rules stay where they are.** `examples/queries/classifier-*.cypher` and `hsb-by-name.cypher` are user-facing examples and the cfdb-057-eval-port-split#2 cross-crate literal pin (`cfdb-eval/tests/conversion_prefix_pin.rs`) parses `classifier-random-scattering.cypher` from that path; `cfdb-classify/src/rules.rs` `include_str!`s them from `../../../examples/queries/` (same depth as `cfdb-cli/src/scope.rs:28-52` — verified; the bytes are unchanged, `sha256` in the 059-0 baseline pins them). `T1_*_CYPHER` literals are inline strings in `check/t1.rs` and move as-is.

### 3.2 Delete, don't carve: `SkillRoutingTable`

`cfdb#A2.3` (council BLOCK-1, solid-architect) ruled that routing a finding to a skill is external to the graph: `Finding` carries no routing field, and a TOML table maps `DebtClass → skill` outside the store. The ruling produced two artefacts: the *invariant* (`finding_no_skill_field.rs`, live and green, and meaningful against any future re-coupling) and a *loader* (`SkillRoutingTable::from_path`) that nothing ever called — no verb, no script, no CI step, no test other than its own unit tests. Moving dead code into a new crate would launder it into a public API with a spec section. This RFC deletes the loader, the TOML file, its spec sections and its standalone doc section in the first slice (059-0), and keeps the invariant test alive next to `Finding` in `cfdb-classify`. Because this is the family's first deletion-shaped slice (056-0/057-0 were additive), the deletion is guarded by a test, not a one-time grep in a PR body (solid R1, §7). If a consumer ever appears, it lives on the consumer's side (agentry-style skill routing is a client concern — the operator's standing "cfdb should not know about its clients" ruling), not in cfdb.

### 3.3 `cfdb-cli` composition and the `classify` feature

- `compose.rs` gains `pub(crate) fn classify_engine(store: &PetgraphStore) -> ClassifyEngine<'_, PetgraphStore>` (pinned in `tests/signatures.toml` like the other seven factories; `tests/signatures.rs` re-parses `compose.rs` source textually, so a `#[cfg(feature = "classify")]` factory is counted identically under both builds — rust-systems R1 verified); handlers for `Scope`/`Classify`/`Check` become: resolve keyspace + workspace → `compose::load_store_with_workspace` → `compose::classify_engine(&store)` → engine call → `output::emit_json` / file / the `violations: N (rule: trigger Tn)` line → `main_exit`. Roughly 150–200 lines of handler stay in `cfdb-cli`; ~2.0k lines move.
- Feature `classify` (default on): gates `dep:cfdb-classify`, the five `Command` variants, their dispatch arms, the `TriggerId`/`parse_trigger_id` import sites (§2), the `compose::classify_engine` factory and the handler `mod`s. `cargo build -p cfdb-cli --no-default-features --features lang-rust` yields the facts-only binary: `cfdb --help` lists no `scope`/`classify`/`check`/`find-canonical`/`list-bypasses`, `cargo tree -p cfdb-cli --no-default-features --features lang-rust -e normal` has zero `cfdb-classify`. Everything else — `extract`, `query`, `violations`, `impact`, `diff`, `list-*`, `enrich-*`, `check-predicate`, `schema-describe` — is unaffected in both builds. `FindCanonical`/`ListBypasses` are Phase-A stubs (`stubs.rs::typed_stub`); they are rescue-shaped, so they ride the feature; deleting them is a behaviour change and out of scope.
- CI: `ci.yml` builds `--all-features` and default; the slim build gets one `cargo build --no-default-features --features lang-rust` step + the two assertions above. Precedent for "proof is a build step, reported, not a compiled test": cfdb-044-broaden-graph-specs-coverage slice **044-C** (§3.3 sub-band 2, "0 `ra-ap-*` entries in the slim `cfdb-cli` dep tree", reported in the PR body — clean-arch R1 corrected the draft's §3.6 citation). Documented in §7.

### 3.4 Migration order (strangler-fig; dead code first, taxonomy second, one engine crossing per context, feature last)

| Slice | Change | Composition-root edit (same PR) | Why this slot / acceptance |
|---|---|---|---|
| 059-0 | Delete `skill_routing.rs`, its re-exports, `.cfdb/skill-routing.toml`, spec sections, `docs/cfdb-classifier.md`'s SkillRoutingTable section, the doc mentions; `toml` out of `cfdb-query` deps; **deletion guard test**; **capture the baseline** (§7): `cfdb scope`/`scope --explain`/`classify --restrict-to-diff <two-SHA diff>`/`check --trigger T1`/`T3` JSON + exit codes on one syn extract of cfdb-self + `sha256` of the eight rule files | none | Pure deletion, zero behaviour change (proof: workspace builds and tests without it; the guard test is RED with the file present, GREEN after). Establishes the diff target before anything moves. |
| 059-A | New crate `cfdb-classify` (Cargo.toml, lib.rs with the lint pair, workspace member); `git mv` `inventory.rs`, `classify.rs`, `tests/finding_no_skill_field.rs` from `cfdb-query`; drop the re-exports from `cfdb-query/src/lib.rs`; flip the six `cfdb-cli/src` + two `cfdb-cli/tests` consumers; `cfdb-cli/Cargo.toml` `+ cfdb-classify` (unconditional in this slice); dep-rules `[cfdb-classify]` + reverse-edge forbids + `architecture_dep_rule.rs`; `specs/concepts/cfdb-classify.md` (types only) + `cfdb-query.md` trimmed | `cfdb-cli` `use cfdb_classify::{…}` at every former `cfdb_query::{DebtClass, …}` site | Types-only crossing: proves the crate, the dep rules and the graph-specs gate before any logic moves. Byte-identical scope/classify/check JSON. |
| 059-B1 | `ClassifyEngine<'s, S: GraphBackend>` (§3.1) with `scope` + `classify`: move `scope/{classifier,helpers,explain_sink}.rs`, the classification half of `scope.rs`, the envelope half of `commands/classify.rs`, `sorted_rows` from `sorted_jsonl.rs`, the `include_str!` rules; genericise the eight signatures; `validate_context` into the engine; tripwires (`PetgraphStore`/`cfdb_petgraph`/`KeyspaceState`/`.raw()`/`from_raw(`/`println!`/`eprint` = 0 in non-test `src/`; module wall); `cfdb-cli` `scope`/`classify` handlers thinned; `compose::classify_engine` + `signatures.toml` row; `scope_classifier_perf.rs` moves to `cfdb-classify/tests/` (it drives rules in-process through `QueryEngine` and imports `cfdb_petgraph::index::spec` for its fixture, `:52-58` — dev-dep) | `compose::classify_engine(&store).scope(…)` / `.classify(…)` replace the in-crate calls | The debt-classification engine crossing. Byte-identical JSON incl. `--explain` rows and warning order; `cargo tree -p cfdb-classify -e normal` shows no `cfdb-petgraph`/`petgraph`, includes `thiserror`. |
| 059-B2 | `ClassifyEngine::check`: move `check.rs` + `check/{t1,t3,tests}.rs`; rewrite `t1::run`/`t3::run` to `(&QueryEngine<'_, S>, &Keyspace) -> Result<CheckReport, ClassifyError>` (no `parse_and_execute`, no print, no emit); `Finding` → `T1Row`, `T3Row` doc rewritten; `CheckReport` + golden test; check-side tripwire (`load_store`/`parse_and_execute` = 0 under `src/check/`); `cfdb-cli` `check` handler thinned (prints the `violations:` line, emits, exits); `commands::parse_and_execute` loses its `check` callers | `compose::classify_engine(&store).check(ks, trigger)` replaces `check::check(db, keyspace, trigger)` | The editorial-drift engine crossing, isolated because it is a rewrite (§3.1) and shares zero code with B1 (§1). Byte-identical `check` JSON + `violations:` line + exit codes (`trigger_t1.rs`, `trigger_t3.rs` unchanged); PR body reports wall-clock before/after on cfdb-self (informational). |
| 059-C | Feature `classify` (default on): cfg-gate variants/arms/handlers/factory/mods/import sites; slim-build CI step + assertions; README crate table + `docs/` + `specs/concepts/cfdb-cli.md`; `.cfdb/concepts/cfdb.toml` | `default = ["lang-rust", "classify"]` | Default build byte-identical (same baseline diff); slim build has no rescue verbs and no `cfdb-classify` in its tree. A real slice, not noise (clean-arch R1: cfg on clap `Subcommand` variants is novel here and deserves its own review surface); may still fold into B2 if the cfg surface is small. |

Each slice's acceptance gate is the existing suite with **zero assertion changes** (test files may move; their asserts do not change) plus the self-dogfood diff (§4). No slice leaves `develop` with a rescue type reachable through two crate roots.

## 4. Invariants

### 4.1 No wire-format change

**No wire-format change. No `SchemaVersion` bump. No graph-specs lockstep.** `Node`/`Edge`/`QueryResult`/`Warning` untouched; `schema-describe` output untouched (the L2 vocabulary stays exactly where it is, §6). `graph-specs-rust` and `qbot-core` reach cfdb through the binary's `extract` + `violations` (§1) — neither verb changes a byte.

### 4.2 Behaviour-identical

**Behaviour-identical, not merely "tests pass"**: on one syn extract of cfdb's own tree, the JSON of `cfdb scope --context cfdb`, `cfdb scope --context cfdb --explain`, `cfdb classify --context cfdb --restrict-to-diff <the two-SHA DiffEnvelope of §7>`, `cfdb check --trigger T1`, `cfdb check --trigger T3` (rows, `warnings`, key order, JSONL row order, the `violations: N (rule: trigger Tn)` stderr line) through the real binary is byte-identical to the 059-0 baseline at every slice; exit codes identical (`main_exit.rs` untouched). The eight rule files are byte-identical (`sha256`).

### 4.3 Determinism preserved

**Determinism (G1) preserved by construction**: `sorted_rows` is the same pure ordering (`DebtClass::Ord`, `qname`); every query goes through `QueryEngine` (cfdb-057-eval-port-split#4 handle-order guarantee); no new `HashMap` iteration reaches output.

### 4.4 Direction is one-way

**Direction is one-way**: `cfdb-classify` → {`cfdb-core`, `cfdb-query`, `cfdb-eval`}; nothing in {`cfdb-core`, `cfdb-query`, `cfdb-eval`, `cfdb-enrich`, `cfdb-petgraph`} names `cfdb-classify` in any Cargo section. Enforced by dep-rules + per-crate `architecture_dep_rule.rs` + the any-section assertion on `cfdb-query`. `cfdb-classify` names no store implementation (tripwire) — it is generic over `S: GraphBackend` like its two siblings; `cfdb-petgraph` may appear only in its `[dev-dependencies]` for fixtures (as `cfdb-eval/Cargo.toml:16` does; `architecture_dep_rule.rs`'s `parse_dependency_names` scans `[dependencies]` only).

### 4.5 The engine never does I/O

**The engine never does I/O**: no `println!`/`eprint` in non-test `cfdb-classify/src`; no `load_store`/`parse_and_execute` under `src/check/`. Printing, file writing and exit codes are `cfdb-cli`'s.

### 4.6 Module wall

**Module wall between the two contexts** (§2): `src/check/**` imports nothing from the taxonomy/scope/classify modules and vice versa — asserted by test from 059-B1 on.

### 4.7 `Finding` carries no routing field

**`Finding` carries no routing field** (`cfdb#A2.3`): `finding_no_skill_field.rs` moves with `Finding` and stays green; the deletion of the loader does not relax the invariant. The loader stays deleted: the 059-0 guard test fails on any reappearance of `SkillRouting`/`skill-routing.toml` in tracked source.

### 4.8 The taxonomy is closed and unchanged

**The taxonomy is closed and unchanged**: `DebtClass::variants()` order and the six `as_str` names are pinned by `classifier_taxonomy.rs` (unchanged asserts); `CLASSIFY_ENVELOPE_SCHEMA_VERSION` and `ENVELOPE_SCHEMA_VERSION` are unchanged.

### 4.9 Component metrics

**Component metrics, recorded (solid R1)**: after the split `cfdb-classify` sits at Ce 3 / Ca 1 / I 0.75 / A 0 / **D 0.25**; `cfdb-query` and `cfdb-eval` each gain a third zero-abstractness consumer edge (D 0.5 → 0.667). Accepted — the same DTO-engine shape cfdb-057-eval-port-split#5.3 accepted for the sibling — with the same trigger condition: revisit abstraction (a `ClassifyBackend` trait, an `Explain` port) when a second consumer or implementor exists.

### 4.10 No-ratchet

**No-ratchet**: no baseline/threshold file; the 059-0 capture lives in the PR bodies and the slice worktree scratch, never in-tree.

## 5. Architect lenses

### 5.1 Clean architecture (`clean-arch`) — R1: REQUEST CHANGES → folds applied → R2: RATIFY

### 5.2 Domain-driven design (`ddd-specialist`) — R1: RATIFY (non-blocking amendments) → folds applied → R2: RATIFY

### 5.3 SOLID / component principles (`solid-architect`) — R1: REQUEST CHANGES (non-blocking, material) → folds applied → R2: RATIFY

### 5.4 Rust systems (`rust-systems`) — R1: REQUEST CHANGES → folds applied → R2: RATIFY

## 6. Non-goals

### 6.1 The L2 vocabulary stays in `cfdb-core`

**Moving the L2 vocabulary out of `cfdb-core` / an opt-in "extension registry" so `schema-describe` shows only L1 by default** (#279's original sketch, "option 2" declined by the operator 2026-08-18). What it would cost: `:RfcDoc`+`REFERENCED_BY` (own `SchemaVersion::V0_2_3`, `schema/version.rs:91-94`), `:Concept`/`:Context`/`LABELED_AS`/`CANONICAL_FOR`/`EQUIVALENT_TO`, the eleven overlay attrs on `:Item` (`schema/describe/nodes/structural.rs:120,176-179,188-190,200-203`), `context_source.rs` — ~330 lines plus a registry mechanism no prior RFC designs (`cfdb-050 §3.3` is the only precedent and chose "no registry, just emit it"; `cfdb-052` parked the opt-in fencing pattern), a `SchemaVersion` bump, a graph-specs lockstep PR, and rewrites of the frozen `describe/tests.rs:25,63` + `specs/concepts/cfdb-core.md:125-127`. What it would buy: a cleaner `schema-describe` for an adopter who has not asked. Every L2 attr and label is already `Provenance`-tagged (`descriptors.rs:36,47`) — an adopter can see which enrich pass produces it. Revisit when a real facts-only adopter needs the describe output pruned; that RFC then has a consumer.

### 6.2 No `ClassifyBackend` trait

**No `ClassifyBackend` trait in `cfdb-core`** (solid R1). Unlike `EnrichBackend` (7 methods, `cfdb-core/src/enrich.rs`) and `QueryBackend` (1 method, `store.rs:96`), `scope`/`classify`/`check` have exactly one call site each (the CLI dispatch arm), no `&dyn` consumer and no second implementor — a trait would be an abstraction with zero clients (YAGNI, the same posture as cfdb-056-enrich-port-split's GAT deferral and cfdb-057-eval-port-split's boxed-composition-root non-goal). Trigger to revisit: `serve --mcp` (#475) or any second consumer of the engine.

### 6.3 `diff` stays in `cfdb-query`

**`diff` stays in `cfdb-query`.** `DiffEnvelope`/`compute_diff` (`cfdb-query/src/diff.rs`) is a snapshot delta over L1 facts (`DiffFact`, `ChangedFact`, `KindsFilter`); it imports no judgment vocabulary (solid R1 verified); its only consumer today is `classify`, but its content is facts, not judgments, and `cfdb diff` is a facts-layer verb. `cfdb-classify` depends on it through `cfdb-query`. If `diff` grows a second consumer or a judgment field, that is the moment to reconsider — not now.

### 6.4 `check-predicate` stays in `cfdb-cli`

**`check-predicate` stays in `cfdb-cli`.** It runs `.cfdb/predicates/*.cypher` with `--param` binding and the `violations` exit contract — a generic L1 rule runner emitting raw `(qname, line, reason)` rows (`check_predicate.rs:1-21`), no derived verdict (ddd R1) — not a judgment.

### 6.5 Provenance mis-declarations are their own issue

**Provenance mis-declarations found on the way** — `LABELED_AS`/`CANONICAL_FOR` declared `Provenance::Extractor` (`describe/edges.rs:191,199`) though produced by `enrich-concepts`; `bounded_context` declared Extractor though it is a crate-prefix judgment with a TOML override; `EQUIVALENT_TO` `Reserved` with no producer (`edges.rs:213`). Descriptor-text fixes; they change `schema-describe` output and the frozen snapshot, so they are their own small issue (filed alongside this RFC), not a rider on a mechanical split.

### 6.6 No change to what the classifier says

**Any change to what the classifier says.** Rule text, taxonomy, `check` verdict vocabulary, `scope` bucket semantics, `#555`'s perf budgets (the perf test moves as-is), `#546`'s NOT-EXISTS false-positive class, `ReachabilityEntry`'s v0.1 non-use — all exactly as they are; characterize, strangle, then re-evaluate.

### 6.7 The `FindCanonical` / `ListBypasses` stubs stay

**Deleting the `FindCanonical`/`ListBypasses` stubs.** Behaviour change (a verb disappears from `--help`); they ride the feature and remain stubs.

### 6.8 No `cfdb-routing` crate

**A `cfdb-routing` crate.** Nothing to put in it (§3.2).

## 7. Issue decomposition

Five slices (§3.4; 059-C may fold into 059-B2). Per cfdb `CLAUDE.md` §2.5, 059-0 and 059-A are mechanical (deletion / type relocation: existing suite byte-identical), 059-B1/B2 are new-capability-shaped only in that they introduce `ClassifyEngine` / `CheckReport` (public types, spec sections required), 059-C is a build-surface change — plus the self-dogfood diff restated every slice because it is the actual strangler-fig acceptance signal, run through the real `cfdb` binary. Slice issues cite `cfdb-059-classify-split#3.4` and carry these blocks verbatim.

**059-0 — Delete the dead policy layer + deletion guard + baseline capture**
```
Tests:
  - Unit: cfdb-query/tests/skill_routing_deleted.rs (RED-first: with skill_routing.rs still present the
    test fails) — walks tracked source (crates/, tools/, .cfdb/, specs/, docs/, README.md) and asserts
    zero occurrences of `SkillRouting` and `skill-routing.toml` outside this test and the RFC mirrors
    under docs/RFC-*.md (historical text); non-vacuity: asserts the walked file count > 0.
    finding_no_skill_field.rs still green in place (it moves in 059-A). `cargo tree -p cfdb-query
    -e normal` shows no `toml`.
  - Self dogfood (cfdb on cfdb): capture the 059-0 baseline on ONE scratch db —
      cfdb extract --workspace . --rev <SHA_A> --db <scratch> --keyspace cfdb-a
      cfdb extract --workspace . --rev <SHA_B> --db <scratch> --keyspace cfdb-b      (SHA_B = develop HEAD
        at capture, SHA_A = a commit with a real facts delta, e.g. its parent; both pinned in the PR body)
      cfdb diff --db <scratch> --a cfdb-a --b cfdb-b --format json > baseline-diff.json
      cfdb scope --db <scratch> --keyspace cfdb-b --context cfdb --workspace .            (+ --explain run)
      cfdb classify --db <scratch> --keyspace cfdb-b --context cfdb --restrict-to-diff baseline-diff.json
      cfdb check --db <scratch> --keyspace cfdb-b --trigger T1 --no-fail   (stdout JSON + stderr line + exit)
      cfdb check --db <scratch> --keyspace cfdb-b --trigger T3 --no-fail
      sha256sum examples/queries/classifier-*.cypher examples/queries/hsb-by-name.cypher
    Saved as the diff target for 059-A/B1/B2/C (a same-tree diff would be empty and exercise the
    restrict path as a no-op — hence two SHAs).
  - Cross dogfood: none — no behaviour change, nothing to compare on the companion.
  - Target dogfood: none — rationale: nothing observable changed on qbot-core.
```

**059-A — `cfdb-classify` crate: taxonomy + envelope cross, consumers flipped same-PR**
```
Tests:
  - Unit: cfdb-classify/tests/architecture_dep_rule.rs (RED-first: a forbidden dep in Cargo.toml fails);
    cfdb-query/tests/architecture_dep_rule.rs gains cfdb_query_never_links_cfdb_classify_in_any_section
    (RED-first); finding_no_skill_field.rs relocated and green; classifier_taxonomy.rs (cfdb-cli,
    assert_cmd) unchanged and green.
  - Self dogfood (cfdb on cfdb): 059-0 baseline diff — byte-identical scope/classify/check JSON + exit
    codes; make graph-specs-check 0/0 with specs/concepts/cfdb-classify.md (types) present and the
    cfdb-query.md rescue sections gone; cargo tree -p cfdb-query -e normal,dev → 0 cfdb-classify;
    cargo tree -p cfdb-classify -e normal → 0 cfdb-petgraph / petgraph.
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): ci/cross-dogfood.sh — 0 findings (extract +
    violations untouched; this is the "nothing leaked" proof).
  - Target dogfood: none — rationale: type relocation; the self-dogfood identity is the signal.
```

**059-B1 — `ClassifyEngine<'s, S: GraphBackend>`: `scope` + `classify` move behind the port**
```
Tests:
  - Unit: cfdb-classify/tests/engine_port_seam.rs tripwire (RED-first) — in non-test sources under
    cfdb-classify/src: zero `PetgraphStore` (imports and signatures), `cfdb_petgraph`, `KeyspaceState`,
    `.raw()`, `from_raw(`, `println!`, `eprint`; cfdb-classify/tests/module_wall.rs (RED-first) — src/check/**
    imports none of DebtClass/ScopeInventory/Finding/CanonicalCandidate/ReachabilityEntry and
    src/{scope,classify,taxonomy,explain,engine} import none of TriggerId/ContextRow/T1Row/T3Row/CheckReport
    (in B1 the check side is still in cfdb-cli — the test is written against the target layout and
    passes vacuously on the check half until B2, with the non-vacuity guard on the scope half);
    scope_classifier_perf.rs relocated (asserts unchanged); a compile pin that ClassifyEngine::new
    compiles for PetgraphStore (dev-dep fixture); ClassifyEnvelope::sorted_rows unit test = the ordering
    sorted_jsonl pinned (moved asserts).
  - Self dogfood (cfdb on cfdb): 059-0 baseline diff byte-identical for scope / scope --explain / classify
    (rows, warnings order, JSONL row order); the assert_cmd suites scope.rs, classify_self_dogfood.rs,
    classifier_taxonomy.rs, pattern_c_canonical_bypass.rs, signature_divergent.rs, diff_handler.rs green
    with zero assertion changes; cargo tree -p cfdb-classify -e normal includes thiserror, excludes
    cfdb-petgraph.
  - Cross dogfood: ci/cross-dogfood.sh — 0 findings.
  - Target dogfood (qbot-core at pinned SHA): cfdb scope --context <one qbot context> JSON identical
    before/after on the same extract; report the context and row count in the PR body.
```

**059-B2 — `ClassifyEngine::check`: T1/T3 rewritten onto the engine, `CheckReport`, `T1Row`**
```
Tests:
  - Unit: engine_port_seam.rs extended (RED-first) — under cfdb-classify/src/check/ zero `load_store` /
    `parse_and_execute` (the per-trigger keyspace re-load is gone by assertion); module_wall.rs now
    non-vacuous on the check half; CheckReport golden test — a synthetic keyspace with one T1 and one T3
    hit yields the same Row/RowValue projection today's t1/t3 emit (fixture from check/tests.rs, moved);
    `grep -rn "struct Finding" crates` = 1 hit (cfdb-classify); T3Row's doc comment no longer mentions
    a second Finding.
  - Self dogfood (cfdb on cfdb): 059-0 baseline diff byte-identical for check T1/T3 (stdout JSON, stderr
    `violations:` line, exit code with and without --no-fail); trigger_t1.rs / trigger_t3.rs green with
    zero assertion changes; PR body reports check wall-clock before/after on cfdb-self (informational —
    the reload elimination should show, but no budget is asserted).
  - Cross dogfood: ci/cross-dogfood.sh — 0 findings.
  - Target dogfood: none — rationale: check T1/T3 need :Context/:Concept enrichment qbot-core's pinned
    extract does not carry; the self-dogfood identity is the signal.
```

**059-C — `classify` feature (default on) + slim-build proof + docs**
```
Tests:
  - Unit: none new — rationale: cfg gating; the proof is a build, not a test (RFC-044 044-C precedent).
  - Self dogfood (cfdb on cfdb): default build — 059-0 baseline diff byte-identical, cfdb --help lists the
    same verbs as before; slim build — cargo build -p cfdb-cli --no-default-features --features lang-rust
    succeeds, cfdb --help lists no scope/classify/check/find-canonical/list-bypasses,
    cargo tree -p cfdb-cli --no-default-features --features lang-rust -e normal → 0 cfdb-classify
    (both as a CI step in ci.yml); tests/signatures.rs still counts classify_engine under both builds;
    make graph-specs-check 0/0.
  - Cross dogfood: ci/cross-dogfood.sh — 0 findings on the default build.
  - Target dogfood: none — rationale: build-surface change only.
```
