# Split-resolution-point fences

Fence-semantics reference for RFC-053 (`docs/RFC-053-match-dispatch-sites.md`).
Read this before writing or reviewing a `:MatchSite` / `MATCHES_ON` fence.

## What a split-resolution-point fence catches

A **split resolution point** is one logical mapping — "translate enum `T` into
values / behavior" — that has drifted across more than one production site.
The canonical scar is cfdb's own history: `parse_syn_visibility` once
constructed `Visibility` variants directly, bypassing `Visibility::FromStr`,
so the `syn::Visibility` → `cfdb_core::Visibility` mapping had two owners
(collapsed by boy-scout #107, commit `2aedd013`). Before RFC-053 cfdb could
see enum-variant *construction* (`:CallSite`) and mapping *outputs*
(`:Literal`) but never the *match* half of a resolution point, so a rule that
would catch the second site reappearing was inexpressible. `:MatchSite` +
`MATCHES_ON` make it writable.

## Multiplicity is a survey metric, NOT a violation

Matching on your own enum from many functions is normal Rust. Raw
`MATCHES_ON` in-degree — "how many sites dispatch on enum `T`" — is a
**survey** signal (`examples/queries/matchsite-top-matched-on-enums.cypher`),
never a ban signal. A **fence** is more than a count: it requires a
**designation** — "type `T`'s dispatch-to-values is owned by module/fn `F`" —
expressed as a scoping predicate in a reviewed `.cypher` rule. cfdb ships the
facts and the templates; each repo designates its own canonical sites
(agentry: the FSM phase enum; qbot-core: alias / normalization enums; cfdb:
the `syn::Visibility` conversion).

## The guardrail (verbatim from RFC-053 ratification)

- **One fence file per fenced type.**
- **At most ONE canonical-site NOT-clause per file.**
- **Never an accreting exception list.** A rule file that grows NOT-clauses
  has become a de facto allowlist and is **rejected on sight** (project
  `CLAUDE.md` §3, no-ratchet rule). This is not a metric-ratchet file: the
  allow-scope is closed and RFC-gated in kind, not a baseline that ratchets
  up per PR.

If a second site is legitimate, it is either folded into the canonical owner
or the designation itself is wrong and the fence is re-thought — you do not
add a second exemption.

## The two template forms

Both live in `examples/queries/` as parameterized, smoke-skipped templates. A
repo instantiates one into a concrete `arch-ban-*.cypher` with the parameters
pinned to literals.

### External-type form — `matchsite-external-type-fence.cypher`

For a fenced type with **no workspace `:Item` node** (a dependency's type,
e.g. `syn::Visibility`). Such a site has a `:MatchSite` with a name-level
`matched_path` but **no** `MATCHES_ON` edge — there is no workspace enum to
resolve to. The name-level `matched_path` regex is the only handle, scoped by
the single canonical-site NOT-clause.

### Workspace-enum form — `matchsite-workspace-enum-fence.cypher`

For a fenced type that **is** a workspace enum. Its sites carry a resolved
`(:MatchSite)-[:MATCHES_ON]->(:Item{kind:"enum"})` edge; anchoring on that
edge is **homonym-proof**, because the edge points at the exact workspace
qname. Prefer this form whenever the type is workspace-local.

### Homonym hazard and the v0.1 subset limit

`matched_path` is the pattern-path prefix **as written**, not a resolved type:
`Visibility`, `syn::Visibility`, and `a::b::Visibility` are three distinct
values. A same-named workspace enum matched *unqualified* produces the same
bare `Visibility` prefix as an imported external type — name-level regex alone
cannot tell them apart.

The principled complement for the external-type form would be "…AND this site
has no `MATCHES_ON` edge to a workspace enum". **It is not realized here: `NOT EXISTS` cannot bind outer-scope vars in the
cfdb-query v0.1 subset**: the subset does not bind an outer-scope node inside a
`NOT EXISTS { ... }` subquery (verified empirically —
`examples/queries/vertical-split-brain-drop.cypher:67-73`), so the resolved-edge
absence cannot be written as a correlated anti-join on the match-site node
via `NOT EXISTS`. A correlated `OPTIONAL MATCH` + null-fill test remains
uninvestigated as an alternative; the anchored form below is the proven shape.

Consequences for the fence author:

- **External-type form:** separate the homonym at the *pattern* level — anchor
  `$type_regex` to the QUALIFIED path the external type is written as
  (`^syn::Visibility$`), not a bare `(^|::)Name$`. The live
  `arch-ban-rfc-053-syn-visibility-split-resolution.cypher` does exactly this:
  cfdb's workspace `Visibility` is matched unqualified (prefix `Visibility`)
  while the external one is matched fully qualified (prefix `syn::Visibility`),
  so the `^syn::Visibility$` anchor selects the external sites and none of the
  workspace ones.
- **Shared name with unqualified uses:** the external-type form cannot
  discriminate — use the workspace-enum form, which keys on the resolved qname.

## Documented evasion paths

A fence is honest about what it cannot see. All three are named recall limits,
measured by the 53-A fixture (RFC-053 §4), never silently absorbed:

1. **`match` arms inside `macro_rules!` DEFINITIONS.** Genuinely opaque — the
   syn extractor never parses a macro-definition token tree, so a dispatch
   hidden inside one emits no `:MatchSite`. (Note: `match` inside a macro
   *invocation* body IS extracted via the shared re-parse helper — invocation
   bodies are **not** an evasion path.)
2. **`matches!()` / `assert_matches!` invocations.** Excluded by name in v0
   (recall limit #3). The idiom is common — 26 production `src/` files in
   cfdb's own workspace — so this is a measured in-scope limit, not a corner
   case. Its `<expr>, <pat> [if <guard>]` token grammar fits none of the three
   existing macro re-parse tiers. Upgrade path (RFC-053 §6): a fourth tier via
   the public `syn::Pat::parse_multi_with_leading_vert`, added as a
   `match_visitor`-local wrapper the first time a live fence demonstrably needs
   it.
3. **Single-segment pattern paths under glob imports.** `use Visibility::*;
   match v { Pub => … }` — a bare `Pub` is indistinguishable from a fresh
   binding at the syn level, so it emits no prefix (recall limit #1).

## Placement — enforced fences go in `examples/queries/`

Although RFC-053 §7 and issue #512 say `.cfdb/queries/`, the **enforcing**
globs are all `examples/queries/arch-ban-*.cypher`: the PR-time self-audit
(`.gitea/workflows/ci.yml`), the nightly HIR self-audit
(`ci-hir-self-audit-nightly.yml`), and cross-dogfood (`ci/cross-dogfood.sh`).
`.cfdb/queries/*.cypher` is only SMOKE-executed (parse + run, row count
ignored) — a fence placed there is inert. Ship live fences as
`examples/queries/arch-ban-*.cypher`, matching the sibling
`arch-ban-rfc-043-*.cypher` rules; this needs zero CI-workflow edits.

`:MatchSite` and `MATCHES_ON` are emitted by the **syn** extractor, so these
fences evaluate on the fast PR-time syn keyspace — they are not deferred to
the nightly HIR pass.

## Not every "N implementations" report is a fence target

The W17 audit (#279) listed a `--format` flag "with 3 implementations" as a
UC1 candidate. Re-verified against the current tree for 53-C (the W17 list is
proven to go stale — RFC-053 §1):

- `crates/cfdb-cli/src/main_dispatch.rs` dispatches on the `OutputFormat`
  **enum**; `crates/cfdb-cli/src/commands/diff.rs` and
  `crates/cfdb-cli/src/commands/classify.rs` match a raw `String` `--format`
  value.
- These are **per-command render sites**, not a split resolution point of one
  mapping. The canonical string → enum resolution is already single-owner in
  `OutputFormat::from_str`; the `match format` sites each render a *different*
  command's report. Multiplicity here is the normal survey shape (§3.6), not
  drift.

Verdict: **do not fence `--format`.** A fence would fire on legitimate
per-command dispatch. This is the "re-verify before using a stale audit
instance as a fence target" discipline RFC-053 §7 mandates.
