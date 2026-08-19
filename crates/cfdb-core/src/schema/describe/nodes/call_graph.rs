use crate::schema::descriptors::{attr, NodeLabelDescriptor, Provenance};
use crate::schema::labels::Label;

pub(in crate::schema::describe) fn call_site_node_descriptor() -> NodeLabelDescriptor {
    use Provenance::Extractor;
    NodeLabelDescriptor {
        label: Label::new(Label::CALL_SITE),
        description: "One concrete call expression in the source tree (caller → callee, file:line).".into(),
        attributes: vec![
            attr("arg_count", "int", "Number of arguments at the call site.", Extractor),
            attr("callee_path", "string", "Best-effort path of the callee (may be unresolved).", Extractor),
            attr("callee_resolved", "bool", "`true` when method dispatch / re-export / trait impl was resolved via HIR; `false` for textual-only syn-based extraction. SchemaVersion v0.1.3+ only. See Label::CALL_SITE discriminator contract. RFC-043: post-RFC-043 the predicate's epistemic precision improved — proc-macro-touched receivers (`#[async_trait]`, `#[derive(Builder)]`, `#[tokio::test]`, cucumber steps) can now resolve to `true` when the sysroot ships `rust-analyzer-proc-macro-srv`. There is no per-keyspace status flag (by design); consumers wishing to disambiguate pre/post-RFC-043 keyspaces must re-extract. The silent probe fallback (RFC-043 §3.3 case 1) produces a keyspace indistinguishable from `--no-proc-macro` — two keyspaces with identical `callee_resolved` distributions may have different recall depending on whether the sysroot had the binary at extract time.", Extractor),
            attr("caller_qname", "string", "Qualified name of the fn that contains this call. Homonym note (RFC-054): the bare DISPLAY qname — same-spelling callers in different cargo targets share this value while their `:CallSite` node ids stay distinct (the id embeds the caller's target-scoped identity).", Extractor),
            attr("file", "string", "Source file relative to workspace root.", Extractor),
            attr("is_test", "bool", "True when the enclosing item is under `#[cfg(test)]` or in `tests/`.", Extractor),
            attr("kind", "enum", "CallSite shape: `call` (ExprCall/MethodCall), `fn_ptr` (path passed as fn-pointer arg), `serde_default` (`#[serde(default = \"...\")]`).", Extractor),
            attr("line", "int", "1-based line number.", Extractor),
            attr("resolver", "enum", "Which extractor produced this node: `syn` (cfdb-extractor, unresolved name-based), `hir` (cfdb-hir-extractor, HIR-resolved), `tree-sitter-php` (cfdb-extractor-php, RFC-045 45-C — syntactic in-workspace resolution), or `tree-sitter-typescript` (cfdb-extractor-ts, RFC-045 45-D — no call resolution, every callee_resolved=false). SchemaVersion v0.1.3+ only (the `tree-sitter-*` values are additive — no version bump). See Label::CALL_SITE discriminator contract.", Extractor),
        ],
    }
}

pub(in crate::schema::describe) fn entry_point_node_descriptor() -> NodeLabelDescriptor {
    use Provenance::Extractor;
    NodeLabelDescriptor {
        label: Label::new(Label::ENTRY_POINT),
        description: "A top-level entry into the system — MCP tool, CLI command, HTTP route, or cron registration. First populated in SchemaVersion v0.2.0 (Issue #86) by `cfdb-hir-extractor::extract_entry_points`. v0.1.x graphs have no :EntryPoint nodes.".into(),
        attributes: vec![
            attr("file", "string", "Source file path where the entry-point declaration lives (relative to workspace root, or absolute).", Extractor),
            attr("handler_qname", "string", "Qualified name of the handler item (fn / struct / enum) this entry point dispatches to.", Extractor),
            attr("kind", "enum", "Entry-point kind: `mcp_tool`, `cli_command`, `http_route`, `cron_job`, `websocket`, `test`, `bench`. v0.2.0 MVP detects `cli_command` (clap `#[derive(Parser/Subcommand)]`) and `mcp_tool` (`#[tool]`); HTTP / cron / websocket kinds added later via call-site detection. `test` / `bench` (RFC-042) detect `#[test]`, `#[tokio::test]`, `#[given]`/`#[when]`/`#[then]` (cucumber BDD), `#[bench]` attributes plus FNs in `tests/` / `benches/` directories. BDD step attributes classify as `test`. NOTE: `kind=\"test\"` on `:EntryPoint` is ORTHOGONAL to `:Item.is_test`. The former classifies the entry surface (this fn is an invocation root for the test runner). The latter classifies the item's compile scope (this item lives under `#[cfg(test)]`). A query that needs items reachable only from test entry points should match on `:EntryPoint{kind:\"test\"}`-reachability via the `:Item.reachable_from_production_entry` attribute (RFC-042 slice 042-B), NOT on `:Item.is_test=true`.", Extractor),
            attr("name", "string", "Public-facing name (tool name, CLI subcommand, route path, cron job id).", Extractor),
            attr("params", "json", "Registered parameters as a JSON array of `{name, type}` objects. v0.2.0 MVP emits `[]`; clap arg + MCP tool input-schema enrichment deferred to follow-up.", Extractor),
        ],
    }
}

