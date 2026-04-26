# RFC-040 — `:ConstTable` node + const-table-overlap detector

Status: **Ratified (R2, 2026-04-26)** — 4/4 RATIFY w/ NITs: clean-arch, ddd-specialist, solid-architect, rust-systems. R1 verdicts: 4/4 REQUEST CHANGES on convergent B1 (BELONGS_TO live) + B2-solid (ElementType wire-string invariant owner) + B2-rust-systems (sha2 dep claim). All blockers addressed in R2; council re-review confirmed resolution. R2 NITs (3 stale BELONGS_TO prose lines + 1 slice-4 acceptance check) absorbed before ratification.
Parent trace: issue #298 → cfdb-recall scar class "shared literal sets across crates" (qbot-core #3656) → **this RFC**
Companion: orthogonal to RFC-cfdb v1.0 split (EPIC #279), RFC-037 schema-producer alignment, EPIC #266 multi-language. No coupling to in-flight epics.

---

## 1. Problem

cfdb's existing detectors find **type-level** split-brain: `hsb-by-name.cypher` (duplicate `name+kind` across crates), `signature-divergent.cypher` (same name, different `Item.signature`), `arch-ban-*.cypher` (forbidden symbol references). None of them look **inside literal `pub const` arrays**. A real and recurring class of split-brain hides inside such arrays: an adapter ships a hand-edited subset of a canonical literal set declared in a different crate, never derives from the canonical, and silently rots when the canonical changes.

The motivating scar is **qbot-core #3656**:

```rust
// crates/adapters/kraken/src/normalize.rs
pub const Z_PREFIX_CURRENCIES: &[&str] = &["EUR", "USD", "AUD", "CAD", "JPY", "GBP", "CHF"];
```

Every entry was already present in the canonical `FIAT_CURRENCIES: [&str; 25]` exported from `domain-market`. The Kraken adapter never derived from the canonical; when `FIAT_CURRENCIES` was extended (e.g. adding `"NZD"`), the Kraken hardcoded list silently went stale until a user reported wrong currency normalization on a Kraken trade.

The bug shape generalizes across three set-relationships:

- **Subset** — adapter's literal list ⊂ canonical literal list (the Kraken case).
- **Intersection ≥ threshold** — two adapter lists share ≥ 80% of entries (suggests a missed shared canonical).
- **Equal sets** — two crates each declare the same literal list with no shared source (prime de-dup target).

cfdb's `:Item` walker already visits every `pub const` declaration (`crates/cfdb-extractor/src/item_visitor/visits.rs:292` — `visit_item_const`), but emits only the item-level node (`name`, `kind="const"`, `visibility`, `signature`). The expression body — the literal list itself — is never walked. The **structural fact "this `pub const` carries this set of strings"** is therefore not in the graph, and no Cypher rule joining on it can be written.

A historical artifact confirms this is a real category, not a theoretical one. The pre-cfdb forged-baseline tool that audited qbot-core declared a finding category `const_table_overlaps` (visible in `.claude/worktrees/*/.discovery/48.md:146` archaeology) but the category was **never built into cfdb proper**. cfdb has the hooks; it lacks the producer + the rule.

---

## 2. Scope

### Deliverables

1. **`Label::CONST_TABLE` constant** — adds `pub const CONST_TABLE: &'static str = "ConstTable";` to `crates/cfdb-core/src/schema/labels.rs`. Free-form label vocabulary already supports new entries without a release boundary; the constant is the canonical name.
2. **`:ConstTable` node descriptor** — declared in `crates/cfdb-core/src/schema/describe/nodes.rs::const_table_node_descriptor`. Attrs (all required, `Provenance::Extractor`):
   - `qname: string` — fully-qualified name of the declaration (e.g. `kraken::normalize::Z_PREFIX_CURRENCIES`). Round-trip key with the parent `:Item.qname`.
   - `crate: string` — Cargo package name.
   - `module_qpath: string` — module path containing the declaration.
   - `name: string` — the const identifier (last segment of qname).
   - `element_type: string` — wire-typed element type. v0.1 vocabulary: `"str"`, `"u32"`, `"i32"`, `"u64"`, `"i64"`. Future variants reserved by RFC. Closed-set wire enum per RFC-038 §3.1 convention (returns `&'static str`).
   - `entry_count: int` — number of entries in the literal.
   - `entries_hash: string` — sha256 of a canonical entries serialization (sorted ascending, NUL-joined for `str`; sorted ascending, decimal-joined for numeric). Stable across runs (G1).
   - `entries_sample: string` — JSON-array string of the first 8 entries in **declaration order** (NOT sorted) for triage. Capped at 8 to keep node size bounded; `entry_count` carries the true cardinality.
   - `is_test: bool` — `true` when the declaration is inside a `#[cfg(test)]` module (mirrors `:Item.is_test`).
3. **New `HAS_CONST_TABLE` edge label + producer** — adds `pub const HAS_CONST_TABLE: &'static str = "HAS_CONST_TABLE";` to `crates/cfdb-core/src/schema/labels.rs` alongside `HAS_FIELD` / `HAS_VARIANT` / `HAS_PARAM`. Edge descriptor `from: [Item], to: [ConstTable]` declared in `crates/cfdb-core/src/schema/describe/edges.rs`. Each recognized `:Item{kind="const"}` emits exactly one `(:Item) -[:HAS_CONST_TABLE]-> (:ConstTable)` edge. Direction: parent → satellite, matching the established pattern. **R1 council convergent on this fix (4/4 lenses, B1).** R1 originally proposed reusing `EdgeLabel::BELONGS_TO`; that label is in fact live in production with the typed semantic `(:Crate) -[:BELONGS_TO]-> (:Context)` (producer: `crates/cfdb-extractor/src/lib.rs:234`; descriptor: `crates/cfdb-core/src/schema/describe/edges.rs:78-85`; tests: `crates/cfdb-extractor/tests/fixture_extraction.rs:590-604`). Reuse would have created a vocabulary homonym — exactly the split-brain class cfdb exists to detect.
4. **Extractor producer** — `cfdb-extractor::item_visitor::visits::visit_item_const` extends to recognize literal arrays of supported element types and emit one `:ConstTable` node + one `HAS_CONST_TABLE` edge per recognized declaration. Non-recognized const shapes (e.g. `pub const FOO: u32 = 7;`, `pub const BAR: &str = "x";`, `pub const BAZ: HashMap<...> = ...;`) emit only the existing `:Item` node, no `:ConstTable`.
5. **`examples/queries/const-table-overlap.cypher`** — pure-Cypher rule joining `:ConstTable` pairwise within a workspace. Emits one row per overlap finding with verdict ∈ {`CONST_TABLE_DUPLICATE`, `CONST_TABLE_SUBSET`, `CONST_TABLE_INTERSECTION_HIGH`}. Verdict precedence: equality dominates subset dominates intersection (an equal pair is reported as `DUPLICATE`, never as `SUBSET`).
6. **Self-dogfood, cross-dogfood, target-dogfood scars** per cfdb CLAUDE.md §2.5 / RFC-033 §3.5 four-row template (§7).
7. **`SchemaVersion::V0_3_2`** — patch bump for the additive `:ConstTable` node + `HAS_CONST_TABLE` producer + `entries_*` attrs. Lockstep `agency:yg/graph-specs-rust` cross-fixture bump per cfdb CLAUDE.md §3 / RFC-033 §4 I2.

### Non-deliverables

- **No per-entry node graph.** Each entry of the literal is NOT modeled as its own node. `entries_hash` + `entries_sample` is sufficient for triage; per-entry nodes would multiply graph size by the average list cardinality with no triage gain. (Future RFC if a use case emerges.)
- **No semantic normalization of entries.** `&["EUR", "USD"]` and `&["eur", "usd"]` produce different `entries_hash` values. Case folding / Unicode-NFC is out of scope; consumers needing it run their own pass.
- **No HIR feature flag.** This is pure-syn. The detector lands behind the existing syn extractor's compile budget — no `ra-ap-*` transitive dep, no `--features hir`.
- **No re-export tracking.** If a const is re-exported via `pub use other_crate::FOO;`, the extractor anchors on the **declaration site only** — the re-export does not produce a second `:ConstTable`. Verified by recall test: a fixture with `pub use` does not double-count.
- **No inline literals in fn bodies.** `let xs = &["a", "b"];` inside a function is NOT walked. Out of scope; the rule targets **published** tables.
- **No macro-generated constants.** `phf!`, `lazy_static!`, `const_for!`, etc. expand outside `syn::ItemConst`. The syn extractor cannot see them; they require HIR (cfdb v0.2+ HIR extractor) or a specialized `proc_macro2` walker. Out of scope; documented limitation.
- **No `numeric range` shorthand.** `[0..100]` is not a literal list and is excluded.
- **No HashMap/BTreeMap/Set initializers.** Same constructor-call rationale as macros.
- **No detection across keyspaces.** The rule joins `:ConstTable` nodes within a single keyspace. Cross-workspace overlap (e.g. cfdb extracting both `qbot-core` and `agentry` and looking for shared tables across them) is a separate design problem.

---

## 3. Design

### 3.1 `:ConstTable` node — wire vocabulary

```text
:ConstTable {
    qname:               string  required  Extractor   // e.g. "kraken::normalize::Z_PREFIX_CURRENCIES"
    name:                string  required  Extractor   // last segment of qname
    crate:               string  required  Extractor   // Cargo package name
    module_qpath:        string  required  Extractor   // e.g. "kraken::normalize"
    element_type:        string  required  Extractor   // closed-set: "str" | "u32" | "i32" | "u64" | "i64"
    entry_count:         int     required  Extractor   // u64 in serialization, conventional `int` in describer
    entries_hash:        string  required  Extractor   // sha256 hex of canonical-sorted entries
    entries_normalized:  string  required  Extractor   // JSON-array of sorted entries (Option-2 commitment, §3.4)
    entries_sample:      string  required  Extractor   // JSON-array of first 8 entries in declaration order
    is_test:             bool    required  Extractor   // true if inside #[cfg(test)] mod
}
```

**Why `element_type` is a string-typed closed-set wire vocabulary (NOT a public typed enum in `cfdb-core`).** cfdb describer attrs already use `"string"` typing for closed-set wire vocabularies (e.g. `:CallSite.resolver` ∈ `{"syn", "hir"}`). RFC-038 introduced `ContextSource` as a **public typed enum in `cfdb-core`** because consumers across multiple crates need `FromStr` round-trip. The `element_type` consumer pattern is different: rules in `examples/queries/*.cypher` filter on the string equality (`a.element_type = b.element_type`), and the only Rust-side constructor is the producer in `cfdb-extractor`. Adding a public `cfdb-core::ConstElementType` enum would expand cfdb-core's API surface for one private consumer.

The corrected design — per R1 solid-architect B2 — is to keep `ElementType` as a **`pub(crate)` enum inside `cfdb-extractor`** with an `as_wire_str() -> &'static str` method. This makes the producer enum the single owner of the wire string (eliminating producer-side split-brain) without expanding `cfdb-core`'s public API. The wire vocabulary is documented in this descriptor's description string; the no-ratchet rule (§4) enforces that the five variants are not silently expanded.

**`entries_hash` canonical form.** sha256 over the UTF-8 bytes produced by:

1. Sort entries ascending (lexicographic for `str`, numeric for integers).
2. For `str`: join with `\0` (NUL) separator. NUL never appears in a Rust `&str` literal under syn parsing, so it's a safe separator that does not require escaping.
3. For numeric: write each entry in decimal (no leading zeros, no underscores, no thousands separators), join with `\n`.
4. Compute sha256 of the resulting byte sequence; emit lowercase hex.

The sort is purely for hash-stability — the original declaration order is preserved in `entries_sample`. Two consts with the same set but different declaration order produce the same `entries_hash` (correct; they ARE the same set) but different `entries_sample` (visible in triage; reviewer can see the divergent declaration).

**Why `entries_sample` is JSON-array string, not a typed list-attr.** cfdb prop values today are `PropValue::{Str, Int, Bool}` — there is no `PropValue::List`. JSON-array-as-string is the existing convention (the same pattern is used elsewhere where lists need to ride in props). Consumers parse with their JSON tooling; the `entries_hash` is the structural-equality key, the sample is purely human-readable.

### 3.2 `HAS_CONST_TABLE` edge — new label, parent → satellite direction

```text
:Item{kind="const"} -[:HAS_CONST_TABLE]-> :ConstTable
```

`HAS_CONST_TABLE` is a new edge label introduced by RFC-040 alongside `HAS_FIELD`, `HAS_VARIANT`, `HAS_PARAM`. Direction: **parent → satellite**, matching the established `HAS_*` family for satellite-node ownership (`crates/cfdb-core/src/schema/describe/edges.rs:26-44`).

**R1 council resolution (4/4 lenses convergent — B1).** R1 proposed reusing `EdgeLabel::BELONGS_TO` on the false premise that it was unused. In fact `BELONGS_TO` is the live `(:Crate) -[:BELONGS_TO]-> (:Context)` edge — producer at `crates/cfdb-extractor/src/lib.rs:234`, typed descriptor at `crates/cfdb-core/src/schema/describe/edges.rs:78-85` (`from: [Crate]`, `to: [Context]`, description "A Crate belongs to its bounded Context"), tests at `crates/cfdb-extractor/tests/fixture_extraction.rs:590-604`. Reusing it for `(:ConstTable) -[:BELONGS_TO]-> (:Item)` would have:

1. **Created a heterogeneous edge label.** Cypher `MATCH ()-[:BELONGS_TO]->()` would match two distinct (from, to) type pairs with no discriminator — exactly the homonym shape cfdb's `hsb-*` rules exist to detect.
2. **Inverted the satellite direction.** Every existing satellite-of-`:Item` edge (`HAS_FIELD`, `HAS_VARIANT`, `HAS_PARAM`) flows **parent → satellite**. The `BELONGS_TO` Crate→Context direction is a special-purpose domain relationship, not the canonical satellite pattern. Inverting direction for `:ConstTable` would surprise every Cypher author.
3. **Required widening the descriptor's typed `from`/`to` lists.** From `[Crate] -> [Context]` to `[Crate, ConstTable] -> [Context, Item]` — the descriptor stops describing anything specific.

The clean-arch (B1+B2), ddd-specialist (B1), solid-architect (B1), and rust-systems (B1) lenses all converged on the same fix: introduce `HAS_CONST_TABLE`. The decision is unambiguous; no remaining design space.

**Why `HAS_CONST_TABLE` does not violate the no-ratchet rule.** The no-ratchet rule (CLAUDE.md §6 / quality-architecture) targets metric baselines and ceiling files — adding a new typed label constant for a new structural relationship is the canonical correct move. The five existing `HAS_*` constants in `labels.rs` are the precedent; a sixth that follows the same pattern is not vocabulary expansion, it is vocabulary completion.

### 3.3 Producer — extractor changes

**Hook.** Extend `crates/cfdb-extractor/src/item_visitor/visits.rs::visit_item_const` to inspect the literal expression after the existing `emit_item` call:

```rust
fn visit_item_const(&mut self, node: &'ast syn::ItemConst) {
    let name = node.ident.to_string();
    let item_id = self.emit_item(
        &name,
        "const",
        span_line(&node.ident),
        &node.vis,
        &node.attrs,
    );
    // RFC-040 — recognize literal arrays of supported element types.
    if let Some(table) = recognize_const_table(node, &self.crate_name, &self.current_module_qpath()) {
        self.emit_const_table(table, &item_id);
    }
}
```

**Recognition.** A const is a `:ConstTable` candidate iff **both**:

1. `node.ty` is one of: `&[T]`, `&'static [T]`, `[T; N]`, `&[T; N]`, `&'static [T; N]` for supported `T`. The type recognizer matches `Type::Reference { lifetime: _, .. }` regardless of lifetime — `&[T]` parses with `lifetime: None`; `&'static [T]` parses with `lifetime: Some(Lifetime { ident: "static" })`. Both are accepted (R1 rust-systems N1).
2. `node.expr` is a literal array expression `syn::Expr::Reference(... syn::Expr::Array)` or `syn::Expr::Array` with all elements being literals of the matching kind.

Supported `T` in v0.1: `str` (i.e. `&str`), `u32`, `i32`, `u64`, `i64`. Anything else (booleans, custom types, nested arrays) is non-recognized; only the parent `:Item` is emitted.

The recognizer is a pure function in a new module `crates/cfdb-extractor/src/const_table.rs`:

```rust
pub(crate) struct RecognizedConstTable {
    pub qname: String,
    pub name: String,
    pub crate_name: String,
    pub module_qpath: String,
    pub element_type: ElementType,
    pub entries: Vec<EntryValue>,    // declaration order
    pub is_test: bool,
}

pub(crate) enum ElementType { Str, U32, I32, U64, I64 }
pub(crate) enum EntryValue { Str(String), Num(i128) }

impl ElementType {
    /// Wire-string canonical owner (RFC-038 §3.1 invariant-owner pattern).
    /// `emit_const_table` MUST call this rather than constructing the wire
    /// string inline. R1 solid-architect B2 — without this, the producer
    /// has a split-brain shape that `audit-split-brain`'s FromStrBypass
    /// check would flag.
    pub(crate) fn as_wire_str(&self) -> &'static str {
        match self {
            ElementType::Str => "str",
            ElementType::U32 => "u32",
            ElementType::I32 => "i32",
            ElementType::U64 => "u64",
            ElementType::I64 => "i64",
        }
    }
}

pub(crate) fn recognize_const_table(
    node: &syn::ItemConst,
    crate_name: &str,
    module_qpath: &str,
) -> Option<RecognizedConstTable> { ... }
```

`ElementType` lives in `cfdb-extractor` as an internal type — no public API surface in `cfdb-core`. The wire string is the schema commitment; the Rust enum is the producer's bookkeeping. This keeps `cfdb-core` free of new public types while still establishing a single invariant owner for the wire string.

**Emission.** A new method on `ItemVisitor::emit_const_table` (sibling of the existing emitters in `item_visitor.rs`) builds:

- `id = format!("const_table:{qname}")` — distinct from the parent `:Item` id (`item:{qname}`). The two id namespaces are disjoint per the RFC-cfdb §6 id-prefix convention.
- The prop map per §3.1. `props.insert("element_type", PropValue::Str(table.element_type.as_wire_str().to_string()))` — the wire string flows through `ElementType::as_wire_str` only.
- `entries_hash` computed via the canonicalization in §3.1 over the sorted entries. **`sha2` is added as a new direct dep of `cfdb-extractor`** (`sha2.workspace = true` in `crates/cfdb-extractor/Cargo.toml`). The crate is already in the workspace `[dependencies]` table (`Cargo.toml:58` — used by `cfdb-cli`'s persistence layer), so no new crate enters the workspace dep tree; only a new dep line on `cfdb-extractor`. **R1 rust-systems B2** — the original draft incorrectly claimed sha2 was already transitive via `cfdb-cli`; in fact `cfdb-cli` depends on `cfdb-extractor`, not the reverse, so the dep does not propagate upward. The corrected fact is that sha2 is workspace-known and a one-line dep addition.
- One `HAS_CONST_TABLE` edge `item:{qname} -> const_table:{qname}`. The two share the same `qname` segment; consumers can join purely on string equality without traversing the edge if needed.

**Test mod handling.** `is_test` is sourced from `self.is_in_test_mod()`, the same mechanism `:Item.is_test` uses. The default Cypher rule (§3.4) excludes `is_test=true` rows so that test-only fixtures (mock currency lists, fixture data) do not trip the detector. Consumers wanting test-mode coverage opt in explicitly.

**Numeric literal parsing (R1 rust-systems N2).** `EntryValue::Num(i128)` covers the v0.1 supported integer range: `i128::MAX = 2^127 - 1 > u64::MAX`, `i128::MIN = -2^127 < i64::MIN`. `syn::LitInt::base10_parse::<i128>()` strips the type suffix before parsing digits, so `42u64`, `42i64`, and bare `42` all parse correctly. `u64::MAX` written as `18446744073709551615u64` parses to `Ok(18446744073709551615i128)` — within `i128` range. No silent overflow in v0.1.

### 3.4 Detector — `examples/queries/const-table-overlap.cypher`

**R2 decision: Option 2 (ship `entries_normalized` + all three verdicts in v0.1).** R1 Q1 left this as the central design question. The four lenses did not block on either option; solid-architect noted an SAP risk for Option 2 mitigable with an explicit invariant statement (R1 solid N1 — added to §4 in R2). RFC author committed to Option 2 in R1; the council weighing surfaces no rejection. Picking Option 1 would re-open the issue cycle with a half-detector that does NOT reproduce qbot-core #3656 (the motivating scar is a strict subset, not equality). Option 2 catches the scar and matches the motivating issue's `Tests:` block.

**Schema impact (Option 2 commitment).** §3.1 gains one attribute:

```text
entries_normalized:  string  required  Extractor   // sorted-canonical JSON-array of all entries
```

For `str` element types, the JSON array is a `["entry0","entry1",...]` of the **sorted** entries (matching `entries_hash` canonicalization, §3.1). For numeric types, decimal-stringified entries in sorted order: `["1","42","100"]`. The full normalized set lives on the node so the rule can compute set relationships purely in Cypher without re-extracting.

**Size impact.** Empirically negligible on a qbot-core-sized graph (170k+ nodes today). For an average 12-entry `&[&str]` table with 4-char entries, the JSON array is ~80 bytes; for the cfdb self-dogfood corpus (~ 50 const-tables), total inflation is ~ 4 KiB. The `entries_normalized` attribute is the structural-equality data the rule operates on; `entries_hash` remains as the fast-path equality short-circuit.

```cypher
// const-table-overlap.cypher — RFC-040 detector for shared literal sets across crates.
//
// Finds :ConstTable nodes in different crates that:
//   - declare the SAME set (entries_hash equal)            → CONST_TABLE_DUPLICATE
//   - one is a strict SUBSET of the other                  → CONST_TABLE_SUBSET
//   - share ≥ 80% of entries by Jaccard, neither subset     → CONST_TABLE_INTERSECTION_HIGH
//
// Verdict precedence: DUPLICATE > SUBSET > INTERSECTION_HIGH (a duplicate is never
// reported as a subset; a strict subset is never reported as intersection).
//
// Excludes is_test=true tables — test fixtures legitimately repeat literal sets.
//
// Same-crate pairs are excluded — multiple sibling consts in one crate are
// usually intentional (e.g. const X for unit-A, const Y for unit-B in the same module).
//
// SUBSET / INTERSECTION_HIGH operate on `entries_normalized` (JSON-array string of
// the sorted entries) parsed at query time. The Cypher implementation depends on
// the cfdb-query subset / intersection / set-cardinality builtins; if those are
// not yet available at RFC-040 slice-4 ship time, the rule starts with DUPLICATE-
// only and the SUBSET/INTERSECTION_HIGH branches land in a follow-up tied to the
// query builtin landing. (The post-R1 rust-systems lens should confirm the
// cfdb-query builtin coverage at slice-4 implementation time — captured as a
// slice-4 acceptance check, not a council blocker.)

MATCH (a:ConstTable), (b:ConstTable)
WHERE a.qname < b.qname               // pair each unordered pair exactly once
  AND a.crate <> b.crate              // cross-crate only
  AND a.is_test = false
  AND b.is_test = false
  AND a.element_type = b.element_type // type-compatible only
WITH a, b,
     // Fast-path equality via hash; the entries_normalized parse only fires
     // when the hash check is inconclusive.
     (a.entries_hash = b.entries_hash) AS hash_equal
RETURN a.qname AS qname_a,
       b.qname AS qname_b,
       a.crate AS crate_a,
       b.crate AS crate_b,
       a.entry_count AS entry_count_a,
       b.entry_count AS entry_count_b,
       a.entries_sample AS sample_a,
       b.entries_sample AS sample_b,
       CASE
         WHEN hash_equal THEN 'CONST_TABLE_DUPLICATE'
         WHEN entries_subset(a.entries_normalized, b.entries_normalized) THEN 'CONST_TABLE_SUBSET'
         WHEN entries_subset(b.entries_normalized, a.entries_normalized) THEN 'CONST_TABLE_SUBSET'
         WHEN entries_jaccard(a.entries_normalized, b.entries_normalized) >= 0.8 THEN 'CONST_TABLE_INTERSECTION_HIGH'
         ELSE NULL
       END AS verdict
WHERE verdict IS NOT NULL
ORDER BY verdict ASC, qname_a ASC, qname_b ASC
```

**Note on cfdb-query builtins.** `entries_subset(...)` and `entries_jaccard(...)` are cfdb-query string-based set predicates over JSON-array strings. If they are not yet shipped in cfdb-query at slice-4 land time, the rule lands with the DUPLICATE branch only and the SUBSET/INTERSECTION_HIGH branches land in a follow-up. Slice 4's acceptance check verifies builtin availability and adapts the rule shape accordingly.

### 3.5 Why a sibling node, not an `:Item` attr

An alternative shape: emit `:Item.const_entries_hash` and `:Item.const_entries_sample` directly on the existing const `:Item`, no new node label. Rejected for two reasons:

1. **`:Item` already has 24+ attrs across Extractor / EnrichGitHistory / EnrichMetrics / EnrichReachability provenance.** Adding 4 more (entry_count, entries_hash, entries_sample, element_type) widens an already-wide row. cfdb's pattern for "rich payload tied to an item" is to factor out a sibling node — the precedent is `:Field`, `:Variant`, `:Param`, `:CallSite`. `:ConstTable` is exactly that shape.
2. **Future v0.2 may add `:ConstTableEntry` per-entry nodes.** If the RFC chose `:Item` attrs instead, that future expansion would require a new node label PLUS a deprecation of the `:Item` attrs. Sibling-node-from-day-one keeps the migration path clean.

### 3.6 Cross-extractor — what happens on `cfdb-hir-extractor`?

`cfdb-hir-extractor` does not currently walk `pub const` declarations as a producer of facts (it produces `:CallSite`, `:EntryPoint`, `EXPOSES`, `REGISTERS_PARAM`, and HIR-resolved `CALLS`). Adding `:ConstTable` to the syn extractor introduces no homonym risk: the HIR extractor will not produce `:ConstTable` nodes, and a future HIR-side producer (if any — unlikely; HIR adds nothing for literal constant arrays that syn can't already see) would need to either share id-namespace or carry a `resolver` discriminator like `:CallSite`. v0.1 punts; the `:ConstTable.resolver` reservation is a future concern, not a v0.1 gate.

---

## 4. Invariants

- **Determinism / G1 byte-stable.** Two extracts of the same tree produce byte-identical canonical dumps. The `entries_hash` canonicalization (§3.1) is order-independent and platform-independent. The `entries_sample` declaration-order is sourced from `syn::Expr::Array.elems`, which is parse-order-deterministic. The `entries_normalized` JSON-array is sorted, so its bytes are determined by the entry set alone.
- **Recall.** Every `pub const X: ARRAY-OF-T = LITERAL;` declaration where `T` is in the v0.1 supported set MUST emit a `:ConstTable` node. Self-dogfood scar enforces 100% coverage on cfdb's own `pub const` declarations of supported types (counted via `git grep` at extract time; gate set at the actually-observed cfdb count).
- **Cardinality (R1 ddd-specialist N3 — absorbed in R2).** Each `:Item{kind="const"}` whose declaration matches the §3.3 recognizer has **exactly zero or one** `:ConstTable` child node. A parent `:Item` cannot have two `:ConstTable` children. Slice 3's tests assert this uniqueness in addition to the count gate.
- **Identity vs value (R1 ddd-specialist N1 — absorbed in R2).** `:ConstTable` is an **Entity** whose identity is its declaration location (`qname` = fully-qualified path of the parent `:Item`). The `entries_hash`, `entries_normalized`, and `entries_sample` attributes are **value-object snapshots** of the literal entry sequence, embedded as Entity attributes. Two `:ConstTable` Entities with identical `entries_hash` represent the same value but are distinct Entities anchored at distinct declaration locations — this is precisely the structural finding the const-table-overlap rule (§3.4) reports.
- **Wire-string single owner.** `cfdb-extractor::const_table::ElementType::as_wire_str()` is the single owner of the `element_type` wire vocabulary. `emit_const_table` MUST call it; producers MUST NOT construct the wire string inline. `audit-split-brain`'s FromStrBypass check enforces this. (R1 solid-architect B2 — absorbed in R2.)
- **Backward-compat (legacy keyspaces).** Pre-RFC-040 keyspaces have no `:ConstTable` nodes. Readers that consult the new node label MUST handle absence (zero `:ConstTable` nodes is a valid graph). Re-extract is the supported upgrade path. SchemaVersion bumps from `V0_3_1` to `V0_3_2`; readers compiled against `V0_3_1` will refuse `V0_3_2` graphs per G4 — the intended signal, since `V0_3_2` graphs may carry `:ConstTable` rows older readers cannot interpret.
- **Forward-compat (future readers).** Future RFC may add per-entry sub-nodes (`:ConstTableEntry`) or extend `element_type` (e.g. `"f64"`, `"bool"`). v0.1 commits only to the five-variant element-type vocabulary and the four-attr entries payload (`entries_hash`, `entries_normalized`, `entries_sample`, `entry_count`).
- **`entries_normalized` is a permanent v0.1 wire commitment (R1 solid-architect N1 — absorbed in R2).** Once shipped, `entries_normalized` cannot be removed without a SchemaVersion **major** bump. Cypher rules built on it (e.g. `entries_subset`, `entries_jaccard` in §3.4) join on it as a stable wire fact. The Option-2 commitment (§3.4) makes this an explicit SAP boundary: removing `entries_normalized` is a breaking change.
- **Single resolution point.** Only `cfdb-extractor::const_table::recognize_const_table` decides which `syn::ItemConst` nodes become `:ConstTable` nodes. No other code path in the workspace independently classifies. `audit-split-brain`'s usual checks (FromStrBypass etc.) verify no second-resolver emerges.
- **No-ratchet rule (CLAUDE.md §6 / quality-architecture).** The five `element_type` wire variants (`"str"`, `"u32"`, `"i32"`, `"u64"`, `"i64"`) are owned by `ElementType::as_wire_str()` (§3.3). Adding a sixth variant (e.g. `"f32"`, `"f64"`, `"bool"`) is a future-RFC change reviewed against the entire community of consumers. Floating-point in particular requires a separate canonicalization story (NaN, ±0, denormals).
- **Stable Abstractions Principle.** `cfdb-core` gains one new node-label constant (`Label::CONST_TABLE`), one new edge-label constant (`EdgeLabel::HAS_CONST_TABLE`), and two descriptor entries — additive only, no API removal, no type changes. `cfdb-extractor` gains one new module (`const_table.rs`) + emission method + a new direct dep on `sha2`. No port surface affected; `StoreBackend` untouched.
- **Cross-fixture lockstep (RFC-033 §4 I2).** SchemaVersion bump to `V0_3_2` REQUIRES a draft PR on `agency:yg/graph-specs-rust` that bumps `.cfdb/cross-fixture.toml` to the cfdb PR's HEAD SHA, per cfdb CLAUDE.md §3. Merge order: cfdb first, then graph-specs fixture bump within minutes. During the window `graph-specs-rust`'s PR-time cross-dogfood may return exit 20 — the documented brief drift code.

---

## 5. Council review

### 5.1 R1 (2026-04-26) — REQUEST CHANGES (4/4)

All four §2.3 lenses reviewed the R1 draft.

| Lens | Verdict | Primary concern |
|---|---|---|
| clean-arch | REQUEST CHANGES | B1: `BELONGS_TO` is live, not unused (`crates/cfdb-extractor/src/lib.rs:234`); reuse creates vocabulary homonym. B2: directionality inverts the `HAS_*` pattern. |
| ddd-specialist | REQUEST CHANGES | B1: same `BELONGS_TO` collision; descriptor at `crates/cfdb-core/src/schema/describe/edges.rs:78-85` is `[Crate]→[Context]`, would homonym on a second producer with different (from, to). |
| solid-architect | REQUEST CHANGES | B1: same `BELONGS_TO`. B2: `ElementType` enum exists but writes wire string inline → producer-side split-brain (FromStrBypass-shape); no `as_wire_str()` invariant owner. |
| rust-systems | REQUEST CHANGES | B1: same `BELONGS_TO`. B2: `sha2` is NOT transitive to `cfdb-extractor` — `cfdb-cli` depends on `cfdb-extractor`, not the reverse; needs explicit `sha2.workspace = true` on `cfdb-extractor`. |

Three BLOCKING items identified across all four lenses, all addressed in this R2 draft:

| # | Item | R2 resolution |
|---|---|---|
| B1 (4/4 convergent) | `BELONGS_TO` reuse on a live edge label with typed `[Crate]→[Context]` semantic creates a vocabulary homonym | §2 deliverable 3 + §3.2 — replaced with new `HAS_CONST_TABLE` edge label, parent → satellite direction, matching `HAS_FIELD` / `HAS_VARIANT` / `HAS_PARAM`. R1's "BELONGS_TO unused" claim corrected. |
| B2-solid | `ElementType` Rust enum exists but does not own the wire string (split-brain shape) | §3.3 — `ElementType::as_wire_str() -> &'static str` added (RFC-038 §3.1 invariant-owner pattern). `emit_const_table` calls it; no inline string construction. `ElementType` stays `pub(crate)` in `cfdb-extractor`, no new public API in `cfdb-core`. |
| B2-rust-systems | `sha2` claimed transitive but is in fact only in `cfdb-cli` (which depends on `cfdb-extractor`, not vice versa) | §3.3 — corrected. `sha2.workspace = true` added as a new direct dep on `cfdb-extractor/Cargo.toml`; the workspace already carries the version (`Cargo.toml:58`). |

NITs absorbed in R2:

- **clean-arch N1** — `recognize_const_table` placement in `cfdb-extractor::const_table` confirmed correct (Dependency Rule: `syn` types live where `syn` is allowed). No edit needed beyond confirmation.
- **clean-arch N2** — `is_test` denormalization-divergence-risk noted as a future maintenance trap, not a blocking item. Captured in §3.3 emission discussion.
- **clean-arch N3** — RFC-cfdb §7 quote should be verified at ratification — **moot** in R2 since `BELONGS_TO` reuse is dropped entirely.
- **ddd-specialist N1** — Entity-vs-VO framing made explicit in §4 ("`:ConstTable` is an Entity; `entries_*` attributes are value-object snapshots").
- **ddd-specialist N2** — "denormalization" → "value-object snapshot" / "serialized snapshot" terminology corrected in §3.4 and §4.
- **ddd-specialist N3** — inverse cardinality invariant ("zero or one `:ConstTable` per parent `:Item`") added to §4.
- **solid-architect N1** — `entries_normalized` SAP risk addressed via explicit "permanent v0.1 wire commitment, removable only with SchemaVersion major bump" invariant in §4.
- **solid-architect N2** — `is_test` replication tradeoff confirmed sound under ISP (no edit).
- **solid-architect N3** + **N4** — slice ordering and CRP confirmed sound (no edit).
- **rust-systems N1** — `&'static [T]` lifetime-matcher specification added to §3.3 ("`Type::Reference { lifetime: _, .. }` regardless of lifetime").
- **rust-systems N2** — `i128` numeric-range coverage + `syn::LitInt` suffix-stripping behavior documented in §3.3 trailing paragraph.
- **rust-systems N3** — `predicate_schema_refs.rs:50` already references `EdgeLabel::BELONGS_TO`; **moot** in R2 since BELONGS_TO is no longer touched. The new `HAS_CONST_TABLE` will need a parallel test reference in slice 1.

Open questions resolved:

- **Q1** (Option-1 vs Option-2 for §3.4 verdicts) — **Option 2** committed in R2 (§3.4). No lens rejected; solid-architect SAP risk mitigated via the §4 invariant. The qbot-core #3656 reproduction now matches the motivating issue's `Tests:` block.
- **Q2** (BELONGS_TO reuse vs new `HAS_CONST_TABLE`) — **new label** per 4/4 convergent B1 finding (R2 §3.2).
- **Q3** (`entries_sample` cap of 8) — confirmed at 8 in R2 (no lens raised).
- **Q4** (`element_type` extension policy) — follow-up RFC, captured in §4 no-ratchet invariant.
- **Q5** (`is_test` replication vs derivation) — replication chosen per solid-architect N2 confirmation; documented in §3.3 + §4.

### 5.2 R2 (2026-04-26) — RATIFIED

All four §2.3 lenses RATIFY. Per cfdb CLAUDE.md §2.3 the RFC is **ratified**; no override recorded, no dissent.

| Lens | Verdict |
|---|---|
| clean-arch | RATIFY w/ NITs |
| ddd-specialist | RATIFY w/ NITs |
| solid-architect | RATIFY w/ NITs |
| rust-systems | RATIFY w/ NITs |

R2 NITs absorbed before ratification:

- **clean-arch + ddd-specialist + solid-architect + rust-systems convergent NIT (3 stale `BELONGS_TO` prose lines).** The R1 → R2 rewrite of §3.2 left three references to the old edge label in §2 deliverable 4, §2 deliverable 7, and §10 landing trail row 3/5. All three corrected to `HAS_CONST_TABLE` in this revision.
- **solid-architect N2 (slice-4 builtin-availability acceptance check).** Slice 4's `Tests:` block now carries an explicit acceptance row: "verify `entries_subset` and `entries_jaccard` builtins are registered in cfdb-query before the SUBSET / INTERSECTION_HIGH branches are wired; if absent, ship DUPLICATE-only and file a follow-up." Prevents silent rule-shape degradation at slice ship time.

NITs flagged for slice-implementer attention (non-blocking, resolve during slice work):

- **clean-arch N2 (carried).** `:ConstTable.is_test` denormalization-divergence-risk: if a future re-enrichment pass changes `:Item.is_test` without re-emitting `:ConstTable`, the two values can diverge. Low-risk in v0.1 (extractor is the sole producer for both). Noted in §3.3 emission discussion as a future maintenance trap.
- **rust-systems N3 (carried).** `crates/cfdb-query/tests/predicate_schema_refs.rs:50` references `EdgeLabel::BELONGS_TO` today; slice 1 must add a parallel reference to `EdgeLabel::HAS_CONST_TABLE` so the test asserts schema-vocabulary completeness on the new label.

### 5.3 Post-ratification

Per cfdb CLAUDE.md §2.4, the §7 Issue decomposition is now the concrete backlog. Each slice is filed as a forge issue with `Refs: docs/RFC-040-const-table-overlap.md` and the prescribed `Tests:` block, and worked via `/work-issue-lib`. Open questions Q1–Q5 in §8 are all resolved by the R2 council consensus.

---

## 6. Non-goals

Restated from §2 for emphasis.

- No per-entry `:ConstTableEntry` nodes (v0.2 if needed).
- No semantic normalization of entries (case folding, Unicode-NFC, alias resolution).
- No HIR producer (cfdb-hir-extractor untouched).
- No re-export tracking (declaration site only).
- No inline-literal-in-fn-body walking.
- No macro / `lazy_static` / `phf!` recognition.
- No HashMap/BTreeMap/Set initializers.
- No cross-keyspace overlap detection.
- No new typed Rust enum in `cfdb-core` for `element_type` — `ElementType` is `pub(crate)` in `cfdb-extractor`.
- One new edge label (`HAS_CONST_TABLE`) — completes the `HAS_FIELD` / `HAS_VARIANT` / `HAS_PARAM` family for satellite-of-`:Item` ownership.

---

## 7. Issue decomposition (post-ratification)

Vertical slices, each filed with `Refs: docs/RFC-040-const-table-overlap.md` and a prescribed `Tests:` block per cfdb CLAUDE.md §2.5 / RFC-033 §3.5.

R2 commits to **Option 2** (§3.4) — `entries_normalized` is part of the v0.1 schema; all three verdicts (DUPLICATE / SUBSET / INTERSECTION_HIGH) ship in slice 4.

### Slice 1 — `:ConstTable` schema declaration + `Label::CONST_TABLE` + `EdgeLabel::HAS_CONST_TABLE`

Adds the pure schema surface — both label constants, the `:ConstTable` node descriptor, and the `(:Item) -[:HAS_CONST_TABLE]-> (:ConstTable)` edge descriptor. No producer wired yet. Also updates `crates/cfdb-query/tests/predicate_schema_refs.rs` to reference the new edge label (R1 rust-systems N3). Unblocks slice 2.

```
Tests:
  - Unit: schema_describe() output includes :ConstTable with the documented attrs in alphabetical order; round-trip the descriptor through the existing schema-describe JSON test.
  - Self dogfood (cfdb on cfdb): N/A — no runtime behaviour change yet.
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): zero finding delta; only describer output changes.
  - Target dogfood: N/A.
```

### Slice 2 — `recognize_const_table` pure function + unit tests

Lands the pure recognizer in `cfdb-extractor::const_table`. No emitter wiring yet. Slice 3 wires it into the visitor.

```
Tests:
  - Unit: 8-fixture suite covering &[&str], [&str; N], &[&str; N], &'static [&str], &[u32], [u32; N], &[i32], &[u64], &[i64].
  - Unit: negative cases — &[bool], &[(u32, u32)], &[CustomType], non-literal expr (let x = ...; const X: &[&str] = x;), pub const X: u32 = 7; (scalar, not array).
  - Unit: entries_hash determinism — shuffle fixture entries 10x, assert identical hash; decoy NUL handling for str.
  - Unit: i128 numeric range — u64::MAX, i64::MIN, 0 all round-trip.
  - Self dogfood: N/A — no runtime emission yet.
  - Cross dogfood: N/A.
  - Target dogfood: N/A.
```

### Slice 3 — extractor emits `:ConstTable` + `HAS_CONST_TABLE`

Wires `recognize_const_table` into `visit_item_const`. Adds `emit_const_table` method. Adds `sha2.workspace = true` to `crates/cfdb-extractor/Cargo.toml`. First runtime emission of `:ConstTable` nodes.

```
Tests:
  - Unit: extractor on a 4-fixture inline syn workspace produces :ConstTable nodes with correct qnames and exactly one (:Item) -[:HAS_CONST_TABLE]-> (:ConstTable) edge per recognized const. Cardinality assertion: each :Item{kind="const"} has zero or one :ConstTable child (R1 ddd-specialist N3).
  - Unit: emit_const_table writes `element_type` exclusively via `ElementType::as_wire_str()` — verified by an audit-split-brain-style check that no string literal `"str"` / `"u32"` / etc. appears outside the `as_wire_str` match arm in `const_table.rs` (R1 solid-architect B2).
  - Self dogfood (cfdb on cfdb): every `pub const X: &[&str]` and `pub const X: [&str; N]` in cfdb itself produces a :ConstTable node (count gate = the actually-observed count at slice ship time, not a magic number — assert exact equality with a `git grep` count).
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): :ConstTable nodes appear; existing rule rows unchanged (zero delta on the existing rule set; new node label is invisible to existing rules).
  - Target dogfood (qbot-core at pinned SHA): report :ConstTable count in PR body for reviewer sanity-check; expected count is double-digit minimum given the qbot-core surface.
```

### Slice 4 — `examples/queries/const-table-overlap.cypher` + qbot-core #3656 reproduction

Lands the detector rule. Reproduces qbot-core #3656 against a checked-in fixture mirroring the Kraken `Z_PREFIX_CURRENCIES` ⊂ `FIAT_CURRENCIES` shape.

```
Tests:
  - Acceptance (R2 solid-architect N2): verify `entries_subset` and `entries_jaccard` builtins are registered in cfdb-query before the SUBSET / INTERSECTION_HIGH branches are wired. If they are absent at slice ship time, ship DUPLICATE-only and file a follow-up tied to the cfdb-query builtin landing — do NOT silently degrade the rule shape without recording the gap.
  - Unit (Cypher fixture): hand-crafted 6-table fixture with one DUPLICATE pair, one SUBSET pair, one INTERSECTION_HIGH pair, one cross-context legitimate-divergence pair (different element_type) — assert exact verdict counts. (If DUPLICATE-only is shipped per the acceptance check above, fixture asserts only the DUPLICATE pair.)
  - Self dogfood (cfdb on cfdb): zero rule rows on cfdb's own tree (or, if any fire, document them as legitimate AND add to the cfdb concept-overrides whitelist or fix in the same PR per the boy-scout rule).
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): zero rule rows on the companion. Exit 30 on any row blocks merge.
  - Target dogfood (qbot-core at pinned SHA): report the rule-row count in PR body. Expected: at least one row reproducing #3656 (Kraken Z_PREFIX_CURRENCIES vs FIAT_CURRENCIES). If zero, that's a bug in the rule or the fixture pin moved past the original fix — investigate before ship.
```

### Slice 5 — graph-specs-rust cross-fixture lockstep bump

Companion-side PR bumping `.cfdb/cross-fixture.toml` to the merged cfdb HEAD SHA. Per RFC-033 §4 I2 / cfdb CLAUDE.md §3 this is mandatory whenever cfdb bumps SchemaVersion. Merge ordering: cfdb slice 1+2+3+4 merge first; slice 5 follows within minutes.

```
Tests:
  - Cross-fixture exit code check: exit 0 against the new cfdb HEAD; exit 30 if any rule fires on the companion.
  - Self-dogfood on graph-specs-rust's own tree: zero delta.
  - No new tests on the cfdb side — slice 5 is purely the companion bump.
```

Slices land in dependency order: 1 → 2 → 3 → 4 → 5. Slice 1 unblocks 2 (label constant available). Slice 2 unblocks 3 (recognizer importable). Slice 3 unblocks 4 (data available for the rule to query). Slice 4 unblocks 5 (companion needs a stable SHA on develop to pin to).

---

## 8. Open questions (closed in R2)

- **Q1 — Option-1 vs Option-2 for §3.4 verdicts.** **RESOLVED — Option 2** (R2 §3.4). `entries_normalized` ships in v0.1 with permanent-wire-commitment invariant (§4); all three verdicts ship in slice 4.
- **Q2 — `BELONGS_TO` reuse vs new `HAS_CONST_TABLE` label.** **RESOLVED — new `HAS_CONST_TABLE` label** (R2 §3.2). 4/4 lenses convergent on B1 finding that `BELONGS_TO` is live with typed `[Crate]→[Context]` semantic; reuse would create a homonym.
- **Q3 — `entries_sample` cap of 8.** **CLOSED** — confirmed at 8 in R2; no lens raised.
- **Q4 — `element_type` extension policy.** **CLOSED** — future-RFC change reviewed against the entire community of consumers (R2 §4 no-ratchet invariant). Floating-point in particular requires its own canonicalization story.
- **Q5 — `is_test` coupling.** **CLOSED** — replicated as its own attr per solid-architect N2 confirmation; clean-arch N2 maintenance trap noted.

---

## 9. Signals that RFC-040 has succeeded

- Every `pub const X: &[&str]` / `pub const X: [&str; N]` / etc. on every cfdb-extracted tree produces a `:ConstTable` node post-slice-3.
- `examples/queries/const-table-overlap.cypher` reproduces qbot-core #3656 (Kraken `Z_PREFIX_CURRENCIES` ⊂ `FIAT_CURRENCIES`) on the qbot-core target dogfood post-slice-4.
- Self-dogfood scar test asserts 100% recall on cfdb's own `pub const` literal-array declarations.
- `cfdb describe --format json` output shows the new `:ConstTable` descriptor.
- Cross-dogfood on graph-specs-rust at pinned SHA: zero rule-row delta.
- Pre-RFC-040 keyspaces refused by post-RFC-040 readers per G4 (the SchemaVersion bump signal); re-extract is the supported upgrade path.

---

## 10. Landing trail (post-ratification)

To be filled as slices merge.

| Slice | Issue | PR | Commit | Subject |
| --- | --- | --- | --- | --- |
| 1/5 | #323 | TBD | TBD | :ConstTable + HAS_CONST_TABLE schema declaration + Label::CONST_TABLE + EdgeLabel::HAS_CONST_TABLE constants |
| 2/5 | #324 | TBD | TBD | recognize_const_table pure function + ElementType::as_wire_str + unit tests |
| 3/5 | #325 | TBD | TBD | extractor emits :ConstTable + HAS_CONST_TABLE |
| 4/5 | #326 | TBD | TBD | const-table-overlap.cypher + qbot-core #3656 reproduction |
| 5/5 | #327 | TBD | TBD | graph-specs-rust cross-fixture lockstep bump |
