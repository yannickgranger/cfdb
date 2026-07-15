# RFC-051 — Non-code / infrastructure-as-code + DDL extraction

- **Status:** **COUNCIL-TRIAGED → KEEP-PARKED (unanimous)** pending (1) a named downstream consumer and (2) a non-rustdoc recall ground-truth design. Verdicts: [`council/RFC-047-052/`](../council/RFC-047-052/). The format-parser registry design is sound; only the *trigger* is absent. (Borrowed candidate **C5** from [`studies/002`](../studies/002-borrowed-from-understand-anything.md).)
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

> **TRIAGED by the RFC-047..052 council — KEEP-PARKED (unanimous).** The design is sound; the trigger is absent. No lens objects to the parked status.

- **ddd — KEEP-PARKED.** `:Service`/`:Resource`/`:Table`/`:Pipeline` describe a *deployment/infrastructure* bounded context, not cfdb's *code-graph* context. Admitting them imports another context's ubiquitous language into cfdb-core with no anti-corruption layer and no ground-truth gate (the §1.2 blocker is real — no rustdoc analog). The one on-charter sliver — modelling cfdb's own `.cfdb/queries/*.cypher` as nodes — is genuinely cfdb's domain but is a separate, much smaller RFC; do not use it to justify the 12-format surface.
- **clean-arch — KEEP-PARKED.** The per-format-parser-behind-the-extractor-registry seam is the right shape *if ever unblocked* (one adapter per format, `cfdb-core` declaring only the new labels; no infra-format type leaking into core), but there is nothing to ratify without consumer pull (§1.1) + ground-truth (§1.2).
- **solid — KEEP-PARKED.** One vertical slice per format (CCP) is good component design; filing the largest schema surface with no reuser would be CRP-inverted. When unblocked, the `.cfdb/queries/*.cypher`-as-nodes slice is the CCP-minimal first slice (the rule files are their own ground-truth).
- **rust-systems — KEEP-PARKED.** Compile cost is real but secondary: each new tree-sitter grammar adds a vendored grammar crate + generated C parser (PHP/TS already vendored per RFC-045; Dockerfile/HCL/K8s/SQL/GraphQL/Protobuf would roughly double the grammar surface). When unblocked: one grammar per slice, measure incremental compile cost per slice, prefer formats with an already-vendored grammar or a pure-Rust parser (`toml`/`serde_yaml` are in-tree; Terraform-HCL is not).

## 6. Non-goals

- LLM-inferred infra relationships (UA infers some edges via LLM) — cfdb stays deterministic.
- Any format without a designed recall ground-truth.
- Shipping before a consumer exists.

## 7. Issue decomposition

**Deliberately omitted while parked.** When/if both §1 blockers clear, decompose one vertical slice per format (node label + edges + recall fixture + `Tests:` block), starting with the self-dogfood slice (`.cfdb/queries/*.cypher` as nodes) which has a built-in ground-truth (the rule files themselves). Until then there is no issue to file.
