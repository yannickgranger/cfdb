# RFC-029 Addendum — cfdb v0.2 + Rescue Mission Protocol

**Status:** DRAFT — **SECOND-PASS COUNCIL APPROVED GREEN** (2026-04-14). Ready for user review and commit. 9 gate items defined.
**Parent:** `.concept-graph/RFC-cfdb.md` (RFC-029, v0.1 locked)
**Author:** session lead (Claude)
**Date:** 2026-04-14 (revision 1 — 7 fixes applied post-council)
**Target:** append to parent RFC as §A1–§A6 after council approval, OR merge into renumbered §17+ (parent RFC §17 is currently References — renumber required at merge time)

**First-pass council verdict:** YELLOW (revise, not rework). 4 reviewers — clean-arch, ddd-specialist, rust-systems, solid-architect — produced 3 convergent blocks (fix_skill DIP violation, `/operate-module` SRP overload, ra-ap-hir architectural replacement) and 4 unique blocks (classifier-as-query layer conflation, 6th "Context Homonym" class missing, crate-vs-context scope, cost estimate conflation). Seven targeted fixes applied in this revision.

**Change log — revision 1:**

| Fix | Section | Council source | Summary |
|---|---|---|---|
| #1 | §A2.2, §A2.3 | clean-arch WARN-2, solid-architect BLOCK-1 | Strip `fix_skill` from `:Finding` schema; move to external `SkillRoutingTable` (DIP-clean) |
| #2 | §A2.2 | clean-arch BLOCK-1 | Recast classifier from "a `.cypher` rule" to enrichment passes + Cypher query joining on enriched graph |
| #3 | §A3.3 | clean-arch BLOCK-2, solid-architect BLOCK-2 | cfdb returns structured inventory only; `/operate-module` formats raid plan markdown (parent RFC §4 invariant) |
| #4 | §A3.4 | solid-architect BLOCK-2 | `/operate-module` SRP-split from 4 to 2 responsibilities (threshold + raid plan), other concerns route externally |
| #5 | §A2.1, §A2.2, §A3.2 | ddd-specialist BLOCK-1 | Added 6th class "Context Homonym"; Context Mapping fix strategy, not mechanical sweep |
| #6 | §A3.2, §A3.3 | ddd-specialist BLOCK-2 | Scope `/operate-module` + raid plan to bounded contexts, not crates; context identification via crate-prefix heuristic + `.cfdb/concepts/*.toml` override |
| #7 | §A1.2, §A1.5, §A6 | rust-systems B1/B2/B3, clean-arch WARN-1 | Reframe `ra-ap-hir` as new parallel `cfdb-hir-extractor` crate (not upgrade); split compile/runtime costs; weekly upgrade tax acknowledged as budgeted cost; boundary architecture test; rust-version floor pin |

---

## A0. Why this addendum exists

v0.1 of cfdb as locked in RFC-029 §13 ships **one** Pattern D rule + **one** CI gate. The triple objective the user holds for the tool is larger than that:

1. **Horizontal split-brain** — same concept duplicated across crates (covered partially in v0.1 as `hsb-by-name.cypher` example, not as a gate item)
2. **Vertical split-brain** — trace from user-facing entry point DOWN through the call chain to identify unwired logic, duplicate implementations, and unclear paths (Pattern B in §3.2, **structurally blocked** in v0.1 per §13 because `:EntryPoint` + `CALLS` edges + `ra-ap-hir` are explicitly out of scope)
3. **Debt-causation taxonomy** — classify findings into root causes (duplicated feature / unfinished refactoring / random scattering / canonical bypass / unwired logic) so they can be acted on with different strategies (`/sweep-epic` vs `/port-epic` vs `/boy-scout`). This is **absent** from RFC-029 entirely — the RFC taxonomizes by *query shape*, not by *root cause*.

Beyond the triple objective, the user has framed a **project rescue mission** dimension:

> "If a module / crate is too infected, we will operate, slice it and make a drop-in replacement by portage of code in a clean prepared architecture → architects define RFC, coders transfert viable code, remove the cancer code … what we usually did: decide one head of hydra to keep and chop the others / rewire but we were blind, there are monsters all accross the board, we need systematical eradication but not stupid moves."

This addendum scopes cfdb v0.2 + a new "operate-module" protocol to close these gaps.

**Empirical justification** (from a 90-day scar log of 50 classified fixes):

| Root cause | Share |
|---|---|
| Duplicated feature | 34% |
| Canonical bypass | 22% |
| Random scattering | 22% |
| Unfinished refactoring | 14% |
| Unwired logic | 4% |
| Meta | 4% |

56% of recent fixes are duplicated-feature + canonical-bypass — the classes where a taxonomy-driven skill router has the highest leverage. Tool-found fixes in the last 10 days (post-`audit-split-brain` zero-tolerance) cluster into cheap sweeps; hand-found fixes in the preceding months trickled for weeks and surfaced as production incidents. The signal is clear: **trustworthy tooling + doughnut-scoped CI turns accumulated debt into mechanical sweeps**.

---

## A1. v0.2 scope expansion

### A1.1 Schema extensions

Extend the §7 fact schema with three first-class node kinds and two edge kinds:

| Addition | Shape | Source | Purpose |
|---|---|---|---|
| `:EntryPoint` | `{name, kind, crate, qname, file, line, visibility}` | extractor — attribute on annotated functions OR heuristic on `main`, `#[tokio::main]`, MCP handler registration sites, CLI command definitions, cron job registrations, HTTP route definitions | Catalogs every way the system is invoked from outside. Required for Pattern B reachability analysis. |
| `:CallSite` | `{id, file, line, col, receiver_type, callee_type, callee_resolved}` | `ra-ap-hir` after method dispatch resolution | Every concrete call the extractor can resolve, including method dispatch through traits and macro expansion sites. `syn` alone delivers ~40–60% recall per §10.1 — insufficient. |
| `CALLS` edge | `(:CallSite)-[:CALLS]->(:Item)` | derived from `:CallSite.callee_resolved` | Materializes call graph for BFS queries. |
| `INVOKES_AT` edge | `(:Item)-[:INVOKES_AT]->(:CallSite)` | structural — the containing function → its call sites | Lets a query walk from `:Item` → its outgoing calls. |
| `EXPOSES` edge | `(:EntryPoint)-[:EXPOSES]->(:Item)` | derived — an entry point's handler function | Lets a query walk from user-facing surface to implementation. |

**Kind vocabulary for `:EntryPoint.kind`** (v0.2 initial set, extensible):

- `mcp_tool` — MCP protocol handler
- `cli_command` — `clap`-registered subcommand
- `http_route` — `axum`/`actix` route
- `cron_job` — `messenger`/`apalis` scheduled job
- `websocket` — WS connection handler
- `test` — `#[test]` / `#[tokio::test]` function (optional, for test-scope analysis)

Entry-point detection is **heuristic, not annotation-driven** in v0.2 — users should not need to annotate existing code. The extractor recognizes each kind by its registration call pattern (e.g., `ToolRegistry::register`, `Command::new(...).subcommand(...)`, `Router::new().route(...)`).

### A1.2 `ra-ap-hir` adoption — new parallel extractor crate, not a dependency upgrade

**Architectural correction (council BLOCK, rust-systems B3).** Adopting `ra-ap-hir` is NOT an incremental dependency upgrade to the existing `cfdb-extractor`. `ra-ap-hir` does not parse — it type-checks. To resolve a method dispatch call, it must load a `ChangeSet` for the entire workspace, run type inference across all crates in dependency order, and materialize a `HirDatabase` (salsa database) for the type system. This is an **architectural replacement** of the extraction model (from file-by-file syn traversal to workspace-scoped type-checked HIR analysis), not an API swap.

**Resolution:** v0.2 ships a **new parallel crate `cfdb-hir-extractor`** alongside the existing syn-based `cfdb-extractor`. The new crate populates `:CallSite`, `CALLS`, `INVOKES_AT`, and `:EntryPoint` edges using HIR. The existing `cfdb-extractor` continues to populate `:Item`, `:Crate`, `:Module` using syn. v0.3 switches the Pattern B/C CI gate to consume `cfdb-hir-extractor` output once a release cycle of stability is proven.

