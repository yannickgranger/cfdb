#!/usr/bin/env python3
"""Refuse unless this tree's RFC mirror is exactly the corpus' own set for
this repository's ident prefix, byte-identical, at the pinned revision.

The mirror's set is every corpus id carrying the prefix at the pin, and
the file each id lives under is the path its `provenance.json` entry
recorded if one exists — provenance is the true record of where an
imported file landed — and the path template for an id with no entry,
which is every document born in doxa since the import. The defect this
shape fixes was the set, never the paths.

usage: doxa-mirror-check.py --doxa DIR --prefix P --path-template T --mirror G
       doxa-mirror-check.py --self-test    the check's own positive control
exit 0 clean, 1 findings or usage
"""
import argparse
import filecmp
import json
import glob
import os
import shutil
import subprocess
import sys
import tempfile


def corpus_idents(doxa, prefix):
    rfc_dir = os.path.join(doxa, "rfc")
    if not os.path.isdir(rfc_dir):
        return None
    out = []
    for name in sorted(os.listdir(rfc_dir)):
        if name.startswith(prefix + "-") and name.endswith(".md"):
            out.append(name[: -len(".md")])
    return out


def recorded_paths(doxa):
    prov = os.path.join(doxa, "provenance.json")
    if not os.path.isfile(prov):
        return {}
    with open(prov, encoding="utf-8") as fh:
        entries = json.load(fh)
    return {k: v["source_path"] for k, v in entries.items() if v.get("source_path")}


def check(doxa, prefix, path_template, mirror_glob):
    idents = corpus_idents(doxa, prefix)
    if idents is None:
        return [f"{os.path.join(doxa, 'rfc')} is absent — the corpus is unreadable at its pin, never a pass"]
    recorded = recorded_paths(doxa)
    bad = []
    declared = set()
    for ident in idents:
        tail = ident[len(prefix) + 1:]
        local = recorded.get(ident) or path_template.format(tail=tail)
        declared.add(local)
        corpus = os.path.join(doxa, "rfc", f"{ident}.md")
        if not os.path.exists(local):
            source = "the path its provenance entry records" if ident in recorded else "the path template, this id being born in doxa"
            bad.append(f"missing in mirror: {local} ({ident}, {source})")
        elif not filecmp.cmp(local, corpus, shallow=False):
            bad.append(f"diverges from corpus: {local} != rfc/{ident}.md")
    mirrored = sorted(glob.glob(mirror_glob))
    for f in mirrored:
        if f not in declared:
            bad.append(f"not carried by the corpus at the pin: {f} — the mirror is the corpus' set for the prefix {prefix}; delete the file or add the RFC to the corpus")
    if not declared and not mirrored:
        bad.append(f"nothing to check: the corpus carries no rfc/{prefix}-*.md and {mirror_glob} matches nothing")
    return bad


def report(bad, declared_count):
    if bad:
        print("FATAL: the RFC mirror is not the corpus' set at the pin — the mirror is read-only; edit the corpus, bump doxa.rev, refresh the mirror:")
        for b in bad:
            print("  " + b)
        return 1
    print(f"mirror ok: {declared_count} files byte-identical to the corpus at the pin")
    return 0


def plant(root, corpus_files, mirror_files, provenance=None):
    if corpus_files is None:
        os.makedirs(os.path.join(root, "doxa"), exist_ok=True)
        corpus_files = {}
    else:
        os.makedirs(os.path.join(root, "doxa", "rfc"), exist_ok=True)
    os.makedirs(os.path.join(root, "docs", "rfc"), exist_ok=True)
    if provenance is not None:
        with open(os.path.join(root, "doxa", "provenance.json"), "w", encoding="utf-8") as fh:
            json.dump(provenance, fh)
    for name, body in corpus_files.items():
        with open(os.path.join(root, "doxa", "rfc", name), "w", encoding="utf-8") as fh:
            fh.write(body)
    for name, body in mirror_files.items():
        with open(os.path.join(root, "docs", "rfc", name), "w", encoding="utf-8") as fh:
            fh.write(body)


