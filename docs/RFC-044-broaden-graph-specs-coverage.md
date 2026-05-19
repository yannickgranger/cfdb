# RFC-044 — broaden graph-specs coverage of cfdb's critical contracts

**Status:** RATIFIED 4/4 (2026-05-19) — see `council/RFC-044/RATIFIED.md`
**RFC SHA base:** `ecdee14` on `origin/develop` at convene time
**Council:** `rfc-044-graph-specs-coverage-council` (TeamCreate per global CLAUDE.md §2b)
**Lineage:** `RFC-cfdb.md` §6 G1–G6 · `RFC-035` §4 · `RFC-037` §4 · `RFC-042` §4 · `RFC-043` §4

## 1. Problem

cfdb has 6 ratified RFCs (`RFC-cfdb`, `RFC-029`, `RFC-032`, `RFC-035`, `RFC-037`, `RFC-042`, `RFC-043`). Of the ~30 named §4 invariants across those documents, **~40% are reviewer-only** — documented in markdown, partially enforced for G1 determinism / dep-direction, but with no corresponding test, `.cfdb/predicates/` file, or `.cfdb/queries/arch-ban-*.cypher` rule. The dominant pattern is "RFC asserts invariant in §4, the RFC's PR satisfies it once, post-merge drift goes uncaught."

Recent example: RFC-043 §4 declares 7 invariants (I1–I7); I4 (Schema unchanged), I6 (descriptor caveat), I7 (`ProcMacroClient` lifetime) are reviewer-only. Without a doctrine for converting RFC invariants to mechanical checks, every new RFC repeats the pattern.

Beyond RFC §4 catalogs, the 2026-05-19 archaeology surfaced 7 cross-cutting gaps in cfdb's critical contracts:

| Gap | Concrete evidence on `ecdee14` |
|---|---|
| Schema vocabulary spec coverage | `specs/concepts/cfdb-core.md` has a prose-paragraph variant listing (line 69-73); adding a new `Label::*` variant doesn't fail `make graph-specs-check`. `SchemaVersion::CURRENT` lockstep is author-discipline + `cross-dogfood.sh exit 20` only (no unit assertion). |
| Integration-seam public signatures pinned | Zero mechanism. `build_hir_database`, `extract_workspace`, `extract_call_sites`, `extract_entry_points`, `PetgraphAdapter::ingest_resolved_call_sites`, `StoreBackend` (7 methods), `cfdb_cli::compose::*` — all reviewer-only. |
| Single-site discipline (dep direction + composition root) | 5 siloed per-crate `tests/architecture_dep_rule.rs` allow-lists; cfdb-cli's slim build (no `hir` feature) has no test that asserts zero `ra_ap_*`; no test asserts `PetgraphStore::new()` is invoked only from `cfdb-cli/src/compose.rs`. |
| qname stability across extractors | Shared `cfdb_core::qname::*` helpers used by both extractors today, but no parity fixture test asserts bit-identical qnames for the same source item; 6 production sites use `format!("item:{...}")` directly, bypassing `item_node_id`. |
| Determinism propagation | `cfdb-extractor/tests/architecture_determinism.rs` (HashMap / `par_iter` / `Instant::now` ban) exists only in `cfdb-extractor`; not propagated to the 4 sibling emitter crates. |
| CLI exit-code contract | Documented in `main.rs:43-60`; three inline `process::exit(30)` in `main_dispatch.rs:48/62/142` — no centralized mapping, no integration test. |
| `#[non_exhaustive]` on schema enums | Zero `#[non_exhaustive]` in `cfdb-core/src/`; 14 closed enums (`PropValue`, `Visibility`, `CfgGate`, `ContextSource`, `Provenance`, `StoreError`, `RowValue`, `WarningKind`, `Aggregation`, plus `query::ast` enums) consumed by ~106 downstream files. Adding a variant is a SemVer landmine for every `match` site. |

These are not abstract concerns. Each gap maps to a concrete drift incident in cfdb's recent history (signature drift in `build_hir_database` between RFC-043 ratification and impl PR; `:CallSite.callee_resolved` descriptor caveat added in RFC-043 with no test to catch silent removal; 6 `format!("item:{...}")` sites bypassing `cfdb_core::qname`).

## 2. Scope

**8 vertical-slice issues**, one PR each, **no shared infrastructure across slices.** Each slice uses the simplest existing enforcement mechanism for its shape.

The RFC is the *organizing principle* for converting reviewer-only invariants into mechanical checks. The RFC is **NOT** a shared abstraction in code. There is no new "graph-specs registry" crate, no unified format crate, no shared base abstraction. Each slice reuses an existing pattern (per-crate `tests/architecture_*.rs`, `specs/concepts/<crate>.md`, `.cfdb/queries/arch-ban-*.cypher`, plain Rust unit tests, `assert_cmd` integration tests).

**Out of scope:** introducing a 9th gap; re-litigating any pre-trim merge; building any shared registry / framework / unified-format mechanism.

## 3. Design

### 3.1 — Slice 044-A — schema vocabulary completeness (+ descriptor narrative freeze)

**Mechanism (three sub-bands, all in the same PR):**

