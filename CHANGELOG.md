# Changelog

All notable changes to cfdb will be documented in this file.

## [0.6.0] - 2026-07-15

### 🚀 Features

- *(cfdb [#462](https://github.com/yannickgranger/cfdb/issues/462))* RFC-045 45-A — PHP IMPLEMENTS edges + resolver attr + php_construct doc
- *(cfdb [#465](https://github.com/yannickgranger/cfdb/issues/465))* RFC-045 45-C — PHP :CallSite + INVOKES_AT + resolved CALLS
- *(cfdb [#463](https://github.com/yannickgranger/cfdb/issues/463))* RFC-045 45-B — TS IMPLEMENTS edges (+ ts_construct doc)
- *(cfdb [#464](https://github.com/yannickgranger/cfdb/issues/464))* RFC-045 45-D0 — TS method-level :Item (prereq for 45-D)
- *(cfdb [#466](https://github.com/yannickgranger/cfdb/issues/466))* RFC-045 45-D — TS :CallSite + INVOKES_AT (zero CALLS)
- *(cfdb [#488](https://github.com/yannickgranger/cfdb/issues/488))* RFC-047a 47-0 — var-length reverse-reachability query mechanics
- *(cfdb [#489](https://github.com/yannickgranger/cfdb/issues/489))* RFC-047a 47-A — canonical reverse-reachability query + HIR dogfood
- *(cfdb [#490](https://github.com/yannickgranger/cfdb/issues/490))* RFC-047 47-B — `cfdb impact` CLI verb (--item/--since/--max-depth)
- *(cfdb [#498](https://github.com/yannickgranger/cfdb/issues/498))* RFC-050 50-A — extract-time :Crate.crate_tier (schema bump)
- *(cfdb)* RFC-050 50-C up-call layering-violation query
- *(cfdb)* RFC-049 49-0 — FrameworkDetector registry seam (recall-neutral)
- *(cfdb)* RFC-048 48-A — per-phase profiling of `cfdb extract`
- *(cfdb)* RFC-053 53-A — :MatchSite + MATCHES_AT end-to-end (V0_7_0 bump)
- *(cfdb)* RFC-049 49-A — register clap detector behind the manifest gate ([#494](https://github.com/yannickgranger/cfdb/issues/494))
- *(cfdb)* RFC-049 49-B — register axum/actix detector behind the manifest gate ([#495](https://github.com/yannickgranger/cfdb/issues/495))
- *(cfdb)* RFC-053 53-B — MATCHES_ON resolution pass
- *(cfdb)* RFC-053 53-C — split-resolution fence templates + syn::Visibility guard

### 🐛 Bug Fixes

- *(ci [#446](https://github.com/yannickgranger/cfdb/issues/446))* Cross-bump opens bump PRs again — token PR/issue write + heal silent skip
- *(ci [#453](https://github.com/yannickgranger/cfdb/issues/453))* Route TMPDIR to disk-backed /cache — /tmp tmpfs OOMs at link time
- *(test [#457](https://github.com/yannickgranger/cfdb/issues/457))* Serialize cfdb-recall tests under nextest (rustdoc-json race)
- *(ci [#459](https://github.com/yannickgranger/cfdb/issues/459))* Recall-nightly — POSIX crate list + drop node-less artifact upload
- *(extract)* Emit an empty graph for a no-recognized-language workspace ([#474](https://github.com/yannickgranger/cfdb/issues/474))
- *(boy-scout [#488](https://github.com/yannickgranger/cfdb/issues/488))* Clear 4 pre-existing quality-metrics violations
- *(cfdb [#486](https://github.com/yannickgranger/cfdb/issues/486))* Enforce G6 G1-exclusion of test_coverage in canonical dump
- *(boy-scout [#513](https://github.com/yannickgranger/cfdb/issues/513))* Recall KEPT_ITEM_KINDS oracle + correspondence guard; flip 3 released RFC markers
- *(boy-scout [#513](https://github.com/yannickgranger/cfdb/issues/513))* Duplicate-safe length asserts on the KEPT_ITEM_KINDS correspondence guard
- *(fact [#478](https://github.com/yannickgranger/cfdb/issues/478))* From_json non-finite Number => Null per contract, not fabricated 0.0
- *(cfdb [#499](https://github.com/yannickgranger/cfdb/issues/499))* Bind tier comparison to call direction + note callee-stub false-clean
- *(cfdb)* RFC-048 48-A — keep the profiling clock out of the extractor
- *(ci [#529](https://github.com/yannickgranger/cfdb/issues/529))* Keep the 26-min impact_hir_dogfood test off the PR hot path
- *(cfdb [#494](https://github.com/yannickgranger/cfdb/issues/494))* Judge containment between like path representations only
- *(ci)* Dogfood-determinism workdir survives runner tmp cleanup; missing capture is infra error, not nondeterminism
- *(cfdb-core [#481](https://github.com/yannickgranger/cfdb/issues/481))* Describe :Item.kind with lowercase wire values
- *(cfdb [#517](https://github.com/yannickgranger/cfdb/issues/517))* Key HIR qname crate segment off the package name, not the bin target name

### 🚜 Refactor

- *(cfdb-hir-extractor [#455](https://github.com/yannickgranger/cfdb/issues/455))* Flatten walk_file (cognitive 58→<10)
- *(cfdb [#467](https://github.com/yannickgranger/cfdb/issues/467))* Split nodes.rs + call_site_emitter.rs below 500-line gate
- *(cfdb-core [#480](https://github.com/yannickgranger/cfdb/issues/480))* Rename query::ast::Param -> ParamBinding
- *(fact [#478](https://github.com/yannickgranger/cfdb/issues/478))* Factor :Item {qname,name,kind,crate} 4-subset into build_item_props_common

### 📚 Documentation

- *(rfc)* RFC-043 — :CallSite argument facts for receiver-type fences
- *(rfc)* RFC-045 — polyglot relationship edges (PHP/TS IMPLEMENTS + :CallSite/CALLS)
- *(cfdb)* Draft RFC-047..052 — capabilities borrowed from Understand-Anything
- *(cfdb)* Reframe RFC-048 as profile-first — discovery shows parsing isn't the bottleneck
- *(cfdb [#477](https://github.com/yannickgranger/cfdb/issues/477))* RFC-046 runtime execution-trace ingest — council-validated
- *(cfdb)* Council-ratify RFC-047/049/050 + 48-A; harden §5; park 051/052
- *(cfdb)* RFC-047a — impact query mechanics complement (council-ratified)
- *(cfdb)* Draft RFC-053 — :MatchSite + MATCHES_ON enum-dispatch facts for split-resolution-point fences
- *(cfdb)* RFC-053 R2 — apply unanimous R1 council REQUEST CHANGES
- *(cfdb)* RFC-053 R2.1 — fold in council forward-guidance on the matches!() non-goal
- *(cfdb)* RFC-053 RATIFIED — R2 council 4/4 + council/RFC-053/RATIFIED.md
- *(cfdb)* RFC-053 post-ratify editorial — inline-test route avoids pub(crate) widening (R2 solid refinement)
- *(cfdb [#480](https://github.com/yannickgranger/cfdb/issues/480))* Reconcile query-dsl.md living doc to ParamBinding

### ⚡ Performance

- *(cfdb)* Index RandomScattering fork join via ConversionPrefix computed key ([#534](https://github.com/yannickgranger/cfdb/issues/534))
- *(ci [#448](https://github.com/yannickgranger/cfdb/issues/448))* Run tests via cargo-nextest (parallel across binaries)
- *(test [#451](https://github.com/yannickgranger/cfdb/issues/451))* Share one cached fixture extract across self-dogfood tests

### 🧪 Testing

- *(cfdb-core [#481](https://github.com/yannickgranger/cfdb/issues/481))* Red — :Item.kind descriptor must list lowercase wire values
- *(cfdb [#517](https://github.com/yannickgranger/cfdb/issues/517))* RED — HIR cli_command EXPOSES/CALLS dangle when [[bin]] name ≠ package name

### ⚙️ Miscellaneous Tasks

- Weekly cross-fixture bump → f66ff89d6dc5
- Move HIR-resolved self-audit (extract --hir + edge-liveness) to nightly
## [0.5.0] - 2026-05-22

### 🚀 Features

- *(cfdb-query)* Support standard escape sequences in string literals
- *(cfdb-core)* Add ContextSource enum + :Context.source schema attr ([#300](https://github.com/yannickgranger/cfdb/issues/300))
- *(cfdb-core)* Add parse_or_default reader helper for :Context.source ([#303](https://github.com/yannickgranger/cfdb/issues/303))
- *(cfdb-concepts)* Compute_bounded_context returns BoundedContext ([#301](https://github.com/yannickgranger/cfdb/issues/301))
- *(cfdb-extractor)* Emit :Context.source via per-context accumulator ([#302](https://github.com/yannickgranger/cfdb/issues/302))
- *(cfdb-core)* :ConstTable + HAS_CONST_TABLE schema declaration (RFC-040 1/5)
- *(cfdb-extractor)* Const_table pure recognizer + ElementType (RFC-040 2/5)
- *(cfdb-extractor)* Emit :ConstTable + HAS_CONST_TABLE (RFC-040 3/5)
- *(examples/queries)* Const-table-overlap.cypher detector — DUPLICATE branch (RFC-040 4/5)
- *([#334](https://github.com/yannickgranger/cfdb/issues/334))* --require-fresh flag refuses from_ref == to_ref
- *([#335](https://github.com/yannickgranger/cfdb/issues/335))* `check-prelude-triggers all` consolidator subcommand
- *([#342](https://github.com/yannickgranger/cfdb/issues/342))* Tools/dogfood-enrich harness scaffolding
- *(cfdb-petgraph)* Entries_subset / entries_jaccard / overlap_verdict UDFs + SUBSET/INTERSECTION_HIGH branches ([#332](https://github.com/yannickgranger/cfdb/issues/332))
- *([#344](https://github.com/yannickgranger/cfdb/issues/344))* Self-enrich-rfc-docs cypher template + grep_rfc_docs helper
- *([#346](https://github.com/yannickgranger/cfdb/issues/346))* Self-enrich-concepts cypher template + scan_concepts helper
- *([#345](https://github.com/yannickgranger/cfdb/issues/345))* Self-enrich-bounded-context via Path B ([#355](https://github.com/yannickgranger/cfdb/issues/355)) — fold into Phase B bundle
- *([#340](https://github.com/yannickgranger/cfdb/issues/340))* Nightly cfdb-recall ratios as Gitea commit status (Phase C)
- *([#347](https://github.com/yannickgranger/cfdb/issues/347))* Self-enrich-reachability cypher template
- *([#348](https://github.com/yannickgranger/cfdb/issues/348))* Self-enrich-metrics cypher template
- *([#349](https://github.com/yannickgranger/cfdb/issues/349))* Self-enrich-git-history cypher template
- *([#297](https://github.com/yannickgranger/cfdb/issues/297) Phase B)* Self-enrich vertical-split-brain `drop` kind
- *([#263](https://github.com/yannickgranger/cfdb/issues/263))* RFC-041 Phase 1 bundle — LanguageProducer trait + RustProducer + dispatcher + slim build
- *([#264](https://github.com/yannickgranger/cfdb/issues/264))* PHP LanguageProducer MVP — tree-sitter-php walker + fixture + 5 tests
- *([#265](https://github.com/yannickgranger/cfdb/issues/265))* TypeScript LanguageProducer MVP — tree-sitter-typescript walker + fixture + 5 tests
- *([#264](https://github.com/yannickgranger/cfdb/issues/264) + [#265](https://github.com/yannickgranger/cfdb/issues/265))* PHP + TypeScript LanguageProducer MVPs (RFC-041 Phase 2 + Phase 3)
- *(cfdb-core)* :Literal label + SchemaVersion v0.4.0 minor bump ([#369](https://github.com/yannickgranger/cfdb/issues/369))
- *(cfdb-extractor)* Emit :Literal nodes per string literal (closes [#370](https://github.com/yannickgranger/cfdb/issues/370))
- *(cfdb-hir-extractor)* Resolve ast::CallExpr path-calls via Semantics::resolve_path (closes [#387](https://github.com/yannickgranger/cfdb/issues/387))
- *(cfdb-hir-extractor [#391](https://github.com/yannickgranger/cfdb/issues/391))* :EntryPoint{kind=test|bench} emission
- *(cfdb-petgraph [#392](https://github.com/yannickgranger/cfdb/issues/392))* :Item.reachable_from_production_entry dual-BFS + cfdb scope --production-only
- *(cfdb-petgraph)* Serde_default callee post-pass marks reachable_from_entry (closes [#396](https://github.com/yannickgranger/cfdb/issues/396))
- *(cfdb-hir-extractor)* Enable proc-macro server (RFC-043 / 043-A)
- *(cfdb-core [#421](https://github.com/yannickgranger/cfdb/issues/421))* #[non_exhaustive] on 9 schema enums + downstream gate
- *(cfdb [#422](https://github.com/yannickgranger/cfdb/issues/422))* 044-C single-site discipline (dep-rules TOML + composition-root + slim-cli guards)
- *(cfdb [#423](https://github.com/yannickgranger/cfdb/issues/423))* 044-E determinism propagation (5 sibling crates + 2 cfdb-petgraph fixes)
- *(cfdb-core [#424](https://github.com/yannickgranger/cfdb/issues/424))* 044-A schema vocabulary completeness (specs + version lockstep + narrative freeze)
- *(cfdb-cli [#426](https://github.com/yannickgranger/cfdb/issues/426))* 044-F centralized CLI exit-code contract
- *(cfdb [#427](https://github.com/yannickgranger/cfdb/issues/427))* 044-H frozen RFC §4 invariant catalog (6 arch-ban-rfc-*.cypher rules)
- *(cfdb [#425](https://github.com/yannickgranger/cfdb/issues/425))* 044-D qname stability (cross-extractor parity fixture + production item: migration)
- *(cfdb [#428](https://github.com/yannickgranger/cfdb/issues/428))* 044-B integration-seam signature pinning (frozen tests/signatures.toml per crate)

### 🐛 Bug Fixes

- *(cfdb-query)* Replace unwrapped() with try_map for u32 parses — prevent overflow panic
- *(cfdb-cli)* Add -- separator to git clone/fetch/worktree-add for user URLs
- *(cfdb-extractor)* Emit IN_MODULE edges from :Item and :File to :Module
- *(cfdb-query)* Wire recursive predicate through subquery_parser
- *(cfdb-cli)* Separate exit code 30 (rule hits) from 1 (runtime error)
- *(cfdb-extractor)* Emit real source-line numbers, not 0 ([#273](https://github.com/yannickgranger/cfdb/issues/273))
- *(cfdb-hir-extractor)* Emit real source-line numbers on :CallSite
- *(cfdb-extractor)* Synthesize :Item for referenced-but-not-walked qnames
- *(cfdb-core)* Tag :Concept-[EQUIVALENT_TO]->:Concept as Provenance::Reserved
- *([#342](https://github.com/yannickgranger/cfdb/issues/342))* Address verify-issue AC gaps — cfdb-core dep + 7 thresholds
- *([#342](https://github.com/yannickgranger/cfdb/issues/342))* Assign dogfood-enrich crate to cfdb bounded context
- *(ci, dogfood-enrich)* Jq install + per-pass --workspace forwarding ([#343](https://github.com/yannickgranger/cfdb/issues/343))
- *(cfdb-hir-petgraph-adapter)* Synthesize stub :Item for unknown CALLS dsts (closes [#388](https://github.com/yannickgranger/cfdb/issues/388))
- *(boy-scout [#374](https://github.com/yannickgranger/cfdb/issues/374))* Extract loop body to hoist 3 clones out of synthesize.rs
- *(boy-scout [#374](https://github.com/yannickgranger/cfdb/issues/374))* Split synthesize.rs tests to sibling file
- *(cfdb-petgraph [#400](https://github.com/yannickgranger/cfdb/issues/400))* Split graph.rs inline tests to sibling file (slice 6.6)
- *([#396](https://github.com/yannickgranger/cfdb/issues/396))* Drop duplicate #![cfg(test)] from sibling test files
- *(examples/queries)* Restrict VSB rules to production :EntryPoint kinds
- *(indexes)* Add Item.dup_cluster_id — closes hsb-cluster smoke 8min regression
- *(cfdb-cli)* Auto-discover .cfdb/indexes.toml from db path (closes [#409](https://github.com/yannickgranger/cfdb/issues/409) for real)
- *(fmt [#421](https://github.com/yannickgranger/cfdb/issues/421))* Rustfmt on with_clause.rs after sentinel helper extraction
- *(boy-scout [#421](https://github.com/yannickgranger/cfdb/issues/421))* Extract canonical build_item_props to cfdb-core

### 🚜 Refactor

- *(cfdb-extractor)* Route impl-method emission through emit_item_with_flags
- *(cfdb-petgraph)* Extract require_keyspace + require_workspace helpers
- *(cfdb-cli)* Extract compose::list_keyspace_names helper
- *(cfdb-cli)* Collapse enrich-verb dispatch to single match
- *(cfdb-extractor)* Extract insert_attr_metadata_props helper
- *(cfdb-cli)* Extract output::emit_json helper
- *(cfdb-extractor)* Extract emit_call_site_node_and_edge helper
- *(cfdb-cli)* Extract compose::ensure_keyspace_exists
- *(cfdb-cli)* Unify --format flag under OutputFormat enum
- *(cfdb-extractor)* Direct syn::Visibility -> Visibility mapping
- *([#342](https://github.com/yannickgranger/cfdb/issues/342))* Simplify pass — error variants, dead-code removal
- *(cfdb-extractor)* Split const_table.rs (724 LOC) and item_visitor/emit.rs (694 LOC) ([#350](https://github.com/yannickgranger/cfdb/issues/350))
- *(cfdb-hir-extractor [#391](https://github.com/yannickgranger/cfdb/issues/391))* Extract classify_fn_entry_point to lower scan_file complexity
- *(cfdb-recall [#394](https://github.com/yannickgranger/cfdb/issues/394))* Extract 4 helpers from main() to drop complexity 18 → <15
- *(cfdb-core [#400](https://github.com/yannickgranger/cfdb/issues/400))* Split schema/labels.rs tests to sibling file
- *(cfdb-extractor-ts [#400](https://github.com/yannickgranger/cfdb/issues/400))* Extract AST emission to emit.rs
- *(cfdb-extractor [#400](https://github.com/yannickgranger/cfdb/issues/400))* Split lib.rs tests to sibling file
- *(cfdb-petgraph [#400](https://github.com/yannickgranger/cfdb/issues/400))* Split git_history.rs tests to sibling file
- *(cfdb-petgraph [#400](https://github.com/yannickgranger/cfdb/issues/400))* Split predicate.rs into udf + tests siblings

### 📚 Documentation

- *(cfdb-petgraph)* Remove rayon-parallelism claim from enrich_metrics
- *(specs)* Add OutputFormat concept spec ([#273](https://github.com/yannickgranger/cfdb/issues/273) Pattern 1 [#4](https://github.com/yannickgranger/cfdb/issues/4))
- Draft RFC-038 — :Context.source discriminator (R1 pending)
- RFC-038 R2 — address B1 (cfdb-concepts→cfdb-core dep arc) + B2 (as_wire_str return type)
- RFC-038 ratified — 4/4 RATIFY at R2
- RFC-038 — fill issue numbers in landing trail ([#300](https://github.com/yannickgranger/cfdb/issues/300)/[#301](https://github.com/yannickgranger/cfdb/issues/301)/[#302](https://github.com/yannickgranger/cfdb/issues/302)/[#303](https://github.com/yannickgranger/cfdb/issues/303))
- RFC-039 — foreign-item stubs for cross-workspace edge endpoints (ratified, 4/4)
- Withdraw RFC-039 — wrong framing (foreign items are dependency surface, not stubs)
- RFC-040 ratified — :ConstTable + const-table-overlap detector (4/4 R2)
- RFC-039 ratified — dogfood 7 enrichment passes (4/4 RATIFY at R2)
- *(specs/tools)* Dogfood-enrich.md — pub type spec entries ([#342](https://github.com/yannickgranger/cfdb/issues/342))
- RFC-041 — pluggable LanguageProducer trait (Phase 1 of META [#266](https://github.com/yannickgranger/cfdb/issues/266))
- *(rfc)* RFC-041 :Literal fact type — RATIFIED 4/4
- *(rfc-042)* R1 council incorporated — 8 EDITs applied
- RFC-043 — enable proc-macro server in cfdb-hir-extractor (RATIFIED 4/4)
- RFC-044 — broaden graph-specs coverage of cfdb's critical contracts (RATIFIED 4/4)

### ⚡ Performance

- *(scope)* Slice-6b prop-to-prop cross-MATCH fast path
- *(scope)* Cache compiled regex, order intersects by size, narrow on reachable+is_test
- *(scope)* Precompute index membership; widen perf budgets for CI
- *(cfdb-petgraph [#409](https://github.com/yannickgranger/cfdb/issues/409))* Cache binding-independent candidate sets across cartesian MATCH leaves
- *(cfdb-petgraph)* Zero-alloc signature_divergent — closes signature-divergent.cypher 9min smoke regression

### 🎨 Styling

- Cargo fmt for [#396](https://github.com/yannickgranger/cfdb/issues/396) — line-break normalization
- Cargo fmt for [#396](https://github.com/yannickgranger/cfdb/issues/396) tests — line-break callee_path insert

### 🧪 Testing

- *(cfdb-query)* Scar tests for out-of-scope keyword false positives
- *(cfdb-query)* Expand negative parser corpus
- *(cfdb-extractor)* Synthetic-workspace :Literal correctness gate (closes [#371](https://github.com/yannickgranger/cfdb/issues/371))

### ⚙️ Miscellaneous Tasks

- *(cfdb-core)* Drop unused indexmap dep + correct lying allowlist comment
- *(cfdb-query)* Drop unused regex dep + correct lying allowlist comment
- *([#343](https://github.com/yannickgranger/cfdb/issues/343))* Self-enrich-deprecation dogfood
- *([#339](https://github.com/yannickgranger/cfdb/issues/339))* Smoke-test every shipped .cypher query against cfdb-self
- *([#338](https://github.com/yannickgranger/cfdb/issues/338))* Activate [#344](https://github.com/yannickgranger/cfdb/issues/344) + [#346](https://github.com/yannickgranger/cfdb/issues/346) default-feature dogfoods (Phase B bundle)
- Re-trigger after PR [#354](https://github.com/yannickgranger/cfdb/issues/354) body update for [#240](https://github.com/yannickgranger/cfdb/issues/240) gate
- *([#338](https://github.com/yannickgranger/cfdb/issues/338))* Activate Phase B nightly trio ([#347](https://github.com/yannickgranger/cfdb/issues/347)+[#348](https://github.com/yannickgranger/cfdb/issues/348)+[#349](https://github.com/yannickgranger/cfdb/issues/349)) + fix git-history path bug
- Enable HIR in cfdb-self extract (closes [#381](https://github.com/yannickgranger/cfdb/issues/381))
- Run self-enrich before edge-liveness so enrichment-fed labels populate (closes [#383](https://github.com/yannickgranger/cfdb/issues/383))
- Promote edge-liveness step to blocking (closes [#385](https://github.com/yannickgranger/cfdb/issues/385))
- *(smoke)* Add per-file timing to RFC-030 §3.2 liveness loop
## [0.4.0] - 2026-04-25

### 🚀 Features

- *(cfdb-extractor)* Render_type_inner — unwrap Vec<T>/Option<T>/Result<T,E>/etc. for RETURNS + TYPE_OF precision ([#239](https://github.com/yannickgranger/cfdb/issues/239))
- *(ci)* Backfill + enforce `Closes #N` footer on PRs ([#240](https://github.com/yannickgranger/cfdb/issues/240))

### 🐛 Bug Fixes

- *(cfdb-petgraph)* Bind edge.var in build_path_binding + emit per-edge for single-hop ([#242](https://github.com/yannickgranger/cfdb/issues/242))
- *(boy-scout [#246](https://github.com/yannickgranger/cfdb/issues/246))* Drain 18 clones-in-loops across cfdb-* + split 2 god files

### 🚜 Refactor

- *(cfdb-cli [#248](https://github.com/yannickgranger/cfdb/issues/248))* Extract classify/sorted_jsonl submodule (641 → 372 LoC)
- *(cfdb-cli [#248](https://github.com/yannickgranger/cfdb/issues/248))* Extract main_command/args submodule (514 → 13 LoC)
- *(cfdb-core [#248](https://github.com/yannickgranger/cfdb/issues/248))* Extract qname/node_id submodule (528 → 445 LoC)
- *(cfdb-cli [#248](https://github.com/yannickgranger/cfdb/issues/248))* Split Command::Extract into ExtractArgs (args.rs 515 → 480 LoC)
- *(cfdb-petgraph [#253](https://github.com/yannickgranger/cfdb/issues/253))* Split pattern.rs path-pattern section → pattern/path.rs

### 📚 Documentation

- *(rfc-037)* Phase-shipped closeout + §6 non-goals disposition ([#238](https://github.com/yannickgranger/cfdb/issues/238))
- *(rfc)* Flip status Draft → Implemented on develop (RFC-030/032/035)

### 🧪 Testing

- *([#242](https://github.com/yannickgranger/cfdb/issues/242))* RED — regression tests for named edge-var binding (count(r), r.prop)
- *([#239](https://github.com/yannickgranger/cfdb/issues/239))* Self-dogfood count assertion + self/cross/target dogfood proofs
## [0.3.0] - 2026-04-24

### 🚀 Features

- *(extractor,core)* :Param node + HAS_PARAM edge producer ([#209](https://github.com/yannickgranger/cfdb/issues/209))
- *(rfc-037)* Qname canonical helpers + RETURNS + :Field attrs
- *(rfc-037)* HAS_VARIANT + :Variant producer + emit_field_list ([#218](https://github.com/yannickgranger/cfdb/issues/218))
- *(rfc-037)* REGISTERS_PARAM 3-paths + TYPE_OF producer ([#219](https://github.com/yannickgranger/cfdb/issues/219) + [#220](https://github.com/yannickgranger/cfdb/issues/220))
- *(rfc-037)* Delete SUPERTRAIT + RECEIVES_ARG; SchemaVersion v0.3.0 ([#221](https://github.com/yannickgranger/cfdb/issues/221))
- *(ci)* Edge-liveness informational harness ([#222](https://github.com/yannickgranger/cfdb/issues/222))
- *([#203](https://github.com/yannickgranger/cfdb/issues/203))* SchemaVersion V0_3_0 → V0_3_1 scaffold — enrich_metrics producer landing
- *([#203](https://github.com/yannickgranger/cfdb/issues/203))* Enrich_metrics producer — ast_signals + coverage + clustering
- *([#203](https://github.com/yannickgranger/cfdb/issues/203))* Cfdb-cli feature pass-through + self-dogfood assertion
- *([#203](https://github.com/yannickgranger/cfdb/issues/203))* Cfdb enrich-metrics CLI verb accepts --workspace
- *([#202](https://github.com/yannickgranger/cfdb/issues/202))* VSB multi-resolver detector + scar corpus (RFC-036 §3.2)
- *([#204](https://github.com/yannickgranger/cfdb/issues/204))* HSB multi-signal cluster query (RFC-036 §3.4 v2)
- *([#205](https://github.com/yannickgranger/cfdb/issues/205))* Raid plan validation queries + bucket convention (RFC-036 §3.5)
- *(cfdb)* Real cfdb diff — keyspace-to-keyspace delta over canonical sorted-JSONL ([#212](https://github.com/yannickgranger/cfdb/issues/212))
- *(cfdb)* Cfdb classify verb — debt-class routing of diff findings ([#213](https://github.com/yannickgranger/cfdb/issues/213))
- *(cfdb-cli)* Cfdb classify --format sorted-jsonl ([#236](https://github.com/yannickgranger/cfdb/issues/236))

### 🐛 Bug Fixes

- *([#203](https://github.com/yannickgranger/cfdb/issues/203))* Demote internal metrics fns to pub(crate); prune spec entries
- *(cfdb-hir-extractor)* Include impl target in fn_name_and_qname ([#227](https://github.com/yannickgranger/cfdb/issues/227))

### 📚 Documentation

- *(specs)* Spec-hygiene amendments for RFC-036 ([#206](https://github.com/yannickgranger/cfdb/issues/206))
- RFC-037 schema-producer alignment
- *(cfdb-cli)* Drop stale snapshot/EPIC-[#3622](https://github.com/yannickgranger/cfdb/issues/3622) framing on cfdb diff stub

### 🎨 Styling

- *([#203](https://github.com/yannickgranger/cfdb/issues/203))* Apply cargo fmt canonical form
- *([#203](https://github.com/yannickgranger/cfdb/issues/203))* Fix clippy::doc_lazy_continuation on EnrichVerb::Metrics doc
## [0.2.0] - 2026-04-23

### 🚀 Features

- *(oss)* Gitea→GitHub one-way mirror + contributor-feedback CI
- *(index)* IndexSpec + .cfdb/indexes.toml loader ([#180](https://github.com/yannickgranger/cfdb/issues/180))
- *(index)* By_prop build pass + stale-entry removal ([#181](https://github.com/yannickgranger/cfdb/issues/181))
- *(qname,index)* Last_segment helper + ComputedKey::evaluate dispatch ([#182](https://github.com/yannickgranger/cfdb/issues/182))
- *(persist,eval)* Rebuild on load + Cypher last_segment unification ([#183](https://github.com/yannickgranger/cfdb/issues/183))
- *(eval,index)* Candidate_nodes fast paths via by_prop ([#184](https://github.com/yannickgranger/cfdb/issues/184))
- *(eval,index)* Cross-MATCH posting-list intersection ([#185](https://github.com/yannickgranger/cfdb/issues/185))
- *(cli,store)* Composition-root wiring for .cfdb/indexes.toml ([#186](https://github.com/yannickgranger/cfdb/issues/186))

### 🐛 Bug Fixes

- *(oss)* Rename secret GITHUB_MIRROR_PAT → MIRROR_PAT
- *(cfdb-recall)* Serialise rustdoc-json build across parallel tests
- *([#169](https://github.com/yannickgranger/cfdb/issues/169))* Push $context filter into Cypher (classifier)
- *([#169](https://github.com/yannickgranger/cfdb/issues/169))* Drop unused scalar_str import after filter removal
- *([#170](https://github.com/yannickgranger/cfdb/issues/170))* Borrow node_id through reference accumulator
- *([#170](https://github.com/yannickgranger/cfdb/issues/170))* Add missing 'a lifetime on find_references scanned arg
- *([#171](https://github.com/yannickgranger/cfdb/issues/171))* Defer author_email and path clones to first-insert
- *([#171](https://github.com/yannickgranger/cfdb/issues/171))* Bind commit.author() to a local before borrowing email()
- *([#172](https://github.com/yannickgranger/cfdb/issues/172))* Chain edge iterators instead of collect+extend
- *([#168](https://github.com/yannickgranger/cfdb/issues/168))* Stream binding table through MATCH pipeline
- *(boy-scout [#182](https://github.com/yannickgranger/cfdb/issues/182))* Metrics + architecture quality fixes
- *(boy-scout [#182](https://github.com/yannickgranger/cfdb/issues/182))* Wire Makefile graph-specs-check target

### 🚜 Refactor

- *(petgraph)* Extract AC1 round-trip test to sibling #[cfg(test)] mod
- *([#184](https://github.com/yannickgranger/cfdb/issues/184))* Extract lookup tests to sibling file

### 📚 Documentation

- Fill out README, scrub target-workspace leaks
- *([#195](https://github.com/yannickgranger/cfdb/issues/195))* Trim PLAN-v1 + add PLAN-v2 substrate for next RFC
- *([#51](https://github.com/yannickgranger/cfdb/issues/51))* Merge RFC-029 v0.2 addendum into parent RFC-cfdb.md
- *([#199](https://github.com/yannickgranger/cfdb/issues/199))* Ratify RFC-036 — cfdb v2 (HSB/VSB/raid validation)

### ⚡ Performance

- *([#184](https://github.com/yannickgranger/cfdb/issues/184))* Zero-alloc intersection via Vec::retain

### 🎨 Styling

- *([#169](https://github.com/yannickgranger/cfdb/issues/169))* Auto-fmt regression test
- *([#172](https://github.com/yannickgranger/cfdb/issues/172))* Auto-fmt after iterator chain refactor
- *([#168](https://github.com/yannickgranger/cfdb/issues/168))* Auto-fmt after streaming rewrite
- *([#180](https://github.com/yannickgranger/cfdb/issues/180))* Auto-fmt
- *([#181](https://github.com/yannickgranger/cfdb/issues/181))* Clippy cleanup (approx_constant + unnecessary_get_then_check)
- *([#184](https://github.com/yannickgranger/cfdb/issues/184))* Iterator-chain form in collect_pattern_hints
- *([#184](https://github.com/yannickgranger/cfdb/issues/184))* Cargo fmt on extracted lookup_tests.rs
- *([#185](https://github.com/yannickgranger/cfdb/issues/185))* Compress pattern.rs docs to stay under 500-LoC ceiling
- *([#185](https://github.com/yannickgranger/cfdb/issues/185))* Drop unused PropValue import in test file
- *([#185](https://github.com/yannickgranger/cfdb/issues/185))* Cargo fmt on target_dogfood_tests

### 🧪 Testing

- *([#169](https://github.com/yannickgranger/cfdb/issues/169))* Add red regression test for context filter pushdown
- *([#185](https://github.com/yannickgranger/cfdb/issues/185))* Target-dogfood measurement against qbot-core

### ⚙️ Miscellaneous Tasks

- *(oss)* Remove agent-workflow cruft from tracked tree
- *(oss)* Scrub private infra refs from root files
## [0.1.0] - 2026-04-21

### 🚀 Features

- *(cfdb-recall)* Gate clap + rustdoc-json behind runner feature ([#24](https://github.com/yannickgranger/cfdb/issues/24))
- *(cfdb-cli)* Typed CfdbCliError replacing Box<dyn std::error::Error> ([#22](https://github.com/yannickgranger/cfdb/issues/22))
- *(cfdb-core)* Split EnrichBackend out of StoreBackend ([#27](https://github.com/yannickgranger/cfdb/issues/27))
- *(ci)* Cross-dogfood fixture + shared SHA parser ([#65](https://github.com/yannickgranger/cfdb/issues/65))
- *(ci)* Wire cross-dogfood CI + cfdb violations --count-only ([#66](https://github.com/yannickgranger/cfdb/issues/66))
- *(ci)* Weekly cross-fixture bump cron — Mon 06:00 UTC ([#67](https://github.com/yannickgranger/cfdb/issues/67))
- *(ci)* Weekly closed-loop cross-check cron — Tue 06:00 UTC ([#70](https://github.com/yannickgranger/cfdb/issues/70))
- *(cfdb)* :Item.visibility + SchemaVersion v0.1.1 ([#35](https://github.com/yannickgranger/cfdb/issues/35))
- *(cfdb)* :Item.cfg_gate + SchemaVersion v0.1.2 ([#36](https://github.com/yannickgranger/cfdb/issues/36))
- *(cfdb)* :CallSite resolver discriminator + SchemaVersion v0.1.3 ([#83](https://github.com/yannickgranger/cfdb/issues/83))
- *(cfdb-hir-extractor)* Scaffold crate + ra-ap-* pins + arch boundary test ([#84](https://github.com/yannickgranger/cfdb/issues/84))
- *(cfdb-hir-extractor)* CallSiteEmitter trait + cfdb-hir-petgraph-adapter scaffold ([#92](https://github.com/yannickgranger/cfdb/issues/92))
- *(cfdb-hir-extractor)* Build_hir_database + resolved :CallSite + CALLS + INVOKES_AT + SchemaVersion V0_1_4 ([#94](https://github.com/yannickgranger/cfdb/issues/94))
- *(cfdb-core,cfdb-hir-extractor)* Address [#94](https://github.com/yannickgranger/cfdb/issues/94) ddd review — normalize_impl_target + trait-dispatch test
- *(cfdb-hir-extractor,cfdb-cli)* :EntryPoint + EXPOSES + cfdb-cli --features hir + SchemaVersion V0_2_0 ([#86](https://github.com/yannickgranger/cfdb/issues/86))
- *(cfdb-core,cfdb-cli)* Slice 43-A prereq — EnrichBackend rename/additions + schema reservations + RFC amendment ([#104](https://github.com/yannickgranger/cfdb/issues/104))
- *(cfdb-core,cfdb-petgraph)* Slice 43-A AC-completion — :Item attribute stubs + PetgraphStore workspace_root + dogfood proofs ([#104](https://github.com/yannickgranger/cfdb/issues/104))
- *(cfdb-extractor,cfdb-core,cfdb-petgraph)* Slice 43-C — #[deprecated] fact extraction + SchemaVersion V0_2_1 ([#106](https://github.com/yannickgranger/cfdb/issues/106))
- *(cfdb-extractor,cfdb-core)* Slice [#42](https://github.com/yannickgranger/cfdb/issues/42) — impl-block :Items + IMPLEMENTS/IMPLEMENTS_FOR edges + SchemaVersion V0_2_2
- *(cfdb-petgraph)* Enrich_git_history real impl — git2 behind git-enrich feature ([#105](https://github.com/yannickgranger/cfdb/issues/105))
- *(cfdb-cli)* Persist enrichment results to disk + target-dogfood proof for [#105](https://github.com/yannickgranger/cfdb/issues/105)
- *(cfdb-petgraph,cfdb-core,cfdb-cli)* Enrich_rfc_docs real impl + SchemaVersion V0_2_3 ([#107](https://github.com/yannickgranger/cfdb/issues/107))
- *(cfdb-petgraph,cfdb-cli)* Enrich_bounded_context re-enrichment + v0.2-9 ≥95% gate ([#108](https://github.com/yannickgranger/cfdb/issues/108))
- *(cfdb-petgraph,cfdb-concepts,cfdb-cli)* Enrich_concepts — :Concept nodes + LABELED_AS/CANONICAL_FOR edges ([#109](https://github.com/yannickgranger/cfdb/issues/109))
- *(cfdb-petgraph,cfdb-cli)* Enrich_reachability — BFS from :EntryPoint over CALLS+INVOKES_AT ([#110](https://github.com/yannickgranger/cfdb/issues/110))
- *(cfdb-cli)* Cfdb extract --rev <sha> — extract against arbitrary git revisions ([#37](https://github.com/yannickgranger/cfdb/issues/37))
- *(cfdb-hir-extractor)* Cron_job + websocket :EntryPoint kinds ([#125](https://github.com/yannickgranger/cfdb/issues/125))
- *(cfdb-hir-extractor)* Http_route :EntryPoint kind (axum + actix-web) ([#124](https://github.com/yannickgranger/cfdb/issues/124))
- *([#127](https://github.com/yannickgranger/cfdb/issues/127))* W2.A — `check-prelude-triggers` Tier-1 binary (5 C-triggers)
- *(cfdb)* Vertical-split-brain.cypher (Pattern B) + scar tests ([#44](https://github.com/yannickgranger/cfdb/issues/44))
- *(cfdb)* Canonical-bypass Pattern C — 4 verdicts, generalized ([#45](https://github.com/yannickgranger/cfdb/issues/45))
- *(cfdb)* Signature_divergent UDF + fn signature emission ([#47](https://github.com/yannickgranger/cfdb/issues/47))
- *(cfdb)* :Finding classifier Cypher + 6-class taxonomy wiring ([#48](https://github.com/yannickgranger/cfdb/issues/48))
- *(cfdb-concepts,cfdb-extractor)* .cfdb/published-language-crates.toml loader + :Crate.published_language prop ([#100](https://github.com/yannickgranger/cfdb/issues/100))
- *(cfdb-cli)* Cfdb extract --rev <url>@<sha> — Option W bilateral drift-lock ([#96](https://github.com/yannickgranger/cfdb/issues/96))
- *(cfdb-cli)* `cfdb check --trigger T1` — editorial-drift detection for TOML concept declarations ([#101](https://github.com/yannickgranger/cfdb/issues/101))
- *(cfdb-cli)* `cfdb check --trigger T3` — concept-name-in-≥2-crates raw Pattern A with is_cross_context flag ([#102](https://github.com/yannickgranger/cfdb/issues/102))
- *(ci,specs)* Extend anti-drift gate to tools/, onboard check-prelude-triggers ([#137](https://github.com/yannickgranger/cfdb/issues/137))
- *(cfdb-cli)* Param_resolver — TOML-backed --param forms ([#145](https://github.com/yannickgranger/cfdb/issues/145))
- *(.cfdb,cfdb-query)* Predicate seed library + schema-ref static check ([#146](https://github.com/yannickgranger/cfdb/issues/146))
- *(cfdb-cli)* Check-predicate verb — dispatch named predicate with resolved params ([#147](https://github.com/yannickgranger/cfdb/issues/147))
- *(cfdb-cli,ci)* Predicate-library dogfood + determinism CI ([#148](https://github.com/yannickgranger/cfdb/issues/148))

### 🐛 Bug Fixes

- *(ci)* Portage studies/spike fixtures for cfdb-petgraph tests
- *(cfdb-petgraph)* Adjust fixture path after sub-workspace → root portage
- *(cfdb-cli)* Mark shell snippet in hir.rs module doc as text (CI doctest fix)
- *(cfdb-extractor, boy-scout [#107](https://github.com/yannickgranger/cfdb/issues/107))* Parse_syn_visibility delegates to Visibility::FromStr
- *([#127](https://github.com/yannickgranger/cfdb/issues/127))* Register check-prelude-triggers in .cfdb/concepts/cfdb.toml
- *(specs)* Revert boy-scout check-prelude-triggers + qa5-utc-now specs ([#48](https://github.com/yannickgranger/cfdb/issues/48))

### 🚜 Refactor

- *(cfdb-query)* Move query composers from cfdb-core ([#25](https://github.com/yannickgranger/cfdb/issues/25))
- *(cfdb-cli)* Consolidate composition root into compose.rs ([#23](https://github.com/yannickgranger/cfdb/issues/23))
- *(cfdb-query)* Unify string-literal scanners ([#28](https://github.com/yannickgranger/cfdb/issues/28))
- *(cfdb-petgraph)* Reduce pattern.rs complexity ([#26](https://github.com/yannickgranger/cfdb/issues/26))
- *(cfdb-core)* Extract canonical qname derivation ([#90](https://github.com/yannickgranger/cfdb/issues/90), prereq for [#85](https://github.com/yannickgranger/cfdb/issues/85) HIR)
- *(cfdb-core)* Add qname_from_node_id inverse + qualified-target test (follow-up [#90](https://github.com/yannickgranger/cfdb/issues/90))
- *(cfdb-concepts)* Extract shared bounded-context resolver crate ([#3](https://github.com/yannickgranger/cfdb/issues/3))
- *(cfdb-cli)* Extract dispatch_enrich helper — claw back run() complexity from seven-arm bloom ([#104](https://github.com/yannickgranger/cfdb/issues/104))
- *(cfdb-*)* Drain 53 pre-existing quality-metrics violations → 0 (closes [#111](https://github.com/yannickgranger/cfdb/issues/111))
- *(cfdb-core)* Split item_node_descriptor attrs by provenance (complexity 16→≤10)
- *(cfdb-petgraph)* Convert enrich_rfc_docs for-loops to iterator chains (clone-in-loop drain)
- *(cfdb-hir-extractor)* Compress entry_point_emitter doc + scan_file to stay under 500-line god-file threshold ([#125](https://github.com/yannickgranger/cfdb/issues/125))
- *(cfdb-cli)* Split main.rs into command/parse/dispatch ([#128](https://github.com/yannickgranger/cfdb/issues/128))
- *(cfdb-core)* Split schema/describe.rs into nodes/edges/tests ([#128](https://github.com/yannickgranger/cfdb/issues/128))
- *(cfdb-extractor)* Split attrs.rs + item_visitor.rs ([#128](https://github.com/yannickgranger/cfdb/issues/128))
- *(cfdb-petgraph)* Split 4 god-files — enrich/{concepts,reachability,rfc_docs} tests + lib.rs EnrichBackend + canonical_dump ([#128](https://github.com/yannickgranger/cfdb/issues/128))
- *(cfdb-cli)* Split check.rs into t1/t3/tests submodules ([#151](https://github.com/yannickgranger/cfdb/issues/151))
- *(cfdb-cli)* Split scope.rs into classifier/helpers submodules ([#151](https://github.com/yannickgranger/cfdb/issues/151))
- *(cfdb-cli)* Split commands.rs into extract/query/rules/aux/tests submodules ([#151](https://github.com/yannickgranger/cfdb/issues/151))
- *(cfdb-hir-extractor)* Split entry_point_emitter.rs into http_route/other_kinds submodules ([#151](https://github.com/yannickgranger/cfdb/issues/151))

### 📚 Documentation

- *(rfc-030)* Anti-drift gate — adopt graph-specs + cfdb self-dogfood
- *(RFC-031)* Absorb orphan audit issues [#22](https://github.com/yannickgranger/cfdb/issues/22)-[#29](https://github.com/yannickgranger/cfdb/issues/29) into architectural RFC
- *(specs)* Initial per-crate concept specs for cfdb workspace
- *(rfc-032)* V0.2 extractor cohort — issues [#35](https://github.com/yannickgranger/cfdb/issues/35)–[#51](https://github.com/yannickgranger/cfdb/issues/51) grouped and sequenced
- *(rfc-030)* Revision 1 — correct dialect, CLI flags, deferred list
- *(RFC-030)* Drop pinned-tag paragraph in §7.4 per user directive
- *(RFC-030)* Fix §3.2 drift — classifier, snapshot format, fabricated cite
- *([#58](https://github.com/yannickgranger/cfdb/issues/58))* Add CLAUDE.md codifying RFC-first methodology + dogfood gates
- Tests + real infra mandatory; architects prescribe in issues ([#62](https://github.com/yannickgranger/cfdb/issues/62))
- *(RFC-033)* Draft cross-dogfood discipline with graph-specs-rust
- *(RFC-033)* Revision 1 — address 4 blockers + 12 mandatory items from review
- *(RFC-033)* Ratify — all four architect lenses RATIFY
- Cross-fixture-bump runbook — canonical orchestration vocab ([#68](https://github.com/yannickgranger/cfdb/issues/68))
- Tests: template + SchemaVersion lockstep note ([#69](https://github.com/yannickgranger/cfdb/issues/69), [#71](https://github.com/yannickgranger/cfdb/issues/71))
- *(runbook)* No manual SHA ceremony in SchemaVersion lockstep
- Ra-ap-hir weekly upgrade protocol runbook ([#39](https://github.com/yannickgranger/cfdb/issues/39))
- *(specs)* Add cfdb-concepts spec for the new shared crate ([#3](https://github.com/yannickgranger/cfdb/issues/3))
- *(council)* [#43](https://github.com/yannickgranger/cfdb/issues/43) enrichment framework decomposition — 4-lens verdicts + synthesis R1
- *(specs)* Cfdb-query classifier types + boy-scout check-prelude-triggers/qa5-utc-now specs ([#48](https://github.com/yannickgranger/cfdb/issues/48))
- *(specs)* Add TriggerId + UnknownTriggerId spec entries for [#101](https://github.com/yannickgranger/cfdb/issues/101)
- *([#149](https://github.com/yannickgranger/cfdb/issues/149))* Query-dsl user guide + homonym note + CLI inventory (Slice 5)

### 🎨 Styling

- Cargo fmt auto-fix (5 files)
- Cargo fmt after CfdbCliError rename ([#22](https://github.com/yannickgranger/cfdb/issues/22))
- Clippy unnecessary_get_then_check
- Move tests mod to end of commands.rs (clippy items_after_test_module)
- *(cfdb)* Cargo fmt after [#48](https://github.com/yannickgranger/cfdb/issues/48) classifier wiring
- *(cfdb-extractor)* Rustfmt fixes from /ship --fix ([#128](https://github.com/yannickgranger/cfdb/issues/128))
- *(cfdb-hir-extractor)* Rustfmt resolve_handler_qname signature ([#151](https://github.com/yannickgranger/cfdb/issues/151))

### 🧪 Testing

- Add architecture dep-rule tests to adapter crates ([#21](https://github.com/yannickgranger/cfdb/issues/21))
- *(cfdb-hir-extractor)* V0.2-1 coverage gate + ground-truth fixture ([#126](https://github.com/yannickgranger/cfdb/issues/126))

### ⚙️ Miscellaneous Tasks

- Add initial CI workflow with Check job
- Install nightly for cfdb-recall rustdoc-json integration tests
- Clean up stale qbot-core/.concept-graph paths post-portage
- Bump rust-version floor 1.75 → 1.80 ([#20](https://github.com/yannickgranger/cfdb/issues/20))
- Commit Cargo.lock — workspace ships two binaries ([#19](https://github.com/yannickgranger/cfdb/issues/19))
- Wire RFC-030 dual-control gates + fix spec drift ([#53](https://github.com/yannickgranger/cfdb/issues/53))
- Fix graph-specs install — package is `application`, bin is `graph-specs`
- Add --force to graph-specs install — track-develop semantics
- *(workspace)* Rust-version 1.80 → 1.85 ([#82](https://github.com/yannickgranger/cfdb/issues/82), close [#39](https://github.com/yannickgranger/cfdb/issues/39) MSRV gap)
- *(ci)* Add no-op Makefile integ targets for quality-preflight contract
- *(cfdb-petgraph,cfdb-cli)* Cargo fmt for [#105](https://github.com/yannickgranger/cfdb/issues/105)
- *(proofs)* Add clippy + audit proofs for [#105](https://github.com/yannickgranger/cfdb/issues/105)
- Cargo fmt trailing newline
- *(cfdb-cli)* Cargo.lock for cfdb-concepts + toml deps ([#145](https://github.com/yannickgranger/cfdb/issues/145))
- *(release-infra)* Add release.yml + git-cliff + Makefile release-prepare
