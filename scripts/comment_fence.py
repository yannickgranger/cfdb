#!/usr/bin/env python3
"""Refuse any comment in Rust source and shell scripts under the given roots
(operator ruling 2026-08-16/2026-08-19: code comments are a braindump with no
ancestor). This repo declares no surviving comment form in .rs files; in .sh
files the shebang is the one declared form. Gate-read comment syntax lives in
.cypher rule files (`// smoke-skip:`), which this fence does not scan.
Directories named `fixtures` (gate-read data corpora) and `ui` (trybuild
compile-fail cases whose .stderr goldens pin line numbers) are excluded
byte-identical. Token-aware in both dialects: markers inside string,
raw-string, byte-string and char literals are not comments, and in shell
neither is a `#` inside `${x#y}`, inside quotes, or in a heredoc body, which
is data being written rather than source.

usage: comment_fence.py ROOT [ROOT...]        exit 0 clean, 1 findings, 2 usage
       comment_fence.py --self-test           the fence's own positive control
"""
import os
import sys

DECLARED = ()
DECLARED_SH = ("#!",)


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


def _skip_squote(src, i):
    j = src.find("'", i + 1)
    return len(src) if j == -1 else j + 1


def _skip_dquote(src, i):
    n = len(src)
    j = i + 1
    while j < n:
        c = src[j]
        if c == "\\":
            j += 2
            continue
        if c == "$" and src.startswith("$(", j):
            j = _skip_cmdsub(src, j + 2)
            continue
        if c == "`":
            k = src.find("`", j + 1)
            j = n if k == -1 else k + 1
            continue
        if c == '"':
            return j + 1
        j += 1
    return n


def _skip_cmdsub(src, i):
    n, depth, j = len(src), 1, i
    while j < n and depth:
        c = src[j]
        if c == "\\":
            j += 2
            continue
        if c == "'":
            j = _skip_squote(src, j)
            continue
        if c == '"':
            j = _skip_dquote(src, j)
            continue
        if c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
        j += 1
    return j


def _heredoc_body(src, i):
    """At a `<<` / `<<-`, return (index after the body, delimiter found)."""
    n = len(src)
    j = i + 2
    strip_tabs = j < n and src[j] == "-"
    if strip_tabs:
        j += 1
    while j < n and src[j] in " \t":
        j += 1
    quote = ""
    if j < n and src[j] in "'\"":
        quote = src[j]
        j += 1
    k = j
    while k < n and (src[k].isalnum() or src[k] in "_-." or (quote and src[k] != quote)):
        k += 1
    delim = src[j:k]
    if quote and k < n and src[k] == quote:
        k += 1
    if not delim:
        return j, False
    body = src.find("\n", k)
    if body == -1:
        return n, False
    j = body + 1
    while j < n:
        eol = src.find("\n", j)
        eol = n if eol == -1 else eol
        candidate = src[j:eol]
        if (candidate.lstrip("\t") if strip_tabs else candidate).strip() == delim:
            return (eol + 1 if eol < n else n), True
        j = eol + 1 if eol < n else n
    return n, False


def lex_sh_comments(src):
    """Yield (line, text) for every comment token in shell source `src`."""
    i, n, line = 0, len(src), 1
    out = []
    word_start = True
    while i < n:
        c = src[i]
        if c == "\n":
            line += 1
            i += 1
            word_start = True
            continue
        if c == "\\" and i + 1 < n:
            if src[i + 1] == "\n":
                line += 1
            i += 2
            word_start = False
            continue
        if c == "'":
            j = _skip_squote(src, i)
            line += src.count("\n", i, j)
            i, word_start = j, False
            continue
        if c == '"':
            j = _skip_dquote(src, i)
            line += src.count("\n", i, j)
            i, word_start = j, False
            continue
        if c == "$" and src.startswith("$((", i):
            j = _skip_cmdsub(src, i + 3)
            j = _skip_cmdsub(src, j) if j < n and src[j - 1] != ")" else j
            line += src.count("\n", i, j)
            i, word_start = j, False
            continue
        if c == "<" and src.startswith("<<<", i):
            i, word_start = i + 3, False
            continue
        if c == "<" and src.startswith("<<", i):
            j, _found = _heredoc_body(src, i)
            line += src.count("\n", i, j)
            i, word_start = j, True
            continue
        if c == "#" and word_start:
            j = src.find("\n", i)
            j = n if j == -1 else j
            out.append((line, src[i:j]))
            i = j
            continue
        word_start = c in " \t;&|()"
        i += 1
    return out


def scan(roots):
    findings = []
    for root in roots:
        for dirpath, dirnames, filenames in os.walk(root):
            dirnames[:] = [d for d in dirnames if d not in ("target", ".claude", ".git", "fixtures", "ui")]
            for fn in filenames:
                if fn.endswith(".rs"):
                    lexer, declared = lex_comments, DECLARED
                elif fn.endswith(".sh"):
                    lexer, declared = lex_sh_comments, DECLARED_SH
                else:
                    continue
                p = os.path.join(dirpath, fn)
                with open(p, encoding="utf-8", errors="replace") as fh:
                    src = fh.read()
                for ln, text in lexer(src):
                    t = text.strip()
                    if ln == 1 and any(t.startswith(d) for d in declared):
                        continue
                    if lexer is lex_comments and any(t.startswith(d) for d in declared):
                        continue
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
    if sh_self_test():
        return 1
    print("comment_fence self-test ok: 5 comments fired, 3 literals ignored")
    return 0


def sh_self_test():
    fixture = """#!/usr/bin/env bash
squote='# not a comment'
dquote="# not a comment either"
sp_squote='a # not a comment'
sp_dquote="a # not a comment either"
trim="${squote#\\# }"
count=${#PATH}
nested="$(printf '%s' "${dquote}" | cut -d'#' -f1)"
done_reading() { read -r _ <<< "$squote"; }
cat > /dev/null <<'EOF'
#!/usr/bin/env bash
# inside a heredoc body, which is data being written
EOF
# a real comment
echo hi  # a trailing comment
"""
    got = [(ln, t.strip()) for ln, t in lex_sh_comments(fixture)]
    flagged = [g for g in got if not (g[0] == 1 and any(g[1].startswith(d) for d in DECLARED_SH))]
    want = [(14, "# a real comment"), (15, "# a trailing comment")]
    if [(1, "#!/usr/bin/env bash")] + want != got or flagged != want:
        print("comment_fence shell self-test FAILED — got %r" % (got,), file=sys.stderr)
        return 1
    print("comment_fence shell self-test ok: 2 comments fired, shebang declared, 9 literals ignored")
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
