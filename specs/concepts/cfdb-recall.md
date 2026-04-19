---
crate: cfdb-recall
rfc: RFC-029, RFC-030
status: approved
---

# Spec: cfdb-recall

Extractor recall audit: compares `cfdb-extractor` output against `rustdoc-json` ground truth to measure what fraction of public items are captured. The 95% recall threshold is a `const` in this crate; raising it requires a reviewed PR. Depends on `cfdb-core` and `cfdb-extractor`; no other workspace dependency.

## Recall gate

### PublicItem

A public item from either the extractor output or the `rustdoc-json` ground truth, normalised to a common qname form for set-comparison. Carries the qname string and the source crate name.

### AuditList

A carve-out list of items excluded from the recall measurement. Each entry carries a qname and a mandatory issue-tracker comment (`#issue: reason`). The audit list is the only valid escape hatch when an item is deliberately not extracted; entries without a tracker comment are a gate violation.

### RecallReport

The output of a recall audit run: the crate name, total public item count from ground truth, matched count, missing item list, and the computed recall ratio. The gate passes when `recall_ratio >= DEFAULT_THRESHOLD`.

## Ground truth adapter

### GroundTruthError

Error type for failures in the `rustdoc-json` ground-truth adapter: missing `rustdoc` binary, malformed JSON output, version mismatch between rustdoc format and the `rustdoc-types` crate.
