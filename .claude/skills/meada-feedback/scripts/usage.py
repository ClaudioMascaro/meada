#!/usr/bin/env python3
"""How a Claude Code session used the meada MCP server, and the lookups it made around it.

Usage: usage.py [session.jsonl] [--subagents] [--limit N]  |  usage.py --selftest

Without a path, reads the newest session of this repository under ~/.claude/projects/. Prints
every `mcp__meada__*` call (input, answer size, first answer line, what the agent ran next), then
the grep lookups of a symbol across directories (Grep tool, shell grep/rg/git grep) that a
`search`, `context` or `trace` could have answered, and the totals.
"""
import json
from pathlib import Path
import re
import shlex
import subprocess
import sys
import tempfile

SYMBOL = re.compile(r"^(?:\^?\\s\*)?(?:(?:pub(?:\(crate\))? )?(?:fn|def|class|struct|enum|trait|impl|func|"
                    r"interface|type|const|static|mod|function|let|var)\s+)?(?:\\b)?([A-Za-z_][A-Za-z0-9_]{3,})"
                    r"(?:\\b|\\\(|\()?$")
SEARCHERS = {"grep", "egrep", "rg"}


def calls(path):
    """The session's tool calls in order: (name, input, result text, is_error)."""
    uses, results = [], {}
    for line in Path(path).read_text(errors="replace").splitlines():
        try:
            entry = json.loads(line)
        except ValueError:
            continue
        content = (entry.get("message") or {}).get("content")
        for item in content if isinstance(content, list) else []:
            if item.get("type") == "tool_use":
                uses.append((item["id"], item["name"], item.get("input") or {}))
            elif item.get("type") == "tool_result":
                body = item.get("content")
                if isinstance(body, list):
                    body = "\n".join(b.get("text", "") for b in body if isinstance(b, dict))
                results[item.get("tool_use_id")] = (body or "", bool(item.get("is_error")))
    return [(name, args, *results.get(i, ("", False))) for i, name, args in uses]


def tree_search(argv):
    """The pattern of a grep/rg/git grep command line that searches directories of this
    repository, or None: stdin, single files, inverted matches and other places are no lookup."""
    walks = argv[:2] == ["git", "grep"] or (argv and Path(argv[0]).name == "rg")
    argv = argv[1:] if argv[:2] == ["git", "grep"] else argv
    if not argv or Path(argv[0]).name not in SEARCHERS:
        return None
    pattern, places, args = None, [], iter(argv[1:])
    for arg in args:
        if arg in ("-e", "--regexp"):
            pattern = next(args, None)
        elif arg in ("-A", "-B", "-C", "-m", "--max-count", "-g", "--glob", "-t", "--type", "--include"):
            next(args, None)
        elif arg in ("-v", "--invert-match"):
            return None
        elif arg.startswith("-") or re.match(r"\d*[<>]", arg):
            continue
        elif pattern is None:
            pattern = arg
        else:
            places.append(arg)
    root = Path.cwd().resolve()
    here = [Path(p) for p in places if "$" not in p and (not Path(p).is_absolute()
                                                         or Path(p).resolve().is_relative_to(root))]
    if any(p.is_dir() or (not p.exists() and not p.suffix) for p in here) or (walks and not places):
        return pattern
    return None


def lookups(name, args):
    """The symbols one call searched the tree for (a directory or no path), as (symbol, shown)."""
    found = []
    if name == "Grep":
        place = args.get("path") or "."
        if not Path(place).suffix:
            found.append((args.get("pattern", ""), f"Grep {args.get('pattern')!r} in {place}"))
    elif name == "Bash":
        for segment in re.split(r"\|\||&&|[|;\n]", args.get("command", "")):
            try:
                argv = shlex.split(segment)
            except ValueError:
                continue
            pattern = tree_search(argv)
            if pattern:
                found.append((pattern, segment.strip()[:120]))
    return [(m.group(1), shown) for pattern, shown in found if (m := SYMBOL.match(pattern))]


