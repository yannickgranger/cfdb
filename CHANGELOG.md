# Changelog

All notable changes to cfdb will be documented in this file.

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
