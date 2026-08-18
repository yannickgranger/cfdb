#!/usr/bin/env bash
# cfdb's own gate, corpus-wide (keel-harness §7 step 3, §3.2, §8): keel
# measures the deployment level keel.json declares — L2: the cascade pin present,
# the doxa corpus checked out at doxa.rev, the whole declared root covered (the
# instrument's own ##/### listing against its node verdicts, zero silence) — and
# carries the corpus-wide cascade run beside the verdict. The gate then refuses
# what an L2 claim leaves visible: silence; a malformed or diverged verdict; any
# code-side finding (a pub type without a concept heading); any non-grounded
# verdict or file-level finding on a document that declares a document default
# (a grounded document runs at L3). Ungrounded documents stay at L2: their count
# is the live worklist (keel-harness §7 step 2), printed, never refused here.
# Exit 0 = the declared level holds on this tree; anything else refuses.
set -euo pipefail
cd "$(dirname "$0")/.."
KEEL="${KEEL:-keel}"
DOXA_REV=$(tr -d '[:space:]' < doxa.rev)
DOXA_DIR="${DOXA_DIR:-.doxa}"
if [ ! -d "$DOXA_DIR/.git" ]; then
  git clone -q https://agency.lab:3000/yg/doxa.git "$DOXA_DIR" || { echo "FATAL: doxa could not be cloned — unavailable, never a pass" >&2; exit 1; }
fi
git -C "$DOXA_DIR" fetch -q origin || true
git -C "$DOXA_DIR" checkout -q "$DOXA_REV" || { echo "FATAL: doxa rev $DOXA_REV not in the clone" >&2; exit 1; }
[ -f "$DOXA_DIR/index.json" ] || { echo "FATAL: doxa checkout carries no index.json" >&2; exit 1; }

echo "==> own gate: keel level --repo . --declaration keel.json --corpus $DOXA_DIR@$DOXA_REV (cascade at $(tr -d '[:space:]' < cascade.rev))"
level_json=$($KEEL level --repo . --declaration keel.json --corpus "$DOXA_DIR" --json 2>/dev/null) && level_rc=0 || level_rc=$?
if [ "$level_rc" -eq 1 ]; then
  echo "FATAL: keel could not measure the declared level (exit 1): $level_json" >&2
  exit 1
fi
grounded_docs=$(grep -lE '^<!-- doc:rfc:[a-z0-9-]+ -->$' specs/concepts/*.md 2>/dev/null | sort | tr '\n' ' ' || true)
python3 - "$level_json" "$level_rc" $grounded_docs <<'PY'
import json, sys
level = json.loads(sys.argv[1]); rc = int(sys.argv[2])
grounded_docs = set(sys.argv[3:])
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
for n in report["nodes"]:
    if n["verdict"] in ("malformed", "diverged"):
        errors.append(f"{n['file']}:{n['line']} `{n['name']}` is {n['verdict']}: {', '.join(n['findings'])}")
    elif n["file"] in grounded_docs and n["verdict"] != "grounded":
        errors.append(f"{n['file']}:{n['line']} `{n['name']}` is {n['verdict']} in a grounded document: {', '.join(n['findings'])}")
for f in report["findings"]:
    cls = f["class"]
    if cls in ("MalformedFrontmatter", "GroundingWithoutConcept", "UnclosedFence") and f.get("file") in grounded_docs:
        errors.append(f"{f['file']}:{f.get('line', '')} {cls} in a grounded document")
    if cls in ("TypeWithoutHeading", "TypeOnlyIllustrative"):
        errors.append(f"code without spec: pub type `{f['type_name']}` ({cls})")
    if cls in ("EmptyRfc", "EmptySpecs", "EmptyCode", "DuplicateClause"):
        errors.append(f"run-level: {json.dumps(f)}")
ungrounded_docs = sorted({n["file"] for n in report["nodes"] if n["verdict"] == "ungrounded"})
print(f"    ungrounded documents (L2, the live worklist — keel-harness §7 step 2): {len(ungrounded_docs)} carrying {c['ungrounded']} ungrounded heading(s); grounded documents (L3): {len(grounded_docs)}")
if errors:
    for e in errors:
        print("ERROR:", e, file=sys.stderr)
    print("keel.json declares L2 corpus-wide: zero silence, no malformed or diverged verdict, no pub type without a concept; a grounded document runs at L3 (keel-harness §3.2, §8)", file=sys.stderr)
    sys.exit(1)
print("    ok — the declared level holds on this tree, zero silence")
PY
