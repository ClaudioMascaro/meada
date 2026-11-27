#!/usr/bin/env python3
"""The release diff (#852, ADR 0029): the links and orphan lines of a candidate against the previous tag.

  release-diff.py run --base-bin B --head-bin H --run DIR [--repos a,b] [--base-label T] [--head-label T]
                      [--sample N]
  release-diff.py diff --run DIR [--base-label T] [--head-label T] [--sample N]   (again, over what `run` captured)
  release-diff.py --selftest                                         (no Meada)

`run` indexes each group of pinned public corpus entries as one workspace with each binary: the entries of
`corpus.tsv`, the A7 entries of `infra.tsv` and its CI entries (A9's corpus). The probes are this skill's own
inputs, not corpora, and enter only when `--repos` names them. It keeps `meada export` of each repository and
`meada status` of the workspace under DIR/base and DIR/head, then writes `diff.json` (every changed statement)
and `summary.md` (counts per pair and per level; a pair is a contract kind and a statement type, GLOSSARY).
A link is the same link when `prove.py`'s link key is (consumer, provider, contract, root); an orphan line is
the same line when its repository, contract and place are. Exit 0 with or without a diff: the diff goes to a
judge, it is never a verdict. Exit 2: a capture failed.

The summary then holds the `calls` rows of every entry, a row being the same when its `id` is: counts per entry
and per use kind and level, a seeded sample of at most N rows per group with their source lines, and every
changed row in `calls.json`; and the cost of each side (index seconds, peak RSS, index and export bytes). Both
are reported and hold nothing: an error in one is a `BLOCKED` line in its section, never the exit code.
"""

import argparse
import collections
import contextlib
import io
import json
import os
import random
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import prove
from prove import corpus, fetch, infra

# Strongest first, as meada-core's `Level` orders them (ADR 0005); a name it does not know is weakest.
LEVELS = ("exact", "declared", "inferred", "decided", "ambiguous")
# `meada status` in English (`LANG`, ADR 0008): `orphan <contract>: no <counterpart>[ in <scope>]; <place>[ [level]]`.
# A tag before #849 names no scope; both sides read one workspace, so the scope is not compared.
ORPHAN = re.compile(r"^\s+orphan (.+?): (no [a-z]+)(?: in [^;]+)?; (.*?)(?: \[(" + "|".join(LEVELS) + r")\])?$")
LINK_TYPES = ("new", "vanished", "raised", "dropped")
ORPHAN_TYPES = ("appeared", "vanished", "level moved")
# What a calls row can show between the two sides; one row can show several.
CALL_TYPES = ("new", "vanished", "raised", "dropped", "retargeted", "candidates removed", "candidates gained",
              "certain lost")
CERTAIN = ("exact", "declared")
# What a judge reads of the changed calls rows: at most SAMPLE of each group, the same for the same captures.
GROUPS = ("new exact", "raised to exact", "lost a candidate", "certain lost")
SEED, SAMPLE = 852, 20


def rank(level):
    return LEVELS.index(level) if level in LEVELS else len(LEVELS)


def kind(contract):
    return contract.split("::", 1)[0]


def table():
    return {**infra(), **corpus()}


def public():
    """Every entry pinned by URL, and this checkout (`self`): the probes are no corpus."""
    return [name for name, (_, url, _) in table().items() if not url.startswith("probe:")]


def groups(names):
    """Workspace name -> its entries: the code corpus, A9's CI corpus, A7's infrastructure."""
    code, out = corpus(), {}
    for name in names:
        packs = table()[name][0]
        out.setdefault("code" if name in code else "ci" if packs == ["ci"] else "infra", []).append(name)
    return out


def identity(name):
    """The repository identity Meada reads from the `origin` that `prove.fetch` sets (lowercase)."""
    url = table()[name][1]
    if url == "self":
        return "github.com/claudiomascaro/meada"
    if url.startswith("probe:"):
        return f"github.com/zz-prove/{name}"
    return url.removeprefix("https://").lower()


def meada(bin, cwd, log, *words):
    env = {k: v for k, v in os.environ.items() if not k.startswith("LC_")}
    started = time.monotonic()
    result = subprocess.run([bin, *words], cwd=cwd, env={**env, "LANG": "en_US.UTF-8"}, text=True,
                            capture_output=True, timeout=prove.TIMEOUT)
    (cwd / f"{log}.stderr").write_text(result.stderr)
    if result.returncode:
        raise RuntimeError(f"{bin} {' '.join(words)} in {cwd} exited {result.returncode}: {result.stderr[-1500:]}")
    return result.stdout, round(time.monotonic() - started, 1)


