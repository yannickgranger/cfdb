// arch-ban-reqwest-client-new.cypher — RFC-027 M2 / #3607 ban rule (issue #3637).
//
// Bans raw `reqwest::Client::new()` and `reqwest::ClientBuilder::new()`
// construction outside `qbot-infra-utils::http_client`. Every HTTP client
// in the workspace must flow through `qbot_infra_utils::default_http_client()`
// (or `default_http_client_builder()` for per-site overrides), which seeds
// the qbot default timeouts and User-Agent from a single source of truth.
//
// The scar that drove RFC-027: `adapters/bybit/src/client/session.rs`
// shipped an untimed client and held request slots indefinitely on a
// stuck socket.
//
// # Coverage vs. the handwritten test
//
// The existing Rust test `architecture_3607_no_raw_http_client.rs` matches
// only the fully-qualified text `reqwest::Client::new()`. It misses the
// shortened form:
//
//   use reqwest::Client;
//   let c = Client::new();  // <-- not caught by string match
//
// This cypher rule catches BOTH forms because `callee_path` preserves the
// author-written path: `reqwest::Client::new` (fully-qualified) and
// `Client::new` (post-import) both end in `Client::new`.
//
// # Scope
//
// The rule fires on source files in `crates/adapters/` and `crates/ports*/`
// (the hexagonal outer ring that talks to the network). Scope is matched
// on `cs.file` path, not on `caller.crate`, because qbot-core's crate names
// (`qbot-jupiter`, `qbot-signal-delivery`, `qbot-bybit`, …) do not carry
// a layer prefix in their Cargo `name` field — the layer signal is in the
// directory path.
//
// The `infra-utils` crate is the canonical home of the forbidden pattern
// and is exempted by the `NOT ... 'infra-utils'` clause. Test callers are
// exempted via `is_test=false` on both ends.
//
// Usage:
//   cfdb violations --db <dir> --keyspace <ks> --rule arch-ban-reqwest-client-new.cypher
//
// Expected: empty result on a clean tree. Any row is a violation.

MATCH (caller:Item)-[:INVOKES_AT]->(cs:CallSite)
WHERE cs.file =~ '.*crates/(adapters|ports).*'
  AND NOT cs.file =~ '.*infra-utils.*'
  AND cs.callee_path =~ '^(reqwest::)?(Client|ClientBuilder)::new$'
  AND caller.is_test = false
  AND cs.is_test = false
RETURN caller.qname, caller.crate, cs.file, cs.callee_path
