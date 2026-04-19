#!/usr/bin/env python3
"""Deterministic generator for the Gate 3 large fixture (~15k nodes / ~80k edges).

Per methodology §6.1.T5, the large fixture is committed alongside its generator so
all spikes load the same input and benchmarks are reproducible.

Schema matches fixture-small.json: 4 node types × 3 edge types.

Usage:
    python3 generate_fixture_large.py > fixture-large.json

Determinism: seeded PRNG, stable iteration order, sha256 of output must match
across invocations on any platform. This is G1 of RFC §12.
"""
from __future__ import annotations

import hashlib
import json
import random
import sys

SEED = 42
N_CRATES = 20
N_ITEMS = 5000
N_FIELDS = 2000
N_CALLSITES = 8000
N_EDGES_TARGET = 80000


def main() -> None:
    rng = random.Random(SEED)

    nodes: list[dict] = []
    edges: list[dict] = []

    # Crates
    crate_names = [f"qbot-crate-{i:02d}" for i in range(N_CRATES)]
    for name in crate_names:
        nodes.append({
            "id": f"crate:{name}",
            "label": "Crate",
            "props": {"name": name, "is_workspace_member": True},
        })

    # Items — 5000 across 20 crates, mix of structs/enums/fns/traits
    kinds = ["struct", "enum", "fn", "trait", "reexport"]
    items: list[str] = []

    # Intentional HSB seeds: shared base names across multiple crates for Pattern A tests.
    hsb_basenames = ["Order", "save_impl", "now_utc", "Config", "Error"]

    for i in range(N_ITEMS):
        crate = crate_names[i % N_CRATES]
        kind = kinds[i % len(kinds)]

        if i < len(hsb_basenames) * N_CRATES:
            # First 100 items are HSB seeds — same base name in every crate
            base = hsb_basenames[i % len(hsb_basenames)]
            qname = f"{crate.replace('-', '_')}::{base}"
        else:
            qname = f"{crate.replace('-', '_')}::item_{i:05d}"

        node_id = f"item:{qname}:{i}"
        items.append(node_id)
        nodes.append({
            "id": node_id,
            "label": "Item",
            "props": {
                "qname": qname,
                "kind": kind,
                "crate": crate,
                "file": f"crates/{crate}/src/lib.rs",
                "line": (i * 7) % 1000 + 1,
                "signature_hash": f"{(i * 2654435761) & 0xFFFFFFFF:08x}",
            },
        })
        edges.append({
            "src": node_id,
            "dst": f"crate:{crate}",
            "label": "IN_CRATE",
        })

    # Fields — each Field belongs to an Item (struct/enum)
    struct_items = [n["id"] for n in nodes if n["label"] == "Item" and n["props"]["kind"] in ("struct", "enum")]
    for i in range(N_FIELDS):
        parent = struct_items[i % len(struct_items)]
        field_id = f"field:{i:05d}"
        nodes.append({
            "id": field_id,
            "label": "Field",
            "props": {
                "name": f"field_{i:05d}",
                "parent_qname": parent,
                "type_qname": f"primitive::type_{i % 50}",
            },
        })
        edges.append({
            "src": parent,
            "dst": field_id,
            "label": "HAS_FIELD",
        })

    # CallSite nodes
    for i in range(N_CALLSITES):
        nodes.append({
            "id": f"cs:{i:05d}",
            "label": "CallSite",
            "props": {
                "file": f"crates/qbot-crate-{i % N_CRATES:02d}/src/impl.rs",
                "line": (i * 13) % 2000 + 1,
                "col": (i * 3) % 80 + 1,
                "in_fn": items[i % len(items)],
            },
        })

    # CALLS edges — bulk-fill until we hit N_EDGES_TARGET
    # Each CALLS edge is CallSite → Item, with bag semantics (multiple edges between same pair allowed).
    # Multiple CALLS from same CallSite is unusual but acceptable for synthetic benchmark scale.
    current_edge_count = len(edges)
    remaining = N_EDGES_TARGET - current_edge_count
    assert remaining > 0, f"already have {current_edge_count} edges, target {N_EDGES_TARGET}"

    for i in range(remaining):
        src = f"cs:{i % N_CALLSITES:05d}"
        # Bias toward earlier items to create a power-law-ish call distribution
        idx = rng.randint(0, len(items) - 1)
        if rng.random() < 0.3:
            idx = idx % max(1, len(items) // 5)  # hot targets
        dst = items[idx]
        edges.append({
            "src": src,
            "dst": dst,
            "label": "CALLS",
            "props": {
                "in_fn": items[i % len(items)],
                "arg_count": i % 5,
            },
        })

    # Sort nodes and edges for determinism — G1 invariant.
    nodes.sort(key=lambda n: n["id"])
    edges.sort(key=lambda e: (e["src"], e["dst"], e["label"], e.get("props", {}).get("in_fn", "")))

    output = {
        "_comment": (
            "Gate 3 large fixture — ~15k nodes / ~80k edges, generated deterministically by "
            "generate_fixture_large.py with SEED=42. Schema matches fixture-small.json. "
            "Intentional HSB seeds (Order/save_impl/now_utc/Config/Error) appear in every crate to exercise "
            "Pattern A queries. sha256 of this file must be identical across platforms when regenerated "
            "from the same seed — that is G1 determinism from RFC §12."
        ),
        "schema_version": 1,
        "seed": SEED,
        "node_counts": {
            "Crate": N_CRATES,
            "Item": N_ITEMS,
            "Field": N_FIELDS,
            "CallSite": N_CALLSITES,
        },
        "total_nodes": len(nodes),
        "total_edges": len(edges),
        "nodes": nodes,
        "edges": edges,
    }

    # Emit canonical JSON — sort_keys for property order determinism.
    json_str = json.dumps(output, sort_keys=True, separators=(",", ":"))

    # Print a manifest line to stderr for the sha256 check in Gate 3.
    sha = hashlib.sha256(json_str.encode("utf-8")).hexdigest()
    print(f"nodes={len(nodes)} edges={len(edges)} sha256={sha}", file=sys.stderr)

    sys.stdout.write(json_str)
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
