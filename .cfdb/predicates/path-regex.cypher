// Params: $pat (regex:<pattern>)
// Returns: (qname, line, reason) — canonical three-column violation format.
//
// File-path regex fallback. Covers the "shell-grep escape hatch" from
// RFC-034 issue #49 without shelling out — Cypher's `=~` regex operator
// on `:File.path` is deterministic and gate-safe (RFC-034 §6 non-goals:
// "Not a Shell-grep escape hatch").
//
// Paths are emitted as `qname` for uniform three-column output shape with
// the other seeds. `line` is 0 because `:File` nodes do not carry a line
// number (files are identified by path, not by a specific line).
MATCH (f:File)
WHERE f.path =~ $pat
RETURN f.path AS qname, 0 AS line, 'file path matched regex' AS reason
ORDER BY qname
