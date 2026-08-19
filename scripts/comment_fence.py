#!/usr/bin/env python3
"""Refuse any comment in Rust source under the given roots (operator ruling
2026-08-16/2026-08-19: code comments are a braindump with no ancestor). This
repo declares no surviving comment form in .rs files; gate-read comment
syntax lives in .cypher rule files (`// smoke-skip:`), which this fence does
not scan. Directories named `fixtures` (gate-read data corpora) and `ui`
(trybuild compile-fail cases whose .stderr goldens pin line numbers) are
excluded byte-identical. Token-aware: comment markers inside string,
raw-string, byte-string and char literals are not comments.

usage: comment_fence.py ROOT [ROOT...]        exit 0 clean, 1 findings, 2 usage
       comment_fence.py --self-test           the fence's own positive control
"""
import os
import sys

DECLARED = ()


def lex_comments(src):
    """Yield (line, text) for every comment token in `src`."""
    i, n, line = 0, len(src), 1
    out = []
    while i < n:
        c = src[i]
        if c == "\n":
            line += 1
            i += 1
            continue
        if c == "/" and i + 1 < n and src[i + 1] == "/":
            j = src.find("\n", i)
            j = n if j == -1 else j
            out.append((line, src[i:j]))
            i = j
            continue
        if c == "/" and i + 1 < n and src[i + 1] == "*":
            depth, j, start_line = 1, i + 2, line
            while j < n and depth:
                if src.startswith("/*", j):
                    depth += 1
                    j += 2
                elif src.startswith("*/", j):
                    depth -= 1
                    j += 2
                else:
                    if src[j] == "\n":
                        line += 1
                    j += 1
            out.append((start_line, src[i:j].split("\n", 1)[0] + (" …" if "\n" in src[i:j] else "")))
            i = j
            continue
        # raw / byte / c strings: r"…", r#"…"#, br"…", b"…", c"…", cr"…"
        if c in "rbc" and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_")):
            j = i
            while j < n and src[j] in "rbc" and j - i < 2:
                j += 1
            if j < n and src[j] in '#"' and "r" in src[i:j]:
                hashes = 0
                while j < n and src[j] == "#":
                    hashes += 1
                    j += 1
                if j < n and src[j] == '"':
                    close = '"' + "#" * hashes
                    k = src.find(close, j + 1)
                    k = n if k == -1 else k + len(close)
                    line += src.count("\n", i, k)
                    i = k
                    continue
            if j < n and src[j] == '"' and "r" not in src[i:j]:
                i = j
                c = '"'
        if c == '"':
            j = i + 1
            while j < n:
                if src[j] == "\\":
                    j += 2
                    continue
                if src[j] == '"':
                    j += 1
                    break
                j += 1
            line += src.count("\n", i, j)
            i = j
            continue
        if c == "'":
            # char literal ('x', '\n', '\u{..}') vs lifetime ('a, 'static)
            if i + 1 < n and src[i + 1] == "\\":
                j = src.find("'", i + 2)
                i = n if j == -1 else j + 1
                continue
            if i + 2 < n and src[i + 2] == "'":
                i += 3
                continue
            i += 1
            continue
        i += 1
    return out


def scan(roots):
    findings = []
    for root in roots:
        for dirpath, dirnames, filenames in os.walk(root):
            dirnames[:] = [d for d in dirnames if d not in ("target", ".claude", ".git", "fixtures", "ui")]
            for fn in filenames:
                if not fn.endswith(".rs"):
                    continue
                p = os.path.join(dirpath, fn)
                with open(p, encoding="utf-8", errors="replace") as fh:
                    src = fh.read()
                for ln, text in lex_comments(src):
                    t = text.strip()
                    if not any(t.startswith(d) for d in DECLARED):
                        findings.append((p, ln, t[:80]))
    return findings


def self_test():
    fixture = '''
fn planted() {
    let url = "https://example.invalid/not/a/comment"; // trailing comment
    let raw = r#"// inside a raw string, not a comment"#;
    let ch = '/'; let lt: &'static str = "'//' in a string";
    // line comment
    /* block
       comment */
}
/// doc comment
//! inner doc
'''
    got = [(ln, t.strip()) for ln, t in lex_comments(fixture)]
    flagged = [g for g in got if not any(g[1].startswith(d) for d in DECLARED)]
    want_lines = [3, 6, 7, 10, 11]
    ok = [g[0] for g in flagged] == want_lines and len(got) == 5
    if not ok:
        print("comment_fence self-test FAILED — got %r" % (got,), file=sys.stderr)
        return 1
    print("comment_fence self-test ok: 5 comments fired, 3 literals ignored")
    return 0


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print(__doc__, file=sys.stderr)
        sys.exit(2)
    if sys.argv[1] == "--self-test":
        sys.exit(self_test())
    f = scan(sys.argv[1:])
    for p, ln, t in f:
        print("%s:%d: %s" % (p, ln, t))
    sys.exit(1 if f else 0)
