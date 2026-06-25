# RFC-046 Council — Validation + Quick-Wins Mandate

Convened as an agent-team council (4 deliberating teammates sharing task list + mailbox, per global CLAUDE.md §2b) to (1) **validate** the hardened RFC-046 and (2) issue a **quick-wins mandate** for the current cfdb codebase. Teammates cross-challenged over the mailbox; nothing contested shipped into the execute tier, and two findings were *corrected by deliberation* (the f64 tie-in refuted; the split-brain scope narrowed).

## 1. Validation — RFC-046: **VALIDATED 4/4**

| Lens | Verdict | Re-verified resolution of its blocker |
|---|---|---|
| clean-arch | VALIDATED | `&Path` no longer in any cfdb-core port; Option C inherent `ingest_trace` + `with_trace_file`. Zero core blast-radius. |
| ddd | VALIDATED | `resolver="runtime"` homonym removed; `OBSERVED_CALLS` label + `profiler` prop carry observed-vs-declared. `:Trace`≠`:EntryPoint`. |
| solid | VALIDATED | No new cfdb-core trait (Zone-of-Pain avoided); `TraceFormat`+`FormatParser` `#[non_exhaustive]` in cfdb-petgraph (OCP). |
| rust-systems | VALIDATED | Re-verified: canonical_dump IS the 3-tuple (I3 accurate); `dispatch_enrich` payload-free; sha256 (I8). |

RFC-046 is council-validated and implementation-ready.

## 2. Quick-Wins Mandate

Honest framing: **cfdb's hygiene is high.** clean-arch and rust-systems each opened with a *positive null result*; the slate is correctly thin. The wins came from solid + ddd, peer-verified.

### Tier 1 — execute now (mechanical sweep; ≤~2h each, byte-identical + recall-safe). rust-systems' consolidated ranking, all-lens concurred:
1. **DRY `collect_where_hints`** — `crates/cfdb-petgraph/src/index/lookup.rs:194-226`. Three byte-identical indexability-guard+push blocks → one `push_if_indexed` closure. Drops the single highest-complexity prod fn (cognitive 16→~10, rust-systems re-ran `ra_query_complexity`; borrow-check pinned — FnMut reborrow of `out`, shared `indexed_pairs`/`label`, resolvers return owned tuples). Hot path, inlines. Slice-5/6/6b fixtures cover it. **[solid · 2nd: rustsys] — strongest.**
2. **Split git-cache out of `extract.rs`** — `crates/cfdb-cli/src/commands/extract.rs` (490L/21fns). The `url@sha` clone+cache cluster (`clone_and_checkout`/`prepare_cache_dir`/`parse_url_at_sha`/`cache_dir_for`/`url_hash_hex16` + `GitWorktree` struct/impl/Drop :404-467) shares zero private state with extract orchestration — CCP/SRP. Lift to `commands/extract/git_cache.rs`. **Caveat (rustsys):** 6 are `pub fn` and `commands/tests.rs:6` imports `super::extract::{…}` → needs `pub use git_cache::*` (or test-import fix) or the test mod won't compile. NOT the #151 move-only split (that was extract-proper; this axis was never separated). **[solid QW-cross-1, arch+rustsys verified] — S-M.**
3. **Fix `:Item.kind` SchemaDescribe doc** — `crates/cfdb-core/src/schema/describe/nodes/structural.rs:112`. Replace the value-list with the **8 lowercase wire values** `struct, enum, fn, const, type_alias, impl_block, trait, static` (match `ItemKind::to_extractor_str`, the producer, the query corpus, the sibling `:CallSite.kind` doc). Current text uses Capitalized council names, invents `Impl` (real `impl_block`), omits `static` — a published-contract lie. ~15min, no test asserts the string. **Do NOT** switch to the Capitalized `as_str` names, and **do NOT** generate-from-`variants()` here (`variants()` itself omits `static` — that's the RFC-gated backlog item). **[ddd · 2nd: rustsys] — HIGH value.**
4. **Flatten `visit_item_struct` nest** — `crates/cfdb-extractor/src/item_visitor/visits.rs:235-254`. 4-deep nest → let-else/filter_map (nesting 4→2). serde_default fixtures cover it. **[solid · 2nd: rustsys].**

Path: one `/fix-mechanical` sweep (existing suite passes byte-identically = the invariant; each `git mv`+import-fix as one atomic commit so `ci/determinism-check.sh`'s git-clean assertion holds).

### Tier 2 — boy-scout rider (not a QW, not 046-D)
- `crates/cfdb-core/src/fact.rs:67` `as_f64().unwrap_or(0.0)` → `None => Null` (per the fn's own contract). The branch is **provably dead** (the only non-test caller of `from_json` is `query.rs:101`, the `--params` query-input path; serde_json w/o arbitrary_precision → `as_f64` is total). rust-systems **refuted** solid's 046-D tie-in: 046-D float props are built directly via `PropValue::Float(x)`, never through this seam — so it carries no I4 risk. Three-lens convergence: a correct-but-dead one-liner, best as a **rider on the next `fact.rs` touch** (rides the build_item_props issue below). Not filed standalone.

### Tier 3 — file as issues (real findings, bigger than a quick win)
- **`:Item` prop-key drift class (build_item_props)** — `cfdb_core::fact::build_item_props` (fact.rs:115) owns `{qname,name,kind,crate,bounded_context}`, but ≥4 emitters hand-roll inserts and diverged. **Corrected scope (solid+ddd, verified):** the genuinely-shared overlap is only the `{qname,name,kind,crate}` 4-subset — TS (`cfdb-extractor-ts/src/emit.rs:239-251`) omits `bounded_context` and layers `ts_construct`; PHP layers `php_construct`; Rust adds `module_qpath`/`impl_target`. Fix = factor the common 4-subset into one helper, each emitter **layers** its language-specific keys (a blind "route everything through build_item_props" would strip `ts/php_construct` or force `bounded_context` onto TS). Per-emitter byte-identical + recall verification → M/L. **[unanimous: issue not QW].**
- **`ItemKind` missing `static` → CLI `--kind static` filter gap** — extractor emits 8 kinds, `item_kind.rs:16` enum has 7; `cfdb list-items --kind static` → `UnknownItemKind` (`main_parse.rs:17` parses `--kind` through the 7-variant enum). NOT corruption (no prod path parses emitted kinds back) — a user-visible filter-capability gap. Adding `ItemKind::Static` is a behavior change → **RFC-gated** (ratified vocab; needs a Tests block), and carries the generate-describe-from-`variants()` DRY follow-up (kills the 3-list drift class at root). **[ddd+rustsys].**
- **`Param` → `ParamBinding` rename** — `specs/concepts/cfdb-core.md:130-132` documents a ratified homonym (RFC-036 §3.1) with a promised-but-undone rename (~45 refs / 10+ files). → `/fix-mechanical` epic. **[ddd].**

### Dropped (positive null result)
- `eval/util.rs`→`projection_helpers.rs` & `scope/helpers.rs` renames (arch, withdrawn). ddd's catch: `util` is a *truthful* name for a 4-concern grab-bag — `projection_helpers` would be a **false specialization**, worse than honest-vague; the only correct move is a *split* (M-effort, not worth it).

## 3. Deliberation record
The strongest win was independently complexity- and borrow-verified by a second lens; a new win (git-cache split) emerged *from* a cross-challenge and reversed its own author's earlier dismissal; the renames were objected-down and withdrawn with a sharper reason than their proposer gave; the f64 tie-in was asserted, then refuted at the source, landing as a rider; the split-brain scope was narrowed before filing. Convergence, not consensus-by-fiat.