def capture(side, bin, workspaces, sources, run):
    """Each workspace indexed by `bin` from its own copies: exports per repository, `status`, seconds."""
    seconds = {}
    for ws, names in workspaces.items():
        folder = run / side / ws
        folder.mkdir(parents=True)
        manifest = ['index_dir = "indexes"']
        for name in names:
            repo = run / "work" / side / ws / name
            shutil.copytree(sources[name], repo, symlinks=True)
            manifest += ["", "[[repository]]", f'repo = "{identity(name)}"', f"path = {json.dumps(str(repo))}"]
        (folder / "workspace.toml").write_text("\n".join(manifest) + "\n")
        _, seconds[ws] = meada(bin, folder, "index", "index", "--workspace", "workspace.toml")
        for name in names:
            out, _ = meada(bin, folder, f"{name}.export", "export", "--workspace", "workspace.toml", "--repo", identity(name))
            (folder / f"{name}.export.json").write_text(out)
        out, _ = meada(bin, folder, "status", "status", "--workspace", "workspace.toml")
        (folder / "status.txt").write_text(out)
    version, _ = meada(bin, run / side, "version", "--version")
    (run / side / "capture.json").write_text(json.dumps({"version": version.strip(), "index_seconds": seconds}, indent=1))


def link_rows(exports):
    """The distinct link rows of a workspace's exports (a link between two of its repositories shows in
    both), by `prove.py`'s link key."""
    distinct = {json.dumps(row, sort_keys=True): row for export in exports for row in export.get("links", [])}
    return prove.rows("links", distinct.values())


def orphan_lines(status):
    """`meada status` orphan lines as `repository | contract | place` -> (counterpart, level or None)."""
    out, repo, seen = {}, "?", collections.Counter()
    for line in status.splitlines():
        if line and not line[0].isspace():
            repo = line.split(" ", 1)[0]
        match = ORPHAN.match(line)
        if match:
            contract, missing, place, level = match.groups()
            key = f"{repo} | {contract} | {place}"
            seen[key] += 1
            out[key if seen[key] == 1 else f"{key} #{seen[key]}"] = (contract, missing, level)
    return out


def load(side):
    """Workspace -> its links and orphan lines, from what `capture` wrote under `side`."""
    out = {}
    for folder in sorted(p for p in side.iterdir() if (p / "status.txt").is_file()):
        exports = [json.loads(p.read_text()) for p in sorted(folder.glob("*.export.json"))]
        out[folder.name] = {"links": link_rows(exports), "orphans": orphan_lines((folder / "status.txt").read_text())}
    return out


def compare(base, head):
    """Every changed statement of the workspaces both sides hold, in a stable order."""
    changes = []
    for ws in sorted(base.keys() & head.keys()):
        b, h = base[ws]["links"], head[ws]["links"]
        for key in sorted(b.keys() | h.keys()):
            old, new = b.get(key), h.get(key)
            if old and new and old.get("level") == new.get("level"):
                continue
            row = new or old
            if not old:
                change, level = "new", new.get("level")
            elif not new:
                change, level = "vanished", old.get("level")
            else:
                change = "raised" if rank(new.get("level")) < rank(old.get("level")) else "dropped"
                level = f"{old.get('level')} -> {new.get('level')}"
            changes.append({"workspace": ws, "statement": "links", "type": change, "kind": kind(row["contract"]),
                            "level": level, "key": key, "base": old, "head": new})
        b, h = base[ws]["orphans"], head[ws]["orphans"]
        for key in sorted(b.keys() | h.keys()):
            old, new = b.get(key), h.get(key)
            if old == new:
                continue
            contract, missing, level = new or old
            if not old:
                change = "appeared"
            elif not new:
                change = "vanished"
            else:
                change, level = "level moved", f"{old[2] or '-'} -> {new[2] or '-'}"
            changes.append({"workspace": ws, "statement": "orphan lines", "type": change, "kind": kind(contract),
                            "level": level or "-", "key": key, "missing": missing})
    return changes