1. **Per-variant `specs/concepts/cfdb-core.md` sections.** Extend the existing `## Label` paragraph (currently `cfdb-core.md:69-73` prose listing) into per-variant sub-sections. Each `Label::*` and `EdgeLabel::*` variant gets a `### :Crate` / `### :Module` / etc. section with: (a) one-paragraph semantic description, (b) authoritative-source declaration. Per ddd-specialist R1: the section MUST declare *"the descriptor at `crates/cfdb-core/src/schema/describe/{nodes,edges}.rs` is authoritative; this spec section mirrors it for discoverability."* This avoids creating a second vocabulary source. `make graph-specs-check` enforces existence of one section per variant.

2. **SchemaVersion::CURRENT lockstep unit test.** New test in `crates/cfdb-core/src/schema/labels/tests.rs` that walks the `SchemaVersion::*` const enumeration (V0_1, V0_2, …) and asserts `SchemaVersion::CURRENT` equals the most recent variant. Catches "forgot to bump CURRENT" silent drift.

3. **Descriptor narrative string-equality tests.** New tests in `crates/cfdb-core/src/schema/describe/tests.rs` that for each `attr(..., "narrative")` literal in `describe/nodes.rs` and `describe/edges.rs` (notably the RFC-043 `callee_resolved` caveat at `nodes.rs:280` and `resolver` enum-as-string at `:286`) assert the narrative string matches a frozen literal in the test. Updating a narrative requires updating the test in the same PR — explicit review surface for RFC §4 I6-shaped concerns.

**Reuse audit:** `specs/concepts/` exists; `.../labels/tests.rs` exists; `.../describe/tests.rs` exists (extending existing test files, no new modules).

### 3.2 — Slice 044-B — integration-seam signature pinning (Q1.c)

**Council Q1 verdict (4/4 unanimous): Q1.c — frozen `tests/signatures.toml` per crate.**

