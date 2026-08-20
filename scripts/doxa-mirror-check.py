#!/usr/bin/env python3
import argparse
import filecmp
import glob
import json
import os
import sys


def main() -> int:
    ap = argparse.ArgumentParser(
        description="Refuses unless every RFC file this tree mirrors is byte-identical to the doxa corpus at the pinned revision: the corpus is the one source; the mirror is read-only — refreshed on a pin bump, never edited here."
    )
    ap.add_argument("--doxa", required=True, help="path of the doxa clone checked out at doxa.rev")
    ap.add_argument("--repo", required=True, help="this repository's provenance source_repo, e.g. yg/cfdb")
    ap.add_argument("--mirror", required=True, help="glob of the mirrored RFC files in this tree, e.g. docs/RFC-*.md")
    args = ap.parse_args()
    prov_path = os.path.join(args.doxa, "provenance.json")
    if not os.path.isfile(prov_path):
        print(f"FATAL: {prov_path} is absent — the corpus is unreadable at its pin, never a pass")
        return 1
    with open(prov_path, encoding="utf-8") as fh:
        prov = json.load(fh)
    bad = []
    seen = set()
    for ident, meta in sorted(prov.items()):
        repo = meta.get("source_repo")
        if repo is None:
            bad.append(f"malformed provenance entry: {ident} carries no source_repo — unattributable")
            continue
        if repo != args.repo:
            continue
        local = meta.get("source_path")
        if local is None:
            bad.append(f"malformed provenance entry: {ident} carries no source_path")
            continue
        seen.add(local)
        corpus = os.path.join(args.doxa, "rfc", f"{ident}.md")
        if not os.path.exists(local):
            bad.append(f"missing in mirror: {local} ({ident})")
        elif not os.path.exists(corpus):
            bad.append(f"missing in corpus: rfc/{ident}.md (provenance names {local})")
        elif not filecmp.cmp(local, corpus, shallow=False):
            bad.append(f"diverges from corpus: {local} != rfc/{ident}.md")
    mirrored = sorted(glob.glob(args.mirror))
    for f in mirrored:
        if f not in seen:
            bad.append(f"not declared by the corpus' provenance: {f} — the mirror is frozen at the declared set; a corpus RFC with no provenance entry is not mirrored, delete the file")
    if not seen and not mirrored:
        bad.append(f"nothing to check: provenance names no file for {args.repo} and {args.mirror} matches nothing")
    if bad:
        print("FATAL: the RFC mirror diverges from the doxa corpus at the pin — the mirror is read-only; edit the corpus, bump doxa.rev, refresh the mirror:")
        for b in bad:
            print("  " + b)
        return 1
    print(f"mirror ok: {len(seen)} files byte-identical to the corpus at the pin")
    return 0


if __name__ == "__main__":
    sys.exit(main())
