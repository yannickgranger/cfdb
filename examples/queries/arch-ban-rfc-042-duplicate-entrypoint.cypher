// arch-ban-rfc-042-duplicate-entrypoint.cypher
//   — RFC-044 §3.8 slice 044-H (issue #427); encodes RFC-042 §4.
//
// # Invariant encoded (RFC-042 §4 — "no duplicate :EntryPoint emission")
//
// The HIR extractor (`cfdb-hir-extractor::extract_entry_points`) must emit
// each distinct entry point EXACTLY ONCE. A duplicate emission — two
// `:EntryPoint` nodes sharing the SAME (name, kind, file, handler_qname)
// 4-tuple — means the entry-point scanner double-counted a single
// declaration (e.g. visiting the same clap `#[derive(Subcommand)]` enum
// from two AST paths, or re-emitting an `#[tool]` fn touched by two
// proc-macro passes). RFC-042 ratified this as a reviewer-only invariant;
// this rule mechanises it.
//
// # Inversion (positive invariant → its negation)
//
// The positive invariant is "every (name, kind, file, handler_qname) tuple
// has at most one :EntryPoint node". The Cypher subset has no universal
// quantifier, so we express the NEGATION: group every :EntryPoint by the
// full identity tuple and surface any group whose `COUNT(*) > 1`. A clean
// tree yields zero such groups. Each returned row IS a duplicate-emission
// bug.
//
// # Why a group-by-count and not a self-join
//
// `:EntryPoint` carries no `line` attribute (it is `null` — see
// `cfdb schema-describe`), so a `MATCH (a:EntryPoint), (b:EntryPoint)`
// self-join has no scalar to break the symmetric pair AND no scalar to
// exclude the `a = b` self-match. The aggregation form
// (`WITH ... COUNT(*) AS n WHERE n > 1`) sidesteps both problems: it groups
// on the identity tuple and counts node multiplicity directly. This mirrors
// `const-table-overlap.cypher`'s WITH-materialise-then-filter shape.
//
// # File location — documented deviation from RFC §3.8
//
// RFC §3.8 literally says `.cfdb/queries/arch-ban-rfc-<n>-<topic>.cypher`.
// This rule ships in `examples/queries/` instead. Rationale: the arch-ban
// family already lives in `examples/queries/`, and BOTH the self-dogfood
// loop (`.gitea/workflows/ci.yml` ~line 201) and the cross-dogfood loop
// (`ci/cross-dogfood.sh` ~line 86) already glob
// `examples/queries/arch-ban-*.cypher`. Placing the rule here makes it
// auto-enforced with ZERO CI-workflow edits — honouring Q2.b's actual
// point (these ARE zero-tolerance ban rules) without forking the runner.
//
// # Usage
//   cfdb violations --db <dir> --keyspace <ks> \
//     --rule examples/queries/arch-ban-rfc-042-duplicate-entrypoint.cypher
//
// Expected: empty on a clean tree. Any row is a duplicate :EntryPoint
// emission (an extractor bug).

MATCH (e:EntryPoint)
WITH e.name AS name,
     e.kind AS kind,
     e.file AS file,
     e.handler_qname AS handler_qname,
     COUNT(*) AS n
WHERE n > 1
RETURN name AS name,
       kind AS kind,
       file AS file,
       handler_qname AS handler_qname,
       n AS n
ORDER BY name ASC, kind ASC, file ASC, handler_qname ASC