pub(in crate::schema::describe) fn argument_node_descriptor() -> NodeLabelDescriptor {
    use Provenance::Extractor;
    NodeLabelDescriptor {
        label: Label::new(Label::ARGUMENT),
        description: "A positional argument at a call site (RFC-043 Slice A). \
            Emitted by both syn-extractor and HIR-extractor for every `ExprCall` \
            and `ExprMethodCall` they visit. For `ExprMethodCall`, position 0 is \
            the implicit `self` receiver; for `ExprCall`, position 0 is the first \
            positional argument. Connected from its owning `:CallSite` via \
            `[:HAS_ARG]`. Node id is `arg:{callsite_id}#{position}` (derived via \
            `cfdb_core::qname::argument_node_id`). Cypher ban-rules MUST NOT use \
            `kind='other'` as a normative fence filter (RFC-043 §4 Invariant 10). \
            SchemaVersion V0_5_0+; pre-V0_5_0 keyspaces carry zero `:Argument` nodes."
            .into(),
        attributes: vec![
            attr(
                "col",
                "int",
                "1-indexed column of the argument expression's first token.",
                Extractor,
            ),
            attr(
                "file",
                "string",
                "Source file relative to workspace root.",
                Extractor,
            ),
            attr(
                "kind",
                "string",
                "Coarse syntactic classification: `\"path\"` (identifier or path \
                 expression), `\"method_call\"` (e.g. `x.clone()`), `\"call\"` \
                 (free-fn invocation), `\"ref\"` (borrow expression), `\"literal\"` \
                 (a literal value), `\"other\"` (any other expression variant). \
                 Closed set; future variants additive. Cypher ban-rules MUST NOT \
                 use `kind='other'` as a normative fence filter — it signals \
                 extractor ignorance, not a domain category (RFC-043 §4 Invariant 10).",
                Extractor,
            ),
            attr(
                "line",
                "int",
                "1-indexed line of the argument expression's first token.",
                Extractor,
            ),
            attr(
                "position",
                "int",
                "Zero-indexed position in the call expression's argument list. \
                 For `ExprMethodCall`, position 0 is the implicit `self` receiver; \
                 for `ExprCall`, position 0 is the first positional argument. \
                 Reference the receiver position by name via \
                 `cfdb_core::schema::labels::RECEIVER_POSITION` (= 0).",
                Extractor,
            ),
            attr(
                "source_text",
                "string",
                "Verbatim source text of the argument expression, produced by \
                 `proc-macro2` token-stream `to_string()` (deterministic for a \
                 given syn AST). Cypher rules MAY match on `source_text` with \
                 `=~`; consumers SHOULD prefer `kind` for coarse classification \
                 (RFC-043 §3.1).",
                Extractor,
            ),
        ],
    }
}

pub(in crate::schema::describe) fn match_site_node_descriptor() -> NodeLabelDescriptor {
    use Provenance::Extractor;
    NodeLabelDescriptor {
        label: Label::new(Label::MATCH_SITE),
        description: "A single `match` expression keyed per distinct name-level \
            matched-path prefix (RFC-053). Emitted by the `cfdb-extractor` \
            `match_visitor` as a third independent per-fn-body visitor pass \
            (alongside :CallSite and :Literal): one node per (`match` expression, \
            distinct arm-pattern-path prefix). `matched_path` is the \
            all-but-last-segment prefix of a multi-segment arm-pattern path AS \
            THE AUTHOR WROTE IT — name-level and UNRESOLVED, same doctrine as \
            `:CallSite.callee_path` (`Visibility`, `syn::Visibility`, \
            `cfdb_core::visibility::Visibility` are three distinct values for the \
            same type). Single-segment pattern paths (indistinguishable from \
            bindings under glob imports) and literal-scrutinee matches emit no \
            node (named recall limits). The resolved \
            `(:MatchSite)-[:MATCHES_ON]->(:Item{kind:\"enum\"})` edge lands in \
            slice 53-B; an external-type prefix (e.g. `syn::Visibility`) keeps \
            its :MatchSite with no MATCHES_ON — the honest name-level-only \
            representation. Node id is the extractor-local \
            `matchsite:{fn_qname}:{prefix}:{local_idx}` (RFC-032 §3 keeps site-id \
            schemes out of core). SchemaVersion V0_7_0+; pre-V0_7_0 keyspaces \
            carry zero :MatchSite nodes."
            .into(),
        attributes: vec![
            attr(
                "arm_count",
                "int",
                "Number of arms of the enclosing `match` expression.",
                Extractor,
            ),
            attr("crate", "string", "Owning crate name.", Extractor),
            attr(
                "file",
                "string",
                "Source file relative to workspace root.",
                Extractor,
            ),
            attr(
                "is_test",
                "bool",
                "True when the enclosing fn is under `#[cfg(test)]` or carries a \
                 bare `#[test]`. Threaded from the enclosing scope's test context \
                 (the same predicate `:CallSite` / `:Literal` use), never \
                 re-evaluated at the match site (RFC-041 §4 fidelity invariant).",
                Extractor,
            ),
            attr(
                "line",
                "int",
                "1-indexed line of the `match` expression start (the `match` \
                 keyword).",
                Extractor,
            ),
            attr(
                "matched_path",
                "string",
                "Name-level, UNRESOLVED all-but-last-segment prefix of a \
                 multi-segment arm-pattern path, as the author wrote it — a \
                 syntactic pattern-path prefix, NOT the scrutinee's resolved type \
                 (that concept, `matched_type`, is reserved for a future HIR \
                 tier). Same doctrine as `:CallSite.callee_path`: `Visibility`, \
                 `syn::Visibility`, `cfdb_core::visibility::Visibility` are three \
                 distinct values for the same type.",
                Extractor,
            ),
            attr(
                "wildcard",
                "bool",
                "True iff the `match` has a wildcard arm — RFC-044 §3.7's \
                 vocabulary. Heuristic (syn does no name resolution): a top-level \
                 `_` (`Pat::Wild`) OR a bare lowercase-leading identifier binding \
                 with no sub-pattern. Covers BOTH forms — the literal `_` and \
                 lowercase catch-all bindings (a documented named recall limit).",
                Extractor,
            ),
        ],
    }
}