def call_changes(base, head):
    """The changed calls rows between one entry's two exports, by row key. A change of `narrowed`,
    `decorates`, `holder` or `span` alone is no change."""
    old_rows = prove.rows("calls", base.get("calls") or [])
    new_rows = prove.rows("calls", head.get("calls") or [])

    def level(row):
        return row.get("level") or "unresolved"

    def kept(row):
        return row and {"level": row.get("level"), "target": row.get("target"),
                        "candidates": row.get("candidates") or [], "span": row.get("span")}

    out = []
    for key in sorted(old_rows.keys() | new_rows.keys()):
        old, new = old_rows.get(key), new_rows.get(key)
        types, removed, gained = [], [], []
        if not old:
            types, label = ["new"], level(new)
        elif not new:
            types, label = ["vanished"], level(old)
        else:
            moved = rank(new.get("level")) - rank(old.get("level"))
            label = f"{level(old)} -> {level(new)}" if moved else level(new)
            if moved:
                types.append("raised" if moved < 0 else "dropped")
            elif old.get("target") != new.get("target"):
                types.append("retargeted")
            before, after = set(old.get("candidates") or []), set(new.get("candidates") or [])
            removed, gained = sorted(before - after), sorted(after - before)
            types += ["candidates removed"] * bool(removed) + ["candidates gained"] * bool(gained)
        if old and old.get("level") in CERTAIN and (
                not new or rank(new.get("level")) > rank(old.get("level")) or new.get("target") != old.get("target")):
            types.append("certain lost")
        if types:
            out.append({"key": key, "use": (new or old).get("kind") or "call", "types": types, "level": label,
                        "removed": removed, "gained": gained, "base": kept(old), "head": kept(new)})
    # The declaration of the target a sample names (the head row's, else the one the base row held), from the
    # symbols of the export whose row is shown.
    for side, export in (("head", head), ("base", base)):
        shown = {c["key"]: c[side]["target"] or (c["base"] or {}).get("target")
                 for c in out if ("head" if c["head"] else "base") == side}
        wanted = set(shown.values())
        spans = {s["id"]: s.get("span") for s in export.get("symbols") or [] if s.get("id") in wanted}
        for c in out:
            if c["key"] in shown:
                c["target_span"] = spans.get(shown[c["key"]])
    return out


def call_tally(changes):
    """(workspace, entry) -> type -> rows, with the candidates removed and gained counted too; and
    (use kind, level) -> type -> rows."""
    entries, pairs = collections.defaultdict(collections.Counter), collections.defaultdict(collections.Counter)
    for c in changes:
        row = entries[c["workspace"], c["entry"]]
        row.update(c["types"])
        row["candidates removed n"] += len(c["removed"])
        row["candidates gained n"] += len(c["gained"])
        pairs[c["use"], c["level"]].update(c["types"])
    return entries, pairs


def entry_calls(run):
    """The changed calls rows of every entry both sides exported, one entry's two exports at a time."""
    out = []
    for folder in sorted(p for p in (run / "base").iterdir() if (p / "status.txt").is_file()):
        other = run / "head" / folder.name
        if not (other / "status.txt").is_file():
            continue
        for path in sorted(folder.glob("*.export.json")):
            if (other / path.name).is_file():
                entry = path.name.removesuffix(".export.json")
                changes = call_changes(json.loads(path.read_text()), json.loads((other / path.name).read_text()))
                out += [{**c, "workspace": folder.name, "entry": entry} for c in changes]
    return sorted(out, key=row_order)


def row_order(change):
    return change["workspace"], change["entry"], change["key"]


def in_group(group, change):
    types = change["types"]
    raised_exact = "raised" in types and change["head"]["level"] == "exact"
    if group == "new exact":
        return "new" in types and change["head"]["level"] == "exact"
    if group == "raised to exact":
        return raised_exact
    if group == "lost a candidate":  # a raise to exact is judged in its own group, dropped ones with it
        return "candidates removed" in types and not raised_exact
    return "certain lost" in types


def call_samples(changes, k=SAMPLE, seed=SEED):
    out = {}
    for group in GROUPS:
        pool = sorted((c for c in changes if in_group(group, c)), key=row_order)
        out[group] = sorted(random.Random(seed).sample(pool, min(k, len(pool))), key=row_order)
    return out


