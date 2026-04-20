# Spec: qa5-utc-now-spike

Experimental spike under `spikes/qa5-utc-now/` that measured the extractor's recall on the `arch-ban-utc-now` Pattern D rule against a known set of production hits. Used to validate `:CallSite` classification counts before RFC-029 §A1.2 lifted the HIR dependency. Not part of the shipped tool surface — the crate is retained for regression rerun value, not for downstream consumption.

## FileStats

Per-file counter bundle — ripgrep line count, call-site production count, call-site test count, call-site non-test count. Used to diff two extraction passes for the spike's parity check.

## Subclass

Classification enum for each `SystemTime::now() / Utc::now()` call site — `Call` (function/method invocation), `FnPtr` (first-class function reference), `SerdeAttr` (used inside a serde attribute), etc. Tied to the Pattern D rule's row-shape; not persisted in any keyspace.

## Totals

Aggregate counters across all scanned files — totals per subclass plus grand totals. Printed at the end of a spike run for eyeball comparison against the ground-truth snapshot.