Rejected alternatives:
- **Q1.a (extend `specs/concepts/<crate>.md` with `## Public functions`).** Rejected by solid-architect — extending the graph-specs-check parser to consume signature blocks would couple buckets 1, 2, and 8 through shared parser infrastructure. No-monolith violation.
- **Q1.b (emit `:Function` nodes with `signature_hash` attribute).** Rejected by all 4 lenses — (i) ddd-specialist: `:Function` homonyms with existing `:Item{kind="fn"}` (two node labels for the same real-world referent forces query authors to choose; populations overlap but don't coincide); (ii) solid-architect: cfdb-core moves toward the Zone of Pain (inner-layer schema shaped by outer-layer enforcement tooling concerns — Dependency Rule inversion); (iii) rust-systems: no `:Function` label exists in `cfdb-core/src/schema/labels.rs` today; introducing one is a `SchemaVersion` bump requiring graph-specs-rust lockstep (RFC-cfdb §3 / `docs/cross-fixture-bump.md`); (iv) clean-arch: inner-layer type shaped by outer-layer needs.

**Mechanism:**

Each crate with a pinned public surface ships a `tests/signatures.toml` file alongside a one-shot test that:
1. Loads `tests/signatures.toml` (TOML with `[crate_path]` sections, each containing `signature = "..."` string for the pinned item).
2. Parses the crate's public surface (via `syn` on `lib.rs` and re-exports) to extract current signatures.
3. Asserts each pinned signature matches the current shape verbatim.

Pinned signatures (initial set):
- `cfdb_extractor::extract_workspace`
- `cfdb_hir_extractor::build_hir_database` (post-RFC-043 3-tuple shape when 043-A merges; current 2-tuple shape until then)
- `cfdb_hir_extractor::extract_call_sites`
- `cfdb_hir_extractor::extract_entry_points`
- `cfdb_hir_petgraph_adapter::PetgraphAdapter::ingest_resolved_call_sites`
- `cfdb_core::store::StoreBackend` — all 7 trait methods
- `cfdb_cli::compose::*` — all 6 `pub(crate)` factory functions (visible to `cfdb-cli` siblings)

**Rotation cost:** when a signature *intentionally* changes, the implementing PR updates `tests/signatures.toml` in the same commit. Review surface is the diff on the TOML file — explicit, single-place.

**Reuse audit:** Plain Rust unit test, no new framework. Zero schema impact (no new `:Function`-with-`signature_hash` extraction pipeline).

### 3.3 — Slice 044-C — single-site discipline (dep-direction + composition root)

**Mechanism (three sub-bands):**

1. **Workspace-level dep-direction declaration (inert TOML).** Per clean-arch R1: this MUST NOT be a new shared Rust crate or `[dev-dependencies]` package — that would introduce inter-bucket coupling. Instead: a single `.cfdb/workspace-dep-rules.toml` (or similar — naming TBD at impl time, simple inert format) declaring the allow-/forbid-graph. Each existing per-crate `tests/architecture_dep_rule.rs` reads this TOML and applies the relevant rows. DRYs the 5 siloed allow-lists without coupling them at the Rust level.

2. **`PetgraphStore::new` regression-guard.** New file-scan test in `cfdb-cli` asserts `PetgraphStore::new()` is invoked only from `cfdb-cli/src/compose.rs` (currently three sites at `:53`, `:150`, `:234`; the `:234` site is `#[cfg(test)]`). Per clean-arch R1 correction: this is a **regression guard on an already-true property**, not a fix. `cfdb-cli/src/hir.rs::extract_and_ingest_hir(store: &mut PetgraphStore, …)` receives the store from the caller — it is **not** a second composition root, contrary to the BRIEF's initial framing.

3. **Slim cfdb-cli no-`ra_ap_*` test.** New test in `cfdb-cli` (gated to run when the `hir` feature is OFF) asserts the transitive dep graph contains zero `ra_ap_*` crates. Documented invariant from `crates/cfdb-cli/Cargo.toml:65-69`; this slice makes it mechanical.

**Reuse audit:** Per-crate `architecture_dep_rule.rs` pattern exists; file-scan test pattern exists in `cfdb-hir-extractor/tests/arch_boundary.rs`; cargo metadata transitive-dep checking pattern exists in cargo ecosystem. No new infrastructure.

### 3.4 — Slice 044-D — qname stability across extractors

**Mechanism (two sub-bands):**

1. **Cross-extractor parity fixture test.** New test in `crates/cfdb-recall/tests/` (or `crates/cfdb-hir-extractor/tests/`) that takes a small synthetic Rust source fixture, runs both `cfdb_extractor::extract_workspace` and `cfdb_hir_extractor::extract_call_sites`, and asserts the qnames emitted for the same source item are bit-identical. Fixture must exercise impl-target normalization (`normalize_impl_target` from `cfdb-core/src/qname.rs`) since that's the historical divergence risk.

2. **Static ban on literal `"item:"` formatting outside `cfdb_core::qname`.** New file-scan test that fails on any `format!("item:{` or `"item:".to_string()` outside `crates/cfdb-core/src/qname.rs`. Per rust-systems R1: **the archaeology found 6 real production violations to fix in the same PR**:
   - `attr_call_resolution.rs:164,171,180,197` (4 sites)
   - `bounded_context.rs:217,368` (2 sites)

   These are not exclusions — they bypass `cfdb_core::qname::item_node_id` and must be refactored to call the canonical helper.

**Reuse audit:** Plain Rust test; file-scan pattern matches existing `arch_boundary.rs`. No new framework.

### 3.5 — Slice 044-E — determinism propagation

**Mechanism:** Copy the `cfdb-extractor/tests/architecture_determinism.rs` file-scan pattern (bans `HashMap`, `HashSet`, `sort_unstable*`, `par_iter`, `Instant::now`, `SystemTime`) into the 4 sibling emitter crates that lack it:

- `crates/cfdb-hir-extractor/tests/architecture_determinism.rs`
- `crates/cfdb-hir-petgraph-adapter/tests/architecture_determinism.rs`
- `crates/cfdb-petgraph/tests/architecture_determinism.rs`
- `crates/cfdb-extractor-php/tests/architecture_determinism.rs`
- `crates/cfdb-extractor-ts/tests/architecture_determinism.rs`

Per rust-systems R1:
- **cfdb-hir-extractor does NOT need additional Salsa-specific bans.** VFS ordering is already mitigated by `call_site_emitter.rs:108-122` (explicit path-sort before traversal) and `:152-159` (stable-sort output). The propagated ban list is sufficient.
- **cfdb-petgraph has 2 existing safe-but-fires sites that must be fixed in the same PR:**
  - `HashSet` in `ast_signals.rs:69` (dedup-then-sort pattern; determinism-safe but the ban fires) — refactor to `BTreeSet` or `Vec` + `sort`/`dedup`.
  - `sort_unstable` in `clustering.rs:65` (in `hash_cluster()` before sha256; output-order-safe but the ban fires) — switch to `sort` (stable).
- **cfdb-extractor-php, cfdb-extractor-ts, cfdb-hir-petgraph-adapter are clean** — propagation lands without code changes.

**Reuse audit:** Pure copy of an existing static-check pattern across siblings. No coupling — each test file is independent. This is the canonical example of *acceptable* duplication per the no-monolith directive.

### 3.6 — Slice 044-F — CLI exit-code contract

**Mechanism (two sub-bands):**

1. **Centralized `exit_code_for` function.** Per solid-architect R1: the function MUST NOT live in `crates/cfdb-cli/src/error.rs` (that file owns the error taxonomy — adding I/O policy gives it a second reason to change, violating SRP). Place `fn exit_code_for(e: &CfdbCliError) -> i32` either in `crates/cfdb-cli/src/main_dispatch.rs` (next to the existing three inline `process::exit(30)` calls at `:48`, `:62`, `:142`) or in a new sibling `crates/cfdb-cli/src/main_exit.rs` (single-purpose module). Replace the three inline `process::exit(30)` calls with `process::exit(exit_code_for(&e))`.

2. **`assert_cmd` integration test.** New test in `crates/cfdb-cli/tests/exit_codes.rs` that runs the cfdb binary against representative inputs (workspace with no findings → exit 0; cargo metadata failure → exit 1; bad CLI args → exit 2; workspace triggering a ban rule → exit 30) and asserts the documented contract from `main.rs:43-60`.

**Out of scope:** wrapper-script exit codes 10 (clone failure) and 20 (SchemaVersion lockstep window) — those live in `ci/cross-dogfood.sh` and are not cfdb-cli's responsibility.

**Reuse audit:** Plain Rust function; `assert_cmd` is an existing dev-dep pattern. No new framework.

### 3.7 — Slice 044-G — `#[non_exhaustive]` on cfdb-core schema enums

**Mechanism:**

1. **Annotate 14 enums** with `#[non_exhaustive]` in `cfdb-core/src/`:
   - `PropValue` (`fact.rs:19`)
   - `Visibility` (`visibility.rs:36`)
   - `CfgGate` (`cfg_gate.rs:37`)
   - `ContextSource` (`context_source.rs:26`)
   - `Provenance` (`schema/descriptors.rs:24`)
   - `StoreError` (`store.rs:25`)
   - `RowValue` (`result.rs:26`)
   - `WarningKind` (`result.rs:74`)
   - `Aggregation` (`query/ast.rs:221`)
   - `Direction` (`query/ast.rs:112`)
   - `Predicate`, `Expr`, `Pattern`, `ProjectionValue`, `Param` (`query/ast.rs`)

2. **Carve-outs (remain exhaustive)** per solid-architect R1:
   - **`ItemKind`** (`query/item_kind.rs:17`) — closed query vocabulary; downstream Cypher evaluators dispatch on every variant. `_ =>` arms would mask "evaluator forgot to handle new kind" bugs.
   - **`CompareOp`** (`query/ast.rs:157`) — evaluator-dispatch enum; same rationale as ItemKind.

3. **Downstream policy** per rust-systems R1: add `#![deny(non_exhaustive_omitted_patterns)]` (clippy 0.1.93+) to **downstream consumer crates** (`cfdb-petgraph`, `cfdb-query`, `cfdb-cli`, etc.), NOT to `cfdb-core` itself. This forces downstream `match` sites to include `_` arms when matching on annotated enums, surfacing future-variant readiness at compile time.

4. **Pre-ship cross-dogfood check.** Before merging this slice, actively run `cargo check` against `agency:yg/graph-specs-rust` (cfdb's principal downstream consumer) at the current pin and confirm all `match` sites compile with the newly-annotated enums. Findings (downstream `_` arm additions) become the same-day companion PR per the existing lockstep pattern.

**Reuse audit:** Pure attribute additions + downstream lint directive. No new module, no new test framework. The clippy lint is upstream-provided.

### 3.8 — Slice 044-H — frozen RFC §4 invariant catalog (Q2.b)

**Council Q2 verdict (4/4 unanimous): Q2.b — `.cfdb/queries/arch-ban-rfc-<n>-<topic>.cypher`.**

Rejected alternative: Q2.a (`.cfdb/predicates/rfc-<n>-*.cypher`) — rejected by all 4 lenses on the basis that `.cfdb/predicates/` is linguistically scoped to *parameterized on-demand queries* (per `.cfdb/predicates/README.md`); RFC §4 invariants are non-parameterized zero-tolerance always-enforced checks — semantically they ARE ban rules. Q2.b reuses the existing `cfdb violations --rule` CI wiring already cited in cfdb CLAUDE.md §3.

**Mechanism:**

For each ratified RFC's §4 invariants, write one `.cfdb/queries/arch-ban-rfc-<n>-<topic>.cypher` file expressing the invariant as a Cypher predicate that returns ROWS WHEN VIOLATED (so the existing `cfdb violations` zero-row-pass semantics applies). Initial catalog (this slice ships at least 6 rules):

| Source | Invariant | Cypher predicate shape |
|---|---|---|
| RFC-cfdb §6 G2 | `query()` is read-only | match patterns that write through `:execute()` paths that should be read-only |
| RFC-cfdb §6 G3 | `enrich_*()` is additive (no fact deletions) | (catalogued; predicate written in this slice) |
| RFC-cfdb §6 G5 | snapshots immutable (only `drop_keyspace` deletes) | (catalogued; predicate written in this slice) |
| RFC-037 §4 G6 | no breaking queries in `.cfdb/queries/` or `examples/queries/` | continuous re-check (currently RFC-time only) |
| RFC-042 §4 | no duplicate `:EntryPoint` emission | (catalogued; predicate written in this slice) |
| RFC-042 §4 | SchemaVersion stability under flag toggle | (catalogued; predicate written in this slice) |
| RFC-043 §4 I4 | Schema unchanged with `--hir` | (catalogued; predicate written in this slice) |
| RFC-043 §4 I7 | `ProcMacroClient` lifetime invariant | (codified as a structural check on `build_hir_database` signature) |

**Continuous catalog policy:** every future RFC's §4 invariants ship a corresponding `arch-ban-rfc-<n>-*.cypher` rule **in the same PR as the RFC's implementation**, not as a follow-up issue. The convener of every future RFC's council MUST verify this at ratification time.

**Reuse audit:** `.cfdb/queries/arch-ban-*.cypher` directory + `cfdb violations` runner exist. This slice adds files to an existing pattern. No coupling across rules — each `.cypher` file is independent.

## 4. Invariants

### I1 — No-monolith
No slice in this RFC introduces a shared abstraction, registry, framework, or unified format consumed by another slice. Each slice uses an existing pattern (per-crate test, spec section, `.cypher` rule, plain unit test, attribute annotation). The convener verifies this at every PR review.

### I2 — Determinism
Every test introduced by this RFC must produce identical results across two consecutive runs on an unchanged tree. The static determinism gate (slice 044-E) catches `HashMap`/`Instant::now`/`par_iter` introductions; tests themselves must be deterministic by construction.

### I3 — Recall
This RFC adds enforcement; it does NOT change the recall surface of any extractor. `cfdb-recall` baseline does not move. The cross-extractor parity test (slice 044-D) is a NEW recall-adjacent test (rustdoc-ground-truth vs both extractors), but it asserts qname equality between extractors, not extractor-vs-rustdoc.

### I4 — Schema unchanged
No `cfdb_core::SchemaVersion` bump. No new `Label::*`, no new `EdgeLabel::*`, no new `PropValue` variant. Slice 044-G annotates existing enums with `#[non_exhaustive]` — this is a SemVer-minor change at the Rust level but does NOT touch the wire schema, so no `SchemaVersion::CURRENT` change is required.

### I5 — Graph-specs-rust lockstep
This RFC ships no `SchemaVersion::CURRENT` bump (per I4), so `.cfdb/cross-fixture.toml` on `agency:yg/graph-specs-rust` does not need an SHA bump. **Exception:** slice 044-G's downstream `_` arm additions in graph-specs-rust (per §3.7 sub-band 4) require a paired companion PR — author discipline, not a SchemaVersion lockstep.

### I6 — No metric ratchets
This RFC introduces zero baseline/ceiling/allowlist files. Every check is zero-tolerance against a hard threshold. To raise a threshold (e.g., to add a new `#[non_exhaustive]` carve-out), edit the source in a reviewed PR — per global CLAUDE.md §6 rule 8.

### I7 — Independence of slices
Each of the 8 slices ships as an independent PR with its own AC. Slice ordering (G → C → E → A → D → F → H → B) is a *recommendation* based on R1 council consensus (see §5). Any slice may ship in a different position if circumstances warrant; the ordering does NOT establish hard merge dependencies. Each slice's `Tests:` block (see §7) is the AC, not the position in the ordering.

## 5. Architect lenses

R1 (and final) verdicts captured at `council/RFC-044/verdicts/`. Headline outcomes:

| Lens | Verdict | Primary findings |
|---|---|---|
| `clean-arch` | RATIFY w/ RC on #3 | BRIEF correction: `hir.rs` is NOT a second composition root (verified `hir.rs:24-25`); workspace dep-direction DRY MUST be inert TOML, NOT a new shared Rust crate; Q1.c + Q2.b prescribed. |
| `ddd-specialist` | RATIFY w/ RC on #1, #8 | Per-variant spec sections MUST declare describe/{nodes,edges}.rs authoritative; Q1.b REJECTED on `:Function` vs `:Item{kind="fn"}` homonym; Q1.c + Q2.b prescribed. |
| `solid-architect` | RATIFY w/ RC on #2, #6, #7 | `ItemKind` + `CompareOp` carve-out from `#[non_exhaustive]`; `exit_code_for` placement → `main_dispatch.rs` or `main_exit.rs` sibling, NOT `error.rs` (SRP); Q1.a REJECTED (couples buckets 1, 2, 8); Q1.c + Q2.b prescribed. |
| `rust-systems` | RATIFY w/ RC on #4, #5 | 6 production `"item:"` violations to fix in 044-D PR (attr_call_resolution.rs:164/171/180/197 + bounded_context.rs:217/368); 2 incidental cfdb-petgraph fix-in-same-PR sites for 044-E (ast_signals.rs:69, clustering.rs:65); cfdb-hir-extractor needs no Salsa-specific bans; clippy 0.1.93 supports `non_exhaustive_omitted_patterns`; pre-ship graph-specs-rust cross-dogfood mandatory; Q1.c + Q2.b prescribed. |

**Q1 (signature pinning) outcome: Q1.c unanimous. Q1.a + Q1.b explicitly rejected with rationale captured in §3.2.**

**Q2 (invariant catalog) outcome: Q2.b unanimous. Q2.a rejected with rationale captured in §3.8.**

**Convener synthesis:** `council/RFC-044/SYNTHESIS-R1.md`. No R2 round — all RC findings are independent text-trim consolidations, not contested mechanisms.

## 6. Non-goals

- Adding a 9th gap or bucket. The 8 buckets are scoped exhaustively against the 2026-05-19 archaeology; any 9th gap is RFC-045 material.
- Introducing a shared "graph-specs framework" / "invariant DSL" / "spec registry" crate. **Explicit non-goal** per the no-monolith directive (§4 I1).
- Building a `cfdb violations --all` mega-runner. The existing `for r in .cfdb/queries/*.cypher; do cfdb violations --rule "$r"; done` shell loop (per cfdb CLAUDE.md §6) is sufficient; this RFC adds rules to that corpus, not a new runner.
- Migrating existing arch-ban Cypher rules to a new format. The existing 4 rules in `.cfdb/queries/` stay where they are; this RFC adds rules alongside them.
- Bumping `cfdb_core::SchemaVersion`. Slice 044-G adds attribute annotations only; no wire-schema change.
- Refactoring the 5 per-crate `tests/architecture_dep_rule.rs` files into a single test. Slice 044-C DRYs the *declaration* (inert TOML) without merging the *tests*. The siloed tests stay.

## 7. Issue decomposition

8 vertical slices, one PR each. Convener-prescribed shipping order (council 3/4 consensus on positions 1-5; 044-B last is 3/4 consensus):

**044-G → 044-C → 044-E → 044-A → 044-D → 044-F → 044-H → 044-B**

Each slice ships independently; the order is a recommendation, not a hard merge dependency (per I7). Tests blocks per slice are below — pending consolidation from the 4 R1 verdict files (`council/RFC-044/verdicts/`). Until consolidation lands, refer to each lens's D2 prescription for the slice's primary lens. The consolidation will be appended below before issues are filed.

Tests blocks below are consolidated from the 4 R1 verdicts' D2 prescriptions. Each row is concrete (named test file / cargo command / assertion shape) per CLAUDE.md §2.5.

### Slice 044-A — schema vocabulary completeness (+ descriptor narrative freeze)

*Primary lens: ddd-specialist.*

Tests:
- Unit: (a) `assert_eq!(SchemaVersion::CURRENT, <latest-const>)` in `crates/cfdb-core/src/schema/labels/tests.rs` — adding a `Label`/`EdgeLabel` variant without updating the completeness constant fails the test. (b) String-equality tests in `crates/cfdb-core/src/schema/describe/tests.rs` over every `attr(..., "narrative")` literal in `schema/describe/nodes.rs` and `edges.rs` against a frozen snapshot; mutating narrative text fails the test. (c) For each `Label::*` pub const, assert a top-level `## <LabelName>` section exists in `specs/concepts/cfdb-core.md` containing the attributes enumerated in the corresponding `NodeLabelDescriptor` (field names only).
- Self dogfood (cfdb on cfdb): `make graph-specs-check` exits 0 on cfdb's own tree after the per-variant sections are added; verifies `cfdb_core::Label::RFC_DOC` and every other `pub const` in `Label`/`EdgeLabel` has a matching section.
- Cross dogfood (cfdb on graph-specs-rust at pinned SHA): no schema-fact change → zero new rule rows. `ci/cross-dogfood.sh` exits 0.
- Target dogfood (on qbot-core at pinned SHA): none — rationale: 044-A is test/spec infrastructure only; no new facts emitted; qbot-core CLI consumption is unchanged. PR body reports "N new label sections added; M variants now covered (previously P)."

### Slice 044-B — integration-seam signature pinning (Q1.c — frozen `tests/signatures.toml` per crate)

*Primary lenses: clean-arch + solid-architect.*

Tests:
- Unit: Per pinned crate (`cfdb-extractor`, `cfdb-hir-extractor`, `cfdb-hir-petgraph-adapter`, `cfdb-core`), a `tests/signatures.toml` fixture is loaded by an inline per-crate unit test; the test uses `syn` to parse the public surface and asserts byte-equality against the frozen TOML entries for the 4–7 pinned seam functions. Intentional signature change requires updating the per-crate `signatures.toml` in the same PR. Parser must be inline (≤10 LOC of `toml` + string comparison) — NO shared `cfdb-signatures-check` crate.
- Self dogfood (cfdb on cfdb): `cargo test -p cfdb-extractor -p cfdb-hir-extractor -p cfdb-hir-petgraph-adapter -p cfdb-core` exits 0 with the pinned signatures committed. PR body must include a "murder test" proof: one signature deliberately mutated locally, test failed with diff; mutation reverted.
- Cross dogfood (cfdb on graph-specs-rust at pinned SHA): no schema or fact change → zero new rule rows. `ci/cross-dogfood.sh` exits 0.
- Target dogfood (on qbot-core at pinned SHA): none — rationale: signatures.toml is a cfdb-internal compile-time contract; qbot-core consumes cfdb-cli binaries, not Rust APIs. PR body lists "N pinned signatures across M crates" for reviewer sanity-check.

### Slice 044-C — single-site discipline (dep-direction TOML + PetgraphStore::new regression-guard + slim cfdb-cli no-`ra_ap_*`)

*Primary lens: clean-arch.*

Tests:
- Unit: (a) Workspace-level dep-direction declaration as an inert TOML/text file (NOT a shared Rust crate or `[dev-dependencies]` package) consumed via `include_str!` by each per-crate `architecture_dep_rule.rs`; per-crate test files remain individually compilable. (b) Regression-guard file-scan in `cfdb-cli/tests/`: grep `crates/` and assert `PetgraphStore::new` appears only in `cfdb-cli/src/compose.rs` (zero hits in `hir.rs`); test body asserts non-empty file list (non-vacuity guard). (c) Slim-cfdb-cli Cargo feature-gate test: `cargo build -p cfdb-cli --no-default-features` followed by `cargo tree -p cfdb-cli --no-default-features` asserting zero `ra-ap-*` entries in the dep tree.
- Self dogfood (cfdb on cfdb): all three assertions above pass on cfdb-self at HEAD; `cargo test -p cfdb-core --test architecture_dep_rule` exits 0 after the DRY extraction.
- Cross dogfood (cfdb on graph-specs-rust at pinned SHA): no schema change → zero new rule rows; companion consumes cfdb as a binary, not as a library, so dep-direction enforcement is cfdb-internal. `ci/cross-dogfood.sh` exits 0.
- Target dogfood (on qbot-core at pinned SHA): none — rationale: dep-direction enforcement is cfdb-internal. PR body reports "1 composition root confirmed (compose.rs); 0 hir.rs PetgraphStore::new sites; 0 ra-ap-* entries in slim cfdb-cli dep tree."

### Slice 044-D — qname stability (cross-extractor parity fixture + fix 6 production `"item:"` violations)

*Primary lens: rust-systems.*

Tests:
- Unit: (a) Cross-extractor parity fixture test: a small in-repo synthetic workspace (3–5 Rust files including at least one `impl Vec<T>` generic impl target and one method call) → `cfdb-extractor::extract_workspace` and `cfdb-hir-extractor::extract_call_sites` on the same source → assert all qnames that appear in both outputs are bit-identical. (b) Fix the 6 production `format!("item:{...}")` violations at `cfdb-petgraph/src/enrich/attr_call_resolution.rs:164,171,180,197` and `bounded_context.rs:217,368` — migrate to `cfdb_core::qname::item_node_id`. (c) Static ban (post-fix): grep prod source for `format!("item:` (no closing `}`) outside `cfdb_core::qname`; non-vacuity guard required. Optional Cypher variant: arch-ban rule asserting "no `:Item` node whose `id` does not match the cfdb_core::qname formula".
- Self dogfood (cfdb on cfdb): `cfdb extract --workspace . --db .cfdb/db --keyspace cfdb` (syn) then `cfdb extract --workspace . --db .cfdb/db --keyspace cfdb --hir` (HIR overlay) → assert zero `CALLS` edges with dangling `dst` (no `item:X` referenced by HIR but absent from syn `Item` set). `cfdb violations --rule .cfdb/queries/arch-ban-qname-literal.cypher` exits 0.
- Cross dogfood (cfdb on graph-specs-rust at pinned SHA): the new arch-ban rule must produce zero rows on graph-specs-rust at pinned SHA. `ci/cross-dogfood.sh` exits 0 (exit 30 on any rule row blocks merge per cfdb CLAUDE.md §3). Implementer verifies before commit and notes companion-side check in PR body.
- Target dogfood (on qbot-core at pinned SHA): none — rationale: qname format is an internal encoding; qbot-core sees only extracted fact files. PR body reports "0 dangling-dst CALLS edges in cfdb-self keyspace; 6 production qname-literal violations fixed."

### Slice 044-E — determinism propagation (copy `architecture_determinism.rs` to 4 sibling crates + fix 2 incidental cfdb-petgraph sites)

*Primary lens: rust-systems.*

Tests:
- Unit: Copy `crates/cfdb-extractor/tests/architecture_determinism.rs` verbatim to `cfdb-hir-extractor/tests/`, `cfdb-hir-petgraph-adapter/tests/`, `cfdb-petgraph/tests/`, `cfdb-extractor-php/tests/`, `cfdb-extractor-ts/tests/`. Each file is a self-contained `#[test]` scanning its own crate's `src/` for the canonical ban list (`HashMap`, `HashSet`, `par_iter`, `rayon`, `sort_unstable`, `Instant::now`, `SystemTime`). No shared ban-list file; no shared runner. Pre-fix the two incidental cfdb-petgraph violations in the same PR: `HashSet → BTreeSet` at `enrich/metrics/ast_signals.rs:17,69`; `sort_unstable() → sort()` at `enrich/metrics/clustering.rs:65`.
- Self dogfood (cfdb on cfdb): `cargo test -p cfdb-hir-extractor -p cfdb-hir-petgraph-adapter -p cfdb-petgraph -p cfdb-extractor-php -p cfdb-extractor-ts` all exit 0. `ci/determinism-check.sh` (runtime G1 complement) exits 0.
- Cross dogfood (cfdb on graph-specs-rust at pinned SHA): no schema or behavior change → zero new rule rows. `ci/cross-dogfood.sh` exits 0.
- Target dogfood (on qbot-core at pinned SHA): none — rationale: determinism enforcement is cfdb-internal. PR body reports "5 new architecture_determinism.rs files added; 0 production violations remaining at ship time (2 cfdb-petgraph incidentals fixed in-PR)."

### Slice 044-F — CLI exit-code contract (`exit_code_for` in `main_dispatch.rs`/`main_exit.rs` + `assert_cmd` integration test)

*Primary lens: solid-architect.*

Tests:
- Unit: (a) Unit test for the new `fn exit_code_for(e: &CfdbCliError) -> i32` in `cfdb-cli/src/main_dispatch.rs` (or sibling `main_exit.rs`) — NOT in `error.rs` (placing it in `error.rs` couples I/O policy to error taxonomy; SRP violation per solid-architect verdict). Asserts: `exit_code_for(RuleViolation) == 30`, `RuntimeError == 1`, `UsageError == 2`, `Ok == 0`. (b) File-scan regression guard: assert no raw `process::exit` calls remain in `main_dispatch.rs` (zero hits outside the one centralized site).
- Self dogfood (cfdb on cfdb): `assert_cmd` integration test against the real `cfdb` binary: `cfdb violations --rule nonexistent.cypher` exits 2; `cfdb violations --rule .cfdb/queries/arch-ban-path-regex.cypher --db .cfdb/db --keyspace cfdb` exits 0 (self-clean); a ban rule with rows on a synthetic keyspace exits 30; `cfdb violations --db <missing>` exits 1.
- Cross dogfood (cfdb on graph-specs-rust at pinned SHA): exit-code contract unchanged → `ci/cross-dogfood.sh` continues to exit 30 on any rule row, exit 0 otherwise (per `ci/cross-dogfood.sh:66-67/80/132-136`). Zero regression.
- Target dogfood (on qbot-core at pinned SHA): none — rationale: exit codes are a cfdb-cli contract consumed by qbot-core CI scripts (documented in CLAUDE.md §3). PR body reports "3 raw `process::exit(30)` call sites consolidated to 1 `exit_code_for(&e)` call" — reduction metric for reviewer.

### Slice 044-G — `#[non_exhaustive]` on cfdb-core schema enums (carve out `ItemKind`, `CompareOp`, `Direction`) + downstream `#![deny(non_exhaustive_omitted_patterns)]`

*Primary lenses: solid-architect + rust-systems.*

Tests:
- Unit: (a) `#[non_exhaustive]` annotation applied to the 9 enums per the per-enum carve-out table (annotate: `PropValue`, `Provenance`, `StoreError`, `Visibility`, `CfgGate`, `ContextSource`, `RowValue`, `WarningKind`, `Aggregation`; CARVE OUT: `ItemKind`, `CompareOp`, `Direction` remain exhaustive — these are closed query algebras where `_ =>` would mask evaluator gaps). (b) `trybuild`-style compile-fail test in `cfdb-core/tests/non_exhaustive_sealed.rs` from an external crate context confirming exhaustive match without wildcard fails to compile on annotated enums. (c) For `Aggregation`: test that evaluator returns `StoreError::Eval("unsupported aggregation")` on the `_ =>` arm rather than silently returning empty.
- Self dogfood (cfdb on cfdb): `cargo clippy -p cfdb-core --all-targets -- -D warnings` exits 0 after annotation. `cargo clippy -p cfdb-petgraph -p cfdb-extractor -p cfdb-hir-extractor -p cfdb-cli -- -D warnings` exits 0 with `#![deny(non_exhaustive_omitted_patterns)]` set on each downstream crate (the lint fires at match sites in downstream crates, NOT in cfdb-core — confirmed available on clippy 0.1.93).
- Cross dogfood (cfdb on graph-specs-rust at pinned SHA): LOAD-BEARING active check. If graph-specs-rust has exhaustive `match` arms on cfdb-core enums (especially `PropValue`, `RowValue`) without wildcards, the PR-time companion build will FAIL with a compile error (not a rule row). Implementer MUST run `cargo check` against the companion before merge and accompany the cfdb PR with a draft graph-specs-rust PR fixing any broken matches; merge graph-specs first (or simultaneously). Expected outcome: `ci/cross-dogfood.sh` exits 0 once companion is fixed; exit 20 briefly during the lockstep window per docs/cross-fixture-bump.md §4.
- Target dogfood (on qbot-core at pinned SHA): if qbot-core's Cargo.lock imports `cfdb-core` directly, run `cargo check -p <qbot-crate>` to confirm no new match-exhaustiveness errors. PR body reports the 9 annotated enums + 3 carve-outs with per-enum rationale, and the companion compile result.

### Slice 044-H — RFC §4 invariant catalog (frozen arch-ban Cypher via `.cfdb/queries/arch-ban-rfc-<n>-*.cypher`)

*Primary lens: ddd-specialist.*

Tests:
- Unit: For each new `.cfdb/queries/arch-ban-rfc-<n>-<topic>.cypher`, a static-check assertion in `cfdb-query/tests/` (reusing the existing `predicate_schema_refs` test surface) confirms the file is a parseable single Cypher statement using only schema-vocabulary `:Labels` and `[:EdgeLabels]` from `cfdb_core`. Each Cypher file's header comment documents which RFC §N invariant it encodes and (for positive invariants expressed as their negation) names the inversion.
- Self dogfood (cfdb on cfdb): `for r in .cfdb/queries/arch-ban-rfc-*.cypher; do cfdb violations --db .cfdb/db --keyspace cfdb --rule "$r"; done` — every new rule exits 0 against cfdb-self at ship time (by construction — cfdb's own source must satisfy its own RFC §4 invariants). A rule producing rows on cfdb-self means the invariant is currently violated; that must be fixed in the same PR per no-metric-ratchets (§6.8).
- Cross dogfood (cfdb on graph-specs-rust at pinned SHA): each new rule must produce zero rows on graph-specs-rust at the pinned SHA — `ci/cross-dogfood.sh` exits 0. A non-zero count on the companion is a hard merge blocker (exit 30). Implementer runs each new rule against the companion keyspace before commit and attaches output to PR.
- Target dogfood (on qbot-core at pinned SHA): run each new arch-ban-rfc-*.cypher rule against the qbot-core keyspace; report row counts in PR body (non-zero is NOT a merge blocker since these are cfdb invariants, not qbot bans, but the count surfaces each rule's scope). PR body reports "N reviewer-only RFC §4 invariants converted to Cypher; 0 violations on cfdb-self; 0 violations on companion; <K> reference findings on qbot-core."
