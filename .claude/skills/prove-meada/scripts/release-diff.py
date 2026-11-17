#!/usr/bin/env python3
"""The release diff (#852, ADR 0029): the links and orphan lines of a candidate against the previous tag.

  release-diff.py run --base-bin B --head-bin H --run DIR [--repos a,b] [--base-label T] [--head-label T]
  release-diff.py diff --run DIR [--base-label T] [--head-label T]   (again, over what `run` captured)
  release-diff.py --selftest                                         (no Meada)

`run` indexes each group of pinned public corpus entries as one workspace with each binary: the entries of
`corpus.tsv`, the A7 entries of `infra.tsv` and its CI entries (A9's corpus). The probes are this skill's own
inputs, not corpora, and enter only when `--repos` names them. It keeps `meada export` of each repository and
`meada status` of the workspace under DIR/base and DIR/head, then writes `diff.json` (every changed statement)
and `summary.md` (counts per pair and per level; a pair is a contract kind and a statement type, GLOSSARY).
A link is the same link when `prove.py`'s link key is (consumer, provider, contract, root); an orphan line is
the same line when its repository, contract and place are. Exit 0 with or without a diff: the diff goes to a
judge, it is never a verdict. Exit 2: a capture failed.
"""

import argparse
import collections
import json
import os
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


def tally(changes):
    """(pair, level) -> type -> count; a pair is `kind statement`."""
    out = collections.defaultdict(collections.Counter)
    for c in changes:
        out[f"{c['kind']} {c['statement']}", c["level"]][c["type"]] += 1
    return out


def summary(changes, base_label, head_label, captured=None):
    lines = ["# Release diff: links and orphan lines against the previous tag", "",
             f"- Base: {base_label}", f"- Candidate: {head_label}"]
    for side, info in (captured or {}).items():
        spent = ", ".join(f"{ws} {s} s" for ws, s in info["index_seconds"].items())
        lines.append(f"- {side}: `{info['version']}`, index seconds per workspace: {spent}")
    if not changes:
        return "\n".join(lines + ["", "No change: the candidate states the same links and orphan lines as the base."]) + "\n"
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


def write(run, changes, base_label, head_label):
    captured = {side: json.loads((run / side / "capture.json").read_text())
                for side in ("base", "head") if (run / side / "capture.json").is_file()}
    (run / "diff.json").write_text(json.dumps(changes, indent=1, sort_keys=True) + "\n")
    text = summary(changes, base_label, head_label, captured)
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
    write(run, compare(load(run / "base"), load(run / "head")), a.base_label, a.head_label)
    return 0


def cmd_diff(a):
    run = Path(a.run).resolve()
    write(run, compare(load(run / "base"), load(run / "head")), a.base_label, a.head_label)
    return 0


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
        assert "No change" in summary([], "v0", "v1")
    # The default selection: every URL-pinned entry and this checkout, in three workspaces, no probe.
    chosen = groups(public())
    assert set(chosen) == {"code", "infra", "ci"} and "meada" in chosen["code"], chosen
    assert all(not table()[n][1].startswith("probe:") for names in chosen.values() for n in names), chosen
    assert identity("meada") == "github.com/claudiomascaro/meada" and identity("a7-iac") == "github.com/zz-prove/a7-iac"
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
    a = parser.parse_args()
    return cmd_run(a) if a.command == "run" else cmd_diff(a)


if __name__ == "__main__":
    sys.exit(main())