def sample_line(change, run):
    """One sampled row with the source line of its site and of its target; `?` without the work copies."""
    side = "head" if change["head"] else "base"
    row = change[side]
    repo = run / "work" / side / change["workspace"] / change["entry"]

    def place(span):
        if not span:
            return "?:? ``"
        line, text = prove.line_of(repo, span["path"], span.get("start", 0))
        return f"{span['path']}:{line} `{text}`"

    target, was = row["target"], (change["base"] or {}).get("target")
    at = f" at {place(change['target_span'])}"
    if target:
        names = f"`{target}`{at}" + (f", was `{was}`" if was and was != target else "")
    else:
        names = "no target" + (f", was `{was}`{at}" if was else "")
    out = f"- {change['workspace']}/{change['entry']} {place(row['span'])} [{change['level']}] -> {names}"
    if in_group("lost a candidate", change):
        out += f", removed `{change['removed'][0]}`, {len(row['candidates'])} left"
    return out


def calls_section(changes, run, k=SAMPLE):
    lines = ["## Calls", ""]
    if not changes:
        return lines + ["No call row changed."]
    entries, pairs = call_tally(changes)
    lines += ["| Entry | New | Vanished | Raised | Dropped | Retargeted | Candidates removed (rows / candidates) | "
              "Candidates gained (rows / candidates) | Certain lost |", "|---|" + "---:|" * len(CALL_TYPES)]
    for (ws, entry), n in sorted(entries.items()):
        cells = [f"{n[t]} / {n[t + ' n']}" if t.startswith("candidates") else str(n[t]) for t in CALL_TYPES]
        lines.append(f"| {ws}/{entry} | " + " | ".join(cells) + " |")
    moves = [t for t in CALL_TYPES if not t.startswith("candidates")]
    lines += ["", "| Use | Level | " + " | ".join(t.capitalize() for t in moves) + " |", "|---|---|" + "---:|" * len(moves)]
    lines += [f"| {use} | {level} | " + " | ".join(str(n[t]) for t in moves) + " |"
              for (use, level), n in sorted(pairs.items()) if any(n[t] for t in moves)]
    for group, picked in call_samples(changes, k).items():
        if picked:
            total = sum(in_group(group, c) for c in changes)
            lines += ["", f"### Sample (seed {SEED}): {group}, {len(picked)} of {total}", ""]
            lines += [sample_line(c, run) for c in picked]
    return lines + ["", "Every changed call row is in calls.json of the artifact."]


def tally(changes):
    """(pair, level) -> type -> count; a pair is `kind statement`."""
    out = collections.defaultdict(collections.Counter)
    for c in changes:
        out[f"{c['kind']} {c['statement']}", c["level"]][c["type"]] += 1
    return out


def summary(changes, base_label, head_label, captured=None):
    lines = ["# Release diff: links, orphan lines, calls and cost against the previous tag", "",
             f"- Base: {base_label}", f"- Candidate: {head_label}"]
    for side, info in (captured or {}).items():
        spent = ", ".join(f"{ws} {s} s" for ws, s in info["index_seconds"].items())
        lines.append(f"- {side}: `{info['version']}`, index seconds per workspace: {spent}")
    if not changes:
        return "\n".join(lines + ["", "No link or orphan line change: the candidate states the same links and orphan lines as the base."]) + "\n"
    counts = tally(changes)
    for statement, types in (("links", LINK_TYPES), ("orphan lines", ORPHAN_TYPES)):
        rows = sorted(k for k in counts if k[0].endswith(" " + statement))
        if not rows:
            continue
        lines += ["", f"## {statement.capitalize()}", "", "| Pair | Level | " + " | ".join(t.capitalize() for t in types) + " |",
                  "|---|---|" + "---:|" * len(types)]
        lines += [f"| {pair} | {level} | " + " | ".join(str(counts[pair, level][t]) for t in types) + " |" for pair, level in rows]
    lines += ["", f"{len(changes)} changed statements; each one is in `diff.json` of the artifact."]
    return "\n".join(lines) + "\n"


def write(run, changes, base_label, head_label, k=SAMPLE):
    captured = {side: json.loads((run / side / "capture.json").read_text())
                for side in ("base", "head") if (run / side / "capture.json").is_file()}
    (run / "diff.json").write_text(json.dumps(changes, indent=1, sort_keys=True) + "\n")
    text = summary(changes, base_label, head_label, captured)
    (run / "summary.md").write_text(text)
    # The sections from here hold nothing, so an error in one is a line in it, never the job's.
    try:
        calls = entry_calls(run)
        (run / "calls.json").write_text(json.dumps(calls, indent=1, sort_keys=True) + "\n")
        text += "\n" + "\n".join(calls_section(calls, run, k)) + "\n"
    except Exception as error:
        text += f"\n## Calls\n\nBLOCKED: {type(error).__name__}: {error}\n"
    (run / "summary.md").write_text(text)
    print(text, end="")