**Boundary test (council WARN-1, clean-arch + rust-systems risk 1):** the architecture test covering parent RFC §14 Q2 (*"cfdb-core/Cargo.toml does not depend on lbug, duckdb, or syn — verified by an architecture test"*) is extended to assert: **no `ra_ap_*` type appears in any `cfdb-core` public type signature**. `ra-ap-hir` is confined to `cfdb-hir-extractor`; conversion to the graph schema happens inside the crate, never at the public boundary.

**Object safety constraint (council rust-systems risk 2):** `HirDatabase` is a salsa query database, explicitly NOT object-safe (uses associated types and generic methods). `cfdb-hir-extractor` must accept it as a monomorphic concrete type, not behind `dyn HirDatabase`. This is a design constraint the extractor must honor.

**Toolchain floor (council WARN-2, rust-systems):** `cfdb` workspace Cargo.toml must pin `rust-version = "1.85"` — `ra-ap-hir 0.0.328` uses `edition = "2024"` which stabilized in 1.85. Without this, developers on older toolchains get confusing compile errors.

**What `ra-ap-hir` gives us that `syn` cannot:**
- Method-dispatch resolution through trait impls (`self.resolve(input)` where `resolve` is a trait method with 5 impls)
- Macro expansion body analysis (procedural and declarative macros generate functions that `syn` cannot see without running the macros)
- Re-export chain resolution (`pub use foo::Bar` → knowing `Bar` refers to `foo::Bar` at the import site) — verified by council as a genuine gap that only hir fills
- Type inference for local variable receivers (`let x = foo(); x.bar()` → what type does `x.bar()` dispatch to?)

**Cost — two distinct categories, not one (council B1, rust-systems):**

| Category | Revised estimate | Basis |
|---|---|---|
| **Compile time** (cfdb-hir-extractor clean build, cold cache) | **+90–150s** | ~18–20 new `ra-ap-*` crates, salsa proc-macros, rustc-internal generics. The original "+30–60s" was sccache-warm inference; cold matters for CI. |
| **Compile time** (sccache warm, extractor-only change) | ~5–10s | Only the touched crate recompiles |
| **Runtime peak RSS** (cfdb extract on the target workspace) | 2–4 GB, **plausible but unverified** | HirDatabase for 23-crate workspace; no public benchmark found; v0.2 gate item measures this, see §A1.5 |

**Maintenance tax — upgraded from "risk" to acknowledged category concern (council B2, rust-systems):** `ra-ap-hir` releases **every 7 days without exception** (9 releases in 9 weeks verified on crates.io). All 10 `ra-ap-*` sub-crates are pinned with `=0.0.N` exact version constraints — every upgrade requires updating 10+ exact-pinned versions in Cargo.toml simultaneously, plus `ra-ap-rustc_type_ir` on independent versioning (2–3×/week). This is not a manageable risk; it is a **weekly maintenance tax** that must be budgeted.

**Protocol required (council proposal):** a dedicated `chore/ra-ap-upgrade-<version>` branch runs weekly, updates all `=0.0.N` pins, runs the cfdb determinism test suite, and either merges or files a compatibility issue. The cfdb CI gate does NOT consume a new `ra-ap-*` version until the chore branch lands. Runbook documented in `.concept-graph/cfdb/docs/ra-ap-upgrade-protocol.md` (post-v0.2 deliverable).

**Breaking changes observed:** rust-systems verified ~4 breaking API changes per year historically. These are not hypothetical — the `fall back to syn if API breaks` mitigation means `cfdb-hir-extractor` must be compilable and its outputs consumable even when the latest `ra-ap-*` release is incompatible. Practically: pin to last-known-good, upgrade deliberately, rollback freely.

**Alternatives considered and rejected (council-verified):**
- **`rust-analyzer` LSP IPC** — too slow, not deterministic, external process dependency
- **Regex-based call resolution** — fails on any trait dispatch, rejected in RFC-029 §10.1
- **`cargo-call-stack`** — verified unmaintained (last release 2024-10-28, 0.1.16), does NOT handle trait dispatch or generics. Removed from the Q9(c) fallback option.
- **`syn` + manual re-export resolver** — viable for shallow chains, degrades badly on nested re-exports (`qbot-ports` re-exporting from `qbot-domain`). Only hir fills this completely.
- **Wait for user annotations** — violates "heuristic not annotation-driven" principle; defeats the purpose of scanning legacy code

### A1.3 Pattern B — vertical split-brain (`vertical-split-brain.cypher`)

**Informal goal:** starting from every `:EntryPoint`, trace reachable `:Item`s through `CALLS*` edges. For parameters that flow through the call chain, detect:

1. **Resolver forks** — the same param key is resolved in two distinct functions on the same reachable path, with different default values or different type targets
2. **Param drop** — a param enters at the entry point, is decoded into a domain type at layer K, but layer K+1 or beyond reads a *different* key that was never populated from the original input
3. **Divergent defaults across paths** — `PairConfig::default()` in use-case-A returns `hedge_lookback=120`, in use-case-B returns `hedge_lookback=None`, both reachable from the same MCP tool

**Output shape:**
```
(entry_point, param_key, layer_a, layer_b, divergence_kind, evidence)
```
where `divergence_kind` ∈ `{fork, drop, divergent_default}` and `evidence` is the file:line of each side.

**Known motivating bugs:** #2651 compound-stop, #3522 pair-resolution, #3545 `build_resolved_config` 3-way scatter, #3654 7 split resolution points.

### A1.4 Pattern C — canonical bypass (`canonical-bypass.cypher`)

The `ledger-canonical-bypass.cypher` shipped in commit `349b153d6` is the prototype. v0.2 generalizes it to any resolver with a declared canonical impl.

**Informal goal:** given a marker (a comment annotation, a trait impl, or a naming convention) declaring "this function is the canonical resolver for concept X", find every call site that resolves X **without** going through the canonical impl. Emit verdict per site:

- `CANONICAL_CALLER` — uses the canonical impl (OK, no action)
- `BYPASS_REACHABLE` — bypasses the canonical impl, reachable from an `:EntryPoint` (action: rewire)
- `BYPASS_DEAD` — bypasses the canonical impl, NOT reachable from any `:EntryPoint` (action: delete)
- `CANONICAL_UNREACHABLE` — canonical impl exists but NOTHING reaches it (action: either wire bypass callers in or delete canonical)

**Known motivating bugs:** #3525 (LedgerService bypass), #3544/#3545/#3546 (parse_params / build_resolved_config scatter), #1526 (Capital.com `LiveTradingService` safety envelope bypass).

### A1.5 v0.2 acceptance gate items

In addition to v0.1 items 1–6 (§13), v0.2 gates on:

| # | Item | Measurable |
|---|---|---|
| v0.2-1 | `:EntryPoint` catalog covers ≥95% of MCP tools + CLI commands on develop | `cfdb query` count compared against `rg`-based baseline of known entry points |
| v0.2-2 | `vertical-split-brain.cypher` reproduces #2651 finding from entry point to divergent resolver | Expected output contains `compound_stop` param + two divergent paths |
| v0.2-3 | `canonical-bypass.cypher` reproduces #3525 (LedgerService bypass) when parameterized on `append_idempotent` | Expected output ≥ the 2 bypass sites from the commit message |
| v0.2-4 | `CALLS` edge recall ≥80% against manually-curated ground truth on 3 representative crates | Ground truth crates: `domain-strategy`, `ports-trading`, `qbot-mcp`. **Instrumentation caveat (council W1):** the 80% target assumes hir resolves the dispatch cases syn cannot. v0.2 ships a pre-hir syn baseline measurement first to validate whether the 80% target is achievable and whether hir is actually required for the queries we care about, or whether a narrower resolution layer would suffice. |
| **v0.2-5a** (compile time) | `cfdb-hir-extractor` clean build (cold cache) ≤ **180s** | `cargo clean && time cargo build -p cfdb-hir-extractor`. Revised from 120s per rust-systems B1 evidence (cold build is +90–150s, not +30–60s). |
| **v0.2-5b** (runtime) | `cfdb extract --workspace .` on the target workspace completes in ≤ **N min**, peak RSS ≤ **M GB** | Measured during acceptance run with `/usr/bin/time -v`. Initial targets N=5 min, M=4 GB — calibrated after first measurement, not guessed. |
| v0.2-5c (maintenance) | `chore/ra-ap-upgrade-<version>` protocol documented and dry-run once before v0.2 ships | Exists as `.concept-graph/cfdb/docs/ra-ap-upgrade-protocol.md` + one proof upgrade recorded |
| v0.2-6 | Architecture test: no `ra_ap_*` type appears in any `cfdb-core` public signature | Extends RFC §14 Q2 test. **UDF scope clarification (second-pass clean-arch OBS-1):** if Cypher UDFs are used by the classifier (e.g., `signature_divergent`, `canonical_bypass_detected`, `confidence_score`), they MUST be registered inside `cfdb-store-lbug`, not in `cfdb-core`. The architecture test asserts UDF registration points do not cross the core boundary. |
| v0.2-7 | `cfdb` workspace pins `rust-version = "1.85"` | `grep rust-version cfdb/Cargo.toml` |
| **v0.2-8** (second-pass convergent follow-up) | `signature_divergent` UDF algorithm documented + ground-truth test | Document the field-set comparison algorithm (equal name + equal field names + field semantics discriminator) in `.concept-graph/cfdb/docs/udf-signature-divergent.md`. Add a unit test against `OrderStatus` (`domain-trading` vs `domain-portfolio`) and `PositionValuation` (`domain-trading` vs `domain-portfolio`, #3618 evidence) as known ground truth for the Shared-Kernel-vs-Homonym discriminator. This UDF is load-bearing for class 2 (Context Homonym) correctness — without a documented algorithm it could misclassify Shared Kernel candidates as homonyms or vice versa. Raised by ddd-specialist second pass. |
| **v0.2-9** (second-pass convergent follow-up) | `enrich_bounded_context` accuracy spot-check on ground-truth crates | During v0.2 acceptance run, manually verify the `bounded_context` label for every `:Item` in `domain-strategy`, `ports-trading`, `qbot-mcp` (the 3 ground-truth crates used in gate v0.2-4). Produce a one-page report showing: crate → context assignment per the heuristic, any overrides from `.cfdb/concepts/*.toml`, any items where the module-level context diverges from the crate-level context (v0.3 concern, but track at v0.2). Gate passes if ≥95% of items receive the human-expected context label. Raised by clean-arch OBS-1 second pass. |

**Instrumentation gate (pre-hir validation, council W1):** before committing to `ra-ap-hir` as a hard v0.2 dependency, v0.2 ships a preliminary `syn`-only extractor that emits `CALLS` edges with `resolved=false` tagging for unresolved dispatch sites. Measurement: what fraction of Pattern B/C findings actually depend on resolved dispatch edges? If the answer is <20%, the 80% gate (v0.2-4) may be reachable with `syn` + a targeted resolution layer, and `ra-ap-hir` adoption can defer to v0.3 without blocking the triple objective. The instrumentation gate is non-blocking — v0.2 proceeds to `cfdb-hir-extractor` regardless — but it informs whether the hir pipeline ships as the primary CI gate or remains a complementary instrument.

**Still deferred to v0.3+:** entry-point annotations, LLM enrichment, cross-project queries, embedding clustering, density-based thresholds (LoC instrumentation ships in v0.2 as §A3.2 telemetry).

### A1.7 `cfdb extract --rev <url>@<sha>` — bilateral cross-repo drift-lock (Option W)

**Issue:** #96 (builds on #37 / PR #123).

**Problem.** qbot-core EPIC #4047 Phase 2 needs a cross-repo drift-lock against `yg/qbot-strategies`. The comparator consumes two fact sets (one per repo), extracted at specific SHAs, and asserts invariants across them. Without URL extraction, the user must maintain two local checkouts and orchestrate `cfdb extract --rev <sha>` separately against each — fragile, easy to misalign. Option W (qbot-core council-4046 tools-devops R2.2) is the chosen mechanism over Option Y (third `qbot-specs` repo).

**Scope.**

- `cfdb extract --rev <url>@<sha>` clones `<url>` once per `(url, sha)` pair into a persistent cache and extracts at `<sha>`.
- `<url>` carries one of `http://` / `https://` / `ssh://` / `file://` schemes. SSH shorthand (`git@host:path`) is NOT accepted in v1 — use the explicit `ssh://…` form. `file://` is supported both for hermetic integration tests and for the self-dogfood case `cfdb extract --rev file://$(pwd)/.git@$(git rev-parse HEAD)`.
- The cache base directory is, in precedence order:
  1. `$CFDB_CACHE_DIR` (explicit override; used by tests for hermeticity).
  2. `$XDG_CACHE_HOME/cfdb/extract`.
  3. `$HOME/.cache/cfdb/extract`.
  4. `std::env::temp_dir()/cfdb/extract` (last resort; non-persistent; emits `eprintln!` warning).
- Cache layout: `<base>/<sha256_hex_first_16(url)>/<full-sha>/`. Full SHA (not `short_rev`) so 12-char prefix collisions remain distinct on disk.
- A sentinel file `.cfdb-extract-ok` inside the cache dir signals a successful clone+checkout; second runs gate off the sentinel and skip the clone (AC-3).
- Auth (AC-2): subprocess `git clone` / `fetch` / `checkout` inherit ambient git credentials — SSH agent, `~/.config/git/credentials`, `GIT_ASKPASS`, `credential.helper`. No new plumbing.

**Design.**

- The `extract` dispatcher in `cfdb-cli/src/commands.rs` discriminates URL@SHA vs. plain SHA at a single match guard — `Some(rev) if is_url_at_sha(rev) => extract_at_url_rev(rev, …)`. The same-repo `extract_at_rev` (PR #123) and its `GitWorktree` RAII guard are UNCHANGED; URL form is a new sibling branch, not a modification.
- `parse_url_at_sha(&str) -> Option<(&str, &str)>` splits on the RIGHTMOST `@` so `ssh://user@host/r@deadbeef` parses correctly. The SHA side must be all-hex and ≥ 7 chars; the URL side must carry a recognised scheme.
- `git clone <url> <cache_dir>` fetches the default branch only; the arbitrary `<sha>` is explicitly fetched next (`git fetch --quiet origin <sha>`) then checked out. Gitea has `uploadpack.allowReachableSHA1InWant=true` by default, which makes this work; other servers may need the setting enabled. The CLI error names this config when fetch fails for that reason.

**Invariants.**

- **Subprocess contract preserved.** `git2` stays behind the `git-enrich` feature gate — default `cfdb-cli` builds still ship zero `git2` in their dep tree (issue #105). `sha2 = "0.10"` is the only new workspace dep (pure-Rust, small, for URL → cache-key hashing).
- **Single resolution point.** URL-vs-SHA discrimination lives ONLY in the `extract` match guard. `extract_at_rev` and `extract_at_url_rev` trust the dispatch — neither re-checks the form.
- **Determinism.** Same `(url, sha)` produces byte-identical canonical dumps on repeat extract (same guarantee as `extract --rev <sha>` today; the extraction pipeline is unchanged).
- **No `SchemaVersion` bump.** The emitted facts are identical to what the same-repo path emits; only the input-source path changes.

**Non-goals.**

- No new `--cache-dir` flag in this issue (env-var override is sufficient; flag may be added in a follow-up if a real need materialises).
- No `git2` library path for the URL clone — subprocess is the contract, matching §2 "Group B" convention.
- No HTTPS auth plumbing new to cfdb — ambient git credentials are the contract (AC-2). A future issue can add `--token` / `CFDB_GITEA_TOKEN` support if manual token injection becomes ergonomic.
- No `url` / `dirs` / `reqwest` crates — env-var path + scheme-prefix string split is sufficient.

**Tests (per CLAUDE.md §2.5).**

- **Unit:** `parse_url_at_sha` / `is_url_at_sha` / `url_hash_hex16` / `cache_base_dir` env-var precedence — covered in `crates/cfdb-cli/src/commands.rs` `#[cfg(test)] mod tests`.
- **Integration:** `crates/cfdb-cli/tests/extract_rev_url.rs` — 5 scenarios: URL form honours the SHA (AC-1), cache reuse (AC-3), unreachable URL surfaces git error (AC-2 shape; real Gitea auth is dogfood), malformed URL@SHA falls through to same-repo path, default keyspace = `short_rev(sha)`. All tests use `file://` URLs against local bare repos — zero network access.
- **Self-dogfood:** `cfdb extract --rev file://$(pwd)/.git@$(git rev-parse HEAD)` produces a keyspace on the cfdb tree.
- **Target-dogfood (manual, PR body):** `cfdb extract --rev https://agency.lab:3000/yg/qbot-core@<pinned-sha>` and the same for `yg/qbot-strategies` — per issue AC-4, reported as manual evidence in the ship PR body.

### A1.8 `.cfdb/published-language-crates.toml` — Published Language marker (Issue #100)

**Issue:** #100 (feeds §A2.1 class 2 Context Homonym classifier).

**Problem.** The six-class taxonomy (§A2.1) distinguishes `Context Homonym` (same name, different bounded contexts, semantically divergent) from a **Published Language** (intentional cross-context consumer — DDD pattern). Without a marker file, the classifier cannot tell a legitimately-shared type like `qbot-prelude::Symbol` from a divergent homonym — both present as "same name across contexts" at the graph level. Issue #48 classifier consumes the marker; the marker itself is what #100 lands.

**Scope.**

- New single-file config at `.cfdb/published-language-crates.toml` (sibling of the directory-based `.cfdb/concepts/*.toml`). Optional — missing file is not an error, and the baseline behaviour is "every `:Crate` emits `published_language: false`".
- On-disk shape:

  ```toml
  [[crate]]
  name = "qbot-prelude"
  language = "prelude"
  owning_context = "core"
  consumers = ["trading", "portfolio", "strategy"]

  [[crate]]
  name = "qbot-types"
  language = "types"
  owning_context = "core"
  consumers = ["*"]
  ```

- Loader shape: `load_published_language_crates(workspace_root: &Path) -> Result<PublishedLanguageCrates, LoadError>` in `cfdb-concepts` (canonical home per PR #103 / issue #3). Reuses the existing `LoadError` enum — `Io` + `Toml` variants cover every failure mode. Duplicate `name` entries in the TOML array are rejected via `LoadError::Io { ErrorKind::InvalidData }` (silent last-wins is forbidden).
- Public API per issue AC-2: `is_published_language(&str) -> bool`, `owning_context(&str) -> Option<&str>`, `allowed_consumers(&str) -> Option<&[String]>`. The loader does NOT interpret the `"*"` wildcard — it passes consumer strings through verbatim; wildcard semantics are the classifier's job (issue #48).
- `:Crate` nodes gain a `published_language: bool` prop materialised at extraction time in `cfdb-extractor::emit_crate_and_walk_targets`. Every `:Crate` carries the prop (no `Option`); missing file ⇒ every crate emits `false`.

**Design — extract-time only, not re-enrichment.**

`enrich_bounded_context` (PR #119) earns its re-enrichment machinery because users routinely edit context-map TOMLs between extractions and want to patch `:Item.bounded_context` without re-walking the workspace. Published Language is a rarely-edited marker (DDD policy-level decision), so the simpler extract-time emission is chosen for the first landing. If the #48 classifier later needs post-extract patching, a follow-up issue can add `crates/cfdb-petgraph/src/enrich/published_language.rs` mirroring the `enrich_bounded_context` pattern; the current extract-time wiring is a clean prerequisite.

**Invariants.**

- **No `SchemaVersion` bump.** `published_language: bool` is additive on an already-declared `:Crate` node shape; `PropValue::Bool` is already in the wire format. No lockstep `graph-specs-rust` fixture bump required (RFC-033 §4 I2: adding an optional prop does not break the schema contract).
- **Single resolution point.** `load_published_language_crates` is canonical at `crates/cfdb-concepts/src/published_language.rs`; `:Crate.published_language` is written at exactly one site (`emit_crate_and_walk_targets`). A second writer would split-brain the prop.
- **`LoadError` reused.** No parallel `PublishedLanguageLoadError` — the canonical enum covers every failure mode.

**Non-goals.**

- No classifier logic in the loader (that's #48).
- No validation that declared `consumers` actually import the crate at compile time (static TOML check only).
- No wildcard expansion at the loader layer — `"*"` passes through.
- No CLI flag — loader is library-only, invoked by `extract_workspace`.
- No sample `.cfdb/published-language-crates.toml` in the cfdb repo itself — cfdb does not currently publish a language; the self-dogfood baseline is "file absent → `published_language=false` on every crate".

**Tests (per CLAUDE.md §2.5).**

- **Unit (8 tests, `crates/cfdb-concepts/src/published_language.rs::tests`):** missing file, empty file, single crate, multiple crates, wildcard consumers, malformed TOML, determinism, duplicate-name rejection.
- **Integration (3 tests, `crates/cfdb-concepts/tests/published_language.rs`):** full-pipeline 3-entry fixture exercising all three public methods; missing `.cfdb/` baseline; `.cfdb/` without PL file baseline.
- **Self-dogfood (2 tests, `crates/cfdb-extractor/tests/published_language_dogfood.rs`):** synthetic 2-crate workspace fixture — one declared, one not; asserts prop value matches per crate. No-PL-file baseline asserts `false` on every `:Crate`.
- **Cross-dogfood:** `ci/cross-dogfood.sh` unchanged (no `SchemaVersion` bump, no new ban rule).

---

## A2. Debt-cause taxonomy (new §A2 to RFC)

### A2.1 The six classes

The existing RFC §3.1–§3.9 taxonomizes by **pattern shape** (HSB, VSB, canonical bypass, …). That answers *"what does the query look like?"*. It does not answer *"what caused this debt, and therefore what fix strategy applies?"*. The six classes below answer the second question.

**Sixth class added per council BLOCK (ddd-specialist):** "Context Homonym" — same name in different bounded contexts. Without this class, `/sweep-epic` would fire on legitimately distinct concepts that share a name across contexts, deleting bounded-context isolation. The fix strategy for a homonym is a **Context Mapping decision** (ACL / Shared Kernel / Conformist / Published Language), not a mechanical sweep — therefore a separate class is load-bearing.

Every cfdb finding (from Pattern A, B, or C rules) is labeled with exactly one class by the classifier:

| # | Class | Informal definition | Signals used | Fix strategy (abstract action — routing in §A2.3) |
|---|---|---|---|---|
| 1 | **Duplicated feature** | Two independent implementations of the same concept, **within the same bounded context**, with independent git history. Two teams or two sessions built the same thing without knowing about each other. | Same `bounded_context` on both items; independent git blame (no common commit in last N months touching both sides); `signature_hash` similarity > threshold; no cross-reference comments | **Consolidate** — pick one head (by usage count or complexity), `pub use` the other, delete the loser. |
| 2 | **Context Homonym** | Same name (or high-Jaccard structural similarity) appearing in items whose owning crates belong to **different bounded contexts**, where the semantics diverge (different domain invariants, different lifecycle, different collaborators). | `a.bounded_context <> b.bounded_context`; divergent collaborator sets in the graph (what edges go in/out differ); divergent field sets despite similar names; no shared RFC owning the concept across both contexts | **Context Mapping decision** — NOT mechanical dedup. Remedy is ACL, Shared Kernel, Conformist, or Published Language. Requires architect judgment. Routed as `council_required` → `/operate-module` with the owning-context declaration. |
| 3 | **Unfinished refactoring** | Old code and new code coexist because a planned migration was never completed. One side is actively used, the other is legacy; an RFC or issue exists that proposed the migration, and the RFC reference is scoped to the owning bounded context. | One side has recent commit cluster referencing a context-owning RFC/EPIC; the other is stale; `TODO(#issue): migrate` comment; `#[deprecated]` attribute; age_delta > 60 days | **Complete migration** — move callers, delete legacy. |
| 4 | **Random scattering** | Copy-paste drift with no refactor intent. Same function body appears N times with small variations, typically short utility helpers. | Identical or near-identical AST shape; no common refactor intent; no RFC reference; no deprecation marker; age_delta < 14 days; typically short functions | **Extract helper** — consolidate inline during adjacent feature work. |
| 5 | **Canonical bypass** | A canonical resolver exists and is correct, but some call sites go around it. This is a *wiring* bug, not a duplication bug. | `canonical-bypass.cypher` verdict = `BYPASS_REACHABLE`; canonical impl has `#[doc = "canonical …"]` marker or is the ports-layer trait impl | **Rewire** — replace direct calls with canonical calls. Delete bypass if dead. |
| 6 | **Unwired logic** | Code exists and compiles but has zero paths from any `:EntryPoint`. Either dead code (delete) or orphan code awaiting wiring (wire or delete). | BFS reachability from `:EntryPoint` set = empty; `cargo-udeps` / `cargo-machete` cross-validation | If `TODO(#issue)` attached → **wire**. Otherwise **delete**. |

**Note on the fix strategy column:** the values above are abstract action names (consolidate, complete migration, extract helper, rewire, context-mapping decision, wire/delete), NOT concrete Claude skill names. The mapping from abstract action → concrete skill (`/sweep-epic`, `/port-epic`, `/boy-scout`, `/operate-module`) is defined in the `SkillRoutingTable` (§A2.3), not in the classifier. This is the DIP-clean form (council BLOCK-1, solid-architect).

### A2.2 Classifier — enrichment passes + query, not a single `.cypher` rule

**Architectural correction (council BLOCK-1, clean-arch).** The classifier cannot be a standalone `.cypher` rule. The signals it joins on require filesystem and subprocess I/O (git log, file reads of `.concept-graph/*.md`, deprecation attribute extraction) that Cypher traversal cannot perform atomically. The classifier is a **two-stage pipeline**: enrichment passes materialize signals into the graph as new edges/attributes, THEN a Cypher query joins on the enriched graph.

**Stage 1 — enrichment passes** (each is an extractor-layer operation that mutates the graph). The table below was revised by the #43 council round 1 synthesis (2026-04-20): six passes (not five) per DDD Q4 finding; `:Concept` node materialization is a distinct sixth pass that #101 and #102 block on. `enrich_metrics` is explicitly deferred out of this pipeline:

| # | Pass | Slice | Input | Output edges / attributes | Layer |
|---|---|---|---|---|---|
| 1 | `enrich_git_history` | 43-B (#105) | git log for each `:Item`'s defining file | `:Item.git_last_commit_unix_ts` (i64 epoch, **not** `git_age_days` — see G1 note), `:Item.git_last_author`, `:Item.git_commit_count` | extractor crate (uses `git2` crate behind `git-enrich` feature flag) |
| 2 | `enrich_rfc_docs` | 43-D (#107) | `docs/rfc/*.md` + `.concept-graph/*.md` keyword match against concept names (scope narrowed — see scope-narrowing note) | `(:Item)-[:REFERENCED_BY]->(:RfcDoc {path, title})` edges + nodes | petgraph impl (reads RFC files once at pass time via workspace path stored on `PetgraphStore`) |
| 3 | `enrich_deprecation` | 43-C (#106) | `#[deprecated]` attribute extraction from syn AST | `:Item.is_deprecated`, `:Item.deprecation_since` | **extractor (extractor-time — not a Phase D enrichment)** — the attribute is syntactic and the AST walker already visits attributes (see deprecation provenance note) |
| 4 | `enrich_bounded_context` | 43-E (#108) | crate-prefix convention + `.cfdb/concepts/*.toml` overrides | `:Item.bounded_context` (re-enrichment of extractor-time output when TOML changes) | petgraph impl (re-enrichment only — extractor already populates `bounded_context` + `BELONGS_TO`) |
| 5 | `enrich_concepts` | 43-F (#109) | `.cfdb/concepts/<name>.toml` declarations | `:Concept {name, assigned_by}` nodes + `(:Item)-[:LABELED_AS]->(:Concept)` + `(:Item)-[:CANONICAL_FOR]->(:Concept)` — **DDD Q4 sixth pass; unblocks #101 + #102** | petgraph impl (reads TOML via `cfdb-concepts::ConceptOverrides`) |
| 6 | `enrich_reachability` | 43-G (#110) | BFS from `:EntryPoint` over `CALLS*` | `:Item.reachable_from_entry = bool`, `:Item.reachable_entry_count` | petgraph impl (runs after HIR extraction; degraded path with `ran: false` + warning when no `:EntryPoint` nodes present) |

**G1 determinism note — timestamps, not ages.** `enrich_git_history` stores `git_last_commit_unix_ts` (i64 epoch seconds), not `git_age_days`. Days-since-now computed at enrichment time violates G1 byte-stability across calendar days (clean-arch verdict B2, council/43/clean-arch.md). The Stage-2 classifier Cypher below computes `age_delta = abs(a.git_last_commit_unix_ts - b.git_last_commit_unix_ts) / 86400` at query time instead of reading a pre-baked `git_age_days` value.

**`enrich_metrics` — deferred out of #43 scope.** The quality-metrics pass (`unwrap_count`, `cyclomatic`, `dup_cluster_id`, `test_coverage`) is orthogonal to the debt-cause classifier pipeline: the six classes in §A2.1 do not consume these signals. The Phase A stub is retained on `EnrichBackend` and in `Provenance::EnrichMetrics` so the surface is stable; a future RFC can resuscitate the pass without a breaking rename.

**Scope narrowing of `enrich_rfc_docs`.** Renamed from the v0.1 `enrich_docs` stub and scope-narrowed to RFC-file keyword matching only. The broader rustdoc rendering implied by the former Phase A stub doc comment is an **explicit non-goal for v0.2** — no #43 slice implements it. Full rustdoc enrichment is deferred beyond v0.2 and may land behind its own RFC and Provenance variant.

**Deprecation provenance — `Provenance::Extractor`, not an enrichment tag.** The RFC's original wording ("reuses existing AST walk") is now explicit: `#[deprecated]` is extracted at extraction time and tagged `Provenance::Extractor`. The `EnrichBackend::enrich_deprecation` method exists for surface symmetry but its `PetgraphStore` impl is a `ran: true, attrs_written: 0` no-op naming the extractor as the real source. This prevents the provenance split-brain DDD Q4 flagged.

**Invariant I6 (v0.2-9 load-bearing gate).** The Stage-2 classifier (issue #48) MUST NOT be deployed until `enrich_bounded_context` (slice 43-E) hits the v0.2-9 ≥95% accuracy gate on the ground-truth crates (`domain-strategy`, `ports-trading`, `qbot-mcp`). Below 95% accuracy the `cross_context` boolean produces enough false positives to misroute mechanical dedup into expensive council deliberations and (worse) false negatives that misroute homonyms into `/sweep-epic --consolidate` — which deletes bounded-context distinctions (DDD Q3 analysis, council/43/ddd.md).

**SchemaVersion bump policy.** Per-slice patch bumps — **not** a batched single bump. Each slice that writes new attributes or labels into the graph bumps the version (V0_2_1, V0_2_2, …) with its own lockstep `graph-specs-rust` cross-fixture PR per cfdb CLAUDE.md §3. Slice 43-A ships schema reservations (`:RfcDoc` label, `REFERENCED_BY` edge, new `Provenance` variants) without bumping the version — stubs write nothing, so no wire-format consumer sees a change. The first real bump lands with whichever of 43-B/43-D/43-G reaches ship first.

**Stage 2 — classifier Cypher query** (reads only enriched facts, no I/O):

```cypher
// classifier.cypher — joins Pattern A/B/C raw outputs with enriched attributes
MATCH (a:Item), (b:Item)
WHERE <pattern A/B/C match conditions>
  AND a.bounded_context IS NOT NULL
  AND b.bounded_context IS NOT NULL
WITH a, b,
     abs(a.git_last_commit_unix_ts - b.git_last_commit_unix_ts) / 86400 AS age_delta,
     (a.bounded_context <> b.bounded_context) AS cross_context,
     exists { (a)-[:REFERENCED_BY]->(:RfcDoc) } AS has_rfc_ref,
     a.is_deprecated OR b.is_deprecated AS has_deprecation,
     a.reachable_from_entry AS a_reachable,
     b.reachable_from_entry AS b_reachable
RETURN a, b,
       case
         when cross_context AND signature_divergent(a, b) then 'context_homonym'
         when has_deprecation then 'unfinished_refactor'
         when has_rfc_ref AND age_delta > 60 then 'unfinished_refactor'
         when NOT (a_reachable OR b_reachable) then 'unwired'
         when canonical_bypass_detected(a, b) then 'canonical_bypass'
         when age_delta < 14 AND NOT has_rfc_ref then 'random_scattering'
         else 'duplicated_feature'
       end AS class,
       confidence_score(a, b) AS confidence
```

**Classifier output schema** (new `:Finding` node type — `fix_skill` removed per council BLOCK-1):

```
:Finding {
  id, pattern, class, confidence,
  canonical_side, other_sides[],
  evidence[], age_delta_days,
  rfc_references[], bounded_contexts[],
  is_cross_context: bool
}
```

where `class ∈ {duplicated_feature, context_homonym, unfinished_refactor, random_scattering, canonical_bypass, unwired}` (6 classes — see §A2.1). **No `fix_skill` field.** Skill routing is a separate concern — see §A2.3.

**Why `fix_skill` is NOT in the schema (council BLOCK-1, solid-architect).** Embedding the skill name in the graph couples the classifier (data layer) to the orchestration policy (skill layer). A skill rename or a `/port-epic`-vs-`/sweep-epic --mode=port` decision would force a graph schema migration. The classifier emits abstract `DebtClass`; a separate `SkillRoutingTable` (see §A2.3) maps class → skill at invocation time. This preserves OCP under skill naming churn and keeps the fact base free of workflow knowledge (parent RFC §4 invariant).

### A2.3 Skill routing — SkillRoutingTable (external to the graph)

**Architectural correction (council BLOCK-1).** The routing table lives **outside the graph schema**, in a separate `SkillRoutingTable` config (either `.cfdb/skill-routing.toml` or a const in the consumer skill). The classifier emits the abstract `class`; the orchestrator consults the routing table to pick a concrete skill. This preserves OCP under skill naming changes and keeps the fact base free of workflow policy (parent RFC §4 invariant).

**Routing table** (initial mapping; lives in `.cfdb/skill-routing.toml`):

| Class | Abstract action | Concrete skill (routing table, not graph) | Notes |
|---|---|---|---|
| `duplicated_feature` | Consolidate within context | `/sweep-epic` | Mechanical, parallelizable |
| `context_homonym` | Context Mapping decision | `/operate-module` with `council_required=true` | Remedy is architectural, not mechanical. Routes to operate-module to trigger raid plan + council. |
| `unfinished_refactor` | Complete migration | `/sweep-epic --mode=port --raid-plan=<path>` | Q14: variant flag on sweep-epic, not a new skill |
| `random_scattering` | Extract helper | `/boy-scout` | Inline fix during adjacent work |
| `canonical_bypass` | Rewire | `/sweep-epic` | Special case of consolidation — acknowledged seam per council WARN-1 |
| `unwired` (with `TODO(#issue)`) | Wire | `/boy-scout` or the issue owner's session | Wire to existing tracked work |
| `unwired` (no tracker) | Delete | `/boy-scout` delete | Clean removal |

**Note on `/port-epic`:** NOT a new skill. Council Q14 vote (clean-arch + solid-architect) = variant flag on `/sweep-epic`, invoked as `/sweep-epic --mode=port --raid-plan=<path>`. The distinguishing factor (approved architecture target + portage list) is an input protocol difference, not a code path difference. Promote to standalone skill only if the variant accumulates ≥3 unique responsibilities (deferred to v0.3 evaluation).

**Why a table, not hardcoded edges:** if `/sweep-epic` is later renamed or `/operate-module` is split into multiple skills, only the routing table changes — the graph schema and the classifier are untouched. This is the DIP-correct form: high-level policy (skill names) lives above the data, not inside it.

---

## A3. Operate-module rescue protocol

### A3.1 Motivation — the hydra problem

Historical pattern (see scar log):

- **#3244 Venue 4-way split-brain** — one concept had 4 incompatible meanings across domain-ledger, executor, capital-adapter, reconciliation. Fix required a coordinated sweep with a rename + two new types + a legacy-string handler. This is NOT a `/boy-scout` job — it's a surgery.
- **#2651 compound-stop** — two parallel resolution paths for trailing activation, with a hardcoded constant preempting the param-driven path. Fix required Council deliberation, 3 canary rules (M1/M2/M3 in CLAUDE.md §7), and a re-architecture of the layer-dominance model. Surgery.
- **#3519 post-forgery curation** — 46 actionable violations across 19 fix clusters. Individual clusters are mechanical, but the *meta* decision (which concept is canonical, which is the head to keep) requires architectural judgment — surgery.

A single finding is a `/boy-scout` job. A **cluster of findings inside one module** is a surgery. We need a protocol for the second case.

### A3.2 Infection threshold — scoped to bounded contexts, not crates

**Architectural correction (council BLOCK, ddd-specialist).** Thresholds operate on **bounded contexts** (groups of crates), not single crates. Crate-scoped thresholds produce the exact #3244 Venue 4-way failure mode: a raid plan reorganizes crates but does not align the bounded-context boundary, and three months later the split-brain is back.

**Bounded context identification (v0.2 initial rules, extensible):**

1. **Crate-prefix convention:** `domain-trading` + `ports-trading` + `adapters-trading` all belong to the `trading` context. Rule: strip `domain-` / `ports-` / `adapters-` / `inmemory-` / `postgres-` / `kraken-` / `mcp-` prefix suffixes, the remainder names the context.
2. **Explicit override:** `.cfdb/concepts/<context>.toml` lists crates belonging to this context, overriding the heuristic. Required for contexts that don't follow the prefix convention (e.g., cross-cutting `messenger`, `sizer`).
3. **Ownership declaration:** each context has exactly one owning RFC and one canonical domain crate declared in the `.toml`. This is load-bearing for §A2.1 class 3 "unfinished refactoring" — the RFC keyword match must be scoped to the owning context.

**A context is "too infected" when it exceeds ANY of:**

- **≥5 canonical bypasses** (Pattern C verdict = `BYPASS_REACHABLE`) counted across all crates in the context
- **≥10 duplicate types** (Pattern A findings where both sides are in crates belonging to the context)
- **≥3 context-homonym findings** (§A2.1 class 2) where this context is one side — triggers surgery regardless of other counts
- **≥3 unwired entry points** in crates belonging to the context
- **≥20% of items classified as `unfinished_refactor`** across the context
- **≥1 Pattern B fork** where divergent paths traverse resolvers in this context

**Telemetry alongside thresholds:** v0.2 ships LoC per crate per context (council WARN-6 from solid-architect) so v0.3 can flip to density-based thresholds (per-kloc) without a second extraction pass. Initial absolute counts, calibration in v0.3.

These thresholds are **initial values**, subject to calibration from v0.2 telemetry.

### A3.3 Raid plan doc — emitted by `/operate-module`, not by cfdb

**Architectural correction (council BLOCK-2, clean-arch + solid-architect).** cfdb does NOT emit the raid plan markdown. cfdb's responsibility ends at returning a **structured infection inventory** — JSON or Cypher result rows containing the findings by class, the canonical candidates, and the reachability data. The `/operate-module` consumer skill (§A3.4) reads that structured inventory and formats it into the raid plan document.

This aligns with parent RFC §4 invariant: *"Not opinionated about workflows. Knows nothing about 'raids', '/prescribe', 'RFCs'. Those are consumer-side compositions."*

**cfdb returns (structured, layer-clean):**

```json
{
  "context": "trading",
  "thresholds_crossed": ["canonical_bypass_ge_5", "context_homonym_ge_3"],
  "crates_in_context": ["domain-trading", "ports-trading", "adapters-trading"],
  "findings_by_class": {
    "duplicated_feature": [...],
    "context_homonym": [...],
    "unfinished_refactor": [...],
    "canonical_bypass": [...],
    "unwired": [...]
  },
  "canonical_candidates": [...],
  "reachability_map": {...},
  "loc_per_crate": {...}
}
```

**`/operate-module` formats this into `raid-plan-<context>.md`** using a template owned by the consumer skill (not by cfdb):

```
# Raid plan: <context-name> (bounded context)

**Status:** draft — council required
**Triggered by:** cfdb threshold (<which ones>)
**Date:** <date>
**Context crates:** <list of crates belonging to this context>

## Current infection inventory
- <table of findings, grouped by class — populated from cfdb structured output>

## Canonical candidates
- <concepts where ≥2 impls exist — populated from cfdb `canonical_candidates` keyed by Pattern I queries from parent RFC §3.9, NOT by a divergent scan>

## Portage list (code belonging to this context)
- <items flagged as viable — move to new home within the context>

## Misplaced list (code belonging to a different context — raid target)
- <items where the classifier flagged `context_homonym` — these belong to another context and must be returned>

## Dead list (no reachable entry points)
- <items where `reachability_from_entry = false` AND no tracker attached — delete>

## Proposed new home architecture
- <empty section — council fills in based on user's strategic intent + owning-context declaration>

## Council decisions required
- Q1: Which impl of the homonym is canonical for which context? (Context Mapping decision)
- Q2: Is the proposed RFC for the new home already drafted?
- Q3: Which consumer sessions will execute the portage?

## Rollback plan
- <git tag before surgery, listing of untouched crates>
```

**Language correction (council WARN-3, ddd-specialist):** the template uses **"Dead list"** (no reachable entry points, delete) + **"Misplaced list"** (belongs to a different context, return) instead of the imprecise "Cancer list". These are two different dispositions requiring two different actions.

**Pattern I integration (council WARN-2, ddd-specialist):** the "Canonical candidates" and "Portage list" sections are populated by the 5 Pattern I Cypher queries defined in parent RFC §3.9 (completeness, dangling-drop, hidden-callers, missing-canonical, clean/dirty-mismatch), not by a separate scan. Tying §A3.3 to Pattern I avoids divergent implementations.

**The raid plan is a draft, not an executable spec.** Council turns it into a concrete RFC before any code moves.

### A3.4 Skill wiring — SRP-compliant decomposition

**Architectural correction (council BLOCK-2, solid-architect).** The original spec bundled four responsibilities into `/operate-module` (extract / threshold-check / raid-plan-emit / boy-scout-fallback). Council SRP audit requires splitting these into distinct skills with independent change vectors.

**Decomposition:**

| Skill | Single responsibility | Inputs | Outputs |
|---|---|---|---|
| **cfdb extract** (existing verb) | Produce fact inventory for a bounded context | context name, workspace path | Structured JSON inventory (§A3.3 shape) — no interpretation |
| **`/operate-module <context>`** (new, slimmed) | Decide if surgery is needed + emit raid plan | structured inventory from cfdb | Raid plan markdown (if threshold crossed), OR "below threshold, route to /boy-scout" verdict |
| **`/boy-scout`** (existing) | Routine debt triage, no surgery | below-threshold inventory OR explicit `class ∈ {random_scattering, unwired}` findings | Inline fix commits, no council required |
| **`/sweep-epic --mode=port --raid-plan=<path>`** (existing with variant flag, Q14) | Execute approved portage per raid plan + RFC | approved RFC + raid plan | Portage commits, scar tests, rollback tag |

**`/operate-module` revised spec (two responsibilities — not four):**

1. **Evaluate threshold** against §A3.2 rules on the structured cfdb inventory
2. **Emit raid plan markdown** (§A3.3 template) if threshold crossed, with council-required marker

It does NOT run cfdb (that's a separate skill call that precedes it). It does NOT execute boy-scout fallback (the orchestrator routes to `/boy-scout` when threshold is not crossed). It does NOT execute the portage (that's `/sweep-epic --mode=port`).

**Invocation flow:**

```
/cfdb-scope <context>          → inventory.json
    → if threshold check trips → /operate-module <context> inventory.json → raid-plan.md → COUNCIL → approved RFC → /sweep-epic --mode=port --raid-plan=raid-plan.md
    → else → /boy-scout applies class-appropriate actions inline
```

**Why two skills, not four (council Q13 + SOLID SRP):** the minimum viable split keeps cfdb extraction and boy-scout as their own skills (already existing, independent change reasons) and consolidates the "go/no-go + document" decision into a single `/operate-module` responsibility. Further splitting (`/operate-threshold` + `/operate-plan`) is premature — one reason to change, no empirical evidence yet for further decomposition.

**`/port-epic` is NOT a new skill (council Q14 unanimous):** the portage workflow becomes a variant flag `--mode=port --raid-plan=<path>` on the existing `/sweep-epic` skill. The distinguishing factor (approved architecture target + portage list) is an input protocol difference, not a code path difference. Re-evaluate in v0.3 after empirical evidence.

### A3.5 Council trigger

The protocol explicitly requires human + council approval between raid-plan emission and code execution. This is the counterweight to "systematical eradication but not stupid moves" — the tool identifies candidates, the council decides fate.

**`/operate-module` is not a one-shot surgery tool.** It is a staging tool that produces an artifact (raid plan) that then enters the normal RFC + approval flow.

---

## A4. Session bootstrap

### A4.1 Project CLAUDE.md §12

A new section `§12 Split-brain Eradication Mission` is added to `/path/to/target-workspace/CLAUDE.md`. Contents (draft in companion edit):

- Triple objective recap (horizontal / vertical / taxonomy)
- Current phase of the roadmap (Phase 0–5 from the session roadmap)
- Zero-tolerance target
- Pointer to `.concept-graph/RESCUE-STATUS.md` (live inventory)
- Pointer to `/operate-module` skill (post-v0.2)
- Pointer to this addendum

**Why project CLAUDE.md:** it is auto-loaded at session start for any session working in this repo. No Redis, no memory system, no separate bootstrap hook required. The user has explicitly asked for a no-repeat-per-session mechanism; CLAUDE.md is the existing channel.

### A4.2 Live inventory file

A new file `.concept-graph/RESCUE-STATUS.md` holds a refresh-able inventory:

```
# Rescue Status

**Last refreshed:** <cfdb run timestamp>
**Baseline proof:** `.proofs/baseline-<date>.txt`

## Counts
- HSB candidates (Pattern A): NN
- VSB candidates (Pattern B): NN
- Canonical bypasses (Pattern C): NN
- Total classified findings: NN

## Distribution by class
| Class | Count |
|---|---|
| Duplicated feature | NN |
| Unfinished refactor | NN |
| Random scattering | NN |
| Canonical bypass | NN |
| Unwired | NN |

## Operate-module candidates
(bounded contexts that crossed infection threshold — §A3.2 is context-scoped, not crate-scoped)
- context-name-1: <reason>, crates: [crate-a, crate-b, crate-c]
- context-name-2: <reason>, crates: [crate-d, crate-e]

## Top 5 infected crates (by total finding count)
1. ...

## Active raid plans
- <pointer to each draft raid plan>
```

**Update cadence:** refreshed by the v0.1 CI gate on every merge to develop, plus a weekly cron (RFC §11 "weekly audit cron").

**Format:** markdown for human + session-agent consumption, NOT machine-parsed. A machine-readable JSON sibling (`.concept-graph/RESCUE-STATUS.json`) may ship post-v0.2 if a consumer materializes.

### A4.3 What is NOT in session bootstrap

- **No Redis state** — this is repo-local doctrine, not cross-session memory
- **No CLAUDE.md scars for individual findings** — the status file holds counts, scars go in code as test fixtures
- **No per-session cfdb run** — the tool runs in CI + cron, session agents read the published status

---

## A5. Decisions for council — FIRST PASS RESOLVED

All Q9–Q14 were voted by the first council pass (2026-04-14). Section retained as historical record. Original rationale below, annotated with vote outcomes.

**First-pass vote summary:**

| Q | Topic | Vote | Outcome |
|---|---|---|---|
| Q9 | `ra-ap-hir` schedule | (b) split — UNANIMOUS (clean-arch + rust-systems) + rust-systems architectural reframing | **RESOLVED:** v0.2 ships new parallel `cfdb-hir-extractor` crate; v0.3 switches CI gate |
| Q10 | Taxonomy classes | 6-class (5 + Context Homonym) — ddd-specialist | **RESOLVED:** 6 classes, §A2.1 updated |
| Q11 | Threshold metric | Absolute + LoC telemetry — solid-architect | **RESOLVED:** absolute for v0.2, density instrumentation in v0.2 for v0.3 flip |
| Q13 | `/operate-module` new vs extend | (a) new, conditional on SRP split — solid-architect | **RESOLVED:** new skill after SRP decomposition (§A3.4) |
| Q14 | `/port-epic` new skill vs variant | Variant flag — UNANIMOUS (clean-arch + solid-architect) | **RESOLVED:** `--mode=port --raid-plan=<path>` on existing `/sweep-epic` |

**Residual question from first pass (not voted, deferred to second pass or v0.2 telemetry):**

- **Q12 (RESCUE-STATUS.md live file vs generated)** — companion deliverable scope, not blocking the RFC. Defer to session-bootstrap review in second pass.

---

*Original council question rationale below (for historical traceability).*



### Q9. `ra-ap-hir` adoption — v0.2 or split into v0.2 prepare / v0.3 ship?

*(Rust systems lens, clean-architect lens)*

Cost per A1.2: +30–60s clean build, +2–4 GB memory. Benefit: unlocks Patterns B, C, G, H, I. Risk: compile cost on every CI run of the extractor.

Options:
- (a) **v0.2 ships `ra-ap-hir` + all call-graph patterns in one release** — biggest jump, largest risk
- (b) **v0.2 ships `ra-ap-hir` extractor only (+ `:EntryPoint` catalog); v0.3 ships Pattern B/C cypher rules** — de-risks by splitting the load-bearing dependency from the rule library
- (c) **v0.2 skips `ra-ap-hir`, delivers `:EntryPoint` via heuristic + syn, v0.3 upgrades to ra-ap-hir** — lowest risk, but delivers Pattern B with the ~40–60% recall ceiling, which is below the v0.2-4 gate item's 80% target

**Recommendation to council:** (b). Rationale: the call-graph extractor is where the real risk lives (memory, compile time, hir stability). Ship the extractor first, prove it stable for a release, then layer rules on top.

### Q10. Five classes — right cut or different taxonomy?

*(Clean-arch lens, DDD lens, product/CPO lens)*

The five classes in A2.1 map 1:1 to fix strategies. Alternatives:

- **3-class version** (duplicated / refactor / other) — simpler, but loses the canonical-bypass and unwired distinctions that map to different skills
- **7-class version** (add "test-only double that leaked into prod" and "ADR reversal not propagated") — more precise, but two of the classes cover <5% of historical findings
- **DDD-lens version** (aggregate boundary violation / ubiquitous-language drift / context-mapping inconsistency) — more conceptually clean, but harder to detect mechanically

**Recommendation to council:** 5-class as proposed. Simplicity + empirical distribution coverage + 1:1 skill mapping.

### Q11. Infection threshold — absolute counts or per-kloc density?

*(Solid-architect lens, rust-systems lens)*

A3.2 uses absolute counts. Alternative: per-kloc density (5 bypasses per 1k LoC). Absolute counts are simpler to reason about and interpret. Per-kloc density adjusts for crate size, so a small crate with 3 bypasses is flagged while a large crate with 20 bypasses (but proportionally fewer) is not.

**Recommendation to council:** start with absolute counts (A3.2), instrument density telemetry in v0.2, flip to density in v0.3 if the absolute-count version produces false alarms on small crates.

### Q12. RESCUE-STATUS.md — live file committed to repo, or generated artifact in CI output?

*(QA lens, product lens)*

Committing generated content to the repo is usually an anti-pattern — it creates diff noise on every refresh. BUT: session agents need to *read* this file at session start without running the tool, which means it must exist in the working copy.

- (a) **Committed to repo, refreshed by a CI job that commits back** — always available, generates diff noise
- (b) **Not committed, refreshed at session start via hook** — no noise, but adds tool dependency to every session and breaks offline work
- (c) **Committed once as scaffold, refreshed by CI into a sibling `.generated` copy, scaffold pointer updated occasionally** — compromise

**Recommendation to council:** (a). The diff noise is acceptable because the refresh is low-frequency (weekly + merge-triggered) and the content is load-bearing for session bootstrapping.

### Q13. `/operate-module` — new skill, or extend an existing skill?

*(Solid-architect lens — SRP)*

Options:
- (a) **New skill `/operate-module`** — clean SRP, clear trigger, clear output
- (b) **Extend `/audit-split-brain`** with a threshold mode — reuses existing skill, but `/audit-split-brain` is currently read-only while `/operate-module` produces a planning artifact
- (c) **Extend `/quality-architecture`** — same concern as (b)

**Recommendation to council:** (a). The "produce a planning artifact that triggers council" workflow is novel enough to deserve its own skill. Reusing audit skills for planning violates SRP.

### Q14. Should `/port-epic` be a new skill or a variant of `/sweep-epic`?

*(Clean-arch lens)*

`/sweep-epic` handles mechanical refactors in parallel. `/port-epic` would handle "move code carefully from cancer site to clean new home per an RFC". Overlap: both parallelize mechanical changes. Difference: `/port-epic` has an approved architecture target and a portage list from a raid plan; `/sweep-epic` has a pattern to apply and a list of sites.

**Recommendation to council:** start as a variant flag on `/sweep-epic` (`--mode=port --raid-plan=...`), promote to standalone skill if the variant accumulates ≥3 unique responsibilities.

---

## A6. Risks & known unknowns

- **`ra-ap-hir` weekly maintenance tax** (council B2, upgraded from risk to acknowledged category concern) — weekly releases (9 in 9 weeks verified) require 10+ exact-pinned Cargo.toml version updates per upgrade. Mitigation: dedicated `chore/ra-ap-upgrade-<version>` branch protocol, ~4 breaking changes per year expected, runbook at `.concept-graph/cfdb/docs/ra-ap-upgrade-protocol.md`. This is a budgeted cost, not a risk we hope to avoid.
- **`ra-ap-hir` API stability** — ~4 breaking changes per year historically. Mitigation: `cfdb-hir-extractor` isolated in its own crate; the `syn`-based `cfdb-extractor` continues to function during hir outages. Pin to last-known-good, upgrade deliberately.
- **`HirDatabase` object-safety constraint** (council risk 2, rust-systems) — not trait-object-safe, must be used as a monomorphic concrete type. cfdb-hir-extractor honors this; no `dyn HirDatabase` abstraction.
- **Classifier false positives** (council WARN-5, ddd) — the age-delta + keyword-match signals for "unfinished refactoring" could mislabel a recent bug fix as a refactor. Compounded risk: keyword-match not scoped to owning bounded context will fire "unfinished refactoring" on homonym cases. Mitigation: (a) ship confidence scores, (b) require human review above threshold, (c) scope RFC keyword match to owning bounded context via `.cfdb/concepts/*.toml`, (d) instrument misclassification telemetry.
- **Bounded context identification heuristic fails** — the crate-prefix convention works for `trading`, `portfolio`, `strategy`, `collection`, but fails for cross-cutting concerns like `messenger`, `sizer`, `executor`. Mitigation: explicit override in `.cfdb/concepts/*.toml` is load-bearing for these cases; v0.2 must ship the override mechanism, not only the heuristic.
- **Threshold calibration** — the §A3.2 numbers are initial guesses. Mitigation: v0.2 ships LoC telemetry alongside thresholds so v0.3 can flip to density-based without re-extraction.
- **Raid plan staleness** — a raid plan emitted on date D may not match repo state on date D+14 if other work lands. Mitigation: raid plans expire after N days, `/operate-module` must be re-run to refresh.
- **Skill seam between `duplicated_feature` and `canonical_bypass`** (council WARN-1, solid) — both route to `/sweep-epic` in the initial routing table but have different fix procedures (canonical-selection vs pure rewire). Known seam, acknowledged, promote to separate skill when canonical-selection workflow is enriched.
- **CI memory budget** — `cfdb extract` on the target workspace with HirDatabase loaded consumes 2–4 GB peak RSS (plausible, unverified). If the CI runner cannot accommodate, the extract either streams or extracts per-context. Mitigation: v0.2-5b measures this before committing.

---

## A7. Out of scope for this addendum

- LLM enrichment of findings (v0.3+)
- Cross-project classifier (multi-project v0.3+)
- Embedding-based concept clustering (v0.4)
- IDE integration / live hints
- Auto-fix (cfdb stays read-only; `/operate-module` produces plans, never edits)

---

## A8. Appendix — companion deliverables

This addendum is paired with:

1. **`<consuming-project>/CLAUDE.md` §12 edit** — project doctrine section, drafted in companion edit
2. **`.concept-graph/RESCUE-STATUS.md` scaffold** — empty-state live inventory file
3. **Issue drafts:**
   - New child of EPIC #3622: "Promote `hsb-by-name.cypher` to v0.1 gate with enriched collect()"
   - Standalone: "Capture true-count baseline post forged-file deletion"
   - EPIC #3622 body update: "Add Phase D — v0.2 vertical + taxonomy (blocked on this addendum)"
   - EPIC #3519 body update: "Cross-reference taxonomy classifier per this addendum §A2"

All companion deliverables are DRAFT pending council approval of this addendum.
