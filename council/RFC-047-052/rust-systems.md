# rust-systems verdict — RFC-047..052

Lens scope: `syn`/tree-sitter parsing strategy, petgraph internals, `cargo_metadata`/HIR
shellout cost, feature flags, determinism mechanics, trait object safety, crate dependency
graphs. Every claim below is verified against `file:line` on `develop`-derived worktree.

## Verdict table

| RFC | Verdict | One-line reason |
|---|---|---|
| 047 | **RATIFY** (one nit) | Pure composition over `query`; only systems concern is an unbounded `CALLS*` traversal — add a default `--max-depth`. |
| 048-A | **REQUEST CHANGES** | The profile is right and necessary, but its phase list is factually wrong: `extract` runs NONE of reachability/dup/recall — those are separate `enrich-*`/`cfdb-recall` verbs. Fix the scope before filing. |
| 048-B | **REJECT (as written) / DEFER** | Incremental *enrichment* is feasible in principle but the RFC mis-locates it: the named "global passes" are not in `extract`. Re-derive 48-B/C only from a corrected 48-A profile. |
| 048-C | **DEFER** | Parse-skip contingent on a corrected profile (the syn walk is the only real `extract`-internal phase besides `cargo metadata` + the optional HIR load). |
| 049 | **REQUEST CHANGES** | 49-A (clap) AND 49-B (Axum/Actix) both duplicate already-shipped HIR detectors (`has_clap_derive` / `HTTP_ROUTE_METHOD_NAMES`). Reframe both as "generalize existing detectors into the registry, byte-identical." Only 49-C/D (PHP/TS) are green-field. Registry is language-scoped. |
| 050 | **REQUEST CHANGES** | CONVERGED resolution C: emit `:Crate.crate_tier` at EXTRACT time from the already-resolved `cargo_metadata` DAG (not §3.3's `enrich_bounded_context`, which reads only `.cfdb/concepts/*.toml`). Build DAG from normal `[dependencies]` only (dev-dep `cfdb-hir-extractor→cfdb-cli` would false-cycle). |
| 051 | **KEEP-PARKED** | Systems cost (N new tree-sitter grammars / compile budget) is real but secondary to the two non-systems blockers; nothing to add beyond confirming grammar-vendoring cost. |
| 052 | **KEEP-PARKED** (never RATIFY) | The G6 "exclusion" precedent does NOT work the way the RFC claims — there is no dump-time filter; G6 works only because the attr is left *unpopulated*. The 052 fence as designed would break G1. |

---

## Per-RFC analysis

### RFC-047 — impact / blast-radius — RATIFY (one checkable nit)

Sound from a systems lens. `impact` is a `cfdb-cli` dispatch branch composing the existing
`query`/`query_with_input` path — no new trait verb, no new petgraph traversal primitive.
The reverse traversal `(:Item)<-[:CALLS*1..]-(affected)` is already expressible in the
Cypher subset and the BFS machinery already exists (`enrich/reachability.rs:246`
`bfs_call_graph` is the same shape, forward). `--since` seeding via `git diff --name-only`
is deterministic given `(workspace SHA, ref)`.

**The one systems concern — unbounded `CALLS*1..`.** §6 already names this and §3.2 offers
`--max-depth`. On the densest crate (`cfdb-petgraph`, 578 fns / `studies/003 §2`) an
unbounded reverse-reachability from a low-tier seed (e.g. a `cfdb-core` leaf reached from
every tier) is a near-whole-graph BFS. That is *fine* for a one-shot CLI (sub-second on
2197 nodes), so I do **not** require a bound to flip to RATIFY — but the RFC should make
the default explicit. **Checkable amendment (nit, not a blocker):** state in §3.2 that
`--max-depth` is *unbounded by default* and document the worst-case (O(V+E) per invocation,
acceptable for a CLI). This is the honest engineering statement; a forced low default would
silently truncate real blast radius, which is worse.

**Verdict: RATIFY.** The nit is documentation, not a code change.

### RFC-048 — incremental extraction (profile-first) — REQUEST CHANGES on 48-A

I lead Q3. The reframe (parsing ≈ 0.575 s, so parse-skip is near-worthless) is correct in
spirit, but the RFC's model of where `extract` spends time is **factually wrong**, and that
poisons every downstream slice.

**Finding 1 — `cfdb extract` runs none of the "global passes" the RFC blames.**
`commands/extract.rs:42-110` (`extract_at_path`) is the entire `extract` pipeline:
1. `crate::lang::available_producers()` → `producer.produce(workspace)` — for Rust this is
   `cfdb_extractor::extract_workspace` (`crates/cfdb-extractor/src/lib.rs:154`), which runs
   `cargo_metadata::MetadataCommand::exec()` (`lib.rs:156-159` — **a `cargo metadata`
   subprocess**) then the per-file `syn` walk + `resolver::resolve_deferred_*`
   (`lib.rs:234,243`).
2. `store.ingest_nodes` / `ingest_edges` (`extract.rs:100-101`).
3. `extract_hir` **iff `--hir`** (`extract.rs:103-105`) → `ra_ap_load_cargo` full workspace
   load (`crates/cfdb-cli/src/main_command/args/extract_args.rs:39`, `hir.rs:46`
   `build_hir_database`).
4. `save_store` (`extract.rs:107`).

`enrich_reachability` (`crates/cfdb-petgraph/src/enrich/reachability.rs`), `enrich_metrics`
/ `dup_cluster_id` (`crates/cfdb-petgraph/src/enrich/metrics/clustering.rs`), and
`enrich_*` generally are **separate CLI verbs**, NOT phases of `extract` — confirmed:
`grep enrich crates/cfdb-cli/src/commands/extract.rs` is empty, and the verbs live behind
`impl EnrichBackend for PetgraphStore` invoked by distinct `cfdb enrich-*` dispatch.

**Finding 2 — `cfdb-recall` is not in `extract` at all.** RFC-048 §1 lists "the
`cargo +nightly rustdoc` recall gate — the heaviest" as an `extract` phase. It is a
**separate gate crate** (`crates/cfdb-recall/`), with its own `rustdoc_json::build` shellout
(`crates/cfdb-recall/src/adapters/ground_truth.rs:118-122`), run as a distinct CI step. It
never executes during `cfdb extract`.

**Consequence.** The real `extract`-internal cost candidates are exactly three:
(a) the `cargo metadata` subprocess, (b) the `syn` walk + deferred-return/type resolution,
(c) the optional `ra_ap_load_cargo` HIR load (only under `--hir`, and this is almost
certainly the dominant cost when enabled — it is a near-full type-checking compile;
`dogfood-enrich.md:9` already records "the 31 `ra_ap_*` crates dominate compile time").

**REQUEST CHANGES condition for 48-A (flips to RATIFY):** rewrite §1/§2/§3.1 so the
profiled phase list is `{cargo-metadata, syn-walk, deferred-resolve, ingest, hir-load
(if --hir), save}` — the *actual* `extract` phases — and split the enrichment/recall
timings into a **separate** "enrich/recall profile" because those are separate processes.
The headline 48-A deliverable (target dogfood: where do extract's seconds go) is still
correct and still the right unconditional first slice; it just must measure the real
pipeline. With that correction I RATIFY 48-A.

**Q3 feasibility judgment on 48-B (incremental enrichment under G1).** Now that the passes
are correctly located, the feasibility answer sharpens:

- The expensive enrich facts ARE genuinely global. `enrich_reachability` (`reachability.rs:119-169`)
  re-seeds a BFS from *every* `:EntryPoint` and writes `reachable_from_entry`/`_count` to
  *every* `:Item` (`write_item_attrs`, `:284-304`). `dup_cluster_id` groups *all* `Fn` items
  by `signature_hash` and recomputes *every* ≥2 cluster (`clustering.rs:30-58`). A one-line
  edit can change a cluster membership or a reachability bit arbitrarily far away.
- Incremental *is* theoretically possible (recompute the closure whose inputs touch a
  changed file, prove byte-identical merge) — but it is materially harder than the RFC
  admits, and the payoff is unproven because **enrichment is not even in the path the
  reframe measured (0.575 s parse)**. The honest sequence is: corrected 48-A first; only if
  it shows an enrich pass dominates do we scope 48-B against *that specific pass*.
- **48-B as currently written is a dead-end target** because it bundles "reachability + dup
  + recall" as one incremental effort when (i) recall isn't in extract, and (ii) the three
  passes have entirely different incrementality profiles (reachability is a graph closure;
  dup is a hash-group; recall is an external rustdoc build). I **REJECT 48-B as written** and
  recommend it be re-derived per-pass from the corrected profile, or folded into the "cache
  rustdoc JSON" RFC the author already flags in §6 (which I agree is the likely higher-value
  separate RFC if the recall *gate* — not extract — is the pain).

**Cache-placement split (CONVERGED with clean-arch).** The two contingent caches are NOT the
same cache and do NOT live in the same adapter — a distinction RFC-048 conflates:
- **48-B incremental-enrichment cache** → the **enrich adapter** (`crates/cfdb-petgraph/src/enrich/`),
  because the global passes it would scope live there (`enrich/reachability.rs`, `enrich/metrics/`).
- **48-C fingerprint parse-skip cache** → the **extract adapter** (`cfdb-extractor`), because the
  syn walk it would skip lives there.
Two different caches, two different adapters, and — critically — **neither on a port**: the 7
`EnrichBackend` (`enrich.rs:91`) + 7 `StoreBackend` (`store.rs:63`) signatures stay free of any
cache handle / fingerprint type / path. RFC-048 must name this split rather than say "the
adapter" loosely.

**Determinism note (in 48-B/C favor):** all three passes already use `BTreeSet`/`BTreeMap`
ordering and stable `sort` (`reachability.rs:37-42`, `clustering.rs:35,66`,
`canonical_dump.rs:46-92`), so an incremental result *can* in principle be merged to a
byte-identical dump — the determinism substrate is sound. The blocker is scope/value, not
determinism mechanics.

### RFC-049 — framework entry-point detectors — REQUEST CHANGES

I own the "does clap-derive read derive input via syn, not expanded output" sub-question.

**Finding 3 — the clap-derive `:EntryPoint` detector ALREADY EXISTS and is shipped.**
RFC-049 §3.2 and slice 49-A propose building a `clap`-derive detector ("a
`#[derive(Subcommand)]` enum's variants → CLI `:EntryPoint`s, each variant's fields →
`REGISTERS_PARAM`"). This is a near-verbatim description of code already in the tree:
- `crates/cfdb-hir-extractor/src/entry_point_emitter.rs:137-151` — `has_clap_derive(&strukt)`
  → `emit(... "cli_command" ...)` + `emit_clap_struct_registers_param`.
- `:158-170` — `has_clap_derive(&enum_)` → `"cli_command"` + `emit_clap_enum_registers_param`.
- The descriptor is canonical: `crates/cfdb-core/src/schema/describe/nodes/call_graph.rs:34`
  — *"v0.2.0 MVP detects `cli_command` (clap `#[derive(Parser/Subcommand)]`) and `mcp_tool`
  (`#[tool]`)"*. Shipped under issue #86 / RFC-037 §3.1.

**Finding 4 — it reads the derive *input*, not macro-expanded output (Q4 answered).**
`registers_param.rs:18-20`: the detector matches *"whose syntax text mentions `Parser` or
`Subcommand`"* — i.e. it inspects the `#[derive(...)]` attribute token text on the AST node
as the parser sees it. There is no macro expansion: cfdb has no expanded `clap::Parser` impl
to read. So the answer to the BRIEF's Q4 systems question is: **yes, derive-input reading is
the only tractable approach and it is exactly what the existing detector does.** Any new
detector RFC-049 adds MUST follow this — recognise the derive attribute on the AST, never
expect expanded output.

**Finding 5 — wrong extractor.** RFC-049 §3.2 says "Detectors consume the AST cfdb already
parses (`syn` for Rust...)". But the existing clap detector is in the **HIR extractor**
(`cfdb-hir-extractor`, rust-analyzer AST + `Semantics`), behind `--features hir`, NOT in the
syn `cfdb-extractor`. The clap detector needs `sema.to_def` / qname resolution
(`entry_point_emitter.rs:291-299`) which is HIR-only. RFC-049 conflates the two Rust
extractors. A syn-only clap detector would emit dangling handler qnames (the HIR side resolves
them) — `entry_point_emitter.rs:230-232` explicitly notes "syn-side emission would dangle src
and be dropped by cfdb-petgraph's ingest."

**REQUEST CHANGES conditions for RFC-049 (flip to RATIFY):**
1. **Rewrite 49-A** from "build a clap-derive detector" to "**generalize the existing HIR
   clap detector** (`entry_point_emitter.rs:137-170`) behind the `FrameworkDetector` registry
   trait, with no recall regression on cfdb-self." The self-dogfood assertion in 49-A is
   already satisfiable today, so it must be reframed as a refactor-into-registry slice that
   preserves the existing emissions byte-for-byte (a mechanical-adjacent move), not a green-
   field detector.
2. **Correct §3.2** to state that Rust framework detectors that need handler-qname resolution
   live in `cfdb-hir-extractor` (HIR/`Semantics`), and only purely-textual detectors can live
   syn-side. Axum/Actix route handlers (49-B) need handler resolution → HIR. State this so the
   registry's home (Q4) is decided correctly: the Rust registry is HIR-side; PHP/TS registries
   are in their tree-sitter extractor crates. A single cross-language `FrameworkDetector` trait
   in `cfdb-lang` (the existing shared seam) is fine as the *abstraction*, but each impl lives
   in its language extractor — detectors must not reach across extractor boundaries (the
   clean-arch concern), and the orphan rule keeps them there anyway.
3. **49-B (Axum/Actix) is ALSO already shipped** (clean-arch surfaced this; verified):
   `entry_point_emitter.rs:8-15` + `HTTP_ROUTE_METHOD_NAMES` (`:54-61`) +
   `entry_point_emitter/http_route.rs:1` detect `axum` `Router::route|get|post|...|nest` and
   `actix_web` `.route(...)`/`.service(...)` chains, emitting `:EntryPoint{kind:"http_route"}`
   (issues #124/#125). So 49-B is *also* a reframe-existing-detector slice, not green-field —
   apply the same "generalize into registry, byte-identical emissions" treatment as 49-A.
4. 49-C/D (Symfony/Laravel PHP, Nest/Express TS) are genuinely new and sound; keep them.
   The manifest-gating `present(manifest)` precondition (§3.1) is the right OCP extension point
   and the right false-positive guard. The "stubs are not arrows" discipline (§3.3 — drop
   unresolved handlers with a Warning, never emit a synthesized stub) is correct and matches the
   existing HIR behavior (`http_route.rs:62` returns early when `resolve_handler_qname` fails).

With 49-A reframed and §3.2 corrected, RATIFY.

### RFC-050 — architectural-layer (tier) overlay — REQUEST CHANGES

I co-lead Q1 with solid-architect.

**Finding 6 — the manifest dep data is ALREADY in-process at extract time (CORRECTED).**
`cfdb-extractor` already runs `cargo_metadata::MetadataCommand::exec()`
(`crates/cfdb-extractor/src/lib.rs:156-160`), iterates `metadata.workspace_packages()`
(`lib.rs:193`), and emits one `:Crate` node per package (`lib.rs:267-289`,
`Label::CRATE` = `labels.rs:25`). **CORRECTION (ddd caught this; verified per "council
foundation claims need verification"):** the call uses **`.no_deps()` (`lib.rs:158`)**, which
suppresses the top-level resolve — `metadata.resolve` (`cargo_metadata-0.23.1/src/lib.rs:431`,
the `Vec<PackageId>` resolved graph) is **NOT** populated. My earlier "the resolved DAG is
already in-process" was wrong. **BUT** the fix is small and needs no resolve and no second
`exec()`: each `Package.dependencies: Vec<Dependency>` (`cargo_metadata-0.23.1/src/lib.rs:515`)
is parsed from the *manifest* and **IS** populated under `.no_deps()`, and each `Dependency`
carries `kind: DependencyKind` (`:470`; variants `Normal`/`Development`/`Build`,
`dependency.rs:15-25`). So tier is computable at extract time by iterating
`workspace_packages()`, reading each `package.dependencies`, filtering to
`kind == Normal` AND target-name ∈ the workspace-member name set (intra-workspace edges only),
and computing longest-path over that — keeping `.no_deps()` intact. This is ddd's **option (a)**
and it is the correct systems choice: no `Metadata.resolve`, no heavier transitive resolve, and
the `kind==Normal` filter is the *same* filter that prevents the dev-dep false cycle
(`cfdb-hir-extractor` dev-deps `cfdb-cli`). ddd's option (b) — dropping `.no_deps()` for a full
resolve — is heavier and unnecessary; reject it. Resolution C (extract-time attribute) stands;
only the sourcing sentence is corrected.

**Finding 7 — §3.3's chosen home is wrong.** RFC-050 §3.3 proposes extending
`enrich_bounded_context` "which already derives crate-level structure" to also emit `tier`.
But `enrich_bounded_context` does **not** read `Cargo.toml` and has **no dependency-graph
access** — it re-reads `.cfdb/concepts/*.toml` overrides and patches `:Item.bounded_context`
(`crates/cfdb-petgraph/src/enrich/bounded_context.rs:1-2,60`). It is a *concept-ownership*
pass, not a *dependency-structure* pass. Bolting tier onto it would force that enrich pass to
re-acquire the Cargo DAG it never touches — a god-pass smell and a layering violation.

**The clean home — "resolution C" (CONVERGED with clean-arch + solid in Phase B):**
- **Extract time** — emit the tier attribute on the `:Crate` node right where `:Crate` is
  already emitted (`lib.rs:267-289`), sourced from each `Package.dependencies` (manifest data,
  populated under `.no_deps()`; filter `kind==Normal` + workspace-member targets — Finding 6).
  It is then a pure extractor fact (`Provenance::Extractor`, whose descriptor
  already reads "walked from syn AST *or `cargo_metadata`*" — `descriptors.rs:26-31`), exactly
  like `:Crate.name`. This sidesteps the verb-ceiling question entirely — it is NOT an enrich
  verb, so the closed-at-7 ceiling is never pressured. A dedicated 8th enrich verb (the only
  alternative) is strictly worse and I reject it.
- **Attribute name — `crate_tier`, not `tier`/`layer` (adopt ddd Amendment 1).** ddd is
  correct that "layer" is a live homonym (Layer-1 structural / Layer-2 enrichment,
  `descriptors.rs:11-16`). From a systems view `crate_tier` is also the more precise name —
  it is a per-`:Crate` topological rank, not a generic "layer." I endorse ddd's blocking
  rename and ddd's kill of 50-B (`:Item.layer`): denormalising tier onto every item is
  redundant — `IN_CRATE` (`labels.rs:118`) already lets any query join an item to its crate's
  `crate_tier`, and 50-C's up-call query joins through `IN_CRATE` anyway.

**REQUEST CHANGES condition (flip to RATIFY):** replace §3.3 — drop the "extend
`enrich_bounded_context`" plan; emit `:Crate.crate_tier` at extract time alongside the existing
`:Crate` node (`cfdb-extractor/src/lib.rs:267-289`), sourced from each `Package.dependencies`
(manifest data under `.no_deps()`; Finding 6). This sidesteps the verb ceiling (it is not an
enrich verb at all) and puts the Cargo-read where the Cargo-read already is. Then 50-A's tests
stand.

**LABEL RECONCILE (for SYNTHESIS/RATIFIED — one resolution, not two).** RFC-050 §3.1 defines
"resolution A" as the *enrich-time* Cargo read inside `enrich_bounded_context` (the god-pass all
four lenses reject) and "resolution B" as materialising a `DEPENDS_ON` edge. solid labels the
converged answer "C." These are the same thing under different names: **the converged resolution
is A's footprint (one `crate_tier` attribute, zero edge) computed at EXTRACT time — call it C.**
"A as written" (enrich-time-in-bounded-context) is REJECTED; "C" (extract-time attribute) is
RATIFIED. The synthesis must carry a single name (C) to avoid a two-names-one-resolution defect
in the ratified record.

**Q1 systems sub-answers:**
- **Longest-path vs shortest-path:** longest-path (§3.2) is correct. Tier = "deepest position
  in the stack," so a crate is one above its *deepest* in-workspace dependency. Shortest-path
  would put a crate that depends on both tier-0 and tier-2 at tier-1, hiding the real depth.
  Longest-path is the standard topological-level / "rank" definition and is what matches
  `studies/003 §2`'s hand-derived DAG.
- **`cfdb-core` reachable at multiple depths:** under longest-path it is unambiguously tier 0
  (it has zero in-workspace deps; `studies/003 §2`). The "depended on from every tier" worry
  is a non-issue for *its own* tier — a crate's tier is a function of what it depends *on*, not
  what depends on *it*. (The "Zone of Pain"/instability angle is afferent coupling — solid's
  SDP/SAP concern, not tier.)
- **Acyclicity / cycle = hard error:** correct (§3.2) — BUT cfdb's own tree would FALSE-FAIL
  this gate under a naive all-deps DAG. **Concrete evidence:** `cfdb-hir-extractor`
  **dev-depends on `cfdb-cli`** (`crates/cfdb-hir-extractor/Cargo.toml [dev-dependencies]`),
  while `cfdb-cli` normal-depends on `cfdb-hir-extractor`. An all-kinds DAG therefore has the
  cycle `cfdb-cli → cfdb-hir-extractor → (dev) → cfdb-cli`, which would trip "cycle = hard
  error" on cfdb-self — the self-dogfood gate (50-A) would fail to even compute. The tier
  computation MUST build the DAG from normal `[dependencies]` only, excluding
  `[dev-dependencies]` and `[build-dependencies]`. **Required amendment to §3.2 (not optional):**
  "tier DAG is built from normal `[dependencies]` only; dev/build deps excluded — verified
  necessary because `cfdb-hir-extractor` dev-deps `cfdb-cli`." Fold into 50-A; without it the
  50-A self-dogfood assertion is unreachable.
- **A-footprint vs B (DEPENDS_ON edge):** the A-footprint (one attribute, zero edge), computed
  at extract time = resolution C. Materialising `DEPENDS_ON` (B) is a real second capability
  ("who depends on crate X") but it is its own RFC with its own consumer pull; pulling it in here
  violates "tool backlog ≠ client chores." Defer B as §6 already does.
- **tier ≠ instability (adopt; pre-empts a future split-brain):** `crate_tier` is topological
  DEPTH — efferent-only, a function of what a crate depends *on*. It is NOT instability
  `I = Ce/(Ca+Ce)` (the afferent/efferent ratio that puts `cfdb-core` in the Zone of Pain).
  050 emits depth only; if a consumer ever wants the Zone-of-Pain metric, that is a SEPARATE
  attribute (`instability`/`distance_from_main_sequence`) and a separate RFC — same bar as
  `DEPENDS_ON`. solid is adding this as an explicit 050 non-goal; I concur.

Schema discipline: `:Crate.tier` is one additive optional attribute → minor `SchemaVersion`
bump + lockstep `graph-specs-rust` `.cfdb/cross-fixture.toml` PR (RFC acknowledges this — §
header + §4). Correctly NOT G6-excluded: tier is a pure function of the manifests, byte-stable,
stays in the G1 dump (§4 is right).

### RFC-051 — non-code / IaC / DDL — KEEP-PARKED

Systems-only addendum to the (correct) parked status. The compile-cost concern is real: each
new format that needs a tree-sitter grammar adds a vendored grammar crate + its generated C
parser to the build. cfdb already vendors PHP + TS grammars (RFC-045); adding Dockerfile,
HCL/Terraform, K8s-YAML, SQL, GraphQL, Protobuf grammars would roughly double the grammar
surface. That is a secondary cost behind the two real blockers (no consumer, no recall ground
truth). Nothing to add — **KEEP-PARKED.** When unparked, the systems prescription is: one
grammar per slice (CCP), measure incremental compile cost per grammar in the slice PR, and
prefer formats with an already-vendored grammar or a pure-Rust parser (e.g. `toml`/`serde_yaml`
are already in-tree; those formats are cheap, Terraform-HCL is not).

### RFC-052 — opt-in LLM enrichment — KEEP-PARKED (never RATIFY)

**Finding 8 (the decisive one) — the G6 precedent does NOT work as RFC-052 claims.** RFC-052
§3.1 and BRIEF §4.4 assert `:Item.test_coverage` is *"excluded from the canonical-dump
sha256"* and that 052 can reuse "the same fence." I read the dump path: `canonical_dump.rs`
(`crates/cfdb-petgraph/src/canonical_dump.rs:45-109`) has **no exclusion list** — it serializes
*every* prop of every node (`props_to_json`, `:133-139`, iterates the full `Props` map). There
is no filter that strips `test_coverage` from a populated node.

How G6 actually holds: `test_coverage` is simply **never populated by default**. The metrics
`Config { coverage_json: None }` (default) leaves it absent (`enrich/metrics/mod.rs:52`,
`specs/concepts/cfdb-petgraph.md:47`), and the self-dogfood test states this verbatim —
*"the default `Config { coverage_json: None }` leaves `test_coverage` unpopulated, so the
exclusion is observed trivially"* (`crates/cfdb-cli/tests/self_dogfood_enrich_metrics.rs:16-18`).
The spec G6 clause (`specs/concepts/cfdb-core.md:209`) describes the *contract*
("excluded from the G1 sha256"), but the *mechanism* is populate-or-not, not dump-then-filter.

**Why this kills the 052 fence as designed.** An LLM `:Item.summary` is, by definition,
*populated* on the items it summarizes — that is the whole feature. The moment it is written to
a node's `Props`, `canonical_dump` serializes it (`canonical_dump.rs:60-69` → `node_envelope_json`
→ `props_to_json`), and two runs with a non-deterministic model produce different dumps → G1
violated. test_coverage gets away with it ONLY because its default state is *absent*; a feature
whose default state is *present* has no equivalent free pass.

**Therefore, IF (and only if) the maintainer ever blesses 052,** the first slice cannot be
"reuse the G6 fence" — there is no reusable fence. It must be: **build a real dump-time
exclusion mechanism** — an explicit `G1_EXCLUDED_ATTRS` set consulted in `props_to_json`/
`node_envelope_json` (`canonical_dump.rs:133,143`) that strips `EnrichLlm`-provenance attrs
before hashing. And per CLAUDE.md §6 rule 8, that exclusion set must be a `const` in source,
never a config/allowlist file. The RFC's §4 "If they cannot be cleanly excluded, the RFC is
dead" is exactly right — and as currently specified, they *cannot* be excluded, because the
claimed precedent doesn't exist. This is the single most important correction in this RFC and
must be recorded before any maintainer go/no-go: the fence is unbuilt, not pre-existing.

`Provenance::EnrichLlm` (the new variant — `descriptors.rs:25` enum is `#[non_exhaustive]`,
confirmed no `EnrichLlm` today) is the right marker, but it is the *input* to the exclusion
filter, not the filter itself. **KEEP-PARKED, never RATIFY** per BRIEF §1 — and flag Finding 8
to the maintainer as a hard precondition, not a detail.

---

## Contested-question positions

### Q3 (I lead) — RFC-048 profile feasibility + is incremental enrichment possible under G1
**Position:** 48-A profile is the right unconditional first slice BUT its phase list is wrong
(Findings 1, 2): `extract` runs `cargo metadata` + syn walk + optional HIR load + save; it runs
NO enrich pass and NO recall. Incremental *enrichment* is determinism-feasible (BTree ordering
substrate is sound) but value-unproven and mis-bundled in 48-B; recall is a separate gate and
"cache rustdoc JSON" is the likely real higher-value RFC.
**Engaged:** sent to `clean-arch` (cache placement) and `ddd-specialist` (fingerprint=build-mech).
**Outcome: CONVERGED.** clean-arch independently confirmed reachability/metrics are whole-graph
petgraph-adapter passes (`enrich_backend.rs:151-193`); the cache lives in that adapter / a sidecar
with NO `EnrichBackend`/`StoreBackend` signature change; they defer feasibility to me. ddd confirmed
fingerprint/staleness is a build mechanism, never vocabulary (no Label/Edge/attr/SchemaVersion),
and prescribes an explicit §4 fence line. All three agree 48-D byte-equivalence is the arbiter.

### Q1 (I co-lead with solid) — RFC-050 sourcing A vs B + tier computation
**Position:** Resolution A, sourced at **extract time** from the already-resolved
`cargo_metadata` graph (Finding 6), NOT via `enrich_bounded_context` (Finding 7). Longest-path
tier; normal-`[dependencies]`-only DAG (exclude dev/build deps to avoid false cycles); cycle =
hard error; `cfdb-core` = tier 0 unambiguously. Defer `DEPENDS_ON` (B) to its own RFC.
**Engaged:** co-lead thread with `solid-architect` (DAG sourcing / SDP); ACK'd their "awaiting
peer ack." **Outcome: UNANIMOUS CONVERGE on "resolution C"** (extract-time `:Crate.crate_tier`
from the already-loaded `cargo_metadata` resolve graph; not enrich, not an 8th verb) — me,
clean-arch, solid. solid's SDP framing: B's `DEPENDS_ON` is a forever Zone-of-Pain edge label
with no consumer → its own RFC. Adopted ddd's blocking rename to `crate_tier` (avoid the
Layer-1/Layer-2 homonym) and ddd's kill of 50-B `:Item.layer` (join via `IN_CRATE` instead).
My dev/build-dep exclusion guard endorsed by clean-arch (verified necessary: `cfdb-hir-extractor`
dev-deps `cfdb-cli`). **Dep-kind rule finalized with solid:** DAG = normal `[dependencies]` only;
exclude `dev-`/`build-dependencies` (false-cycle guard), but **INCLUDE optional/feature-gated
deps** (still a real dependency edge — excluding understates tier depth). `cargo_metadata` exposes
dep-kind per edge so the filter is trivial at extract time. The §5 "`cfdb-core` reachable at
multiple depths" worry is a non-issue: tier is a function of what a crate depends *on*, not its
afferent fan-in (that is instability/Zone-of-Pain, a separate attribute, out of 050 scope).
**clean-arch A→C reconciliation:** clean-arch initially proposed A (a new `crate::enrich::tier::run`
pass); on re-verifying that `cargo_metadata` already runs in `cfdb-extractor` and citing the
precedent that `:Item.bounded_context` is ALREADY computed at extract time — `compute_bounded_context`
is called inline in `cfdb-extractor/src/lib.rs:276` during crate emission (NOT the same-named
*re-enrichment* no-op pass `cfdb-petgraph/src/enrich/bounded_context.rs`, which §3.3 must NOT touch
— two files, one concept name, keep them straight), clean-arch converged to extract-time placement
= C. Source/algorithm
are orthogonal: tier = longest-path computed OVER the intra-workspace dep graph built from each
`Package.dependencies` (manifest data, `kind==Normal`, workspace-member targets — NOT
`Metadata.resolve`, which `.no_deps()` suppresses; see corrected Finding 6). Final state: A→C
unanimous, no split.
No open disagreement.

### Q4 (I engage) — RFC-049 clap-derive via syn reads derive input not expansion
**Position:** Confirmed (Findings 3, 4): the existing HIR detector reads the `#[derive(...)]`
attribute token text directly (`registers_param.rs:18-20`); there is no expanded output to read,
and derive-input reading is the only tractable path. Registry home: Rust detectors that resolve
handler qnames are HIR-side (Finding 5); the cross-language `FrameworkDetector` trait can live in
`cfdb-lang` but each impl lives in its language extractor (orphan rule + no cross-boundary reach).
**Engaged:** sent registry-home position to clean-arch. **Outcome: CONVERGED.** clean-arch
independently verified clap (49-A) AND Axum/Actix (49-B) are BOTH already shipped in
`cfdb-hir-extractor`; the genuinely-new work is PHP/TS (49-C/D), which need new entry-point
emission in their tree-sitter crates (a bigger lift the RFC must scope). Unanimous: registry is
language-scoped (shared `FrameworkDetector` *contract* may live in a shared seam; each impl stays
in its language extractor; no cross-boundary reach — `cfdb-hir-extractor` imports `ra_ap_hir`,
Rust-only). solid adds the ISP point: `detect()` takes the language-specific AST, not a unioned
super-AST. **Concrete trait shape (Phase B, with solid):** make `FrameworkDetector` generic over
an **associated `Ast` type** — `trait FrameworkDetector { type Ast; fn detect(&self, ast: &Self::Ast, ...) -> Vec<EntryPoint>; }`.
Each impl names its own AST (Rust → `syn::File` for textual derive-scan, or rust-analyzer
`SourceFile`/`Semantics` for handler-resolving detectors; PHP → tree-sitter-php tree). The registry
holds per-language detector sets where `Ast` is monomorphic, so any `dyn` erasure happens only
*within* a language, never across — ISP + clean-arch's no-cross-boundary rule for free, with no
unioned-super-AST trait-object tension. No disagreement.

### Q5 — DDD concept ownership (ddd leads)
**Systems input:** "tier" is a deterministic function of the Cargo DAG (extractor provenance),
not an opinion — it belongs on `:Crate` as an extractor fact, reinforcing ddd's "orthogonal to
`:Context`" position. "framework" is extraction-time provenance (the detector that fired), not a
queryable concept in v1 (RFC-049 §3.4 / §6 deferral is right). Deferred to ddd's lead.

---

## Test-surface prescription notes

- **48-A:** the `Tests:` block is fine in shape but the *target* must be the corrected phase
  list. Amend the Self-dogfood + Target-dogfood rows to read "phase breakdown over
  `{cargo-metadata, syn-walk, deferred-resolve, ingest, hir-load (if --hir), save}`" — NOT
  "each `enrich_*`" (those aren't in `extract`). Add a separate, optional enrich/recall profile
  as its own slice if the corrected 48-A shows extract is already fast.
- **49-A:** Unit row is fine. **Self-dogfood row must change** from "assert cfdb-cli's
  subcommands appear as CLI `:EntryPoint`s" (already true today) to "assert the registry-routed
  detector emits the **byte-identical** set the pre-registry HIR path emitted (no recall
  regression, no new/dropped entry)" — i.e. prove the refactor preserves behavior. This is the
  honest test for a reframe-existing-code slice.
- **49-B:** Self-dogfood "inert on cfdb (no Axum/Actix dep)" is correct and is the negative
  manifest-gate proof; keep it. Same for 49-C/D.
- **50-A:** Add a Unit row asserting the tier DAG **excludes dev/build-dependencies** (the
  false-cycle guard, Q1) — without it a workspace with a dev-dep back-edge would wrongly error.
  The "cycle errors" assertion should specify *normal-deps cycle* errors.
- **50-C up-call query:** Self-dogfood "assert zero up-calls in cfdb" is a strong, real
  assertion — but note it depends on `CALLS` edges crossing crate boundaries, which are
  HIR-resolved (`--features hir`); state the feature requirement so the gate isn't run on a
  syn-only (recall-incomplete) graph and reported as falsely clean.
- **052 (if ever unparked):** the prescribed first slice ("a test that an `EnrichLlm` attr
  cannot enter the canonical dump") is correct in intent but currently **untestable green**,
  because no exclusion mechanism exists (Finding 8). The slice must first *build* the
  `G1_EXCLUDED_ATTRS` filter in `canonical_dump.rs`, then the test asserts a populated
  `EnrichLlm` attr is absent from `canonical_dump` output. Red-first is real here: the test
  fails today because the attr WOULD appear.
