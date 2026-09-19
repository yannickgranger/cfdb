# RFC-062 — `lang-php`: declared shapes (types, supertypes, attributes, defaults, superglobal reads) and the calls `$this` makes

- Status: **reduced council 2026-09-18, three seats: R1 3/3 REQUEST CHANGES on stated fact and shape, no seat asking for a redesign; author fold applied in full; R2 3/3 RATIFY, each seat re-verifying the folded text against the pin.** RATIFIED on merge to doxa `develop` by the operator.
- Refs: cfdb #720 (declared types, `extends`, attributes, constant defaults), cfdb #721 (method calls through a declared-type property, and the closure argument a call site sits in), cfdb #712 (PHP `:CALLS` carries no `resolved`). Amends the non-goals of `cfdb-060-php-fact-model#6` (the `extends` fact, PHP attributes, method-call dispatch resolution) now that a consumer names each. Builds on `cfdb-060-php-fact-model#3.1` (`:Import`, the closed-world record-a-name discipline), `#3.3` (`:Argument`), `cfdb-045` (the PHP producer, `IMPLEMENTS` closed-world). Consumer: `yg/cours-coreen`, under its operator ruling of 2026-09-18 (graph-specs, cfdb and cascade are the only tools allowed to check a call graph or a contamination between contexts; a missing check is built in their repositories).
- Grounding: cfdb `develop` `e438443` (every cfdb path below resolves there); `tree-sitter-php 0.23.11` `node-types.json` for every node kind and field named below; consumer `cours-coreen` at `a9533be28eb7b85c40c18ed9611e6888a57464f0`, 2118 `.php` files under `src/` + `tests/`. Consumer counts below are `git grep` line counts at that SHA, labelled as such; each slice's target dogfood reports the extracted figure against them.

## 1. Problem

The PHP producer emits `:Crate`, `:Module`, `:File`, `:Import`, `:Item`, `:CallSite`, `:Argument` and the edges `IN_CRATE`, `IN_MODULE`, `HAS_IMPORT`, `IMPLEMENTS`, `INVOKES_AT`, `CALLS`, `HAS_ARG`. Nothing in `crates/cfdb-extractor-php` reads a parameter, a property, a return type, a `base_clause`, an attribute, a constant or a default value. `emit_method` (`crates/cfdb-extractor-php/src/lib.rs:276-325`) emits the method `:Item` and walks its body; `walk_declaration_list` (`:259-274`) visits `method_declaration` only, so `property_declaration` and `const_declaration` are never read. `emit_class_like` (`:202-257`) reads `class_interface_clause` and not `base_clause`.

Every `->` call is a `:CallSite` with `resolve_target: None` (`crates/cfdb-extractor-php/src/call_walker.rs:127-134`), so no `$this->…` call ever yields a `CALLS` edge. `visit` (`call_walker.rs:50`) recurses into every child, closures included, and attributes a call inside a closure to the enclosing method with no trace of the closure.

The consumer holds these checks today in five hand-built checkers (1174 lines) the ruling retires. The last column says what this RFC's facts reach of each, and no more: retiring a checker is the consumer's judgment on its own evidence, not this RFC's promise.

