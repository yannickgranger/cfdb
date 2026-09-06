# RFC-045 — polyglot relationship edges

**Status:** RATIFIED (2026-06-04).
**Date:** 2026-06-04
**Parent:** META #266 (cfdb multi-language roadmap) — Phase 4 (relationship edges); cfdb-041-language-backend-trait ratified the producer seam, #263/#264/#265 shipped structure-only MVPs
**Lineage:** cfdb-041-language-backend-trait#3 (LanguageProducer trait + Published Language invariant) · cfdb-032-v02-extractor#3 (resolver-discriminator concept) · `cfdb-extractor/src/call_visitor.rs` (the `callsite:` id format) · cfdb-043-call-site-arguments (`:CallSite` argument facts) · cfdb-029-code-facts-database#6 (CALLS/INVOKES_AT/IMPLEMENTS contracts)
**RFC sha base:** 553627b on origin/develop at draft time

## §1 — Problem

cfdb-041-language-backend-trait shipped a clean `cfdb_lang::LanguageProducer` seam and three producers: Rust (`cfdb-extractor`, reference), PHP (`cfdb-extractor-php`, #264), TypeScript (`cfdb-extractor-ts`, #265). The PHP/TS MVPs are **structure-only**. Real extracts at draft time (`cfdb extract`, V0_5_0 binary, on the crates' own fixtures):

```
PHP php-minimal:   nodes {Item:5, Module:1, Crate:1}   edges {IN_MODULE:5, IN_CRATE:5}
TS  ts-minimal:    nodes {Item:3, Module:1, Crate:1}   edges {IN_MODULE:3, IN_CRATE:3}
Rust (reference):  edges {CALLS, INVOKES_AT, IMPLEMENTS, IMPLEMENTS_FOR, TYPE_OF, HAS_ARG, BELONGS_TO, ...}
```

PHP and TS emit **zero relationship edges** — only structural containment. META #266's "done" acceptance is *"cypher returns reasonable results for **find all callers of X**, **list all implementors of interface Y**"*. Both are unanswerable today: "callers" needs `:CallSite`/`CALLS`/`INVOKES_AT`; "implementors" needs `IMPLEMENTS`. This is why #266 META is still open after #263/#264/#265 closed. This RFC emits the relationship edges that make PHP/TS graphs queryable for those two relationships.

## §2 — Scope

In scope — for **both** `cfdb-extractor-php` and `cfdb-extractor-ts`:

1. **Interface-implementation edges** (`IMPLEMENTS`, with a `resolver` discriminator) so "implementors of Y" resolves, **emitted only when both endpoints are in-workspace `:Item` nodes** (§3.2 — dangling edges are dropped at ingest; external targets produce no edge).
2. **Call facts** — `:CallSite` + `INVOKES_AT` (the textual surface that answers "callers of X" via `callee_path`) + `CALLS` **only where the callee resolves to an in-workspace `:Item`** (§3.4). In practice: PHP statically-resolvable scoped/qualified calls emit `CALLS`; **TS emits zero `CALLS`** this RFC (no import/type resolution — same posture as the Rust syn producer, which also emits none). "Callers of X" is answered by `MATCH (cs:CallSite)<-[:INVOKES_AT]-(caller) WHERE cs.callee_path …`, not by `CALLS`.
3. **TS method-level `:Item` (slice 45-D0, a prerequisite)** — `cfdb-extractor-ts` models classes as a single `:Item{kind:"struct"}` today and emits NO method items, so there is nothing to anchor `INVOKES_AT`/`caller_qname` to. 45-D0 adds method `:Item` emission + a method-qname scheme. (PHP already emits method items — `lib.rs:291-342` — so the PHP call slice 45-C does not need this. The two languages are NOT symmetric; the earlier "mirror 45-C" framing was wrong.)
4. **No new node labels, no new `:Item.kind`** (Published Language invariant, cfdb-041-language-backend-trait#4).
5. **Coordinated `cfdb-core` schema change** — a `resolver` attribute on the `IMPLEMENTS` edge descriptor, plus extension of the `:CallSite.resolver`/`kind` closed-enum descriptors for the new producer values. **Additive at the type level → no `SchemaVersion` bump / no graph-specs lockstep**, BUT it is NOT churn-free: it requires recomputing `FROZEN_NARRATIVE_DIGEST` and updating `specs/concepts/cfdb-core.md` + the `:CallSite` narrative pins in the same PR (§4.1, enumerated in the test blocks).
6. **Determinism preserved**; new nodes/edges sort into the existing byte-stable order.

Deferred (§6 expands): inheritance/`extends` edges (D3-a, §3.3), HIR-grade call resolution, **external/cross-package implements-target resolution** (closed-world this RFC), cross-language qname unification, constructor calls (`new X()`), docblock/JSDoc relationships.

## §3 — Design

### §3.1 — Where the code lives; the shared-helper boundary (clean-arch + solid BLOCKERs, folded)

Emission is an **additive pass inside each producer's existing `produce()`** path. **`LanguageProducer` is NOT extended** — no new trait method; relationship edges are simply additional members of the `(Vec<Node>, Vec<Edge>)` `produce()` already returns. A structure-only producer remains valid (ISP preserved). `cfdb-cli`, `cfdb-lang` composition root: **untouched**.

**`cfdb-extractor-shared` is syn-scoped and MUST NOT receive tree-sitter helpers.** Verified: its sole export is `classify_arg_kind(expr: &syn::Expr)` (`cfdb-extractor-shared/src/lib.rs:18`); PHP/TS have zero `syn` dependency (0% utilization → CRP violation if they took it on). **Decision: inline the resolver-stamp + qname formatting in each producer.** The two producers have **incompatible emit primitives** — PHP uses a local `Emitter` struct + `Node::new().with_prop()` builder (`lib.rs:422`, with `has_node` dedup); TS threads bare `&mut Vec<Node>`/`Props::new()` (`emit.rs:205`, no Emitter). So the resolver-stamp **cannot be textually identical** across PHP+TS; a `cfdb-extractor-common` extraction would couple two crates over code that only *semantically* matches. **No common crate.** The §8-Q2 "measure duplication" escalation is withdrawn — it rested on a false symmetry premise. **CI guard:** `cargo tree -p cfdb-extractor-php --invert syn` and `... -p cfdb-extractor-ts ...` MUST find no path (mechanically enforces syn-isolation; in the 45-A/45-C/45-D test surface).

The producers do NOT recurse into function/method bodies today (`cfdb-extractor-php/src/lib.rs` `emit_function`/`emit_method`; `cfdb-extractor-ts/src/emit.rs` `walk_program`). **The recursive body-traversal walk is the primary implementation work of §3.4**, descending into nested bodies (PHP closures, TS arrow functions).

### §3.2 — `IMPLEMENTS` edges — the "implementors of Y" query

**Syntax → edge:** PHP `class C implements I1, I2` → one `IMPLEMENTS` per interface; TS likewise.

**Real emitted kinds.** PHP maps both `class_declaration` and `interface_declaration` to `:Item{kind:"trait"}` and disambiguates with `php_construct = node.kind()` (`cfdb-extractor-php/src/lib.rs:256,265`). TS maps `class_declaration` to `:Item{kind:"struct"}` (`cfdb-extractor-ts/src/emit.rs:208`) but currently emits **no** class/interface discriminator. **45-B prerequisite:** the TS producer MUST gain a construct discriminator prop (e.g. `ts_construct = "class_declaration"|"interface_declaration"`) analogous to PHP's `php_construct`, so an `IMPLEMENTS` source (class) is distinguishable from its target (interface). A "implementors of Y" query is therefore source-kind-agnostic: Amendment 2026-09-06: at `cfdb` 0.8.0 the PHP producer's `php_construct` values are exactly `class_declaration`, `interface_declaration`, `trait_declaration`, `method_declaration` and `function_definition` (observed by extraction of a PHP fixture with the producer at c42274d); a producer emits a sixth value only after this sentence is amended to carry it, so that a reader keyed on the set refuses an unknown value rather than dropping the item. Amendment 2026-09-07: the set carries a sixth value, `enum_declaration` — a PHP enum is `:Item{kind:"enum", php_construct:"enum_declaration"}`, `kind` `"enum"` being already in `cfdb-041-language-backend-trait` §4's closed set, so no kind, no label and no schema version is added; measured need: at 0.8.0 five enum files under cours-coreen's `src/Enrolment/Domain/` yield no item and no rule can reach a name declared in them. The reader keyed on the set accepts the value before the producer emits it — the reader's refusal is the strict one, and a producer emitting first turns a working fence into a refused keyspace for the length of the window. Register pass 2026-09-06 (third), measured at cfdb 0.8.0: the Rust producer stamps every `:Item` with `crate` and `bounded_context` (286 of 286 on cascade's tree, the external trait stub `serde::Serialize` included, which carries no `file`, `visibility` or `module_qpath`); the PHP producer stamps every `:Item` with exactly `kind`, `line`, `name`, `php_construct`, `qname` (912 of 912 on cours-coreen) and never `crate` or `bounded_context`. A reader discriminates the producer by those two sets and by nothing on the common props `kind`, `name`, `qname`.
```cypher
MATCH (c)-[:IMPLEMENTS {resolver: $lang}]->(:Item {qname: 'Y'})
WHERE c.php_construct = 'class_declaration'   // or ts_construct = 'class_declaration'
```

**D1 ruling — direct edge, no synthetic node.** Emit `IMPLEMENTS` directly from the class `:Item` to the interface `:Item`. **D1-b (synthesizing a pseudo-impl-block) is REJECTED** — it invents a node with no source-text counterpart, violating "stubs are not arrows" (§4.4).

**`IMPLEMENTS_FOR`-asymmetry invariant.** Rust's `IMPLEMENTS` is impl-block→trait, joined with `IMPLEMENTS_FOR` (impl-block→type). **PHP/TS emit `IMPLEMENTS` only — never a companion `IMPLEMENTS_FOR`** (there is no impl-block). Cross-language queries MUST start from the source `:Item` (the implementing class), never from an impl-block intermediary. This is documented in the `IMPLEMENTS` descriptor.

**Homonym resolution — `resolver` attribute on `IMPLEMENTS`.** Rust `IMPLEMENTS` (source `kind:"impl_block"`) and PHP/TS `IMPLEMENTS` (source `kind:"trait"`/`"struct"`) are structurally distinct on the source endpoint. Rather than split the label, add `resolver: string` to the `IMPLEMENTS` edge — `"syn"` (Rust), `"tree-sitter-php"`, `"tree-sitter-typescript"` — exactly mirroring the `:CallSite.resolver` precedent (`cfdb-core/src/schema/labels.rs:37`). Consumers filter on `resolver` to pick a language's impl-shape. Additive at the type level (no `SchemaVersion` bump) but **not churn-free** — see §4.1 coordinated edits (frozen digest + spec markdown + the Rust producer must backfill `resolver="syn"` on its existing `IMPLEMENTS` emission so the attribute is uniformly present).

**`php_construct` / `ts_construct` are documented props now.** PHP already emits `php_construct` (`lib.rs:265`) but it is **absent from the `:Item` node descriptor** (`describe/nodes.rs`); TS emits no construct discriminator at all. Because these props become **load-bearing for "implementors of Y"** (disambiguating the class source from the interface target — both are `kind:"trait"` in PHP), this RFC adds **both** `php_construct` and `ts_construct` to the `:Item` descriptor (retroactively documenting the existing PHP prop), incurring the §4.1 digest/spec recompute. Keeping them undocumented-but-load-bearing was rejected (a query depends on an unspecified prop = latent drift).

**synthesize-pass non-interaction.** `cfdb-extractor/src/synthesize.rs:32` stamps `IMPLEMENTS` dst nodes `kind="trait"` — this is the **Rust producer's** pass and runs only inside `cfdb-extractor`; PHP/TS never invoke it. PHP/TS perform their **own** two-pass resolution (below) and MUST NOT reuse the trait-stamping synthesize. The RFC records this so no future refactor wires PHP/TS through `synthesize.rs`.

**D2: emit only resolved edges; no `target_resolved` prop; external targets produce no edge.** The dry-run proved (compiled, `cfdb-petgraph/src/graph.rs:227-237`) that `ingest_one_edge` **drops any edge whose `dst` node id is unknown**, and the PHP/TS extract path runs no synthesize pass. So the R1 design (emit `IMPLEMENTS{target_resolved=false}` to a non-existent external node) is self-defeating: the edge is silently dropped at ingest, making `target_resolved` un-queryable.

> **A producer emits an `IMPLEMENTS` edge if and only if the target interface qname matches an in-workspace `:Item`.** Determined by a **producer-local two-pass**: pass 1 walks all files and buffers `(source_class_id, target_qname)` pairs while emitting every class/interface `:Item`; pass 2 (after the full node set exists — the target may be in a later-sorted file) emits an `IMPLEMENTS` edge for each buffered pair whose `target_qname` resolves to an emitted `:Item`, and **drops the rest**. No placeholder node is ever created; no edge is ever emitted that would dangle. There is no `target_resolved` attribute — *edge present ⟺ resolved*.

**Closed-world limitation (documented, like D3-a).** A class implementing an external interface (a `vendor/` PHP interface, a `.d.ts`/built-in TS type) produces **no** `IMPLEMENTS` edge. "Implementors of Y" works when Y is in-workspace (the #266 case); "implementors of `\Symfony\…\Serializable`" does not. A future RFC may add external-target resolution. The 45-A/45-B test surface includes an external-target fixture asserting **zero** `IMPLEMENTS` edge AND zero synthetic `:Item` (the limitation is stable + stub-free).

**Pass-2 insertion point.** The pass operates on the `(Vec<Node>, Vec<Edge>)` returned by `emitter.finish()` (PHP `lib.rs:422-452`) / `produce()` (TS) before the final sort: build a `HashSet<qname>` (PHP can also use `Emitter::has_node` pre-`finish`), then emit the resolved `IMPLEMENTS` edges. No `Emitter` API change is required for PHP; TS linear-scans its `Vec<Node>` (acceptable at fixture/repo scale).

### §3.3 — `extends` (inheritance) — DEFERRED (D3-a), documented as a known false-negative

`class C extends B` / `interface I extends J` have **no existing edge label**. **D3-a (defer) is ratified** by all lenses (solid: no stability objection; YAGNI + avoids a V0_6_0 bump + graph-specs lockstep). D3-c (overload `IMPLEMENTS` with a prop) is rejected as a vocabulary smell. **D3-b (add `EXTENDS` now)** is recorded as the optional 45-E slice if a concrete consumer query needs the type hierarchy.

**Known query-correctness gap (this is a false-negative, not just a missing feature).** For
```php
interface Stringable {}
interface JsonSerializable extends Stringable {}
class User implements JsonSerializable {}
```
`MATCH (c)-[:IMPLEMENTS]->(:Item{qname:'Stringable'})` returns **empty** — `User` transitively implements `Stringable` via `JsonSerializable`, but only the syntactically-declared interface is recorded. Consumers MUST treat "implementors of Y" as **syntactic-only, non-transitive** until a follow-up RFC adds `EXTENDS`. The `IMPLEMENTS` descriptor and the 45-A/45-B issue bodies document this; the test surface includes a **negative-assertion fixture** verifying the gap is stable (a sub-interface implementor does NOT appear), so it is understood, not accidentally bridged.

### §3.4 — `:CallSite` / `CALLS` / `INVOKES_AT` — the "callers of X" query

Mirrors the Rust **syn** extractor's syntactic resolution. **Id format aligns with the existing namespace**: `callsite:{caller_qname}:{callee_path}:{local_idx}` with a **per-caller per-`callee_path` occurrence counter** (mirroring `cfdb-extractor/src/call_visitor.rs:185-192`). NOT the draft's `cs:{file}:{line}:{col}` (which collided with the existing scheme). Property name is **`callee_path`** (+ `callee_last_segment` where the Rust producer emits it), NOT `callee_name`, for cross-language query parity. The draft's "mirroring cfdb-032-v02-extractor cs_id" footnote was wrong (cfdb-032-v02-extractor defines the resolver concept, not an id format) and is corrected to cite `call_visitor.rs`.

For each call expression, emit:
1. `:CallSite` node carrying the **full Rust-parity prop set** (emitting only 4 props breaks cross-resolver query parity): `caller_qname`, `callee_path`, `callee_last_segment`, `file`, `line`, `kind` (`"call"`), `is_test`, `resolver` (`"tree-sitter-php"|"tree-sitter-typescript"`), `callee_resolved`. Matches the Rust `:CallSite` shape (`item_visitor/emit/mod.rs:80-97`).
2. `INVOKES_AT`: **`:Item{containing fn/method} -[:INVOKES_AT]-> :CallSite`** (Item→CallSite). **Contradiction folded:** the descriptor `describe/edges.rs:118-124` currently declares `from:[CALL_SITE] to:[ITEM]`, which is *backwards* vs the Rust emitter (`emit/mod.rs:104-108`, Item→CallSite) and `cfdb-extractor/src/lib.rs:7` ("INVOKES_AT (Item → CallSite)"). This RFC pins **Item→CallSite** and **corrects the descriptor's `from/to`/text in slice 45-C** (a pre-existing bug the new edges would otherwise entrench).
3. `CALLS` (only when the callee resolves to an **in-workspace `:Item`**): `:Item{caller} -[:CALLS]-> :Item{callee}`. **No `CALLS` on unresolved** — no guessed target, and (per D2's ingest finding) a `CALLS` to a non-existent callee would be dropped anyway. **TS emits zero `CALLS` this RFC** (no import/type resolution — the same posture as the Rust syn producer, which hardcodes `callee_resolved=false` and emits none, `emit/mod.rs:96`). PHP emits `CALLS` for the statically-resolvable, in-workspace cases in the table below. **"Callers of X" is therefore answered via `:CallSite.callee_path` + `INVOKES_AT`, not `CALLS`** — the universally-available surface.

**PHP `scoped_call_expression` resolution table.** All four PHP call forms are confirmed in tree-sitter-php-0.23.11; resolution by `scope`/receiver:

| Call form | `callee_path` | `callee_resolved` / `CALLS`? |
|---|---|---|
| `\Ns\foo()` (`function_call_expression`, qualified) | `\Ns\foo` | resolved iff in-workspace `:Item` |
| `foo()` (unqualified free fn) | `foo` resolved against current namespace | resolved iff in-workspace |
| `C::bar()` (`scoped_call_expression`, scope=`name`/`qualified_name`) | `C::bar` | resolved iff in-workspace |
| `self::bar()` / `static::bar()` / `parent::bar()` (scope=`relative_scope`) | enclosing-class-qname `::bar` (`self`/`static` → enclosing class; `parent` → unresolved this RFC, no superclass edge until D3) | `self`/`static` resolved iff in-workspace; `parent` unresolved |

> **`static::` precision.** `static::bar()` is resolved to `enclosing-**declaring**-class-qname::bar` at syntactic scope — i.e. treated as `self::`. PHP's runtime late-static-binding (the actual subclass receiver) is NOT resolved without HIR. Consumers relying on `static::` for subclass call tracking must use HIR-grade resolution.
| `$x->foo()` / `$x?->foo()` (`member_call_expression` / `nullsafe_member_call_expression`) | `foo` (method name only) | `callee_resolved=false`, no `CALLS` |
| `$cls::foo()` (scope=`variable_name`/dynamic) | `foo` | `callee_resolved=false` |

**TS `call_expression` callee-shape table.** `function` field is the `expression` supertype; emit a `:CallSite` for every `call_expression`, deriving:

| `function` shape | `callee_path` | `callee_resolved` |
|---|---|---|
| `identifier` (`foo()`) | `foo` | false (no import resolution this RFC → no `CALLS`) |
| `member_expression` `obj.m()` | `obj.m` | false |
| `member_expression` w/ `this`/`super` object (`this.m()`/`super.m()`) | `this.m`/`super.m` (preserved — carries semantic info) | false |
| optional-chain `obj?.m()` | `obj?.m` (preserve `?.`) | false |
| chained `a()()` (`function` is a `call_expression`) | raw callee text | false |
| IIFE `(()=>{})()` (`function` is `parenthesized_expression`) | raw text | false |
| tagged template `tag\`x\`` — this IS a `call_expression` w/ `arguments=template_string` | `tag` | false |

`new_expression` (`new X()`) is a **distinct** grammar node (confirmed both grammars) and a §6 non-goal — the `call_expression` walk does not capture it; tests assert no spurious `:CallSite` for it.

**Recursive body traversal is the primary work.** Neither producer descends into bodies today. A free `walk_call_sites(node, caller_item_id, caller_qname, &mut counts, …)` recurses through PHP `compound_statement` / TS `statement_block` + arrow/closure bodies, threading the per-caller per-`callee_path` occurrence counter (reset per fn/method body, matching Rust). The dry-run confirmed the existing tree-sitter cursor pattern supports this without restructuring (PHP); TS first needs method `:Item`s (45-D0) to supply `caller_item_id`/`caller_qname`.

**TS `implements` walk path.** Verified path: `class_declaration` → `class_heritage` (child) → `implements_clause` (child of heritage) → interface refs. **There is no intermediate `type` node** (`type` is an inlined tree-sitter supertype; `node.kind()` returns the concrete subtype). `implements_clause`'s named children are concrete type-system nodes — common shapes + extraction: `type_identifier` → whole text; `generic_type` → `name` field for the bare name (full byte-range `Generic<T>` per §6 — argument stripping deferred); `nested_type_identifier` → whole text (`ns.I`). **Other shapes** (`intersection_type` from `implements A & B`, `union_type`, `member_expression`, `object_type`, `parenthesized_type`) are grammatically valid in implements position — the walk extracts them as **raw byte-range text** and emits `IMPLEMENTS` iff that text matches an in-workspace `:Item` qname (union/intersection almost never resolve → edge silently dropped; a completeness gap for exotic positions, not a correctness risk). An implementer coding only the three common branches must add this fallback or silently drop `implements A & B`. `class_heritage` also holds `extends_clause` — excluded. **PHP analogue:** `class_interface_clause` (implements, children `name`/`qualified_name`) vs `base_clause` (extends) — confirmed children of `class_declaration`; the walk extracts only `class_interface_clause`. `interface I extends J` uses `base_clause` (so the negative-assertion fixture correctly emits no `IMPLEMENTS`).

**qname endpoints (D4).** Edges reference `:Item` by each producer's existing per-language qname scheme. PHP: `\Ns\Class::m` (method qname exists today, `lib.rs:291-342`). **TS: the R1 claim `module::Class.method` was wrong** — TS emits `{crate}::{module_qpath}::{name}` with **no class infix and no method items at all**. Slice **45-D0** defines the TS method qname (proposed `{crate}::{module}::{Class}::{method}`, `::`-separated for consistency with PHP, NOT `.`) as part of adding method `:Item`s. **Constraint:** no shared qname normalizer unifies `\` vs `::` — qname formatting stays producer-local (inline, §3.1).

### §3.5 — Wire format / SchemaDescribe (coordinated edits — dry-run proved "additive ≠ free")

`:CallSite` from PHP/TS serializes identically to Rust with the new `resolver` value, BUT `:CallSite.resolver` and `:CallSite.kind` are **pinned closed-enum descriptors** (`describe/nodes.rs:284,286`: `resolver ∈ {syn,hir}`, `kind ∈ {call,fn_ptr,serde_default}`) with a narrative-pin test (`describe/tests.rs:357`). Adding the `tree-sitter-*` resolver values **extends those enum descriptors and updates the narrative pin**. Likewise `SchemaDescribe`'s `IMPLEMENTS` descriptor gains the `resolver` attribute doc, and the `:Item` descriptor gains `php_construct`/`ts_construct`. No new labels, no new `:Item.kind`, no `SchemaVersion` bump.

## §4 — Invariants

1. **Coordinated additive `cfdb-core` change, no `SchemaVersion` bump — but NOT churn-free.** Every cfdb-core descriptor touch in this RFC requires, **in the same PR**: (1) the `describe/*.rs` edit; (2) recompute `FROZEN_NARRATIVE_DIGEST` (`describe/tests.rs:285`) — the sanctioned narrative-change path (`tests.rs:270-277`), NOT a forbidden ratchet; (3) update `specs/concepts/cfdb-core.md` (`## EdgeLabel` / `## Item` sections, the `make graph-specs-check` surface, `spec_sections_cover_all_schema_labels` `tests.rs:455`); (4) for `:CallSite`, update the resolver/kind enum-narrative pins (`tests.rs:357`). `SchemaVersion` stays V0_5_0; no graph-specs cross-fixture lockstep (additive optional attrs, cfdb §5). Only the optional 45-E (`EXTENDS`) would bump.
2. **Published Language (cfdb-041-language-backend-trait#4).** No new node label, no new `:Item.kind`. `IMPLEMENTS.resolver` and `:CallSite.resolver` are closed sets including `{"tree-sitter-php","tree-sitter-typescript"}`.
3. **Determinism.** New nodes/edges sort by the existing `sort_key`; file collection is pre-sorted; tree-sitter traversal is deterministic; the IMPLEMENTS two-pass and the per-caller occurrence counter are deterministic. sha256-stable re-extract; +1 in-workspace class implementing an in-workspace interface ⇒ +1 `IMPLEMENTS` edge (tested).
4. **Stubs are not arrows.** No synthetic placeholder `:Item` is ever created. `IMPLEMENTS` is emitted **iff** both endpoints are in-workspace nodes (the two-pass drops the rest — they would be dropped at ingest anyway, `graph.rs:227`); `:CallSite{callee_resolved=false}` carries **no** `CALLS`. The graph never invents a node, and never emits an edge that would dangle. There is no `target_resolved` discriminator (edge present ⟺ resolved).
5. **Resolver-discriminator carries down — scoped to single-producer keyspaces.** PHP/TS `:CallSite`/`CALLS`/`IMPLEMENTS` carry `resolver`. The `:CallSite` **id** (`callsite:{caller}:{callee_path}:{idx}`) has no `resolver` segment (it matches the Rust format the rust-systems BLOCKER demanded), so cross-resolver id-disjointness rests on qname-scheme disjointness (`\` vs `::`) — **guaranteed only within a single-producer keyspace** (`extract` selects exactly one producer per keyspace, `commands/extract.rs:76`). A future polyglot-merge RFC that co-locates languages in one keyspace must resolver-prefix ids; that is out of scope here.
6. **syn-isolation (CRP).** `cfdb-extractor-php`/`-ts` gain no `syn` dependency. CI: `cargo tree -p cfdb-extractor-{php,ts} --invert syn` finds no path.
7. **Recall gap acknowledged.** `cfdb-recall` is rustdoc-only; no PHP/TS oracle. Test surface = dogfood-against-fixtures + real-repo target dogfood, NOT a recall-corpus extension. A polyglot recall oracle is a separate #338 RFC.

## §5 — Architect lenses (Council R1 + coder dry-run + Council R2 — verdicts inline)

### §5.1 — Clean architecture — REQUEST CHANGES → folded

### §5.2 — DDD — REQUEST CHANGES (1 BLOCKER) → folded

### §5.3 — SOLID / component — REQUEST CHANGES (1 BLOCKER) → folded

### §5.4 — Rust systems — REQUEST CHANGES (2 BLOCKERS) → folded
ABI 0.23 OK (no grammar/feature bump). CallSite-id determinism: byte-column stable.

## §6 — Non-goals
- Inheritance/`extends` edges (D3-a defer) unless R2 rules D3-b (optional 45-E).
- Type-resolved/HIR-grade call targets; receiver-type inference for `$x->foo()`/`obj.foo()`.
- Constructor calls (`new X()` / `new_expression`).
- Cross-language qname unification / cross-language concept matching.
- **Qualified TS `callee_path`.** TS `:CallSite.callee_path` is a **textual identifier, not a qualified name** — `callee_path='foo'` matches every call to any TS function named `foo` regardless of module; PHP `callee_path` IS namespace-qualified where the syntax provides it (`\Ns\foo`), TS is not (no import tracking this RFC). Consumers must not treat TS `callee_path` as a qualified name. Documented on the `:CallSite.callee_path` descriptor.
- A PHP/TS recall ground-truth oracle (separate #338 RFC).
- Docblock/JSDoc relationships, PHP magic methods, TS declaration merging, JSX-component graph.
- Type-argument handling in interface qnames (`implements Generic<T>`): the edge target qname is the raw text; argument stripping is a documented follow-up, not this RFC.
- Default-on activation of `lang-php`/`lang-typescript` (stays opt-in).

## §7 — Issue decomposition

Vertical slices under #266. Ordering: IMPLEMENTS first; TS method `:Item`s (45-D0) gate the TS call slice. **Every cfdb-core descriptor touch carries the §4.1 coordinated-edit checklist** (describe/*.rs + FROZEN_NARRATIVE_DIGEST recompute + specs/concepts/cfdb-core.md + relevant narrative pins) — called out per slice.

### 45-A — PHP `IMPLEMENTS` edges (+ `resolver` attr + `php_construct` doc in cfdb-core)
cfdb-core: add `resolver` to the `IMPLEMENTS` descriptor; document `php_construct` on the `:Item` descriptor; backfill `resolver="syn"` on the Rust producer's `IMPLEMENTS` emission. cfdb-extractor-php: emit `IMPLEMENTS` for `class_interface_clause` via the **two-pass (emit only when target is an in-workspace `:Item`)**. **§4.1 coordinated edits apply** (digest + spec + no pin for IMPLEMENTS since it has no closed-enum narrative pin).
```
Tests:
  - Unit: class implements I1,I2,I3 (all in-workspace) → exactly 3 IMPLEMENTS; class extends B implements I → 1 IMPLEMENTS (base_clause NOT emitted); qualified `implements \Ns\I` (in-workspace) → target qname qualified; resolver="tree-sitter-php"; source php_construct="class_declaration", target "interface_declaration"; **external interface → ZERO IMPLEMENTS edges AND zero synthetic :Item** (closed-world + stubs-not-arrows); negative-assertion `interface A extends B {} class C implements A {}` → "implementors of B" empty (D3-a gap stable).
  - Self dogfood: extract php-minimal + a richer multi-interface PHP fixture; IMPLEMENTS count/endpoints + sha256-stable re-extract + (+1 in-workspace class ⇒ +1 edge); **assert the Rust producer's existing IMPLEMENTS edges now carry resolver="syn" after the backfill** (clean-arch R2 NIT); cargo-tree --invert syn finds no path; cfdb-core suite green incl. recomputed FROZEN_NARRATIVE_DIGEST + graph-specs-check.
  - Cross dogfood: n/a (PHP path doesn't run on the Rust companion); PR body: "additive cfdb-core attrs only, no SchemaVersion bump; develop zero-violation confirmed."
  - Target dogfood (real composer PHP repo at pinned SHA): report IMPLEMENTS count + a sampled "implementors of Y" result in the PR body.
```

### 45-B — TS `IMPLEMENTS` edges (+ `ts_construct` doc in cfdb-core)
cfdb-core: add `ts_construct` to the `:Item` descriptor (§4.1 coordinated edits). cfdb-extractor-ts: emit `IMPLEMENTS` via `class_declaration→class_heritage→implements_clause→{type_identifier|generic_type|nested_type_identifier}`, two-pass in-workspace-only.
```
Tests:
  - Unit: `class C extends Base implements I1,I2` → exactly 2 IMPLEMENTS, 0 from extends_clause; `class C implements Generic<T>` → target qname = full byte-range `Generic<T>`; `implements ns.I2` (nested_type_identifier) → qname `ns.I2`; resolver="tree-sitter-typescript"; ts_construct disambiguates class vs interface; external interface → ZERO edges, no synthetic node; D3-a negative-assertion.
  - Self dogfood: extract ts-minimal + richer fixture; IMPLEMENTS + determinism + cargo-tree syn guard + cfdb-core suite (digest + spec).
  - Cross dogfood: n/a (note unaffected).
  - Target dogfood (real TS repo at pinned SHA): report count + sampled implementors query.
```

### 45-D0 — TS method-level `:Item` (PREREQUISITE for 45-D; dry-run finding)
cfdb-extractor-ts currently emits one `:Item` per class and **no methods**. Descend `class_body` and emit a method `:Item` for **two distinct node kinds**: (a) `method_definition` children (regular methods, getters `get foo()`, setters `set foo(v)`, async, generators — all parse as `method_definition` with anonymous modifier tokens); (b) `public_field_definition` children whose `value` field is an `arrow_function` (`foo = () => {}` arrow-assigned properties). Omit `abstract_method_signature`/`method_signature`/`index_signature` (no body → no call sites). qname `{crate}::{module}::{Class}::{method}` (`::`-separated, defined here). No cfdb-core change (reuses `:Item{kind:"fn"}` + IN_MODULE/containment).
```
Tests:
  - Unit: a class with 2 methods → 2 method :Items with the `::Class::method` qname; arrow-field method → emitted; getter/setter handling documented; nested class (rare) → qname nesting; determinism.
  - Self dogfood: extract ts-minimal (add a method) → assert method :Item count + qname shape; sha256-stable.
  - Cross dogfood: n/a.
  - Target dogfood (real TS repo): method :Item count reported.
```

### 45-C — PHP `:CallSite` + `INVOKES_AT` + resolved `CALLS`
Recursive `compound_statement` body-walk in cfdb-extractor-php; emit `:CallSite` (full Rust-parity prop set, id `callsite:{caller}:{callee_path}:{idx}` w/ occurrence counter, resolver="tree-sitter-php") + `INVOKES_AT` (Item→CallSite) + `CALLS` only to in-workspace callees per the §3.4 PHP scope table. **Also fix the `INVOKES_AT` descriptor `from/to`/text in cfdb-core** (§4.1 coordinated edits) + extend `:CallSite.resolver`/`kind` enum pins.
```
Tests:
  - Unit: each call form per the §3.4 table — function_call/scoped `C::bar` [resolved iff in-workspace]/`self::`+`static::` [enclosing-class qname, resolved iff in-workspace]/`parent::` [unresolved]/member `$x->foo` + nullsafe `$x?->foo` [unresolved, no CALLS]/`$cls::foo` [unresolved]; full prop set asserted; nested `foo(bar())` → 2 CallSites; two same-callee calls in one body → distinct ids; `new MyClass()` → no CallSite; determinism sha256.
  - Self dogfood: PHP fixture (resolvable + dynamic calls) → CallSite/CALLS counts + determinism + cargo-tree syn guard + cfdb-core suite (INVOKES_AT descriptor fix + resolver/kind pins).
  - Cross dogfood: n/a.
  - Target dogfood (real PHP repo): CallSite/CALLS counts + a sampled "callers of X" via callee_path + INVOKES_AT.
```

### 45-D — TS `:CallSite` + `INVOKES_AT` (zero `CALLS`)
Recursive body-walk (`statement_block` + arrow bodies) in cfdb-extractor-ts on top of 45-D0's method `:Item`s; emit `:CallSite` (full prop set, resolver="tree-sitter-typescript") + `INVOKES_AT`. Per the §3.4 TS table, **`callee_resolved=false` for all shapes → zero `CALLS`** (no import resolution); "callers of X" via `callee_path`+`INVOKES_AT`. NOT a mirror of 45-C — depends on 45-D0.
```
Tests:
  - Unit: identifier `foo()`, member `obj.foo()`, `this.m()`/`super.m()` [callee_path preserves this/super], optional `obj?.m()` [?. preserved], chained `a()()`, IIFE, tagged template `tag\`x\`` [IS a CallSite] per the §3.4 table; `new MyClass()` (new_expression) → NO CallSite; assert ZERO CALLS; full prop set incl. caller_qname from 45-D0 method qname; determinism sha256.
  - Self dogfood: TS fixture; CallSite count + zero-CALLS assertion + determinism + cargo-tree syn guard.
  - Cross dogfood: n/a.
  - Target dogfood (real TS repo): CallSite count + a sampled "callers of X" (callee_path + INVOKES_AT).
```

(Optional 45-E — only if R2 rules D3-b: add `EXTENDS` edge label → V0_6_0 + graph-specs lockstep — drafted separately to isolate the schema-bump decision.)

## §8 — Open questions for Council R2 (post coder dry-run; most R1 questions now resolved)

