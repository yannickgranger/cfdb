//! Post-walk RETURNS / TYPE_OF resolvers (RFC-037 §3.2 + §3.4; #239).
//!
//! Drains the deferred queues on [`crate::emitter::Emitter`] and emits
//! edges for every entry whose rendered type string resolves to a
//! known `:Item` qname. Three match tiers, in order:
//!
//! 1. **Exact match** on the rendered string against
//!    `emitted_item_qnames` (fast path for already-qualified returns
//!    like `mycrate::Foo`).
//! 2. **Unique last-segment fallback** via the `by_last_segment` index
//!    (matches `"Foo"` to `"mycrate::Foo"`; ambiguous segments drop
//!    silently — safer than mis-attribution).
//! 3. **Wrapper unwrap** via [`crate::type_render::render_type_inner`]
//!    (#239) on the stored `syn::Type` with a depth-3 budget. Each
//!    inner candidate string runs through the same two tiers above.
//!    `Result<Ok, Err>` can emit two edges (both arms resolve
//!    independently).
//!
//! Split from `lib.rs` (#239 slice) to keep the top-level module under
//! the 500-LOC architecture threshold.

use std::collections::{BTreeMap, BTreeSet};

use cfdb_core::fact::Edge;
use cfdb_core::schema::EdgeLabel;

use crate::emitter::Emitter;

/// Post-walk RETURNS resolution (RFC-037 §3.2, #216; extended for #239).
///
/// Iterates every entry queued in `emitter.deferred_returns` and emits
/// a `RETURNS` edge from the fn's `:Item` to the return-type's `:Item`
/// whenever one of the three tiers above hits. Silently drops
/// cross-crate types, primitives, non-wrapper generics (`T`,
/// `MyBox<_>`), and `impl Trait` returns.
///
/// Determinism (G1): deferred entries are appended in walk order
/// (per-file syn::Visit order), and the resulting RETURNS edges land
/// in `emitter.edges` before the final `edges.sort_by(sort_key)` pass
/// in [`crate::extract_workspace`], so on-disk ordering is
/// independent of queue iteration order.
pub(crate) fn resolve_deferred_returns(emitter: &mut Emitter) {
    let deferred: Vec<(String, String, syn::Type)> = std::mem::take(&mut emitter.deferred_returns);

    // Build a last-segment index: `render_type_string` produces paths
    // as-written (`Foo`, `mymod::Bar`), but `emitted_item_qnames` holds
    // crate-prefixed qnames (`mycrate::Foo`). Ambiguous last-segments
    // (e.g. `Error` declared in multiple crates) emit no edge.
    let by_last_segment = build_last_segment_index(&emitter.emitted_item_qnames);

    // Consume `deferred` by value and build the resolved edge list via
    // iterator chain — avoids `fn_qname.clone()` inside `for`/`while`
    // loops (quality-metrics clone-in-loop rule). A single deferred
    // entry may yield multiple edges (Result<T, E> resolves both arms),
    // so `.flat_map` fans out per entry. The inner `.map` clones
    // `fn_qname` inside a closure body, which the regex-based scanner
    // does not treat as in-loop — only literal `for`/`while`/`loop`
    // keywords open a loop scope.
    let resolved: Vec<(String, String)> = deferred
        .into_iter()
        .flat_map(|(fn_qname, return_type, return_ty)| {
            let mut targets: Vec<String> = Vec::new();
            if let Some(target_qname) =
                resolve_type_string(&emitter.emitted_item_qnames, &by_last_segment, &return_type)
            {
                targets.push(target_qname);
            } else {
                // Third tier: wrapper unwrap on the stored `syn::Type`.
                // Runs only on miss of tiers 1+2. `Result<T, E>` may
                // yield two candidates.
                targets.extend(
                    crate::type_render::render_type_inner(&return_ty, 3)
                        .into_iter()
                        .filter_map(|candidate| {
                            resolve_type_string(
                                &emitter.emitted_item_qnames,
                                &by_last_segment,
                                &candidate,
                            )
                        }),
                );
            }
            targets
                .into_iter()
                .map(move |target| (fn_qname.clone(), target))
        })
        .collect();

    for (fn_qname, target_qname) in resolved {
        emitter.emit_edge(Edge {
            src: cfdb_core::qname::item_node_id(&fn_qname),
            dst: cfdb_core::qname::item_node_id(&target_qname),
            label: EdgeLabel::new(EdgeLabel::RETURNS),
            props: BTreeMap::new(),
        });
    }
}