| consumer need | missing fact | held today by (lines) | reached by this RFC |
|---|---|---|---|
| a context crossing made through a parameter, property or return type with no `use` line | parameter, property and return declared types, each named arm resolved | `tests/Quality/EveryCrossingIsDeclaredAndExportedTest.php` (422) | the declared-type crossings; a crossing through a local variable or a docblock is not reached (I5) |
| no `float` in the declared types of guarded namespaces | the same, `float` readable as an arm | `tests/Quality/PhpStan/NoFloatIsAdmittedInTheseNamespacesRule.php` (148) | the declared-type half only, as #720 states it |
| no `new` under `Domain/` of a subtype of an I/O class (`SplFileObject`, `PDO`, `Connection`, `HttpClientInterface`, `LoggerInterface`, …) | the declared supertypes of a class, including names outside the workspace | `tests/Quality/PhpStan/NoInputOutputUnderDomainRule.php` (107) | **one arm of three**: the `new` arm. The function-call arm is expressible on today's `:CallSite`; the method call on a PHPStan-inferred receiver type is not reached (I5) |
| no `#[Autowire…]` outside the wiring | an attribute, with its resolved name, on its owner | `tests/Quality/PhpStan/ConfigurationReachesAClassThroughTheWiringRule.php` (134) | fully |
| no class constant, property default or promoted-parameter default carrying an address (`^[a-z][a-z0-9+.-]*://`, or a concatenation headed by one) | the default / value expression of a constant, property or parameter | the same rule | a literal or a literal-headed concatenation; a value reached through constant folding is not (§6) |
| no `$_ENV` / `$_SERVER` read outside the wiring | a superglobal read | the same rule | fully; `getenv()` is already a `:CallSite` |
| every write an Enrolment application service makes goes through its port inside the one `AtomicWrite::inOneWrite` closure (coreen-reservation#10) | `$this->prop->m()` and `$this->m()` resolved to `CALLS`; a call site's enclosing closure argument | `tests/Quality/EveryWriteOfEnrolmentGoesThroughItsPortTest.php` (363, two hand lists of 7 write and ~36 read methods) | writes through `$this` and a declared-type property; a write through a local variable or a closure not passed directly is not reached |

Consumer shapes at the pinned SHA (`git grep` line counts over `src/` + `tests/`): 924 ` extends ` lines; 206 attribute lines (127 under `src/`, of which 84 `#[Route`, 14 `#[AsCommand`, 0 `#[Autowire`); 142 superglobal lines, **all under `tests/`**, none under `src/`; about 2552 promoted-parameter lines and 1909 typed-property lines; 53 `float` in a type position; 6771 `$this->prop->m(` and 10496 `$this->m(`; 138 `inOneWrite(`.

**Zero occurrences is not zero need.** `cfdb-060-php-fact-model#6` declined attributes because no consumer leg asked for one, not because the tree held no `#[Autowire]`. A consumer check now exists and is live (`ConfigurationReachesAClassThroughTheWiringRule`, holding `coreen-domaine-pur#8`), and the ruling moves it into cfdb. A ban fence that returns zero rows on a clean tree is the fence holding, so §2.5 and §2.7 relocate a check that already exists rather than build one for a leg nobody has.

**None of this needs type inference.** Every fact below is declared syntax, or a lookup of a declared type in facts the same extraction emits.

## 2. Scope

### 2.1 Declared types on parameters, properties and returns

`:Param` + `HAS_PARAM` for every parameter of a PHP method or function; `:Field` + `HAS_FIELD` for every property and every promoted constructor parameter; declared-type strings on each, and on the fn `:Item` for its return type; `TYPE_OF` and `RETURNS` for each named arm that resolves to an in-workspace `:Item` (§3.1).

### 2.2 Declared supertypes

An `EXTENDS` edge for a `base_clause` name that resolves in-workspace, and one `:Supertype` node per declared `extends` or `implements` name, owned by its class through `HAS_SUPERTYPE`, carrying the resolved name whether or not it is a node (§3.2).

### 2.3 Receiver resolution and the `resolved` property

`$this->m()` and `$this->prop->m()` resolve through the enclosing class and the declared type of `prop`; every PHP `CALLS` carries `resolved` (§3.3). Closes cfdb #712.

### 2.4 The closure a call site sits in

An `ENCLOSED_BY` edge from a `:CallSite` to the `:Argument` whose expression is the nearest enclosing closure or arrow function (§3.4).

### 2.5 Attributes

`:Attribute` + `HAS_ATTRIBUTE` from the `:Item`, `:Param` or `:Field` it decorates, carrying the attribute's resolved name (§3.5).

### 2.6 Defaults and constants

Class and top-level constants as `:Item` of `kind` `"const"` carrying `value_text`; `default_text` on `:Param` and `:Field` (§3.6).

### 2.7 Superglobal reads

`:GlobalRead` + `READS_GLOBAL` from the enclosing fn `:Item` for every read of a PHP superglobal inside a walked body (§3.7).

### 2.8 Does not ship

Everything in §6. In particular: no type inference, no local-variable typing, no Rust or TypeScript emission change, no `SchemaVersion` bump.

## 3. Design

**Target language level: PHP 8.4.** Every clause reads PHP 8.4 source as `tree-sitter-php 0.23.11` parses it. A property with hooks (`property_hook_list`) is a `:Field` like any other, and each hook body is walked for call sites, superglobal reads and closures with caller `{class qname}::${property}::{get|set}`. Asymmetric visibility (`public private(set)`) changes no fact. `new Foo()->m()` without wrapping parentheses is a construction followed by a member call, and both arms already exist. A typed class constant (`const string URL = …`) carries `type_path` / `type_normalized` on its `:Item` under §3.1's rules.

### 3.1 `:Param`, `:Field`, `TYPE_OF`, `RETURNS`

`:Param`, `:Field`, `HAS_PARAM`, `HAS_FIELD`, `TYPE_OF` and `RETURNS` exist (`crates/cfdb-core/src/schema/labels.rs`; descriptors `crates/cfdb-core/src/schema/describe/edges.rs:23-63,89-95`, nodes in `describe/nodes/structural.rs`). This clause adds a producer, not a vocabulary. Ids through the existing constructors `param_node_id(parent_qname, index)` and `field_node_id(parent_qname, field_name)` (`crates/cfdb-core/src/qname/node_id.rs:83-90`), never hand-built.

**Parameters.** Every child of `formal_parameters` — `simple_parameter`, `variadic_parameter`, `property_promotion_parameter` — is one `:Param`, `index` its zero-based position among those children, `name` the `variable_name` without `$`, `parent_qname` the method or function qname, `is_self` `false` (PHP has no receiver parameter). Edge `HAS_PARAM` from the fn `:Item`.

**Fields.** Every `property_element` of a `property_declaration` is one `:Field` (a declaration `private int $a, $b;` yields two, each carrying the declaration's type), and every `property_promotion_parameter` is **also** a `:Field` — it declares a property and a parameter in one token, and a rule asking either question must find it. `name` without `$`, `parent_qname` the class qname, `index` the zero-based order of declaration in the class body with promoted parameters numbered after the body's properties. Edge `HAS_FIELD` from the class `:Item`.

**Type strings.** Both labels already declare `type_path` (as written) and `type_normalized`. For PHP:

- `type_path` — the bytes of the `type` field as written, absent when the declaration has none.
- `type_normalized` — the arms, flattened in source order and joined by `|`. `optional_type` `?T` contributes the arms `T` and `null`. A `named_type` arm is resolved through `ImportTable::resolve` (`crates/cfdb-extractor-php/src/imports.rs:11`) to its FQN with no leading `\`; `self` and `static` resolve to the enclosing class; `parent` to the enclosing class's resolved `base_clause` name, or `parent` when it has none. A `primitive_type` arm is its lower-cased text. An `intersection_type` arm is its resolved members joined by `&` in source order and parenthesised. A `disjunctive_normal_form_type` (`(A&B)|null`) is its own top-level kind in the pinned grammar, not a `union_type` wrapping an `intersection_type`, and is valid in every position this clause reads: its children are the arms, each `intersection_type` child taking the intersection rule and every other child the rule for its kind. So `?Foo` in `App\X` with no import reads `App\X\Foo|null`, and a rule asks for `float` with `=~ '(^|.*\\|)float(\\|.*)?$'`.

The `|`-joined string is the shape Rust `type_normalized` already is — a scalar, because `PropValue` is scalar-only (`crates/cfdb-core/src/fact.rs:10-16`) — and not the serialized-list shape `cfdb-060-php-fact-model#3.1.3` refused: that refusal was for a variable-length list of independent declarations, where this is one declared type in its normal form. The distinction is the test §3.2 applies to supertypes and fails: several `extends` and `implements` names are several declarations, so they are nodes.

**Return types.** The fn `:Item` gains `return_type_path` and `return_type_normalized`, same rules, from the `return_type` field of `method_declaration` / `function_definition` (a `bottom_type` `never` is a primitive arm). New attributes on a shared label, emitted by the PHP producer alone — the precedent `php_construct` already sets.

**Edges.** `TYPE_OF` from a `:Param` or `:Field`, and `RETURNS` from the fn `:Item`, to the `:Item` of each named arm that resolves to an in-workspace qname — one edge per distinct target, emitted in the resolution pass after every file is walked, as `IMPLEMENTS` already is (`resolve_pending_implements`, `lib.rs:58`). **Closed-world, as PHP `IMPLEMENTS` is:** an arm naming a vendor or builtin type yields no edge and no node; the string carries it. This differs from the Rust producer, whose `synthesize_referenced_items` (`crates/cfdb-extractor/src/synthesize.rs:14`) mints an `:Item` for an unresolved `TYPE_OF` / `RETURNS` target. The `TYPE_OF` and `RETURNS` descriptors state the asymmetry, and a rule that reads the edge on both producers fences on the target's `language`.

The context-crossing need reads `(p)-[:TYPE_OF]->(t:Item)` and compares the owner's and target's namespaces; a fully-qualified `\App\Other\Domain\X` with no `use` resolves exactly like an imported one, which is the case the consumer's text checker exists to catch.

### 3.2 `EXTENDS` and `:Supertype`

`base_clause` is a child of both `class_declaration` (one name) and `interface_declaration` (one or more names); `node-types.json` gives it children `name | qualified_name` and no fields.

- `EXTENDS` — **new edge label**, `:Item` → `:Item`, from the class-like `:Item` to the `:Item` of each `base_clause` name resolving in-workspace, with a `resolver` attribute mirroring `IMPLEMENTS`' (`tree-sitter-php`). Closed-world, same resolution pass. A separate label rather than widening `IMPLEMENTS`, because `IMPLEMENTS`' descriptor says in terms that `extends` is not an `IMPLEMENTS` edge and consumers already rely on that; `IMPLEMENTS|EXTENDS*` is how a rule walks the declared supertype graph.
- `:Supertype` + `HAS_SUPERTYPE` — **new label and edge**, the `:Import` shape of `cfdb-060-php-fact-model#3.1.2` applied to a class's declared supertypes. One `:Supertype` per name in a `base_clause` or a `class_interface_clause`, owned by the class-like `:Item` through `HAS_SUPERTYPE`, id `supertype:{class qname}#{idx}` (idx the zero-based order of declaration, `extends` names first) built by a new `cfdb_core::qname::supertype_node_id`. Attributes: `relation` (`extends` or `implements`), `written` (as written), `fqn` (resolved through `ImportTable::resolve`, no leading `\`, **whether or not a node**), `file`, `line`. This is what reaches **outside** the workspace: `PDO`, `Psr\Log\LoggerInterface` and `Doctrine\DBAL\Connection` are never nodes, and the I/O rule's roots are exactly those names. A rule walks `(c)-[:IMPLEMENTS|EXTENDS*0..]->(s:Item)-[:HAS_SUPERTYPE]->(t:Supertype)` and tests `t.fqn`.

A joined `supertypes` string on the class `:Item` was the first draft and is refused: `extends Foo implements Bar, Baz` is three independent declarations, the variable-length list `cfdb-060-php-fact-model#3.1.3` refused for imports, and a rule over it becomes a regex over a serialized list. The resolved edges and the `:Supertype` nodes are not a duplication either: the edges are the in-workspace relation between two items, the nodes the declarations, one per name, as `HAS_IMPORT` and `:Import` sit beside the resolution `CALLS` uses.

`crates/cfdb-extractor-php/tests/php_implements.rs:97-111` asserts `extends` yields no `IMPLEMENTS`; it stays green and is joined by the `EXTENDS` positive test.

### 3.3 Receiver resolution, and `resolved` on every PHP `CALLS`

`PendingCallSite` gains an owned receiver shape, captured in `visit` while the node is alive (the eager-extraction constraint of `cfdb-060-php-fact-model#3.3`): `This` when a member call's `object` is the `variable_name` `$this`; `ThisProperty(name)` when it is a `member_access_expression` whose `object` is `$this` and whose `name` is a `name`; nothing otherwise. `nullsafe_member_call_expression` is treated as `member_call_expression`.

Resolution runs in the existing pass (`resolve_pending_call_sites`, `lib.rs:59`), where every file's `:Item` and `:Field` are known. **Order is load-bearing:** the `EXTENDS` resolution of §3.2 runs before it, beside `resolve_pending_implements` (`lib.rs:58`), so the chain both steps below walk is complete when call sites resolve; 062-C's inherited-method case goes red on the wrong order.

1. **Owner.** `This` → the enclosing class. `ThisProperty(p)` → the `:Field` `p` of the enclosing class, or of the nearest class up its in-workspace `EXTENDS` chain declaring it; its `type_normalized` must hold **exactly one** arm that is neither `null` nor a primitive, and that arm must resolve to an in-workspace `:Item`, which is the owner. Anything else leaves the call site unresolved.
2. **Method.** The first `:Item` `{C}::m` for `C` the owner, then each class up the owner's in-workspace `EXTENDS` chain, nearest first. An interface owner walks its `EXTENDS` names in source order. None found → unresolved.

`callee_path` stays the method name as written; `resolve_target` and `callee_resolved` carry the outcome; a resolved site yields a `CALLS` edge through the path that exists today (`crates/cfdb-extractor-php/src/emitter.rs:115-123`). A trait's methods are not followed (§6).

**`resolved`.** The PHP producer emits a `CALLS` edge only when the callee resolved, and writes no `resolved` property (cfdb #712), so `examples/queries/arch-ban-rfc-043-calls-edge-unresolved.cypher` (`r.resolved = false`) is silent on PHP rather than satisfied. Every PHP `CALLS` edge carries `resolved: true`. The `resolved` descriptor (`edges.rs:110-113`, "resolved via HIR type inference") is reworded to name what `true` means per producer — HIR type inference for `cfdb-hir-extractor`; resolution to a declared in-workspace item through imports, the enclosing class, or a declared property type for the PHP producer — and that `false` remains the syn textual baseline. **The two `true`s are not the same strength.** HIR's is a type inference; PHP's is a lookup of a declared type in a dynamically typed language, where a declared property type is a promise the runtime enforces on assignment and not a proof of dispatch. The descriptor states the asymmetry and carries the mandate `cfdb-060-php-fact-model` I8 gives `source_text`: a rule reading `resolved` across producers MUST fence on `:CallSite.resolver`.

### 3.4 `ENCLOSED_BY`

**New edge label**, `:CallSite` → `:Argument`: the call site lies lexically inside an `anonymous_function` or `arrow_function` that is the expression of that argument (after the wrapper rules of `cfdb-060-php-fact-model#3.4`), and that closure is the **nearest** function-like ancestor of the call site.

- **Closures only.** `inOneWrite($this->repo->save($x))` evaluates `save` before `inOneWrite` runs; the call is not inside the write. An edge for every argument nesting would put an eager call inside a region it precedes.
- **Nearest only.** A closure nested in a closure yields one edge to the inner argument; the outer is reached through the enclosing `:CallSite`'s own `ENCLOSED_BY`. A closure that is not directly an argument (assigned to a variable, returned) resets: a call site inside it has no edge, even if that closure sits in an outer argument closure, because it may run anywhere.
- **Implementation.** `visit` carries the id of the current enclosing argument, or none. Its recursion today is a uniform per-child loop (`call_walker.rs:87-90`); this clause replaces it with a dispatch at the `arguments` / `argument` boundary that reuses the enumeration `collect_arguments` already does (`call_walker.rs:206-242`), so positions are counted once and identically. It is a rewrite of `visit`, not an added parameter. The `:CallSite` id is computed at visit time (`call_walker.rs:69-84`) and `argument_node_id(callsite_id, position)` is a pure function of it (`crates/cfdb-core/src/qname/node_id.rs:98`), so the target id is known before the subtree is walked; the recursion descends each `argument` with the enclosing id set when its expression is a closure, and descends any other function-like node with it cleared.
- `caller_qname` of a call site inside a closure stays the enclosing method (unchanged); `ENCLOSED_BY` is the only new information.

The consumer's leg then reads: every `CALLS` from an `App\Enrolment\Application` method to a write method of a Domain port has its call site `ENCLOSED_BY` an argument of a call site whose `resolve_target` is `…\AtomicWrite::inOneWrite`, directly or through `$this->m()` calls resolved by §3.3.

### 3.5 `:Attribute` and `HAS_ATTRIBUTE`

**New label and edge.** One `:Attribute` per `attribute` node (inside `attribute_list` → `attribute_group`) on a class-like, method, function, parameter, property or promoted parameter. Id `attr:{owner id}#{idx}`, idx the zero-based order on that owner, built by a new `cfdb_core::qname::attribute_node_id` (the one-owner-per-formula rule of `cfdb-060-php-fact-model#3.1.1`). Attributes: `written` (the name as written), `fqn` (resolved through `ImportTable::resolve`, no leading `\`, whether or not a node — `cfdb-060-php-fact-model#3.1.2`'s rule for `:Import.fqn`), `file`, `line`. `HAS_ATTRIBUTE` from the owner `:Item`, `:Param` or `:Field`. An attribute on a promoted parameter is **two** `:Attribute` nodes, one owned by the `:Param` and one by the `:Field`, each with its own id, for the reason §3.1 makes it both. The attribute's arguments are not modelled (§6).

A property on the owner (a joined `attributes` string) was considered and rejected: an attribute is an independent declaration with its own name, and the list is the variable-length shape `#3.1.3` refused.

### 3.6 Constants, `value_text`, `default_text`

- Every `const_element` of a class-body or top-level `const_declaration` is an `:Item` with `kind` `"const"` — the member Rust `const` items already carry — `qname` `{class qname}::{NAME}` or `{ns}\{NAME}`, `php_construct` `const_declaration`, `IN_CRATE` / `IN_MODULE` as every PHP `:Item`, and `value_text`: the verbatim bytes of its value expression.
- `default_text` on `:Param` and `:Field`: the verbatim bytes of the `default_value` field, absent when none.

Both byte-faithful, the tree-sitter normalisation `cfdb-060-php-fact-model` I8 names; neither is comparable to a Rust attribute. The address rule reads `=~ '^[\'"][a-z][a-z0-9+.-]*://.*'`, which covers a literal and a concatenation headed by one; a value reaching an address through another constant is out of reach without evaluation, and the consumer's rule does not reach it either except through phpstan's constant folding (§6).

### 3.7 `:GlobalRead` and `READS_GLOBAL`

**New label and edge.** Inside a walked body, a `variable_name` whose name is one of the nine superglobals — `GLOBALS`, `_SERVER`, `_GET`, `_POST`, `_FILES`, `_COOKIE`, `_SESSION`, `_REQUEST`, `_ENV` — is one `:GlobalRead`: id `globalread:{caller_qname}:{name}:{idx}` through a new `cfdb_core::qname::global_read_node_id`, attributes `name` (without `$`), `caller_qname`, `file`, `line`; edge `READS_GLOBAL` from the enclosing fn `:Item`. A read and a write both count: a superglobal written outside the wiring is configuration held by a class all the same.

Not a `:CallSite`: a read has no callee, and `callee_path` would gain a second meaning.

## 4. Invariants

- **I1 — Determinism.** Every new id derives from a qname, a workspace-relative path and a source-order ordinal; the resolution pass iterates sorted maps; `produce_facts` sorts nodes and edges (`lib.rs:62-63`). Two extracts of an unchanged tree are byte-identical.
- **I2 — No `SchemaVersion` bump.** New labels `:Supertype`, `:Attribute`, `:GlobalRead`; new edges `EXTENDS`, `HAS_SUPERTYPE`, `ENCLOSED_BY`, `HAS_ATTRIBUTE`, `READS_GLOBAL`; new attributes `return_type_path`, `return_type_normalized`, `value_text`, `default_text`; a new value `true` of an existing attribute on PHP `CALLS`. All additive under cfdb `CLAUDE.md` §5; no `graph-specs-rust` lockstep.
- **I3 — No Rust or TypeScript emission changes.** Descriptor text changes only: `TYPE_OF`, `RETURNS`, `CALLS.resolved`, `:Param`, `:Field`, `:Item.kind`. cfdb's self-extract is byte-identical.
- **I4 — Closed world.** No PHP edge targets a qname that is not an emitted node, and no node is minted for an unresolved name. Names outside the workspace live in the declaration that names them (`type_normalized`, `:Supertype.fqn`, `:Attribute.fqn`), never in stubs.
- **I5 — No inference.** Resolution reads `$this`, the enclosing class, declared property types and the in-workspace `EXTENDS` chain, and nothing else: never a local variable, a parameter's type at a call, a return value, a docblock, or a trait.
- **I6 — Cross-dogfood stays at zero.** `graph-specs-rust` carries no `composer.json`, so `PhpProducer::detect` (`lib.rs:30`) is false there and nothing this RFC adds can reach `ci/cross-dogfood.sh`'s surfaces.
- **I7 — No rule in this lineage fences on `is_test` or on `:Argument.kind = 'other'`.** A closure argument classifies `other`; the §3.4 rule reads `ENCLOSED_BY`, never the kind.

## 5. Architect lenses

R1, 2026-09-18, reduced council of three seats on the pin `e438443`, consumer at `a9533be2`: **`ddd-specialist`, `rust-systems` and `solid-architect` all REQUEST CHANGES**; no seat asked for a redesign. The seats converged by direct messaging before reporting.

- **Domain-driven design** — `EXTENDS` as its own label (the `IMPLEMENTS` descriptor already defers `extends`), `ENCLOSED_BY` (`REFERENCED_BY` is the passive-voice precedent), `:GlobalRead`, `:Attribute.fqn` as `:Import.fqn`'s concept, `value_text` kept apart from `default_text`, `:Item.kind='const'` reuse and the closed-world `TYPE_OF`/`RETURNS` asymmetry (already true of `IMPLEMENTS`) all ratified. Held on: the joined `supertypes` string (blocking, converged with `solid`), and the two strengths of `resolved` under one boolean.
- **Rust systems** — every node kind and field verified against `tree-sitter-php 0.23.11`; `param_node_id`/`field_node_id` shown collision-free for the promoted parameter; `:GlobalRead`'s qname-only id matches `:Item`/`:Field`/`:Param`. Held on: no rule for `disjunctive_normal_form_type` (blocking), the unstated `EXTENDS`-before-call-sites order, `visit`'s rewrite understated, and the promoted parameter's attribute count.
- **SOLID and component principles** — seven slices, each vertical, the §7.8 order genuine, the reuse of `:Param`/`:Field`/`TYPE_OF`/`RETURNS` a real CRP gain, every `Tests:` block compliant with cfdb `CLAUDE.md` §2.5 including the red-first #712 regression. Held on: the §1 table reading as full retirement where one checker is reached on one arm of three, and five slices growing `lib.rs` (451 lines) and `call_walker.rs` (343) with no module named. Its YAGNI challenge to 062-E and 062-G was **withdrawn** on `ddd`'s evidence: `ConfigurationReachesAClassThroughTheWiringRule` is a live check of `coreen-domaine-pur#8` (cours-coreen `CLAUDE.md:232,235`), and zero rows is that fence holding.

**Author fold.** All applied: `supertypes` replaced by `:Supertype` + `HAS_SUPERTYPE` (§3.2); the DNF rule (§3.1); the order and the two strengths of `resolved` with the `resolver` fence (§3.3); `visit`'s rewrite (§3.4); the promoted parameter's two attributes (§3.5); the §1 table's reach column and the zero-is-not-zero-need paragraph; a module named per slice (§7).

**R2**, each seat re-verifying only its own findings against the folded text: `ddd-specialist`, `rust-systems` and `solid-architect` all **RATIFY**, unconditional, no new finding.

## 6. Non-goals

- **Type inference of any kind**: local variables, `$x = new Foo(); $x->m()`, parameter types at the call, fluent returns, `@var` docblocks. `$param->m()` stays unresolved even when `$param` is declared — a candidate for a later round with its own consumer.
- **Trait `use` inside a class body**: methods and properties a trait brings in are not followed by §3.3, and the trait `use` remains the unread construct of `cfdb-060-php-fact-model#6`.
- **Attribute arguments.** `#[Route('/path')]`'s arguments are not `:Argument` nodes; no leg reads them.
- **Constant folding.** A default reaching an address through another constant or `sprintf` is not seen.
- **Receivers other than `$this` and `$this->prop`**: `self::$prop->m()`, `static::…`, a property of a property.
- **Top-level script code**, which no fn body contains, keeps being unwalked, and a superglobal read there yields nothing.

## 7. Issue decomposition

Seven vertical slices. Each names the module its extraction lands in, keeping the crate's module-per-concern layout (`lib.rs:9-13`) rather than growing `lib.rs` (451 lines) and `call_walker.rs` (343): `types.rs` (062-A, shared by 062-F), `supertypes.rs` (062-B), `receiver.rs` (062-C), `attributes.rs` (062-E), `global_reads.rs` (062-G); 062-D rewrites `call_walker.rs::visit` in place. Each lands its target dogfood in its own PR body, updates `FROZEN_NARRATIVE_DIGEST` (`crates/cfdb-core/src/schema/describe/tests.rs`) when a descriptor string moves, and names its `specs/concepts/cfdb-core.md` and `specs/concepts/cfdb-extractor-php.md` entries. The acceptance bar is cfdb `CLAUDE.md` §3 unchanged.

### 7.1 Slice 062-A — declared types (§3.1)

```
Tests:
  - Unit: crates/cfdb-extractor-php/tests/php_declared_types.rs — a class with a promoted `private readonly ?Port $port`, a property `private int|float $a, $b = 2;`, a method `m(self $s, \Vendor\X&Y $i, (A&B)|null $d, string ...$rest): static`. Asserts the :Param set (index, name, type_path, type_normalized — `App\C\Port|null`, `(Vendor\X&App\C\Y)`, `(App\C\A&App\C\B)|null` for the DNF parameter), the promoted parameter as both :Param and :Field, two :Field for `$a, $b` with the shared type, return_type_normalized `App\C\C`, one TYPE_OF per in-workspace arm and none for `Vendor\X` or `float`, and zero minted :Item.
  - Unit (schema): descriptors for the new :Item attributes and the TYPE_OF/RETURNS asymmetry; FROZEN_NARRATIVE_DIGEST moves.
  - Self dogfood (cfdb on cfdb): extract byte-identical, sha256 both sides.
  - Cross dogfood (cfdb on graph-specs-rust at pinned SHA): all arch-ban rules zero, exit 0.
  - Target dogfood (on cours-coreen at a9533be2): :Param, :Field and TYPE_OF counts against the ~2552 promoted and ~1909 typed-property grep lines; the count of :Param/:Field/return types whose type_normalized carries a `float` arm against the 53 grep lines; the count of TYPE_OF edges whose target namespace's second segment differs from the owner's.
```

### 7.2 Slice 062-B — `EXTENDS` and `:Supertype` (§3.2)

```
Tests:
  - Unit: crates/cfdb-extractor-php/tests/php_extends.rs — class extends an in-workspace class (one EXTENDS, resolver tree-sitter-php), class extends `\PDO` (no EXTENDS edge, one :Supertype relation `extends` fqn `PDO`), interface extending two interfaces one vendor (two :Supertype, one EXTENDS), class with extends + implements (idx order: extends first; relation per node); supertype_node_id hoisted in cfdb-core with its unit test. php_implements.rs:97-111 stays green.
  - Self / Cross dogfood: as 062-A.
  - Target dogfood: EXTENDS count against 924 ` extends ` grep lines; the classes under `src/*/Domain/` whose IMPLEMENTS|EXTENDS* closure reaches a :Supertype whose fqn is in the consumer's THINGS_THAT_REACH_OUT list.
```

### 7.3 Slice 062-C — receiver resolution and `resolved` (§3.3), after 062-A and 062-B

```
Tests:
  - Unit: crates/cfdb-extractor-php/tests/php_receiver_resolution.rs — `$this->m()` on own method (CALLS), on a parent's method via EXTENDS (CALLS to parent), on an undeclared method (unresolved); `$this->port->save()` with promoted `Port $port` (CALLS to `Port::save` on an interface item), with `?Port` (resolved, null arm ignored), with `Port|Other` (unresolved), with a vendor-typed property (unresolved); `$local->m()` (unresolved, I5). Every PHP CALLS carries resolved=true.
  - Regression (red first): a PHP fixture where the arch-ban-rfc-043-calls-edge-unresolved rule is shown reaching PHP edges — a CALLS edge with the property absent is the red state (#712).
  - Self / Cross dogfood: as 062-A.
  - Target dogfood: CALLS count before and after; the share of the 6771 `$this->prop->m(` and 10496 `$this->m(` grep lines now resolved; unresolved residue by cause (union, vendor type, trait, inherited-from-vendor).
```

### 7.4 Slice 062-D — `ENCLOSED_BY` (§3.4), independent

```
Tests:
  - Unit: crates/cfdb-extractor-php/tests/php_enclosed_by.rs — `w->inOneWrite(function () { $this->a(); })` (edge to argument #1), `fn () => $this->a()` (same), `w->inOneWrite($this->a())` (no edge, eager), nested closure in closure (edge to inner only), closure assigned to a variable inside an argument closure (no edge), call site's caller_qname unchanged.
  - Self / Cross dogfood: as 062-A.
  - Target dogfood: ENCLOSED_BY edges whose target argument belongs to an `inOneWrite` call site against 138 `inOneWrite(` grep lines; with 062-C landed, the rows the coreen-reservation#10 query returns on the pin against the verdict of EveryWriteOfEnrolmentGoesThroughItsPortTest.
```

### 7.5 Slice 062-E — attributes (§3.5)

```
Tests:
  - Unit: crates/cfdb-extractor-php/tests/php_attributes.rs — `use Symfony\...\Autowire; #[Autowire('%x%')]` on a promoted parameter (fqn resolved, owner :Param and :Field), a grouped `#[A, B]` (two, idx 0/1), attributes on class, method, property; attribute_node_id hoisted in cfdb-core with its unit test.
  - Self / Cross dogfood: as 062-A.
  - Target dogfood: :Attribute count against 206 attribute lines; count by fqn tail (84 Route, 14 AsCommand expected); 0 Autowire.
```

### 7.6 Slice 062-F — constants and defaults (§3.6), after 062-A

```
Tests:
  - Unit: crates/cfdb-extractor-php/tests/php_defaults.rs — class const `URL = 'https://x'`, top-level `const K = 1;`, property default `'redis://' . $h` shape via constant concatenation, promoted parameter default, a parameter with no default (attribute absent); value_text and default_text byte-exact including quotes.
  - Self / Cross dogfood: as 062-A.
  - Target dogfood: const :Item count; count of value_text/default_text matching the address pattern under `src/` outside the wiring.
```

### 7.7 Slice 062-G — superglobal reads (§3.7)

```
Tests:
  - Unit: crates/cfdb-extractor-php/tests/php_global_reads.rs — `$_ENV['A']`, `$_SERVER['B'] ?? 'x'`, a write `$_SESSION['k'] = 1`, two reads of one name in one method (idx 0/1), `$env` (not a superglobal, nothing); global_read_node_id hoisted with its unit test.
  - Self / Cross dogfood: as 062-A.
  - Target dogfood: :GlobalRead count against 142 grep lines, all expected under tests/; 0 under src/.
```

### 7.8 Ordering

062-A, 062-B, 062-D, 062-E, 062-G stand alone; 062-C needs 062-A (property types) and 062-B (the chain); 062-F needs 062-A (`:Param` / `:Field` to carry `default_text`).
