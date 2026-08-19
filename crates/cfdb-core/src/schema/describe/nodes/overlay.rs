use crate::schema::descriptors::{attr, NodeLabelDescriptor, Provenance};
use crate::schema::labels::Label;

pub(in crate::schema::describe) fn concept_node_descriptor() -> NodeLabelDescriptor {
    use Provenance::EnrichConcepts;
    NodeLabelDescriptor {
        label: Label::new(Label::CONCEPT),
        description:
            "An overlay label assigned to items by concept rules (RFC §6.1 — Layer 2 enrichment)."
                .into(),
        attributes: vec![
            attr(
                "assigned_by",
                "enum",
                "Source of the assignment: `doc`, `rule`, `llm`, `manual`.",
                EnrichConcepts,
            ),
            attr(
                "name",
                "string",
                "Concept identifier (e.g. `CanonicalTimeframeResolver`).",
                EnrichConcepts,
            ),
        ],
    }
}

pub(in crate::schema::describe) fn context_node_descriptor() -> NodeLabelDescriptor {
    use Provenance::Extractor;
    NodeLabelDescriptor {
        label: Label::new(Label::CONTEXT),
        description: "A bounded context owning one or more crates (council-cfdb-wiring §B.1.3).".into(),
        attributes: vec![
            attr("canonical_crate", "string?", "Crate nominated as the authoritative owner of this context (if declared in `.cfdb/concepts/<name>.toml`; else empty).", Extractor),
            attr("name", "string", "Context identifier (e.g. `trading`, `strategy`, `cfdb`).", Extractor),
            attr("owning_rfc", "string?", "RFC identifier attached to this context (if declared in override TOML).", Extractor),
            attr(
                "source",
                "string",
                "Provenance discriminator: `\"declared\"` if the context name appears in `.cfdb/concepts/<name>.toml`; `\"heuristic\"` if the name was auto-derived by `cfdb_concepts::compute_bounded_context` via crate-name prefix stripping (RFC-038).",
                Extractor,
            ),
        ],
    }
}

pub(in crate::schema::describe) fn const_table_node_descriptor() -> NodeLabelDescriptor {
    use Provenance::Extractor;
    NodeLabelDescriptor {
        label: Label::new(Label::CONST_TABLE),
        description: "A literal const slice/array recognized as a table of values (RFC-040). Emitted when the extractor recognizes `&[T]`, `&'static [T]`, `[T; N]`, `&[T; N]`, or `&'static [T; N]` over `T ∈ {str, u32, i32, u64, i64}` with all-literal entries. The closed-set `element_type` wire vocabulary {\"str\", \"u32\", \"i32\", \"u64\", \"i64\"} is owned by `cfdb_extractor::const_table::ElementType::as_wire_str` (RFC-038 §3.1 invariant-owner pattern) — no consumer parses it back to a typed enum. SchemaVersion v0.3.2+ (#323 reservation; #325 first emissions). Pre-V0_3_2 keyspaces carry zero `:ConstTable` nodes.".into(),
        attributes: vec![
            attr(
                "crate",
                "string",
                "Containing crate name.",
                Extractor,
            ),
            attr(
                "element_type",
                "string",
                "Closed-set wire vocabulary owned by `cfdb_extractor::const_table::ElementType::as_wire_str`: one of `\"str\"`, `\"u32\"`, `\"i32\"`, `\"u64\"`, `\"i64\"`. Adding a sixth variant requires an RFC bump and a coordinated update to the producer enum (no-ratchet rule, RFC-040 §4).",
                Extractor,
            ),
            attr(
                "entries_hash",
                "string",
                "sha256 hex (lowercase) over the canonical-sorted entries: ascending sort (lexicographic for `str`, numeric for integers), join `str` entries with `\\0` and numeric entries with `\\n` after decimal rendering, then sha256 the resulting bytes. Two consts with the same set produce the same hash regardless of declaration order — this is the structural-equality key for the overlap detector (RFC-040 §3.4).",
                Extractor,
            ),
            attr(
                "entries_normalized",
                "string",
                "JSON array of the canonical-sorted entries in the same order used to compute `entries_hash`. Permanent wire commitment: producers re-emit byte-identical normalization across builds; consumers may rely on `JSON.parse` returning a flat array of either strings or integers (matching `element_type`). Required for downstream rules that need the actual entry set without re-walking the source.",
                Extractor,
            ),
            attr(
                "entries_sample",
                "string",
                "JSON array of the first 8 entries in DECLARATION order (not sorted). Purely human-readable triage aid: reviewers reading a `const-table-overlap` finding see the original literal layout. Two consts with the same set but different declaration order produce identical `entries_hash` and `entries_normalized` but divergent `entries_sample` — the divergence is informational, not a correctness signal.",
                Extractor,
            ),
            attr(
                "entry_count",
                "int",
                "Number of literal entries in the recognized table. Equal to `entries_normalized.len()` after JSON parsing; convenience attribute for queries that filter on table size without parsing the JSON.",
                Extractor,
            ),
            attr(
                "is_test",
                "bool",
                "True when the const is declared inside a `#[cfg(test)]` module, sourced from the same `is_in_test_mod()` heuristic used by `:Item.is_test`. The default `const-table-overlap.cypher` rule excludes `is_test=true` rows so test fixtures (mock currency lists, etc.) do not trip the detector; consumers wanting test-mode coverage opt in explicitly.",
                Extractor,
            ),
            attr(
                "module_qpath",
                "string",
                "Fully-qualified path of the enclosing module (e.g. `kraken::normalize`).",
                Extractor,
            ),
            attr(
                "name",
                "string",
                "Last segment of `qname` — the const identifier.",
                Extractor,
            ),
            attr(
                "qname",
                "string",
                "Fully-qualified name of the parent const item (e.g. `kraken::normalize::Z_PREFIX_CURRENCIES`). Shares its segment with the parent `:Item.qname`; the two are joined structurally via the `HAS_CONST_TABLE` edge and can also be joined on string equality without traversing the edge.",
                Extractor,
            ),
        ],
    }
}

