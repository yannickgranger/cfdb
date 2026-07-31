# RFC-054 — Target-scoped `:Item` identity + ingest-contention diagnostics

- Status: RATIFIED (2026-07-31 — R1: 4× REQUEST CHANGES → amendments folded → R2: 4× RATIFY; council record: `council/RFC-054/RATIFIED.md`)
- Refs: issue #542 (W2.1, ledger #547), #517 (HIR qname keying), RFC-037 §3.8 B8 (canonical id helpers), RFC-032 §3 (resolver-discriminator on `:CallSite` ids), RFC-035 §4 (inherent-method observability precedent), RFC-036 §3.1 (homonym-elimination precedent)

## 1. Problem

`qname` is node identity (`item_node_id(qname) = "item:{qname}"`, `cfdb-core/src/qname/node_id.rs:16`), and the syn walker keys the crate segment of every qname off the cargo **package** name for **all** lib+bin targets of a package (`cfdb-extractor/src/lib.rs:465-480` collects target `src_path`s, dropping target kind and name; `file_walker.rs:52` seeds `module_stack = [package.name underscored]`). Every `src/bin/*.rs` is a separate crate root in rustc's model, but cfdb renders all of them into the lib crate's namespace:

- N binaries' `fn main` → one qname → one `item:` id. `graph.rs:138` (`ingest_one_node`) resolves the contention by `*existing = node;` — **no warning, no error, no diagnostic**. Losers vanish.
- Measured on yg/agentry (#542): 20 `fn main` in one crate's `src/bin/`, 5 `:Item` mains in the whole 69k-node workspace; `:CallSite` recall on identical constructs across the 20 bins is 1-2 in 20.
- **The collision is not confined to `:Item`.** `cs_id = "callsite:{caller_qname}:{callee_path}:{local_idx}"` (`item_visitor/emit/mod.rs:46`) is byte-identical for syntactically identical calls in sibling bins, so `:CallSite` nodes collide *independently* of their caller item. Every id formula derived from a parent qname (`param:`, `field:`, `variant:`, `arg:` — `cfdb-core/src/qname/node_id.rs`) inherits the same flaw.
- #517 (`crate_qname_prefix`, `cfdb-hir-extractor/src/crate_name.rs`) aligned HIR qnames onto the package-name convention to fix EXPOSES/CALLS dangles — correct for the join, but it means **both producers now collide identically**. Before #517 the HIR side keyed off the bin target name and did not collide (it dangled instead).
- Council-found downstream consequence (DDD lens): cfdb's own shipped Horizontal Split-Brain rule (`.cfdb/queries/hsb-cluster.cypher:92`) dedups pairs via `a.qname < b.qname` — structurally never true between equal strings, so the detector is *silent on the exact same-qname population this RFC makes visible*.

Why this lands first (from #542): a silent drop makes a query's zero-result indistinguishable from a genuinely clean tree — the same failure shape as a structurally dead rule reporting green. Any node kind added to the walk (`:Use`, `DEPENDS_ON` work in W2.2/W2.6) inherits it.

**Why target-name-only prefixes cannot fix it:** a default bin (`src/main.rs`) and any `[[bin]]` named like its package (e.g. `cfdb-recall`'s bin `cfdb-recall`) produce a target name whose underscored form **equals the lib crate name** — the collision survives. rustc itself treats lib and same-named bin as two distinct crates with no textual path distinction. Identity therefore needs a **target discriminator**; no spelling of the path alone is sufficient.

## 2. Scope

Ships:

1. **Ingest-contention diagnostic** (producer-agnostic): identity contention at ingest that would silently replace a *different* node surfaces as a `Warning` (new `WarningKind::IdentityContention` variant) on the existing `ingest_warnings` channel, reachable from both the query path (already wired) and the extract path (new inherent method, §3.4).
2. **Target-scoped identity for Rust targets**: `:Item` ids (and every parent-qname-derived id) gain a target discriminator so distinct cargo targets of one package occupy distinct identity namespaces, in **both** producers (syn + HIR).
3. **Identity/display separation enforced at the type level**: emit helpers return `(id, display_qname)` pairs; a suffix-aware display helper supersedes the prefix-only `qname_from_node_id`; all 9 reconstruction call sites audited (§3.5).
4. **Consumer-side resolution threading**: the post-walk resolvers and the enrich pass that reconstruct `item:` ids from bare qnames are threaded through discriminated identity (§3.5) — silent recall regressions in RETURNS/TYPE_OF/MATCHES_ON and serde-default reachability are in scope, not accepted collateral.
5. New `:Item` prop `target` (§3.2) + `SchemaDescribe` update + **homonym notes** in the describe entries and `specs/concepts/cfdb-core.md` (blocking scope, not polish — RFC-036 ParamBinding precedent).
6. `hsb-cluster.cypher` tie-break amendment (rule edits are RFC-gated; this RFC is the gate; the amendment rides in 54-B's PR with develop-parity proof first).
7. `cfdb-recall`: no new fact kind, but the producer-parity surface (`qname_parity.rs`) extends to bin-target cases.

Does not ship: anything for PHP/TS identity semantics (the diagnostic in (1) covers *detection* across all producers; see §6).

## 3. Design

### 3.1 Identity formula (cfdb-core::qname)

The display `qname` prop is **unchanged** — package-rooted, Rust-path-faithful (`cfdb_recall::main`). Identity separates from display. The discriminator is a plain domain type in `cfdb-core` (no `cargo_metadata`/`ra_ap` re-exports — cfdb-core stays serde/serde_json/thiserror-only):

```rust
// cfdb-core::qname
pub enum TargetDiscriminator {
    Lib,
    Bin { name: String },   // the cargo target name, verbatim (dashes kept)
}

item_node_id_for_target(qname, disc) =
  Lib          → "item:{qname}"                 // byte-identical to today
  Bin { name } → "item:{qname}#bin:{name}"
```

- `#` after the path is the established non-path-metadata discriminator (`param:…#0`, `variant:…#1`, `arg:…#0` — same module). Verified safe: no production code parses node ids on `#` anywhere in the workspace; ids are write-only strings keyed opaquely by `id_to_idx` (rust-systems lens, full grep).
- Lib-target ids are byte-stable vs today ⇒ the overwhelming majority of the keyspace, every existing rule/query, and the cross-dogfood contract are untouched.
- The **suffix** position (not a prefix rewrite) keeps `item:` ids prefix-searchable and makes id round-trips through prefix-stripping helpers self-correcting for *edge* endpoints (solid-architect lens) — the residual hazard is display-prop leakage, closed in §3.3/§3.5.
- **`callsite:` ids get centralized** (they never were): new `cfdb_core::qname::node_id::callsite_node_id(caller_identity, callee_path, local_idx)` mirroring `argument_node_id`, replacing the ad-hoc `format!` duplicates (syn `call_visitor.rs:146`, `emit/mod.rs:446`; HIR `call_site_emitter/facts.rs:98`; the PHP/TS twins stay on bare qnames by design, §6). Derived formulas (`param:`, `field:`, `variant:`, `arg:`) take the **discriminated parent identity** instead of the bare parent qname, so children of bin-target items separate for free.

### 3.2 `target` prop on `:Item`

Always-emitted string prop on Rust-producer `:Item`s: `"lib"` or `"bin:{target_name}"`. **Absence ⇒ pre-RFC-054 extract *or* non-Rust producer** (PHP/TS items never carry it — it layers in at the Rust-producer level, not in the contract-locked 4-key `build_item_props_common`). No enum in cfdb-core for the wire value — `kind` and `resolver` are already plain wire strings; same convention.

Naming note (DDD lens): `target` here means *cargo build target*, unrelated to the edge-endpoint sense of "target" used throughout the evaluator and to `:Item.impl_target`. The describe text spells this out.

**Homonym notes (blocking scope):** `:Item.qname`'s SchemaDescribe text (`describe/nodes/structural.rs:158`) currently reads "Fully-qualified name (`crate::module::Item`)" with no uniqueness caveat, and `specs/concepts/cfdb-core.md` §Node glosses "a stable id (qname)". Both become false-by-omission under this RFC: qname stays the display name and is **no longer unique** across targets (N bins ⇒ N `:Item`s sharing one qname). Both texts gain an explicit homonym note, modeled on the ParamBinding note at `cfdb-core.md:135` (#538 precedent: describe-docs must tell the truth). `caller_qname`'s describe entry gets the same caveat.

### 3.3 Producer changes

- **syn**: `extract_workspace` (`lib.rs:465`) stops discarding target identity — it threads `(TargetKind→TargetDiscriminator, target.name)` per target root into `visit_file`, which carries it like `bounded_context` already is carried (per-target-root context, same threading pattern — solid-architect lens confirmed no new reason-to-change).
  **Identity/display split is compiler-enforced**: `emit_item_with_flags` (`emit/mod.rs:236`), `emit_item` (`emit/mod.rs:192`) and `emit_variant` (`sub_items.rs:189`) return `(String, String)` = `(id, display_qname)` instead of id-only, and the ~7 `visits.rs` call sites destructure both instead of reconstructing the qname via `qname_from_node_id(&id)` — the reconstruction idiom is what would otherwise leak `#bin:{name}` into `:CallSite.caller_qname` / `:Param|:Field|:Variant.parent_qname` (three seats converged on this; the leak is prop-side only, edge endpoints round-trip correctly). `emit_call_site_node_and_edge` takes the discriminated `caller_identity` and the bare `caller_display` as **separate parameters** — it no longer derives one from the other.
- **HIR** (rewritten after rust-systems' vendored-source verification of ra_ap 0.0.328 — the previously claimed "ra_ap exposes bin vs lib on the crate data" is **false**: `CrateData`/`ExtraCrateData`/`hir::Crate` carry no target kind; `TargetData.kind` is consumed only into `is_proc_macro` at `workspace.rs:1662` and discarded; `CrateOrigin::is_lib()` means "non-member dep", a false friend): `build_hir_database` (`hir_db.rs:66-93`) switches from the one-shot `load_workspace_at` to the public two-step `ProjectWorkspace::load(..)` + `load_workspace(..)` and **retains the `CargoWorkspace`** (`ProjectWorkspaceKind::Cargo { cargo, .. }`, all pub; `ra_ap_project_model` is already a direct dep). A map `AbsPathBuf(target root) → (TargetKind, target name)` is built once per load from `TargetData`; emitters correlate `hir::Crate::root_file(db)` → `Vfs::file_path` → `VfsPath::as_path()` against it. This is an explicit **public-signature change to `build_hir_database`** — the correlation map becomes a fourth returned value threaded to the emitters (today the target data is consumed and discarded *inside* `load_workspace_at` and never survives to the returned triple); the function's RFC-029 §A1.2 concrete-`RootDatabase` contract is unaffected (the addition is a plain value, not a trait object). Two-lens convergence: rust-systems from the ra_ap API surface, clean-arch from composition-root feasibility. This root-file correlation is the **only** reliable mechanism for the same-named-bin case, where `origin` (package name) and `display_name` (target name) are byte-identical between the lib and bin crate inputs. #517's package-name qname stays; only the *id* gains the suffix.

### 3.4 Contention diagnostic (cfdb-petgraph)

- New `WarningKind::IdentityContention` variant — additive on the `#[non_exhaustive]` enum (`cfdb-core/src/result.rs:75-89`). **Not** a reuse of `EmptyResult`, which already does double duty for "query bound no rows" and "edge dropped, unknown id" (`graph.rs:217-235`); a third silent reuse would make kind-filtering meaningless (DDD + clean-arch lenses, convergent).
- In `ingest_one_node`: id already mapped ⇒ compare incoming vs existing on `file` (fallback: full prop inequality when either lacks `file`). Same-file ⇒ legitimate re-ingest, silent (documented behavior, unchanged). Different ⇒ push the warning naming the id and both files, then replace as today (replace semantics and their determinism are pre-existing and unchanged; the warning makes the loss loud, it does not alter outcomes).
- **Extract-path surfacing** (clean-arch lens: `extract.rs` never calls `execute()` and touches `.warnings` zero times today — the query-path wiring alone would leave `cfdb extract` blind): new inherent method `PetgraphStore::ingest_warnings(&self, keyspace) -> Vec<Warning>`, deliberately **off** the `StoreBackend` trait, same pattern as `execute_explained` (RFC-035 §4 — "the observability surface stays internal to cfdb-petgraph"). `compose::empty_store()` already returns the concrete `PetgraphStore`, so the CLI reaches it without trait changes. `cfdb extract` prints contention warnings to stderr and exits 0 (diagnostic, not failure).

### 3.5 Consumer-side id reconstruction (council-found class; three instances + kernel fix)

Root issue: a discriminated id and its bare display qname are conflated wherever code *reconstructs* ids from qnames or qnames from ids. The fix point is the shared kernel `cfdb-core::qname` (DDD lens: syn emit, extractor synthesize, and the HIR adapter are all unmediated consumers of the same primitive — per-producer patches cannot close the class), plus per-instance threading:

1. **`qname_from_node_id` successor**: suffix-aware `display_qname_from_node_id` (strips `item:` prefix AND any `#bin:…` suffix) lands in `cfdb-core::qname`; the prefix-only original gets a doc-warning/deprecation so no new call site regrows the idiom. All 9 production call sites audited: `visits.rs:72/189/244/287` + `synthesize.rs:86` + `hir-petgraph-adapter/lib.rs:192` (+ the 3 tuple-return sites that stop needing reconstruction entirely).
2. **Post-walk resolvers** (`resolver.rs:107-108/173/257`, RETURNS/TYPE_OF/MATCHES_ON): `emitted_item_qnames: BTreeSet<String>` structurally cannot represent "qname claimed by N target-scoped items" — it becomes a qname → identities map. Resolution policy for a multi-claimed qname: prefer the candidate in the **same target** as the source, else the **lib** candidate, never a foreign bin (mirrors rustc name resolution: a bin sees its own items and the lib, not sibling bins); residual ambiguity ⇒ skip the edge + warning. The `by_last_segment` index (`resolver.rs:267-295`, `build_last_segment_index`/`resolve_type_string`) becomes target-aware the same way — this is **named design surface in `emitter.rs`/`resolver.rs`**, not mere formula threading at the walk sites. The failure chain being closed (rust-systems, verified): a bare-`dst` edge into a bin-target item dangles, and `synthesize_referenced_items` (`synthesize.rs:87`) then *correctly by its own contract* declines to synthesize a compensating stub — `emitted_item_qnames.contains(dst_qname)` is true because the real item exists, just under a different id — so the dangle is invisible both to the one pass designed to catch dangling qnames and to 54-A's diagnostic (which fires on id collision at ingest, not on edges into never-ingested ids). A silent end-to-end recall regression in the exact population #542 measures.
3. **Enrich pass** (`attr_call_resolution.rs:160-185`, serde-default reachability, misses silent by design): its three bare-qname candidate strategies never match a discriminated id. It runs post-ingest where the `target` prop and the qname prop index exist; candidates are resolved within the caller's target context (implementer chooses mechanism; the regression test is prescribed in 54-B).
4. **Ban-rule blast radius**: `hsb-cluster.cypher:92` tie-break `a.qname < b.qname` → falls back to id ordering on equal qnames (e.g. `a.qname < b.qname OR (a.qname = b.qname AND a.id < b.id)` or the file-ordered equivalent), amended in 54-B's PR with proof that develop is zero-delta on the rule *before* the fixture that exercises the new pair shape.

## 4. Invariants

- **Determinism**: id derivation stays a pure function of facts; replace-on-contention semantics unchanged; `ci/determinism-check.sh` unaffected.
- **Recall**: rustdoc-json ground truth is lib-target-scoped; lib ids are byte-stable ⇒ `cfdb-recall` corpus results unchanged. The §3.5 resolver/enrich threading carries prescribed regression tests — **no silent recall regressions ride along**. Parity surface extends per §3.3.
- **Keyspace backward-compat**: old keyspaces load unchanged (ids are opaque strings to the store; no wire-shape change; `target` absent = old extract or non-Rust producer). New extracts change ids **only** for bin-target items and their derived children.
- **Cross-dogfood**: rules match on props, not ids; graph-specs at the pinned SHA must stay zero-row (verified in-slice).
- **SchemaVersion**: **bump V0_7_0 → V0_8_0 — unanimous, 4/4 lenses.** Precedent: V0_6_0 bumped for the structurally identical always-emitted `:Crate.crate_tier`; this RFC additionally changes wire identity for the bin-target population, which `cfdb diff`/`cfdb impact` observe across extracts. Graph-specs lockstep PR per RFC-033 I5 / CLAUDE.md §3.
- **No-ratchet**: no baselines/allowlists; the diagnostic warns unconditionally.

## 5. Architect lenses

R1 verdicts (all four REQUEST CHANGES; all amendments folded above). **R2: unanimous RATIFY** (2026-07-31), each seat re-verifying its own amendments against source. ddd's R2 note, recorded: the same-target-preference policy can mis-prefer a bin-local shadow of a lib name — the same imprecision class `resolve_type_string`'s last-segment fallback already accepts by documented tradeoff; extends existing doctrine, introduces no new failure mode.

### 5.1 Clean architecture (`clean-arch`)

R1: REQUEST CHANGES — (A) `Warning` has no `code` field ⇒ named `WarningKind::IdentityContention` variant (§3.4); (B) `cfdb extract` has no channel to `ingest_warnings` ⇒ inherent `PetgraphStore::ingest_warnings()` on the RFC-035 §4 precedent (§3.4); (C) consumer-side id reconstruction in `attr_call_resolution.rs` ⇒ §3.5.3 + 54-B regression row; (D, R1 follow-up) HIR composition root: the target data never survives `load_workspace_at`, so 54-C requires the `build_hir_database` signature change now named in §3.3 (independent convergence with rust-systems Finding 1). Ratified: `TargetDiscriminator` placement (cfdb-core deps verified serde/serde_json/thiserror-only), threading pattern, StoreBackend untouched, V0_8_0.

### 5.2 Domain-driven design (`ddd-specialist`)

R1: REQUEST CHANGES — (1) prefix-only `qname_from_node_id` leaks the suffix into display values ⇒ §3.5.1 kernel successor + audit; (2) qname-as-unique-key consumers degrade silently, incl. shipped `hsb-cluster` rule structurally silent on equal qnames ⇒ §3.5.2/§3.5.4; (3) ubiquitous-language debt ⇒ §3.2 homonym notes as blocking scope (ParamBinding/#538 precedents). Ratified: `target` vocabulary (with the §3.2 disambiguation sentence), identity/display split direction, V0_8_0.

### 5.3 SOLID + component principles (`solid-architect`)

R1: REQUEST CHANGES — (1) `callsite:` ids never centralized + `INVOKES_AT` src independently derived ⇒ §3.1 `callsite_node_id` + explicit-parameter `emit_call_site_node_and_edge`; (2) resolver.rs discriminator-blind ⇒ §3.5.2. R1 follow-up: proved edge round-trips self-correct (prefix-strip/suffix algebra) and the real defect is display-prop contamination ⇒ §3.3 tuple returns (compiler-enforced role split). Ratified: 2-site `:Item`-emission fan-out (not 23), threading SRP, all three YAGNI trims, always-emitted `target` (with the §3.2 absence-wording fix), V0_8_0. Implementer note: `emit/mod.rs` (461 LOC) may cross the 500-LOC convention — budget a #350-style split.

### 5.4 Rust systems (`rust-systems`)

R1: REQUEST CHANGES — (1) "ra_ap exposes bin vs lib on the crate data" false for vendored 0.0.328 (verified at source) ⇒ §3.3 HIR rewrite to CargoWorkspace target-root correlation via the two-step load API; (2) single-variable role conflation at emit sites ⇒ same-fixture id+prop test discipline in 54-B; (2-upgrade, R1 follow-up, cross-confirmed with ddd-specialist) resolver.rs is a systemic edge-dangle for all three resolved-edge kinds, made invisible by synthesize.rs's qname-membership contract ⇒ §3.5.2 expanded (target-aware qname + last-segment indexes as named design surface) + the 54-B two-same-qname-bins fixture row. Ratified: §3.1 formula (incl. `#` safety — ids verified write-only workspace-wide), §3.4 diagnostic, cargo edge cases (default bins, duplicate cross-package bin names, required-features bins, autobins all covered or pre-existing), determinism, perf (noise), V0_8_0.

## 6. Non-goals

- **PHP/TS identity semantics.** PHP qnames follow real PHP namespace rules (a collision there is a PHP error); TS qnames are file-scoped by `module_qpath`. The §3.4 diagnostic detects any residual cross-producer contention; changing those producers' identity is out of scope.
- **Display-qname changes.** `cfdb_recall::main` stays the rendered path; we do not invent synthetic module segments (`::bin::…`) that rustc doesn't have.
- **#504 (cli_command EXPOSES the clap type).** Reachability seeding is a separate defect (W2.4); this RFC only guarantees the bins' items/call sites *exist*.
- **Example/bench targets.** `extract_workspace` filters to `is_lib() || is_bin()` today; widening target kinds is future work and would slot into the same discriminator.

## 7. Issue decomposition

Ordering: 54-A → 54-B → 54-C. 54-A is independently shippable; the core formula lands with syn (54-B) and HIR aligns onto it (54-C), same direction as #517.

### 54-A — ingest-contention diagnostic (lands first, producer-agnostic)

Silent replace becomes loud everywhere (including any cross-language contention) and measures the blast radius on real trees before identity changes land.

```
Tests:
  - Unit: contention classifier (same-file re-ingest silent / different-file warns) as pure fn on Node pairs; the emitted Warning pinned to WarningKind::IdentityContention by variant, not by message text
  - Self dogfood (cfdb on cfdb): synthetic 2-bin workspace fixture asserts the warning fires end-to-end through BOTH `cfdb query` result warnings AND `cfdb extract` stderr (the new PetgraphStore::ingest_warnings surface); 1-bin control emits none (vacuity guard); report cfdb-self contention count in PR body
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): all rules still zero-row; exit 30 on any row blocks merge
  - Target dogfood (on qbot-core at pinned SHA): report contention-warning count in PR body for reviewer sanity-check
```

### 54-B — target-scoped identity, syn producer + core formula

`cfdb-core::qname` gains `TargetDiscriminator`, `item_node_id_for_target`, `callsite_node_id`, `display_qname_from_node_id`; syn walker threads target identity; emit helpers return `(id, display_qname)`; `target` prop emitted; resolver + enrich threading per §3.5; `SchemaDescribe` + concepts-spec homonym notes; `hsb-cluster` tie-break; SchemaVersion V0_8_0 + graph-specs lockstep PR.

```
Tests:
  - Unit: id formulas (lib byte-stable vs today; bin suffix shape; callsite_node_id formula pinned mirroring the argument_node_id test; derived ids inherit the discriminated parent); display_qname_from_node_id round-trips a discriminated id (strips prefix AND suffix); ONE shared bin-target fixture asserts ids/edge-src are discriminated AND caller_qname/parent_qname props on :CallSite/:Param/:Field/:Variant stay bare — same fixture for both, so a conflated-variable bug cannot pass the two assertions separately
  - Self dogfood (cfdb on cfdb): 4 bin targets ⇒ 4 distinct `main` :Items with target='bin:*'; :Item with target='lib' > 0; 2-bin fixture asserts both mains and both syntactically-identical :CallSites present (red-first vs develop); RETURNS/TYPE_OF/MATCHES_ON edges targeting bin-target items resolve to discriminated ids on a fixture with TWO same-qname bin targets (the #542 shape), asserted end-to-end including synthesize.rs producing zero spurious stubs and zero silent dangles (regression, §3.5.2); a #[serde(default=...)] callee defined in a bin resolves through attr_call_resolution (regression, §3.5.3); hsb-cluster reports a deliberately-duplicated same-qname/same-signature cross-bin pair (rule tie-break amended in the same PR; develop-parity on the rule proven first)
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): zero rule rows
  - Target dogfood (on qbot-core at pinned SHA): distinct :Item mains == workspace bin-target count from cargo metadata; report both numbers in PR body
```

### 54-C — HIR producer alignment

`hir_db.rs` moves to the two-step load and retains `CargoWorkspace`; target-root correlation map per §3.3 (public `build_hir_database` signature gains the map as a returned value); entry-point + call-site emitters route ids through the shared discriminated formula; `qname_parity.rs` pins syn≡HIR on bin-target fixtures.

```
Tests:
  - Unit: discriminator derivation via CargoWorkspace target-root correlation (root_file → VfsPath → AbsPath matched against TargetData.{kind,name,root}), INCLUDING the same-named-bin case where origin and display_name are byte-identical between lib and bin crate inputs (new fixture — tests/entry_point_bin_name.rs covers only the differing-name #517 case)
  - Self dogfood (cfdb on cfdb): in-process --hir parity on a bin-target fixture including a call site whose callee is a bin-local type (proc_macros=true mandatory); EXPOSES/CALLS endpoints land on discriminated ids with no dangle (the #517 regression test extended, not replaced)
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): zero rule rows
  - Target dogfood (on qbot-core at pinned SHA): none — rationale: HIR extract on qbot-core is the ~26-min nightly path (#507); the nightly self-audit covers it post-merge
```