def report(sessions, limit):
    every = [c for s in sessions for c in calls(s)]
    out = []
    meada = [(i, c) for i, c in enumerate(every) if c[0].startswith("mcp__meada__")]
    errors = sum(1 for _, c in meada if c[3])
    out.append(f"meada calls: {len(meada)}, {errors} failed")
    for i, (name, args, text, error) in meada[:limit]:
        first = text.strip().splitlines()[0][:100] if text.strip() else "(empty)"
        after = ", ".join(n for n, *_ in every[i + 1:i + 4]) or "-"
        flag = " FAILED" if error else ""
        out.append(f"  {name[12:]} {json.dumps(args)}{flag} -> {len(text)} chars: {first} | next: {after}")
    found = [(i, s, shown) for i, c in enumerate(every) for s, shown in lookups(c[0], c[1])]
    near = sum(1 for i, *_ in found if any(c[0].startswith("mcp__meada__") for c in every[max(0, i - 3):i]))
    out.append(f"symbol lookups by grep across the tree: {len(found)}, {near} within three calls after a meada call")
    for _, symbol, shown in found[:limit]:
        out.append(f"  {symbol}: {shown}")
    if len(found) > limit:
        out.append(f"  ... {len(found) - limit} more")
    count = lambda n: sum(1 for c in every if c[0] == n)
    out.append(f"tool calls: {len(every)}; Bash {count('Bash')}, Grep {count('Grep')}, Read {count('Read')}")
    return "\n".join(out)


def newest():
    root = subprocess.run(["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True).stdout.strip()
    folder = Path.home() / ".claude/projects" / re.sub(r"[^A-Za-z0-9]", "-", root or str(Path.cwd()))
    found = sorted(folder.glob("*.jsonl"), key=lambda p: p.stat().st_mtime)
    if not found:
        sys.exit(f"no session under {folder}")
    return found[-1]


def selftest():
    def use(i, name, args):
        return {"type": "assistant", "message": {"content": [{"type": "tool_use", "id": i, "name": name, "input": args}]}}

    def result(i, text, error=False):
        return {"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": i, "is_error": error,
                                                          "content": [{"type": "text", "text": text}]}]}}
    lines = [use("1", "mcp__meada__search", {"query": "toset"}), result("1", "src/iac/functions.rs:329 toset\n"),
             use("2", "Grep", {"pattern": "fn exact_call", "path": "src"}), result("2", "src/iac.rs:917"),
             use("3", "Bash", {"command": "grep -rn 'printed_as_written' src/ | head -5"}), result("3", ""),
             use("4", "Bash", {"command": "grep -n 'x' src/iac.rs; rg -A 3 TemplateExpr"}), result("4", ""),
             use("5", "mcp__meada__trace", {"symbol": "nope"}), result("5", "no such symbol", True),
             use("6", "Read", {"file_path": "src/iac.rs"}), "not json",
             use("7", "Bash", {"command": "ls | grep depth; grep -v grep x/; grep -rn panic /tmp/elsewhere"
                                          " a.rs 2>/dev/null"})]
    with tempfile.TemporaryDirectory() as tmp:
        session = Path(tmp) / "s.jsonl"
        session.write_text("\n".join(json.dumps(x) if isinstance(x, dict) else x for x in lines) + "\n")
        text = report([session], 40)
    assert "meada calls: 2, 1 failed" in text, text
    assert 'search {"query": "toset"} -> 31 chars: src/iac/functions.rs:329 toset | next: Grep, Bash, Bash' in text, text
    assert "trace" in text and "FAILED" in text, text
    assert "symbol lookups by grep across the tree: 3, 3 within three calls after a meada call" in text, text
    assert "exact_call: Grep 'fn exact_call' in src" in text, text
    assert "printed_as_written: grep -rn 'printed_as_written' src/" in text, text
    assert "TemplateExpr: rg -A 3 TemplateExpr" in text, text
    assert "x: " not in text, text  # a one-file grep reads that file, no graph question
    assert "tool calls: 7; Bash 3, Grep 1, Read 1" in text, text
    assert "depth" not in text and "panic" not in text and "grep: " not in text, text
    print("usage.py selftest passed")


def main(argv):
    if argv == ["--selftest"]:
        return selftest()
    limit = int(argv[argv.index("--limit") + 1]) if "--limit" in argv else 40
    paths = [a for a in argv if a.endswith(".jsonl")]
    session = Path(paths[0]) if paths else newest()
    sessions = [session]
    if "--subagents" in argv:
        sessions += sorted((session.parent / session.stem / "subagents").glob("*.jsonl"))
    print(f"session: {session}")
    print(report(sessions, limit))


if __name__ == "__main__":
    main(sys.argv[1:])
