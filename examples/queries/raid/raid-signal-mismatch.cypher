// raid-signal-mismatch.cypher — RFC-036 §3.5 raid template 5/5.
//
// Detects items tagged "portage (clean)" whose enrich_metrics signals
// (`unwrap_count`, `test_coverage`, `dup_cluster_id`) contradict the
// "clean" claim. If the author marked an item for straight portage
// but it's structurally duplicated or has a high unwrap budget, the
// "clean" assertion is false — the item is either risky to move or
// will carry its scars into the new context.
//
// # v2 signals checked
//
//   unwrap_count    > $max_unwraps   — gating
//   test_coverage   < $min_coverage  — gating
//   dup_cluster_id  (any value)      — reported, not gated
//
// `dup_cluster_id` presence is returned in the output columns so the
// caller can cross-reference with `hsb-cluster.cypher`. The v0.3
// cfdb-query subset does not support `IS NOT NULL`, so gating on
// cluster membership is a v2.1 extension point.
//
// # Parameters
//
//   $portage:       List(String)  — item qnames tagged clean portage
//   $max_unwraps:   Scalar(Int)   — threshold; typical `0`
//   $min_coverage:  Scalar(Float) — threshold in [0.0, 1.0]; typical `0.6`
//
// # Output
//
//   qname           — the portage item with mismatched signals
//   unwrap_count    — populated only when the extractor + step-3 ran
//   test_coverage   — populated only when `--features llvm-cov` built
//                     the keyspace with coverage JSON attached
//   dup_cluster_id  — cluster id when item is structurally duplicated;
//                     absent = no dup detected (informational)

MATCH (i:Item)
WHERE i.qname IN $portage
  AND i.kind = 'fn'
  AND (i.unwrap_count > $max_unwraps OR i.test_coverage < $min_coverage)
RETURN i.qname AS qname,
       i.unwrap_count AS unwrap_count,
       i.test_coverage AS test_coverage,
       i.dup_cluster_id AS dup_cluster_id
ORDER BY qname ASC