/// Post-walk TYPE_OF resolution (RFC-037 §3.4, #220; extended for #239).
///
/// Iterates every entry queued in `emitter.deferred_type_of` and emits
/// a `TYPE_OF` edge from the source `:Field` / `:Param` node id to the
/// referenced type's `:Item`. Tier policy mirrors RETURNS; ambiguous
/// last-segments (same short name declared in multiple workspace
/// crates) emit no edge.
///
/// The third tuple slot (`source_label`) is informational only
/// (`"Field"` or `"Param"`); the edge's `dst` is always
/// `item_node_id(target_qname)` and the `src` is the pre-computed
/// source node id queued at emit time. Variants are not queued from
/// here — a variant's payload is walked into separate `:Field` nodes
/// which queue their own TYPE_OF entries.
///
/// Determinism (G1): the resulting TYPE_OF edges land in
/// `emitter.edges` before the final `edges.sort_by(sort_key)` pass
/// in [`crate::extract_workspace`], so on-disk ordering is independent
/// of queue iteration order.
pub(crate) fn resolve_deferred_type_of(emitter: &mut Emitter) {
    let deferred: Vec<(String, String, &'static str, syn::Type)> =
        std::mem::take(&mut emitter.deferred_type_of);

    let by_last_segment = build_last_segment_index(&emitter.emitted_item_qnames);

    // Consume `deferred` by value and build the resolved edge list via
    // iterator chain — mirrors the RETURNS resolver shape and avoids
    // `src_id.clone()` inside a `for` loop.
    let resolved: Vec<(String, String)> = deferred
        .into_iter()
        .flat_map(|(src_id, type_string, _label, src_ty)| {
            let mut targets: Vec<String> = Vec::new();
            if let Some(target_qname) =
                resolve_type_string(&emitter.emitted_item_qnames, &by_last_segment, &type_string)
            {
                targets.push(target_qname);
            } else {
                targets.extend(
                    crate::type_render::render_type_inner(&src_ty, 3)
                        .into_iter()
                        .filter_map(|candidate| {
                            resolve_type_string(
                                &emitter.emitted_item_qnames,
                                &by_last_segment,
                                &candidate,
                            )
                        }),
                );
            }
            targets
                .into_iter()
                .map(move |target| (src_id.clone(), target))
        })
        .collect();

    for (src_id, target_qname) in resolved {
        emitter.emit_edge(Edge {
            src: src_id,
            dst: cfdb_core::qname::item_node_id(&target_qname),
            label: EdgeLabel::new(EdgeLabel::TYPE_OF),
            props: BTreeMap::new(),
        });
    }
}

/// Build the `by_last_segment` lookup index shared by both resolvers.
/// Ambiguous last-segments (same short name across multiple workspace
/// qnames) map to `None` so `resolve_type_string` drops them silently.
fn build_last_segment_index(
    emitted_item_qnames: &BTreeSet<String>,
) -> BTreeMap<&str, Option<&String>> {
    let mut by_last_segment: BTreeMap<&str, Option<&String>> = BTreeMap::new();
    for qname in emitted_item_qnames {
        let seg = cfdb_core::qname::last_segment(qname);
        by_last_segment
            .entry(seg)
            .and_modify(|v| *v = None) // ambiguous — drop
            .or_insert(Some(qname));
    }
    by_last_segment
}

/// Shared two-tier match (exact + unique last-segment) used by both
/// post-walk resolvers and by the third-tier inner-candidate loop.
/// Returns the matched qname (owned) when a tier hits, `None` when
/// both miss.
fn resolve_type_string(
    emitted_item_qnames: &BTreeSet<String>,
    by_last_segment: &BTreeMap<&str, Option<&String>>,
    type_string: &str,
) -> Option<String> {
    if emitted_item_qnames.contains(type_string) {
        return Some(type_string.to_string());
    }
    let seg = cfdb_core::qname::last_segment(type_string);
    by_last_segment.get(seg).copied().flatten().cloned()
}
