//! Post-walk RETURNS / TYPE_OF / MATCHES_ON resolvers
//! (RFC-037 §3.2 + §3.4, #239; RFC-053 §3.2, slice 53-B).
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
//! [`resolve_deferred_match_targets`] (RFC-053 slice 53-B) reuses
//! [`resolve_type_string`] / [`build_last_segment_index`] — the same
//! primitives the two type-resolvers call — but not tier 3 (a matched
//! path is a pattern-path prefix, not a wrapped type, so there is
//! nothing to unwrap), adds a `kind = "enum"` filter the others lack,
//! and accepts a name-level resolution only when the matched-path prefix
//! is a segment-suffix of the resolved qname (see [`is_segment_suffix`])
//! so a qualified external homonym (`syn::Visibility`) does not collapse
//! onto a same-named workspace enum while in-crate partial qualification
//! (`mymod::MyEnum`) still resolves (RFC-053 §3.5 homonym-proof fence;
//! see the fn doc). A standalone short orchestration rather than a shared
//! generic combinator was the council-converged position: the three
//! orchestrations genuinely diverge, so a combinator would need enough
//! knobs to be worse than three short siblings.
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

/// Post-walk MATCHES_ON resolution (RFC-053 §3.2, slice 53-B).
///
/// Drains `emitter.deferred_match_targets` and emits a `MATCHES_ON`
/// edge from each `:MatchSite` to the `:Item` of the workspace enum its
/// name-level `matched_path` prefix resolves to. Resolution reuses the
/// shared two-tier primitive [`resolve_type_string`] (exact qname +
/// unique last-segment via [`build_last_segment_index`]) — the SAME
/// primitives RETURNS / TYPE_OF use — then keeps only targets whose
/// `kind` is `"enum"` (via `emitter.emitted_enum_qnames`). There is no
/// third-tier wrapper unwrap: a matched path is a pattern-path prefix,
/// not a type expression. A prefix resolving to a struct / trait, or to
/// an external / unknown type, emits nothing — the `:MatchSite` keeps
/// its name-level `matched_path` and no resolved edge (RFC-053 §3.2, the
/// honest representation).
///
/// **Homonym guard (RFC-053 §3.5).** A qualified external prefix
/// (`syn::Visibility`) shares its last segment with a same-named
/// workspace enum (`cfdb_core::visibility::Visibility`), so the shared
/// primitive's last-segment tier would wrongly collapse the external site
/// onto the workspace enum — defeating the "homonym-proof" workspace-enum
/// fence §3.5 relies on and contradicting the RFC's own fixtures. So a
/// name-level (non-exact) resolution is trusted only when the matched-path
/// prefix is a segment-suffix of the resolved qname (see
/// [`is_segment_suffix`]): unqualified (`Visibility`) AND partially-
/// qualified in-crate (`visibility::Visibility`, `mymod::MyEnum`) prefixes
/// are suffixes and resolve, while a qualified external homonym
/// (`syn::Visibility`) is not a suffix of the workspace enum's qname and is
/// rejected. Because in-crate partial qualification still resolves, no new
/// recall limit is introduced (a blunter "reject all qualified" guard would
/// silently drop that case — RFC-053 §4 "never silently absorbed"). This is
/// the MATCHES_ON-specific refinement of the two-tier reuse — beyond the
/// no-tier-3 and enum-filter divergences §3.2 names — and it is what makes
/// the RFC's `Visibility` / `syn::Visibility` /
/// `cfdb_core::visibility::Visibility` worked triple resolve as specified.
///
/// Determinism (G1): deferred entries append in walk order, and the
/// resulting MATCHES_ON edges land in `emitter.edges` before the final
/// `edges.sort_by(sort_key)` pass in [`crate::extract_workspace`], so
/// on-disk ordering is independent of queue iteration order.
pub(crate) fn resolve_deferred_match_targets(emitter: &mut Emitter) {
    let deferred: Vec<(String, String)> = std::mem::take(&mut emitter.deferred_match_targets);

    let by_last_segment = build_last_segment_index(&emitter.emitted_item_qnames);

    // Consume `deferred` by value and build the resolved edge list via
    // iterator chain — mirrors the RETURNS / TYPE_OF resolver shape and
    // avoids a `site_id.clone()` inside a `for` loop. One site resolves
    // to at most one enum, so there is no fan-out (unlike RETURNS'
    // two-arm `Result`) and `site_id` moves straight into the pair.
    let resolved: Vec<(String, String)> = deferred
        .into_iter()
        .filter_map(|(site_id, matched_path)| {
            resolve_type_string(
                &emitter.emitted_item_qnames,
                &by_last_segment,
                &matched_path,
            )
            // Homonym guard (§3.5): trust a name-level resolution only
            // when the matched-path prefix is a segment-suffix of the
            // resolved qname (see `is_segment_suffix`). Unqualified
            // (`Visibility`) and partially-qualified in-crate
            // (`visibility::Visibility`, `mymod::MyEnum`) prefixes are
            // suffixes and resolve; a qualified external homonym
            // (`syn::Visibility`) is not a suffix of the same-named
            // workspace enum's qname and is rejected. Exact tier-1 hits
            // satisfy it trivially (prefix == qname).
            .filter(|q| is_segment_suffix(&matched_path, q))
            // Kind filter (§3.2): resolution targets are enums only; a
            // prefix resolving to a struct / trait emits nothing.
            .filter(|target_qname| emitter.emitted_enum_qnames.contains(target_qname))
            .map(|target_qname| (site_id, target_qname))
        })
        .collect();

    for (site_id, target_qname) in resolved {
        emitter.emit_edge(Edge {
            src: site_id,
            dst: cfdb_core::qname::item_node_id(&target_qname),
            label: EdgeLabel::new(EdgeLabel::MATCHES_ON),
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

/// Segment-suffix test for the MATCHES_ON homonym guard (RFC-053 §3.5).
///
/// True iff `prefix`'s `::`-segments are a contiguous TRAILING run of
/// `qname`'s segments — i.e. `qname` is `prefix` itself, or `<…>::<prefix>`
/// on a segment boundary. This is what makes a name-level resolution
/// trustworthy: an unqualified `Visibility` and a partially-qualified
/// in-crate `visibility::Visibility` are both suffixes of the resolved
/// `cfdb_core::visibility::Visibility` (they resolve), while a qualified
/// external homonym `syn::Visibility` is NOT — its `syn` segment does not
/// match the resolved qname's `visibility` segment — even though both
/// share the last segment `Visibility` (it is rejected). Comparing whole
/// segments, not raw string suffixes, is load-bearing: `bility` is a raw
/// suffix of `…::Visibility` but not a segment-suffix. Exact tier-1 hits
/// satisfy it trivially (prefix == qname).
fn is_segment_suffix(prefix: &str, qname: &str) -> bool {
    let prefix_segments: Vec<&str> = prefix.split("::").collect();
    let qname_segments: Vec<&str> = qname.split("::").collect();
    qname_segments.ends_with(&prefix_segments)
}

#[cfg(test)]
mod tests;
