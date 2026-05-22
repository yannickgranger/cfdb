// arch-ban-rfc-042-entrypoint-handler-mismatch.cypher
//   — RFC-044 §3.8 slice 044-H (issue #427); encodes RFC-042 §4
//     (:EntryPoint / EXPOSES structural consistency).
//
// # Invariant encoded (RFC-042 §4 — handler_qname ≡ EXPOSES target)
//
// A `:EntryPoint` carries the handler's qualified name TWICE: once as the
// `handler_qname` ATTRIBUTE (the declared dispatch target) and once
// STRUCTURALLY as the qname of the `:Item` reached by its `EXPOSES` edge
// (`(:EntryPoint)-[:EXPOSES]->(:Item)`). The HIR extractor writes both from
// the same source fact (`entry_point_emitter.rs::emit`), so they MUST agree:
//
//     EntryPoint.handler_qname == (the :Item reached by EXPOSES).qname
//
// A mismatch is a split-brain between the attribute and the edge — the
// scalar `handler_qname` says one thing while the structural EXPOSES edge
// points elsewhere. Any consumer that trusts the attribute (e.g. a CLI that
// resolves handler_qname) would then disagree with any consumer that
// traverses EXPOSES (e.g. reachability enrichment). RFC-042 §4 holds the
// emitter to single-source consistency; before this rule it was
// reviewer-only.
//
// # Inversion (positive invariant → its negation)
//
// Positive invariant: for every EXPOSES edge, the target :Item.qname equals
// the source :EntryPoint.handler_qname. Negation: match the edge and select
// pairs where `h.qname <> e.handler_qname`. A clean tree yields none.
//
// # File location — documented deviation from RFC §3.8
//
// Ships in `examples/queries/` (not `.cfdb/queries/`) so the existing
// `examples/queries/arch-ban-*.cypher` globs in `.gitea/workflows/ci.yml`
// (~line 201) and `ci/cross-dogfood.sh` (~line 86) auto-enforce it with
// zero CI-workflow edits.
//
// # Usage
//   cfdb violations --db <dir> --keyspace <ks> \
//     --rule examples/queries/arch-ban-rfc-042-entrypoint-handler-mismatch.cypher
//
// Expected: empty on a clean tree. Any row is a :EntryPoint whose declared
// handler_qname disagrees with the qname of the :Item its EXPOSES edge
// reaches.

MATCH (e:EntryPoint)-[:EXPOSES]->(h:Item)
WHERE h.qname <> e.handler_qname
RETURN e.name AS name,
       e.kind AS kind,
       e.handler_qname AS declared_handler,
       h.qname AS exposes_target,
       e.file AS file
ORDER BY file ASC, name ASC
