# Spec: check-prelude-triggers

Tier-1 binary that evaluates RFC-034 v3.3 C-triggers (C1 cross-context, C3 port-signature, C7 financial-precision, C8 pipeline-stage, C9 workspace-cardinality) against a git diff and emits the deterministic trigger envelope consumed by `/freshness`, `/discover`, `/prescribe`, `/gate-contract`, and `/pre-council`. Ships as a separate binary under `tools/` so the load-independent floor can run in consumer repos that don't vendor cfdb's library surface.

## PreludeTriggerReport

RFC-034 §4.2 envelope emitted on stdout: `schema_version`, `from_ref`, `to_ref`, `triggers_fired` (union of fired IDs across all checks in this invocation), `evidence` (shallow map keyed by `TriggerId`). Consumed by `/freshness` Step 2g which merges per-trigger envelopes into the per-issue `.triggers/<issue>.json` source of truth.

## TriggerId

Enum of the C-trigger identifiers — `C1`, `C3`, `C7`, `C8`, `C9`. Additive (OCP): future Tier-2 promotions append new variants without breaking consumers that parse the string. Serde rename is the RFC-034 wire spelling.

## TriggerOutcome

Result of evaluating one C-trigger against a diff snapshot — `fired: bool` (pre-council review MANDATED when true) plus an `evidence` payload embedded in the outer `PreludeTriggerReport.evidence[id]`. The evidence shape is trigger-specific so each handler can carry its own minimal fact set.
