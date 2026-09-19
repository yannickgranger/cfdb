# RFC-061 — a rule proves it can fire: `cfdb violations` reads a rule's declared positive control and refuses a rule that returns 0 on it

- Status: **reduced council 2026-09-08 (pre-draft by the lead, three seats, one adversarial round; every sentence below is one the seats converged on).** RATIFIED on merge to doxa `develop` by the operator.
- Refs: cfdb #719 (the measurement and the routing RFC-first under cfdb's `CLAUDE.md` §1); agentry #4659 (six rules at 0 whose control tree declared no autoload root, merged green); `keel-harness#2.2` (a verdict instrument ships positive controls proving each finding class can fire), `keel-harness#3.1`, `keel-harness#3.2`, `keel-harness#6.5`; `agentry-agent-execution-engine#14` (a control proves liveness, never coverage; both are required; a header is never evidence); `cfdb-060-php-fact-model` (the PHP producer the measurement ran under).
- Grounding: measured on `cours-coreen` with cfdb at `ec4ed688`, `--features lang-php`, 2026-09-07 (cfdb #719).

## 1. Problem

`cfdb violations --rule <r>` answers how many rows a rule matched, not whether the rule can match. Rewriting a rule's predicate to `i.name =~ '^NeverMatches.*'` leaves the verb at exit 0 on the tree and exit 0 on the rule's own planted control (cfdb #719): a rule that holds and a rule that cannot match are byte-identical to every consumer. Six rules with no autoload root in their control tree returned 0 each and merged green (agentry #4659). The consumer's arch gate carries the liveness check outside the tool — `agentry-agent-execution-engine#14` records that arch-check requires every rule to flag a planted positive control — so the reading surface a coder is handed in place of a compiler is unfalsifiable at its root and falsifiable only by a script beside it.

## 2. Scope

Ships one refusal on one existing verb, `cfdb violations`, and the reading of one declaration a rule already carries. Ships no new verb and no flag: a rule with no control is refused, never exempted (no allowlist). Ships no coverage claim: a rule firing on its fixture and blind to the vector it claims stays the second half `agentry-agent-execution-engine#14` requires and this RFC does not provide.

## 3. Design

### 3.1 The declaration is a pointer; the run is the evidence

A rule declares its positive control on its first line, `// Positive control: <fixture>`, the data form agentry's `CLAUDE.md` already names; the declaration names the fixture and, where the producer requires one, the extraction context — the autoload root, the manifest — the control keyspace is extracted under. The header is a pointer and never evidence: `agentry-agent-execution-engine#14` rules a citation to a rule's own header inadmissible as evidence, and the evidence here is the verb's run over the control keyspace.

### 3.2 The refusal

`cfdb violations` reads the declaration, extracts the control keyspace from the fixture alone under the declared context — never joined with the live keyspace — runs the rule over it, and refuses, non-zero exit naming the rule and the control, a rule that returns 0 rows on its own control. The refusal is on the run's outcome, never on the header's presence.

### 3.3 The verdict mapping, declared in the manifest entry

`keel-harness#3.1` closes the node verdicts at five and `keel-harness#6.5` has an instrument declare the mapping from its own finding classes in its manifest entry. cfdb declares three: a control that resolves to no file, or to a tree with no root the producer requires, is `malformed` — a declaration exists and the tool cannot determine what it points to; a control keyspace that extracts to zero items is vacuity, a run over a zero-item code root refused top-line (`keel-harness#3.2`); a rule returning 0 rows over a non-empty control is `diverged` — the declaration resolves and the target does not match the claim. No sixth verdict; no amendment of keel is owed. Keel's words are carried as precedent and not as ancestor, in the form `agentry-dreaming-team#7.5` uses for the same borrowing.

### 3.4 The control of the control

The verb's own self-test plants a never-match rule and refuses it; a self-test that does not refuse the plant is the verb's own red. That is the observation `agentry-agent-execution-engine#14` requires before any consumer's enforced table names this refusal.

## 4. Invariants

- No rule reaches a consumer's gate without a declared control that the verb has run (3.2); a rule declaring none is refused as `malformed`.
- A control keyspace is never joined with the live keyspace (3.2).
- The mapping of 3.3 is declared in cfdb's manifest entry and is the only mapping the verb emits.

## 5. Ancestry, stated as an extension

`keel-harness#2.2` places a verdict instrument's positive controls in the instrument's own tree, one fixture per finding class the instrument claims. A ban rule is user-authored data, not one of the instrument's finding classes, so reading a control per rule is an extension of that clause from the instrument's classes to a consumer's rules, declared here as one and never read as a transcription. `agentry-agent-execution-engine#14` supplies the liveness half this RFC moves into the verb.

## 6. Non-goals

Coverage (a rule blind to its vector); a `--no-control` flag or any exemption; the rules' predicates themselves; the extraction context's vocabulary beyond what the producer already requires.
