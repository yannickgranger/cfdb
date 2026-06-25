# RFC-051 — Non-code / infrastructure-as-code + DDL extraction

- **Status:** DRAFT — **PARKED** pending (1) a named downstream consumer and (2) a non-rustdoc recall ground-truth design. Drafted for architect triage, not for near-term implementation. (Borrowed candidate **C5** from [`studies/002`](../studies/002-borrowed-from-understand-anything.md).)
- **Issue:** none (will not be filed until both blockers in §1 clear).
- **Schema impact:** **large** — multiple new node labels (`:Resource`, `:Table`, `:Pipeline`, `:Service`) + infra edges, across several minor `SchemaVersion` bumps.
- **Companion:** **required per bump** — each new label is a lockstep `graph-specs-rust` fixture bump.
- **Origin:** `Understand-Anything`'s 12 non-code parsers (Dockerfile, docker-compose, Kubernetes, Terraform, SQL DDL, GraphQL, OpenAPI, Protobuf, GitHub Actions, YAML, env, Makefile).

---

## 1. Problem & why it is parked

A real system's behaviour also lives in its IaC, CI, and schema files. cfdb stops at code (Rust/PHP/TS). The discovery in [`studies/003 §1`](../studies/003-cfdb-understand-discovery.md) found cfdb's own tree carries 45 `.cypher`, 9 YAML, 2 infra, and 72 TOML files that the current extractor does not model as graph inputs — UA ingested all of them. There is even a self-referential angle: cfdb could model its **own** `.cfdb/queries/*.cypher` ban-rules as graph nodes.

**Two hard blockers keep this parked:**
1. **No consumer pull.** Per the standing boundary ("cfdb owns the agnostic capability + its own dogfood + the graph-specs companion lockstep only" — and "tool backlog ≠ client chores"), a large new fact surface must be pulled by a real consumer (e.g. agentry needing deploy-topology facts). None exists today. This is capability-driven only, which is **not** sufficient justification for the largest schema expansion in the backlog.
2. **No recall ground-truth.** cfdb's correctness gate is "extractor ≡ `rustdoc --output-format=json`." Non-code facts have **no rustdoc analog**. A new ground-truth (hand-curated per-format fixtures, or a second reference parser) must be designed *before* any non-code fact can be trusted — otherwise cfdb would emit unverified facts, violating §5 schema discipline.

## 2. Scope (only if unblocked)

New deterministic parsers emitting:
- `:Service` (Dockerfile stages, compose services), `:Resource` (Terraform resources, K8s objects), `:Table` (SQL DDL), `:Pipeline` (CI jobs/steps), reusing/extending `:EntryPoint` for routes.
- Infra edges: `deploys` / `serves` / `provisions` / `triggers` / `migrates` / `routes` / `defines_schema` (the `Understand-Anything` infra edge family).

Strictly deterministic (regex + tree-sitter / format parsers); no LLM.

## 3. Design (sketch — to be expanded only post-unblock)

Mirror `Understand-Anything`'s per-format parser plugins, ported to cfdb's extractor as new language/format extractors behind the existing extractor-registry seam. Each format is one vertical slice (one node label + its edges + its recall fixture). The self-dogfood proof of concept is modelling `.cfdb/queries/*.cypher` as nodes — cfdb extracting its own ruleset.

## 4. Invariants

- **Determinism (`G1`).** Format parsers are pure; byte-stable.
- **Recall — the blocker.** Each format needs a designed ground-truth (§1.2). Without it, the fact is inadmissible.
- **Additive schema (`G4`)** + lockstep companion per label.
- **No scope without a consumer (§1.1).**

## 5. Architect lenses

> **DRAFT — for triage, not full review.** The architects' first job is to decide **whether to keep this parked**. Pre-seeded:
- **ddd:** are `:Service`/`:Resource`/`:Table` concepts cfdb's bounded context should own, or are they a consumer's domain? (This is the §1.1 question in DDD terms.)
- **clean-arch / solid:** registry seam for format parsers; one slice per format (CCP/SRP).
- **rust-systems:** which formats reuse tree-sitter grammars already vendored vs. need new deps; compile-cost of N new grammars.

## 6. Non-goals

- LLM-inferred infra relationships (UA infers some edges via LLM) — cfdb stays deterministic.
- Any format without a designed recall ground-truth.
- Shipping before a consumer exists.

## 7. Issue decomposition

**Deliberately omitted while parked.** When/if both §1 blockers clear, decompose one vertical slice per format (node label + edges + recall fixture + `Tests:` block), starting with the self-dogfood slice (`.cfdb/queries/*.cypher` as nodes) which has a built-in ground-truth (the rule files themselves). Until then there is no issue to file.