def cmd_run(a):
    run = Path(a.run).resolve()
    names = a.repos.split(",") if a.repos else public()
    unknown = sorted(set(names) - table().keys())
    if unknown:
        sys.exit(f"unknown corpus entries {unknown}; see {prove.HERE}/corpus.tsv and infra.tsv")
    run.mkdir(parents=True, exist_ok=False)
    workspaces = groups(names)
    sources = {}
    for name in names:
        sources[name] = run / "work" / "src" / name
        fetch(name, sources[name])
    try:
        for side, bin in (("base", a.base_bin), ("head", a.head_bin)):
            capture(side, str(Path(bin).resolve()), workspaces, sources, run)
    except (RuntimeError, subprocess.TimeoutExpired) as error:
        print(f"BLOCKED: {error}")
        return 2
    write(run, compare(load(run / "base"), load(run / "head")), a.base_label, a.head_label, a.sample)
    return 0


def cmd_diff(a):
    run = Path(a.run).resolve()
    write(run, compare(load(run / "base"), load(run / "head")), a.base_label, a.head_label, a.sample)
    return 0


def selftest_calls():
    def call(id, level, target=None, candidates=(), kind=None):
        row = {"id": id, "caller": "r::a.ts", "target": target, "level": level,
               "span": {"path": "a.ts", "start": 0, "end": 1}}
        if candidates:
            row["candidates"] = list(candidates)
        if kind:
            row["kind"] = kind
        return row

    base = {"calls": [call("same", "exact", "r::b.ts::f"), call("order", "ambiguous", None, ["x", "y"]),
                      call("up", "ambiguous", None, ["r::b.ts::f", "z"]), call("down", "exact", "r::b.ts::f"),
                      call("moved", "exact", "r::b.ts::f"), call("trim", "ambiguous", None, ["p", "q", "r"]),
                      call("gone", "declared", "r::b.ts::g"), call("free", None)],
            "symbols": [{"id": "r::b.ts::f", "span": {"path": "b.ts", "start": 5, "end": 9}}]}
    head = {"calls": [call("same", "exact", "r::b.ts::f"), call("order", "ambiguous", None, ["y", "x"]),
                      call("up", "exact", "r::b.ts::f"), call("down", "inferred", "r::b.ts::f"),
                      call("moved", "exact", "r::b.ts::h"), call("trim", "ambiguous", None, ["p", "q"]),
                      call("new", "exact", "r::b.ts::f", kind="type"), call("free", None)],
            "symbols": base["symbols"]}
    got = {c["key"]: c for c in call_changes(base, head)}
    assert sorted(got) == ["down", "gone", "moved", "new", "trim", "up"], got
    assert got["up"]["types"] == ["raised", "candidates removed"] and got["up"]["level"] == "ambiguous -> exact"
    assert got["down"]["types"] == ["dropped", "certain lost"]
    assert got["moved"]["types"] == ["retargeted", "certain lost"] and got["moved"]["level"] == "exact"
    assert got["trim"]["types"] == ["candidates removed"] and got["trim"]["removed"] == ["r"]
    assert got["gone"]["types"] == ["vanished", "certain lost"] and got["gone"]["level"] == "declared"
    assert got["new"]["types"] == ["new"] and got["new"]["use"] == "type"
    assert got["new"]["target_span"] == {"path": "b.ts", "start": 5, "end": 9}
    assert call_changes({"symbols": []}, {"calls": []}) == [] == call_changes(base, base)
    entries, pairs = call_tally([{**c, "workspace": "w", "entry": "e"} for c in got.values()])
    assert entries["w", "e"]["certain lost"] == 3 and entries["w", "e"]["candidates removed n"] == 3
    assert pairs["type", "exact"]["new"] == 1 and pairs["call", "ambiguous -> exact"]["raised"] == 1
    return base, head