pub(in crate::schema::describe) fn literal_node_descriptor() -> NodeLabelDescriptor {
    use Provenance::Extractor;
    NodeLabelDescriptor {
        label: Label::new(Label::LITERAL),
        description: "A single string literal occurring in production source (`crates/*/src/**/*.rs`), modelled at the `:CallSite` abstraction level (RFC-041). Emitted by the `cfdb-extractor` `syn` walk for `syn::Lit::Str` occurrences; literals inside `#[cfg(test)]` modules / `#[test]` fns are flagged via the inherited `is_test` context (same predicate as `:Item.is_test`, never re-evaluated at the literal site). `value` is the RAW source bytes between the delimiters WITHOUT Rust escape decoding (NOT `syn::LitStr::value()`) so a Cypher `=~` matches what a developer would `grep` for in source (RFC-041 §3.1). Raw strings store inner bytes without the `r`/`#` delimiters; `cfg(feature=)` / `#[serde(default=)]` / `macro_rules!`-body literals are EXCLUDED (RFC-041 §6 — split-brain / syn-opaque). Deliberately NO `kind` attr — a three-way homonym vs `:Item.kind` and `:ConstTable.element_type`; future non-string literals use `lit_syntax` (RFC-041 §3.1, ddd lens). SchemaVersion v0.4.0+ (#369 reservation; #370 first emissions). Pre-V0_4_0 keyspaces carry zero `:Literal` nodes.".into(),
        attributes: vec![
            attr(
                "col",
                "int",
                "1-indexed column of the literal's first delimiter.",
                Extractor,
            ),
            attr("crate", "string", "Containing crate name.", Extractor),
            attr(
                "file",
                "string",
                "Source file relative to workspace root — same normalization as `:Item.file`.",
                Extractor,
            ),
            attr(
                "is_test",
                "bool",
                "True when the literal is inside a `#[cfg(test)]` module or `#[test]` fn. Inherited from the enclosing scope's test context via the exact same parameter-threading `:CallSite` uses (`attrs.rs` cfg/hash-test predicates + `is_in_test_mod` depth counter) — NOT a divergent reimplementation (RFC-041 §4 is_test fidelity invariant).",
                Extractor,
            ),
            attr(
                "line",
                "int",
                "1-indexed line of the literal's first delimiter.",
                Extractor,
            ),
            attr(
                "value",
                "string",
                "Raw source bytes between the delimiting quotes/pounds, WITHOUT Rust escape decoding (NOT `syn::LitStr::value()`). `\\n`/`\\t`/`\\\\` appear verbatim so a Cypher `=~ '\\\\n'` matches a source `\\n` (RFC-041 §3.1 — the `=~`-matches-`grep` invariant). Raw strings (`r#\"...\"#`) store the inner bytes without the `r`/`#` delimiters. Multiline literals store embedded newlines verbatim (documented edge — single-line-anchored `=~` dialects will not span them).",
                Extractor,
            ),
        ],
    }
}

pub(in crate::schema::describe) fn rfc_doc_node_descriptor() -> NodeLabelDescriptor {
    use Provenance::EnrichRfcDocs;
    NodeLabelDescriptor {
        label: Label::new(Label::RFC_DOC),
        description: "An RFC document file (`docs/rfc/*.md`, `.concept-graph/*.md`, etc.) scanned by `enrich_rfc_docs()` for concept-name matches (RFC addendum §A2.2 row 2). Reserved in #43-A; first emissions land in slice 43-D (issue #107) with a SchemaVersion patch bump.".into(),
        attributes: vec![
            attr("path", "string", "Workspace-relative path of the RFC file.", EnrichRfcDocs),
            attr("title", "string?", "First `# ` heading of the file; `None` when the file has no level-1 heading.", EnrichRfcDocs),
        ],
    }
}