def run_script(root):
    return subprocess.run(
        [
            sys.executable,
            os.path.abspath(__file__),
            "--doxa",
            "doxa",
            "--prefix",
            "demo",
            "--path-template",
            "docs/rfc/{tail}.md",
            "--mirror",
            "docs/rfc/*.md",
        ],
        cwd=root,
        capture_output=True,
        text=True,
        check=False,
    )


def self_test():
    corpus = {
        "demo-001-first.md": "one\n",
        "demo-002-second.md": "two\n",
        "other-009-not-ours.md": "elsewhere\n",
    }
    mirror = {"001-first.md": "one\n", "002-second.md": "two\n"}
    cases = [
        ("the corpus' set mirrored byte-for-byte",
         corpus, mirror, 0, "mirror ok: 2 files"),
        ("a corpus RFC absent from the mirror",
         dict(corpus, **{"demo-003-third.md": "three\n"}), mirror, 1, "missing in mirror"),
        ("a mirrored file the corpus does not carry",
         corpus, dict(mirror, **{"004-extra.md": "four\n"}), 1, "not carried by the corpus"),
        ("a byte changed in a mirrored file",
         corpus, {"001-first.md": "one\n", "002-second.md": "two!\n"}, 1, "diverges from corpus"),
        ("the corpus unreadable at its pin", None, mirror, 1, "unreadable at its pin"),
        ("nothing on either side", {}, {}, 1, "nothing to check"),
    ]
    prov = {"demo-001-first": {"source_path": "docs/rfc/legacy.md"}}
    recorded_mirror = {"legacy.md": "one\n", "002-second.md": "two\n"}
    cases += [
        ("an id lives where its provenance entry recorded, and a doxa-born id under the template",
         corpus, recorded_mirror, 0, "mirror ok: 2 files", prov),
        ("a provenance-pathed id whose file is missing",
         corpus, {"002-second.md": "two\n"}, 1, "the path its provenance entry records", prov),
        ("a doxa-born id missing under the template",
         corpus, {"legacy.md": "one\n"}, 1, "the path template, this id being born in doxa", prov),
    ]
    failures = []
    for case in cases:
        name, corpus_files, mirror_files, want_rc, want_text = case[:5]
        provenance = case[5] if len(case) > 5 else None
        root = tempfile.mkdtemp()
        try:
            plant(root, corpus_files, mirror_files, provenance)
            got = run_script(root)
            out = got.stdout + got.stderr
            if got.returncode != want_rc:
                failures.append(f"{name}: expected exit {want_rc}, got {got.returncode} — {out.strip()}")
            elif want_text not in out:
                failures.append(f"{name}: expected output naming {want_text!r}, got {out.strip()!r}")
        finally:
            shutil.rmtree(root, ignore_errors=True)
    if failures:
        print("doxa-mirror-check self-test FAILED — the check does not refuse what it claims to refuse:")
        for f in failures:
            print("  " + f)
        return 1
    refused = sum(1 for c in cases if c[3] == 1)
    print(f"doxa-mirror-check self-test ok: {len(cases)} plants driven through the script, "
          f"{len(cases) - refused} clean and {refused} refused, each on its exit status")
    return 0


def main():
    if len(sys.argv) == 2 and sys.argv[1] == "--self-test":
        return self_test()
    ap = argparse.ArgumentParser(
        description="Refuses unless this tree's RFC mirror is exactly the corpus' set for its ident prefix, byte-identical, at the pinned revision: the corpus is the one source; the mirror is read-only — refreshed on a pin bump, never edited here."
    )
    ap.add_argument("--doxa", required=True, help="path of the doxa clone checked out at doxa.rev")
    ap.add_argument("--prefix", required=True, help="this repository's corpus ident prefix, e.g. graph-specs")
    ap.add_argument("--path-template", required=True, help="where an ident's tail lands in this tree, e.g. 'docs/rfc/{tail}.md'")
    ap.add_argument("--mirror", required=True, help="glob of the mirrored RFC files in this tree, e.g. 'docs/rfc/*.md'")
    ap.add_argument("--self-test", action="store_true", help="run the check's own positive control and exit")
    args = ap.parse_args()
    bad = check(args.doxa, args.prefix, args.path_template, args.mirror)
    idents = corpus_idents(args.doxa, args.prefix) or []
    return report(bad, len(idents))


if __name__ == "__main__":
    sys.exit(main())