def selftest_samples(tmp, base, head):
    for side, export in (("base", base), ("head", head)):
        (tmp / side / "w").mkdir(parents=True)
        (tmp / side / "w" / "status.txt").write_text("")
        (tmp / side / "w" / "a.export.json").write_text(json.dumps(export))
    (tmp / "work" / "head" / "w" / "a").mkdir(parents=True)
    (tmp / "work" / "head" / "w" / "a" / "a.ts").write_text("x = f()\n")
    changes = entry_calls(tmp)
    assert {c["entry"] for c in changes} == {"a"} and all(c["workspace"] == "w" for c in changes)
    many = [{"workspace": "w", "entry": f"e{i % 50}", "key": f"k{i}", "types": ["new"], "use": "type",
             "level": "exact", "removed": [], "gained": [], "base": None,
             "head": {"level": "exact", "target": "r::b.ts::f", "candidates": [], "span": {"path": "a.ts", "start": 0}},
             "target_span": None} for i in range(30000)]
    picked = call_samples(many, k=20)
    assert len(picked["new exact"]) == 20 and picked == call_samples(list(reversed(many)), k=20)
    assert all(len(v) == 0 for g, v in picked.items() if g != "new exact")
    lines = calls_section(many, tmp, k=20)
    assert sum(l.startswith("- w/") for l in lines) == 20 and sum(l.startswith("| w/") for l in lines) == 50
    line = sample_line({**many[0], "entry": "a"}, tmp)
    assert "a.ts:1 `x = f()`" in line and "at ?:? ``" in line, line      # target span unknown
    assert ":? ``" in sample_line(many[0], tmp / "absent")              # a replay without work copies
    assert calls_section([], tmp) == ["## Calls", "", "No call row changed."]
    # A certain row lost to an ambiguous or another target still names the target it held.
    got = {c["key"]: c for c in changes}
    lost = {**got["moved"], "head": {**got["moved"]["head"], "level": "ambiguous", "target": None}}
    assert "no target, was `r::b.ts::f`" in sample_line(lost, tmp), sample_line(lost, tmp)
    assert "`r::b.ts::h`" in sample_line(got["moved"], tmp) and "was `r::b.ts::f`" in sample_line(got["moved"], tmp)
    assert "removed `r`, 2 left" in sample_line(got["trim"], tmp)
    assert [c["key"] for c in call_samples(changes)["lost a candidate"]] == ["trim"], "a raise to exact is judged once"


def selftest_write(tmp, base, head):
    def side(run, name, export):
        (run / name / "w").mkdir(parents=True)
        (run / name / "w" / "status.txt").write_text("")
        (run / name / "w" / "a.export.json").write_text(json.dumps(export))

    link = {"consumer": "a::get", "provider": "b::serve", "contract": "http::GET::/x", "level": "inferred"}
    for run, head_calls in ((tmp / "good", head["calls"]), (tmp / "bad", [5])):
        side(run, "base", {**base, "links": [link]})
        side(run, "head", {**head, "calls": head_calls, "links": [{**link, "level": "exact"}]})
    for run in (tmp / "good", tmp / "bad"):
        with contextlib.redirect_stdout(io.StringIO()):
            write(run, compare(load(run / "base"), load(run / "head")), "v0", "v1", k=5)
    text = (tmp / "good" / "summary.md").read_text()
    assert text.index("## Links") < text.index("## Calls") and (tmp / "good" / "calls.json").is_file(), text
    # An export `load` takes and the calls comparison cannot: the section says so, the rest stands.
    text = (tmp / "bad" / "summary.md").read_text()
    assert "## Calls\n\nBLOCKED: AttributeError" in text and "## Links" in text, text
    assert (tmp / "bad" / "diff.json").is_file()


