// arch-ban-rfc-043-discriminator-syn-resolved.cypher — RFC-044 §3.8 slice 044-H (issue #427); RFC-029 §A1.2 / RFC-043 §4 / cfdb-045-polyglot-relationship-edges#3

MATCH (c:CallSite)
WHERE c.callee_resolved = true
  AND NOT c.resolver IN ['hir', 'tree-sitter-php']
RETURN c.caller_qname AS caller_qname,
       c.callee_path AS callee_path,
       c.resolver AS resolver,
       c.file AS file,
       c.line AS line
ORDER BY file ASC, line ASC
