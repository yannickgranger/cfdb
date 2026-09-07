// arch-ban-rfc-043-calls-edge-unresolved.cypher — RFC-044 §3.8 slice 044-H (issue #427); RFC-029 §A1.2 / RFC-043 §4

MATCH (caller:Item)-[r:CALLS]->(callee:Item)
WHERE r.resolved = false
RETURN caller.qname AS caller_qname,
       callee.qname AS callee_qname
ORDER BY caller_qname ASC, callee_qname ASC
