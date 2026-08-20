#!/usr/bin/env bash
# cfdb's own gate, corpus-wide (keel-harness §7 step 3, §3.2, §8): keel measures
# the deployment level keel.json declares — L2, corpus-wide advisory: the cascade
# pin present, the doxa corpus checked out at doxa.rev, the full declared root
# every run, every ##/### heading of specs/concepts carrying a verdict (§3.2: any
# shortfall is silence — refused regardless of how clean the visited subset is),
# findings visible, nothing else refuses (§8, the L2 row). What refuses here holds
# at every level: silence (§3.2); a level declared but not running (§8: a repo
# never claims a level it is not running); the corpus absent or unreadable at
# its pin (keel-harness §3.1 / keel-dialect §3.3: unavailable, never a pass).
# Run-level findings (empty RFC / specs / code, duplicate clause) are printed and
# not refused: keel-dialect §7 keeps them out of the node verdicts and §11 (c)
# leaves their verdict to a ruling. The ungrounded count is the live worklist for
# §7 step 2 (own concept docs 100 % grounded); when it reaches zero the
# declaration moves to L3 and CI refuses on any corpus-wide finding (§8).
# Exit 0 = the declared level holds on this tree; anything else refuses.
set -euo pipefail
cd "$(dirname "$0")/.."
KEEL="${KEEL:-keel}"
DOXA_REV=$(tr -d '[:space:]' < doxa.rev)
DOXA_DIR="${DOXA_DIR:-.doxa}"
if [ ! -d "$DOXA_DIR/.git" ]; then
  git clone -q https://agency.lab:3000/yg/doxa.git "$DOXA_DIR" || { echo "FATAL: doxa could not be cloned — the corpus is unavailable, never a pass (keel-harness §3.1)" >&2; exit 1; }
fi
git -C "$DOXA_DIR" fetch -q origin || true
git -C "$DOXA_DIR" checkout -q "$DOXA_REV" || { echo "FATAL: doxa rev $DOXA_REV not in the clone — the corpus is unreadable at its pin, unavailable, never a pass (keel-harness §3.1; keel-dialect §3.3)" >&2; exit 1; }
[ -f "$DOXA_DIR/index.json" ] || { echo "FATAL: doxa checkout carries no index.json — unreadable at its pin, unavailable (keel-harness §3.1)" >&2; exit 1; }

echo "==> mirror: docs/RFC-*.md is a byte-identical mirror of the doxa corpus at $DOXA_REV — read-only, the corpus is the one source"
python3 scripts/doxa-mirror-check.py --doxa "$DOXA_DIR" --repo yg/cfdb --mirror 'docs/RFC-*.md' || exit 1

echo "==> own gate: keel level --repo . --declaration keel.json --corpus $DOXA_DIR@$DOXA_REV (cascade at $(tr -d '[:space:]' < cascade.rev))"
level_json=$($KEEL level --repo . --declaration keel.json --corpus "$DOXA_DIR" --json 2>/dev/null) && level_rc=0 || level_rc=$?
if [ "$level_rc" -eq 1 ]; then
  echo "FATAL: keel could not measure the declared level (exit 1): $level_json" >&2
  exit 1
fi
python3 - "$level_json" "$level_rc" <<'PY'
import json, sys, collections
level = json.loads(sys.argv[1]); rc = int(sys.argv[2])
errors = []
cov = level.get("coverage")
if cov is None:
    print("ERROR: keel level ran no coverage — the declaration names no instrument or no roots", file=sys.stderr); sys.exit(1)
report = cov["run"]; c = report["counts"]
print(f"    declared {level['declared']} — {'holds' if level['holds'] else 'NOT RUNNING IT'}; cascade pin {'present' if level['instruments'][0]['pin'] else 'MISSING'}, corpus {'at' if level['corpus_pinned'] else 'NOT AT'} its pin")
print(f"    nodes {c['nodes']} (listing {cov['listed']}) grounded {c['grounded']} ungrounded {c['ungrounded']} malformed {c['malformed']} diverged {c['diverged']} findings {c['findings']} run-level {c['run_level']} exit {rc}")
if not level["holds"]:
    for r in level["reasons"]:
        errors.append(f"declared level not running: {r}")
if not cov["covered"]:
    errors.append(f"silence: {cov['silence']} listed heading(s) carry no verdict ({cov['listed']} listed, {cov['verdicts']} verdicts) — refused regardless of how clean the visited subset is (keel-harness §3.2)")
for f in report["findings"]:
    if f["class"] in ("EmptyRfc", "EmptySpecs", "EmptyCode", "DuplicateClause", "UnclosedFence"):
        print(f"    run-level finding, visible (keel-dialect §7; its verdict is an open ruling, §11 c): {json.dumps(f)}")
classes = collections.Counter(f["class"] for f in report["findings"])
print("    findings visible (L2 — keel-harness §8): " + (", ".join(f"{k} {v}" for k, v in sorted(classes.items())) or "none"))
for n in report["nodes"]:
    if n["verdict"] in ("malformed", "diverged"):
        print(f"    visible: {n['file']}:{n['line']} `{n['name']}` {n['verdict']} — {', '.join(n['findings'])}")
ungrounded_docs = sorted({n["file"] for n in report["nodes"] if n["verdict"] == "ungrounded"})
grounded_docs = sorted({n["file"] for n in report["nodes"] if n["verdict"] == "grounded"})
print(f"    ungrounded documents (the live worklist for keel-harness §7 step 2): {len(ungrounded_docs)} carrying {c['ungrounded']} ungrounded heading(s); documents with a grounded heading: {len(grounded_docs)}")
if errors:
    for e in errors:
        print("ERROR:", e, file=sys.stderr)
    print("keel.json declares L2 corpus-wide: the full declared root every run, zero silence, findings visible (keel-harness §3.2, §8)", file=sys.stderr)
    sys.exit(1)
print("    ok — the declared level holds on this tree, zero silence")
PY