def selftest():
    def link(consumer, contract, level):
        return {"consumer": consumer, "provider": "github.com/acme/b::serve", "contract": contract, "level": level,
                "reason": "same name"}

    def side(folder, links, status):
        (folder / "w").mkdir(parents=True)
        (folder / "w" / "a.export.json").write_text(json.dumps({"links": links[:2]}))
        (folder / "w" / "b.export.json").write_text(json.dumps({"links": links}))  # a shared link shows twice
        (folder / "w" / "status.txt").write_text(status)
        return load(folder)

    links = [link("a::get", "http::GET::/x", "exact"), link("a::post", "http::POST::/y", "inferred"),
             link("a::run", "ci::github.com/acme/b/.github/workflows/build.yml", "exact"),
             link("a::lib", "package::pypi::acme", "inferred")]
    status = ("github.com/acme/a (a) indexed, 3 files\n"
              "  orphan channel::sns::alerts: no use; main.tf:3 module.t[0].aws_sns_topic.this (root live)\n"
              "  orphan resource::s3::logs: no provisioner; app.py:9 logs [inferred]\n"
              "github.com/acme/b (b) indexed, 1 files\n"
              "  orphan channel::sns::alerts: no use in the workspace; main.tf:3 module.t[0].aws_sns_topic.this (root live)\n"
              "  config files: 1 read, 0 skipped\n")
    with tempfile.TemporaryDirectory(prefix="release-diff-selftest-") as tmp:
        tmp = Path(tmp)
        base = side(tmp / "base", links, status)
        assert len(base["w"]["links"]) == 4, "a link two exports hold is one link"
        assert sorted(base["w"]["orphans"]) == [
            "github.com/acme/a | channel::sns::alerts | main.tf:3 module.t[0].aws_sns_topic.this (root live)",
            "github.com/acme/a | resource::s3::logs | app.py:9 logs",
            "github.com/acme/b | channel::sns::alerts | main.tf:3 module.t[0].aws_sns_topic.this (root live)"], base
        assert compare(base, side(tmp / "same", [dict(row) for row in reversed(links)], status)) == [], "equal sides differ"
        # One of each type: POST raised, CI dropped, package vanished, a new HTTP link, the SNS line of `a`
        # vanished, an image line appeared, and the S3 use moved its level.
        planted = [links[0], {**links[1], "level": "exact"}, {**links[2], "level": "ambiguous"},
                   link("a::put", "http::PUT::/z", "inferred")]
        moved = (status.replace("  orphan channel::sns::alerts: no use; main.tf:3 module.t[0].aws_sns_topic.this (root live)\n", "", 1)
                 .replace("[inferred]", "[exact]")
                 + "  orphan image::ghcr.io/acme/api: no builder; deploy.yaml:7 api\n")
        changes = compare(base, side(tmp / "head", planted, moved))
        got = {(c["statement"], c["type"], c["kind"], c["level"]) for c in changes}
        assert got == {("links", "raised", "http", "inferred -> exact"), ("links", "dropped", "ci", "exact -> ambiguous"),
                       ("links", "vanished", "package", "inferred"), ("links", "new", "http", "inferred"),
                       ("orphan lines", "vanished", "channel", "-"), ("orphan lines", "appeared", "image", "-"),
                       ("orphan lines", "level moved", "resource", "inferred -> exact")} and len(changes) == 7, changes
        counts = tally(changes)
        assert counts["http links", "inferred"] == {"new": 1}, counts
        text = summary(changes, "v0 (base)", "v1 (head)")
        assert "| http links | inferred -> exact | 0 | 0 | 1 | 0 |" in text, text
        assert "| image orphan lines | - | 1 | 0 | 0 |" in text, text
        assert "No link or orphan line change" in summary([], "v0", "v1")
    # The default selection: every URL-pinned entry and this checkout, in three workspaces, no probe.
    chosen = groups(public())
    assert set(chosen) == {"code", "infra", "ci"} and "meada" in chosen["code"], chosen
    assert all(not table()[n][1].startswith("probe:") for names in chosen.values() for n in names), chosen
    assert identity("meada") == "github.com/claudiomascaro/meada" and identity("a7-iac") == "github.com/zz-prove/a7-iac"
    with tempfile.TemporaryDirectory(prefix="release-diff-selftest-") as tmp:
        base_export, head_export = selftest_calls()
        selftest_samples(Path(tmp) / "samples", base_export, head_export)
        selftest_write(Path(tmp) / "write", base_export, head_export)
    print("release-diff selftest: equal sides give no diff; one planted change of each type is counted right")


def main():
    if sys.argv[1:] == ["--selftest"]:
        selftest()
        return 0
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    r = sub.add_parser("run")
    r.add_argument("--base-bin", required=True)
    r.add_argument("--head-bin", required=True)
    r.add_argument("--repos", help="comma-separated corpus.tsv or infra.tsv names (default: every public entry)")
    d = sub.add_parser("diff")
    for p in (r, d):
        p.add_argument("--run", required=True)
        p.add_argument("--base-label", default="base")
        p.add_argument("--head-label", default="candidate")
        p.add_argument("--sample", type=int, default=SAMPLE, metavar="N", help="calls rows sampled per group")
    a = parser.parse_args()
    return cmd_run(a) if a.command == "run" else cmd_diff(a)


if __name__ == "__main__":
    sys.exit(main())
