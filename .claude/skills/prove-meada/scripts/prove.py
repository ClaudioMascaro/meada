#!/usr/bin/env python3
"""Scripted checks of prove-meada: drive two meada binaries over the same inputs and compare.

  prove.py run --base-bin B --head-bin H --repos meada,click --run DIR [--queries F] [--http] [--overlay D]
  prove.py rebless --base SHA --run DIR
  prove.py deploy --base SHA --run DIR
  prove.py values --base-bin B --head-bin H --repos a7-probe,meada --run DIR [--http]
  prove.py iac --base-bin B --head-bin H --repos a7-iac,a7-names,terraform-sqs --run DIR [--http]   (python-hcl2)
  prove.py clean DIR

`run` indexes each corpus repository twice from cold with each binary (determinism and the
budget), exports it, refreshes after an mtime touch, drives `meada mcp` over stdio with the same
queries, and with `--http` serves the head's indexes over streamable HTTP for a parity check. It
prints "checks passed" or the differences found: the verdict is the checker's, never this script's.
`values` (A7) reads every config file of each entry with its own YAML and JSON oracle, then asks
the head for `meada status` and `search` with the kind `value` and compares each answer, line by
line and in order, with the oracle's; the base binary answers the default search and the export.
Exit 0: no difference, 1: differences or budget overruns, 2: a run failed (BLOCKED).
"""

import argparse
import difflib
import json
import os
import re
import shutil
import socket
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

import yaml  # PyYAML: the A7 oracle's own YAML reader

HERE = Path(__file__).resolve().parent
ROOT = Path(subprocess.check_output(["git", "rev-parse", "--show-toplevel"], cwd=HERE, text=True).strip())
CACHE = Path(os.environ.get("PROVE_MEADA_CACHE", Path.home() / ".cache/prove-meada"))
# Budgets set by the maintainer (2026-11-02): head over base, per corpus repository.
TIME, RSS, SIZE = 1.25, 1.25, 1.50
TIME_FLOOR = 2.0  # seconds: below it, timing noise outweighs the budget
TIMEOUT = 900
KEYED = {
    "symbols": ("id",),
    "calls": ("id",),
    "packages": ("ecosystem", "directory", "name"),
    "links": ("consumer", "provider", "contract"),
    "providers": ("contract", "site"),
    "consumers": ("contract", "site"),
}
WATCHED = {"calls": ("level", "target", "candidates"), "links": ("level",), "packages": ("version",), "consumers": ("via",)}
OTHER = ("files", "imports", "documents", "bindings", "publishes", "dependencies", "communities", "processes")
FIXED_GIT = {
    "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_AUTHOR_NAME": "prove-meada", "GIT_AUTHOR_EMAIL": "prove@example.com", "GIT_AUTHOR_DATE": "2026-01-01T00:00:00Z",
    "GIT_COMMITTER_NAME": "prove-meada", "GIT_COMMITTER_EMAIL": "prove@example.com", "GIT_COMMITTER_DATE": "2026-01-01T00:00:00Z",
}


def git(*args, cwd, **kw):
    return subprocess.run(["git", *args], cwd=cwd, env={**os.environ, **FIXED_GIT}, check=True,
                          capture_output=True, text=True, **kw).stdout


def corpus(table="corpus.tsv"):
    """name -> (packs or A7 flows, url, sha); `infra.tsv` adds a license column, kept for people."""
    rows = {}
    for line in (HERE / table).read_text().splitlines():
        if line and not line.startswith("#"):
            name, packs, url, sha = line.split("\t")[:4]
            rows[name] = (packs.split(","), url, sha)
    return rows


def infra():
    return corpus("infra.tsv")


def fetch(name, into):
    """A clean git checkout of corpus or infra entry `name` at `into`."""
    _, url, sha = {**infra(), **corpus()}[name]
    if url.startswith("probe:"):  # an A7 probe of this skill, a repository of its own
        shutil.copytree(HERE / url[len("probe:"):], into, symlinks=True)
        if (into / ".gitignore").is_file():  # one file stays on disk, ignored by .gitignore
            (into / "zz-gitignored.yaml").write_text("ignored: gitignored-zzprove\n")
        git("-c", "init.defaultBranch=main", "init", "-q", cwd=into)
        git("remote", "add", "origin", f"https://github.com/zz-prove/{name}.git", cwd=into)
        git("add", "-A", cwd=into)
        git("commit", "-q", "-m", "a7 probe", cwd=into)
        return
    if url == "self":
        into.mkdir(parents=True)
        archive = subprocess.run(["git", "archive", "HEAD"], cwd=ROOT, check=True, capture_output=True).stdout
        subprocess.run(["tar", "-x", "-C", str(into)], input=archive, check=True)
        git("-c", "init.defaultBranch=main", "init", "-q", cwd=into)
        git("remote", "add", "origin", "https://github.com/ClaudioMascaro/meada.git", cwd=into)
        git("add", "-A", cwd=into)
        git("commit", "-q", "-m", "corpus", cwd=into)
        return
    cached = CACHE / "corpus" / f"{name}@{sha}"
    if not (cached / ".git").is_dir():
        tmp = cached.with_name(cached.name + ".tmp")
        shutil.rmtree(tmp, ignore_errors=True)
        tmp.mkdir(parents=True)
        git("-c", "init.defaultBranch=main", "init", "-q", cwd=tmp)
        git("remote", "add", "origin", url + ".git", cwd=tmp)
        git("fetch", "-q", "--depth", "1", "origin", sha, cwd=tmp)
        git("checkout", "-q", "FETCH_HEAD", cwd=tmp)
        tmp.rename(cached)
    shutil.copytree(cached, into, symlinks=True)


# Linux carries a forked child's pre-exec peak into its ru_maxrss, so a child of this (large)
# process would report this process's size: a small fresh interpreter spawns and measures it.
MEASURE = """import json,os,subprocess,sys,threading,time
out,err,limit,cmd=sys.argv[1],sys.argv[2],float(sys.argv[3]),sys.argv[4:]
with open(out,"wb") as o,open(err,"wb") as e:
    t=time.monotonic();p=subprocess.Popen(cmd,stdin=subprocess.DEVNULL,stdout=o,stderr=e)
    k=threading.Timer(limit,p.kill);k.start();_,s,u=os.wait4(p.pid,0);k.cancel()
print(json.dumps({"exit":os.waitstatus_to_exitcode(s),"seconds":round(time.monotonic()-t,3),"rss_kb":u.ru_maxrss}))
"""


def timed(cmd, cwd, out, err):
    """Runs `cmd`, stdout to `out` and stderr to `err`; its own exit, wall time and peak RSS."""
    r = subprocess.run([sys.executable, "-I", "-c", MEASURE, str(out), str(err), str(TIMEOUT), *map(str, cmd)],
                       cwd=cwd, capture_output=True, text=True, check=True)
    return json.loads(r.stdout)


class Stdio:
    """A minimal MCP client over `meada mcp`'s stdio: no roots, so it answers about `cwd`."""

    def __init__(self, bin, cwd, log):
        self.p = subprocess.Popen([bin, "mcp"], cwd=cwd, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=open(log, "wb"), text=True, bufsize=1)
        self.id = 0
        self.info = self.call("initialize", {"protocolVersion": "2025-11-25", "capabilities": {},
                                             "clientInfo": {"name": "prove-meada", "version": "1"}})
        self.p.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n")

    def call(self, method, params):
        self.id += 1
        self.p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": self.id, "method": method, "params": params}) + "\n")
        while True:
            line = self.p.stdout.readline()
            if not line:
                raise RuntimeError(f"meada mcp closed stdout during {method}")
            msg = json.loads(line)
            if msg.get("id") == self.id and "method" not in msg:
                return msg

    def close(self):
        self.p.stdin.close()
        self.p.wait(timeout=30)


class Http:
    """A minimal streamable HTTP MCP client for one repository of `meada mcp --listen`."""

    def __init__(self, url):
        self.url, self.session, self.id = url, None, 0
        self.info = self.call("initialize", {"protocolVersion": "2025-11-25", "capabilities": {},
                                             "clientInfo": {"name": "prove-meada", "version": "1"}})
        self.post({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def post(self, body):
        headers = {"Content-Type": "application/json", "Accept": "application/json, text/event-stream"}
        if self.session:
            headers["Mcp-Session-Id"] = self.session
        req = urllib.request.Request(self.url, json.dumps(body).encode(), headers)
        with urllib.request.urlopen(req, timeout=120) as r:
            self.session = r.headers.get("Mcp-Session-Id") or self.session
            text = r.read().decode()
        if r.headers.get_content_type() == "text/event-stream":
            datas = [line[5:].strip() for line in text.splitlines() if line.startswith("data:")]
            text = next((d for d in datas if d), "")
        return json.loads(text) if text.strip() else None

    def call(self, method, params):
        self.id += 1
        return self.post({"jsonrpc": "2.0", "id": self.id, "method": method, "params": params})


def answer(msg):
    """The text of a tools/call reply, or the error, so both sides compare the same way."""
    if "error" in msg:
        return "ERROR " + json.dumps(msg["error"], sort_keys=True)
    result = msg["result"]
    text = "".join(c.get("text", "") for c in result.get("content", []))
    return ("TOOL ERROR " if result.get("isError") else "") + text


def short_name(sym):
    return sym.rsplit("::", 1)[-1].split("(")[0].rsplit(".", 1)[-1]  # Kotlin and Swift signatures carry dots


def auto_queries(export):
    """Three symbols with the most resolved callers, each through every tool; then every tool
    argument once, on the first of them: an argument the server parses and drops leaves the
    default answers identical (audit 2026-11-03, M1 and M2)."""
    count = {}
    for c in export.get("calls", []):
        if c.get("target"):
            count[c["target"]] = count.get(c["target"], 0) + 1
    top = sorted(count, key=lambda s: (-count[s], s))[:3]
    queries = []
    for sym in top:
        queries += [{"tool": "search", "arguments": {"query": short_name(sym)}},
                    {"tool": "context", "arguments": {"symbol": sym}},
                    {"tool": "trace", "arguments": {"from": sym}}]
    if top:
        sym, name = top[0], short_name(top[0])
        queries += [{"tool": "search", "arguments": {"query": name, "budget": 300}},
                    {"tool": "search", "arguments": {"query": name, "kind": "contract"}},  # a symbol name: contracts only
                    {"tool": "context", "arguments": {"symbol": sym, "budget": 300}},
                    {"tool": "trace", "arguments": {"from": sym, "direction": "up", "budget": 300}}]
    contracts = sorted({p["contract"] for p in export.get("providers", [])})
    if contracts:
        queries.append({"tool": "search", "arguments": {"query": contracts[0], "kind": "contract"}})
    # Error parity: an unknown kind and an unknown symbol must be refused the same way by both binaries.
    queries.append({"tool": "search", "arguments": {"query": "zz_prove_missing", "kind": "zz_prove_kind"}})
    queries.append({"tool": "context", "arguments": {"symbol": "zz_prove_missing"}})
    return queries


def body(export_bytes):
    """An export without `commit` and `dirty`: checkout facts read at export time (ADR 0004, #512),
    absent while an index is not fresh, so they never say whether the index itself moved."""
    return re.sub(rb'(?m)^  "(commit|dirty)": [^\n]*\n', b"", export_bytes)


def quoted(sym_id):
    return ('"' + sym_id + '"').encode()


def rename_in_place(repo, export):
    """A same-size edit of one symbol's name in its file, so the refresh path has new content to
    pick up (audit 2026-11-03, M7: a size-only shortcut never re-indexed it). Returns the file,
    its original bytes, the edit, the symbol's id and its renamed id, or None when no symbol suits."""
    for s in sorted(export.get("symbols", []), key=lambda s: s["id"]):
        name = short_name(s["id"])
        rel = s.get("span", {}).get("path", "")
        path = repo / rel
        head, sep, tail = s["id"].partition(f"::{rel}::")
        if len(name) < 3 or not name.isidentifier() or name.endswith("Q") or not sep or not path.is_file():
            continue
        data = path.read_bytes()
        word = re.compile(rb"\b" + re.escape(name.encode()) + rb"\b")
        new = word.sub(name[:-1].encode() + b"Q", data)
        if new != data:
            path.write_bytes(new)
            new_id = f"{head}::{rel}::" + word.sub(name[:-1].encode() + b"Q", tail.encode()).decode()
            return path, data, f"{rel}: {name} -> {name[:-1]}Q", s["id"], new_id
    return None


def drive(bin, repo, out, queries_file):
    """Everything one binary does over one repository, recorded in `out`."""
    out.mkdir(parents=True)
    rec = {"bin": str(bin), "version": subprocess.check_output([bin, "--version"], text=True).strip()}
    for n in (1, 2):
        shutil.rmtree(repo / ".meada", ignore_errors=True)
        rec[f"index{n}"] = timed([bin, "index"], repo, out / f"index{n}.out", out / f"index{n}.err")
        rec[f"export{n}"] = timed([bin, "export"], repo, out / f"export{n}.json", out / f"export{n}.err")
        if rec[f"index{n}"]["exit"] or rec[f"export{n}"]["exit"]:
            rec["failed"] = f"index or export {n} exited non-zero, see {out}/*{n}.err"
            (out / "record.json").write_text(json.dumps(rec, indent=1, sort_keys=True))
            return rec
    export = json.loads((out / "export1.json").read_text())
    if not export.get("symbols"):
        rec["failed"] = f"the export of {repo} has no symbol: an empty index proves nothing"
        (out / "record.json").write_text(json.dumps(rec, indent=1, sort_keys=True))
        return rec
    cold1 = (out / "export1.json").read_bytes()
    rec["deterministic"] = cold1 == (out / "export2.json").read_bytes()
    tracked = git("ls-files", cwd=repo).splitlines()
    if tracked:
        os.utime(repo / tracked[0])  # same bytes, new mtime: the freshness path must give the same export
        rec["refresh"] = timed([bin, "export"], repo, out / "export3.json", out / "export3.err")
        rec["refresh_equal"] = (out / "export3.json").read_bytes() == cold1
    edited = rename_in_place(repo, export)
    if edited:  # same size, new bytes: the warm export must equal a cold index of the edited tree
        path, original, rec["content_refresh_edit"], old_id, new_id = edited
        rec["content_refresh"] = timed([bin, "export"], repo, out / "export4.json", out / "export4.err")
        shutil.rmtree(repo / ".meada", ignore_errors=True)
        rec["index3"] = timed([bin, "index"], repo, out / "index3.out", out / "index3.err")
        rec["export5"] = timed([bin, "export"], repo, out / "export5.json", out / "export5.err")
        if rec["index3"]["exit"] or rec["export5"]["exit"]:
            rec["failed"] = f"index or export after the in-place edit exited non-zero, see {out}/*3.err and export5.err"
            (out / "record.json").write_text(json.dumps(rec, indent=1, sort_keys=True))
            return rec
        warm, cold = (out / "export4.json").read_bytes(), (out / "export5.json").read_bytes()
        rec["content_refresh_equal"] = body(warm) == body(cold)
        rec["content_refresh_changed"] = body(cold) != body(cold1)  # else the edit reached nothing indexed: inconclusive
        rec["content_refresh_stale"] = body(warm) == body(cold1)  # the edit never reached the index: the M7 symptom itself
        rec["content_refresh_renamed"] = quoted(new_id) in warm  # the direct question: is the renamed symbol in the warm export
        if rec["content_refresh_equal"]:
            (out / "export4.json").unlink()
            (out / "export5.json").unlink()
        path.write_bytes(original)  # back through the refresh path: the queries below are about the original tree
        rec["restore"] = timed([bin, "export"], repo, out / "export6.json", out / "export6.err")
        back = (out / "export6.json").read_bytes()
        rec["restore_equal"] = body(back) == body(cold1)
        rec["restore_original"] = quoted(old_id) in back and quoted(new_id) not in back
        if rec["restore_equal"]:
            (out / "export6.json").unlink()
    rec["export_bytes"] = len(cold1)
    for n in ("export2.json", "export3.json"):  # kept only when they differ: an export can be 28 MB
        if (out / n).exists() and (out / n).read_bytes() == cold1:
            (out / n).unlink()
    if not queries_file.exists():
        queries_file.write_text("".join(json.dumps(q) + "\n" for q in auto_queries(export)))
    queries = [json.loads(line) for line in queries_file.read_text().splitlines() if line.strip()]
    try:
        mcp = Stdio(bin, repo, out / "mcp.err")
        rec["tools"] = mcp.call("tools/list", {}).get("result", {}).get("tools")
        rec["answers"] = [answer(mcp.call("tools/call", {"name": q["tool"], "arguments": q["arguments"]})) for q in queries]
        mcp.close()
    except Exception as e:  # a crashed server is BLOCKED, never an empty answer
        rec["failed"] = f"mcp stdio: {e!r}, see {out}/mcp.err"
    rec["queries"] = queries
    (out / "record.json").write_text(json.dumps(rec, indent=1, sort_keys=True))
    return rec


def keyed(section, v):
    return " | ".join(str(v.get(p)) for p in KEYED[section])


def short(field, value):
    """golden_diff's short form: a target is `set`, a list is counted, so moves group."""
    if field == "target" and isinstance(value, str):
        return "set"
    if isinstance(value, list):
        return f"[{len(value)} items]"
    s = str(value)
    return s if len(s) <= 60 else s[:57] + "..."


def line_of(repo, path, offset):
    try:
        data = (repo / path).read_bytes()
    except OSError:
        return "?", ""
    n = data.count(b"\n", 0, offset) + 1
    start = data.rfind(b"\n", 0, offset) + 1
    end = data.find(b"\n", offset)
    return n, data[start:end if end >= 0 else None].decode(errors="replace").strip()[:120]


def export_diff(base, head, repo=None, cap=20):
    """Per-section differences of two exports, golden_diff's keys; and the new or changed
    resolutions worth confirming in the source."""
    lines, sample = [], []
    for field in ("format", "repository", "commit", "dirty"):
        if base.get(field) != head.get(field):
            lines.append(f"  {field}: {base.get(field)} -> {head.get(field)}")
    symbols = {s["id"]: s for s in head.get("symbols", [])}
    for section, _ in KEYED.items():
        b = {keyed(section, v): v for v in base.get(section, [])}
        h = {keyed(section, v): v for v in head.get(section, [])}
        added = sorted(set(h) - set(b))
        removed = sorted(set(b) - set(h))
        changed = sorted(k for k in set(b) & set(h) if b[k] != h[k])
        if not (added or removed or changed):
            continue
        lines.append(f"  {section}: +{len(added)} -{len(removed)} ~{len(changed)}")
        lines += [f"    + {k}" for k in added[:cap]] + ([f"    ... {len(added) - cap} more added"] if len(added) > cap else [])
        lines += [f"    - {k}" for k in removed[:cap]] + ([f"    ... {len(removed) - cap} more removed"] if len(removed) > cap else [])
        moves = {}
        for k in changed:
            for f in WATCHED.get(section, ()):
                if b[k].get(f) != h[k].get(f):
                    old, new = short(f, b[k].get(f)), short(f, h[k].get(f))
                    moves.setdefault(f"{f}: changed" if old == new else f"{f}: {old} -> {new}", []).append(k)
        for label, keys in sorted(moves.items()):
            lines.append(f"    {label}: {len(keys)}")
            lines += [f"      {k}" for k in keys[:cap]]
        for k in added + changed:
            v = h[k]
            if section == "calls" and v.get("target") and v.get("level") in ("exact", "inferred"):
                sample.append((section, k, v, b.get(k)))
            if section == "links":
                sample.append((section, k, v, b.get(k)))
    for section in OTHER:
        bs = {json.dumps(v, sort_keys=True) for v in base.get(section, [])}
        hs = {json.dumps(v, sort_keys=True) for v in head.get(section, [])}
        if bs != hs:
            lines.append(f"  {section}: +{len(hs - bs)} -{len(bs - hs)}")
    rendered = []
    for section, k, v, old in sample:
        if section == "calls" and repo is not None:
            site = v["span"]
            sl, st = line_of(repo, site["path"], site["start"])
            tgt = symbols.get(v["target"], {}).get("span")
            tl, tt = line_of(repo, tgt["path"], tgt["start"]) if tgt else ("?", "")
            rendered.append(f"- call `{k}` [{v['level']}] (was {old.get('level') if old else 'absent'})\n"
                            f"  site {site['path']}:{sl}: `{st}`\n"
                            f"  target `{v['target']}` at {tgt['path'] if tgt else '?'}:{tl}: `{tt}`")
        elif section == "links":
            rendered.append(f"- link `{k}` [{v.get('level')}] (was {old.get('level') if old else 'absent'}), reason `{v.get('reason')}`")
    return lines, rendered


def spread(items, n):
    """At most `n` items spread evenly over the sorted list, so the pick is the same every run."""
    if len(items) <= n:
        return items
    step = len(items) / n
    return [items[int(i * step)] for i in range(n)]


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def http_parity(bin, name, repo, run, rec):
    """Serves one head repository alone over streamable HTTP and asks the stdio queries again.
    Alone: a manifest of several repositories makes one workspace, and `search` spans it."""
    d = run / "http" / name
    d.mkdir(parents=True)
    identity = json.loads((run / "head" / name / "export1.json").read_text())["repository"]
    (d / "meada.workspace.toml").write_text(f'index_dir = "indexes"\n\n[[repository]]\nrepo = "{identity}"\npath = "{repo}"\n')
    idx = timed([bin, "index", "--workspace", "meada.workspace.toml"], d, d / "index.out", d / "index.err")
    if idx["exit"]:
        return f"  {name}: BLOCKED, meada index --workspace exited {idx['exit']}, see {d}/index.err", True
    port = free_port()
    server = subprocess.Popen([bin, "mcp", "--listen", f"127.0.0.1:{port}", "--workspace", "meada.workspace.toml"],
                              cwd=d, stdin=subprocess.DEVNULL,
                              stdout=open(d / "server.out", "wb"), stderr=open(d / "server.err", "wb"))
    (d / "server.pid").write_text(str(server.pid))
    try:
        for _ in range(100):
            try:
                socket.create_connection(("127.0.0.1", port), timeout=1).close()
                break
            except OSError:
                time.sleep(0.2)
        client = Http(f"http://127.0.0.1:{port}/{identity}")
        tools = client.call("tools/list", {}).get("result", {}).get("tools")
        same_tools = json.dumps(tools, sort_keys=True) == json.dumps(rec.get("tools"), sort_keys=True)
        answers = [answer(client.call("tools/call", {"name": q["tool"], "arguments": q["arguments"]})) for q in rec["queries"]]
        (d / "answers.json").write_text(json.dumps(answers, indent=1))
        diffs = [i for i, a in enumerate(answers) if a != rec["answers"][i]]
        return (f"  {name}: tools {'same' if same_tools else 'DIFFER'}, {len(answers) - len(diffs)}/{len(answers)} answers "
                f"byte-equal to stdio" + (f", differ at queries {diffs} ({d}/answers.json)" if diffs else "")), bool(diffs) or not same_tools
    except Exception as e:
        return f"  {name}: BLOCKED, HTTP client: {e!r}, see {d}/server.err", True
    finally:
        server.terminate()
        server.wait(timeout=30)


def cmd_run(a):
    run = Path(a.run).resolve()
    names = list(corpus()) if a.repos == "all" else a.repos.split(",")
    unknown = [n for n in names if n not in corpus()]
    if unknown:
        sys.exit(f"unknown corpus entries {unknown}; see {HERE}/corpus.tsv")
    run.mkdir(parents=True, exist_ok=True)
    report, differences, blocked = [], 0, False
    records = {"base": {}, "head": {}}
    for name in names:
        src = run / "work" / "src" / name
        if not src.exists():
            fetch(name, src)
            if a.overlay:  # a probe: the same changed source reaches both binaries
                shutil.copytree(a.overlay, src, dirs_exist_ok=True)
                git("add", "-A", cwd=src)
                git("commit", "-q", "-m", "probe overlay", cwd=src)
        qfile = run / "queries" / f"{name}.jsonl"
        qfile.parent.mkdir(exist_ok=True)
        if a.queries and not qfile.exists():
            shutil.copy(a.queries, qfile)
        for side, bin in (("head", a.head_bin), ("base", a.base_bin)):  # head first: it writes the auto queries
            repo = run / "work" / side / name
            shutil.rmtree(repo, ignore_errors=True)
            shutil.copytree(src, repo, symlinks=True)
            records[side][name] = drive(Path(bin).resolve(), repo, run / side / name, qfile)
        b, h = records["base"][name], records["head"][name]
        report.append(f"## {name}\n\n{b['version']} (base) vs {h['version']} (head)\n")
        if b.get("failed") or h.get("failed"):
            blocked = True
            report.append(f"BLOCKED: base {b.get('failed')}; head {h.get('failed')}\n")
            continue
        flags = ("refresh_equal", "content_refresh_equal", "content_refresh_changed", "content_refresh_renamed",
                 "restore_equal", "restore_original")
        det = [r["deterministic"] and all(r.get(k, True) for k in flags) for r in (b, h)]
        differences += det.count(False)
        report.append(f"Determinism: cold twice equal base {b['deterministic']}, head {h['deterministic']}; "
                      f"refresh equal base {b.get('refresh_equal')}, head {h.get('refresh_equal')}; "
                      f"content refresh equal base {b.get('content_refresh_equal')}, head {h.get('content_refresh_equal')} "
                      f"(edit {h.get('content_refresh_edit')}); renamed symbol in the warm export base "
                      f"{b.get('content_refresh_renamed')}, head {h.get('content_refresh_renamed')}; "
                      f"restore equal base {b.get('restore_equal')}, head {h.get('restore_equal')}; "
                      f"original symbol back base {b.get('restore_original')}, head {h.get('restore_original')}"
                      + ("" if all(r.get("content_refresh_changed", True) for r in (b, h)) else "  EDIT CHANGED NOTHING: inconclusive")
                      + ("" if not any(r.get("content_refresh_stale") for r in (b, h)) else
                         f"  STALE: the edit never reached the index (base {b.get('content_refresh_stale')}, head {h.get('content_refresh_stale')})")
                      + ("" if all(det) else "  NOT DETERMINISTIC"))
        if not all(det):  # the diff the checker would otherwise make by hand on 28 MB; a head-only move shows next to a shared one
            for side in ("base", "head"):
                d = run / side / name
                for label, x, y in (("warm vs cold (edited tree)", "export4.json", "export5.json"),
                                    ("original vs restored", "export1.json", "export6.json")):
                    if (d / x).exists() and (d / y).exists():
                        lines, _ = export_diff(json.loads((d / x).read_text()), json.loads((d / y).read_text()), cap=4)
                        report.append(f"  {side} {label}:\n" + ("\n".join("  " + l for l in lines) or "    identical"))
        budget = []
        bt, ht = min(b["index1"]["seconds"], b["index2"]["seconds"]), min(h["index1"]["seconds"], h["index2"]["seconds"])
        br, hr = max(b["index1"]["rss_kb"], b["export1"]["rss_kb"]), max(h["index1"]["rss_kb"], h["export1"]["rss_kb"])
        for label, bv, hv, limit, applies in (("index seconds", bt, ht, TIME, bt >= TIME_FLOOR),
                                              ("peak RSS KiB", br, hr, RSS, True),
                                              ("export bytes", b["export_bytes"], h["export_bytes"], SIZE, True)):
            over = applies and hv > bv * limit
            differences += over
            budget.append(f"  {label}: {bv} -> {hv} (limit x{limit}{'' if applies else ', not applied under 2 s'})"
                          + (" OVER BUDGET" if over else ""))
        report.append("Budget:\n" + "\n".join(budget))
        lines, sample = export_diff(json.loads((run / "base" / name / "export1.json").read_text()),
                                    json.loads((run / "head" / name / "export1.json").read_text()),
                                    run / "work" / "head" / name)
        differences += bool(lines)
        report.append("Export, base -> head:\n" + ("\n".join(lines) if lines else "  identical"))
        if sample:
            picked = spread(sample, a.sample)
            report.append(f"Confirm in the source ({len(picked)} of {len(sample)} new or changed resolutions):\n" + "\n".join(picked))
        if json.dumps(b["tools"], sort_keys=True) != json.dumps(h["tools"], sort_keys=True):
            differences += 1
            report.append("MCP tools/list: DIFFERS from base (the 1.0 surface, tests/golden/mcp.tools.json)")
        for i, q in enumerate(h["queries"]):
            if b["answers"][i] != h["answers"][i]:
                differences += 1
                diff = list(difflib.unified_diff(b["answers"][i].splitlines(), h["answers"][i].splitlines(), "base", "head", lineterm="", n=1))
                report.append(f"MCP {q['tool']} {json.dumps(q['arguments'])}: differs\n```\n" + "\n".join(diff[:60]) + "\n```")
        same = sum(b["answers"][i] == h["answers"][i] for i in range(len(h["queries"])))
        report.append(f"MCP stdio: {same}/{len(h['queries'])} answers byte-equal; queries in {run}/queries/{name}.jsonl\n")
    if a.http and not blocked:
        lines = []
        for n in names:
            line, failed = http_parity(Path(a.head_bin).resolve(), n, run / "work" / "head" / n, run, records["head"][n])
            lines.append(line)
            differences += failed
        report.append("## MCP HTTP vs stdio (head)\n\n" + "\n".join(lines) + "\n")
    verdict = "BLOCKED: a run failed" if blocked else ("checks passed" if not differences else f"differences found: {differences}")
    (run / "compare.md").write_text(f"# prove.py run\n\n{verdict}\n\n" + "\n".join(report) + "\n")
    print(verdict)
    print(f"full report: {run}/compare.md")
    sys.exit(2 if blocked else 1 if differences else 0)


def cmd_rebless(a):
    """Every golden the change touched, against the golden at the base: the base is the answer key."""
    run = Path(a.run).resolve()
    run.mkdir(parents=True, exist_ok=True)
    status = git("diff", "--name-status", a.base, "--", "tests/golden", cwd=ROOT).splitlines()
    status += [f"A\t{p}" for p in git("ls-files", "--others", "--exclude-standard", "tests/golden", cwd=ROOT).splitlines()]
    out = [f"# Goldens changed since {a.base}\n"]
    for row in status:
        kind, path = row.split("\t")[0], row.split("\t")[-1]
        out.append(f"## {kind} {path}\n")
        if kind.startswith("M") and path.endswith(".export.json"):
            base = json.loads(git("show", f"{a.base}:{path}", cwd=ROOT))
            lines, _ = export_diff(base, json.loads((ROOT / path).read_text()))
            out.append("\n".join(lines) or "  only formatting changed")
        elif kind.startswith("A"):
            text = (ROOT / path).read_text(errors="replace").splitlines()
            out.append("New golden, first lines:\n```\n" + "\n".join(text[:40]) + "\n```")
        else:
            diff = git("diff", a.base, "--", path, cwd=ROOT).splitlines()
            out.append("```diff\n" + "\n".join(diff[:150]) + ("\n... truncated" if len(diff) > 150 else "") + "\n```")
        out.append("")
    (run / "rebless.md").write_text("\n".join(out))
    print(f"{len(status)} golden(s) changed since {a.base}" + ("" if status else ": nothing to audit"))
    for row in status:
        print("  " + row.replace("\t", " "))
    print(f"full report: {run}/rebless.md")


COMPOSE = (  # name, files and profiles, env file: the renders scripts/compose-check checks
    ("local", ["-f", "compose.yaml", "--profile", "local"], ".env.example"),
    ("generations", ["-f", "compose.yaml", "--profile", "generations"], ".env.example"),
    ("console", ["-f", "compose.yaml", "-f", "compose.console.yaml", "--profile", "generations", "--profile", "console"], ".env.example"),
    ("postgres", ["-f", "compose.yaml", "-f", "compose.console.yaml", "-f", "compose.postgres.yaml",
                  "--profile", "generations", "--profile", "console"], ".env.example"),
    ("accounts", [], ".env.accounts.example"),
)


def renders(root, out):
    """Every Compose and Helm render of the tree at `root`, normalised, into `out`."""
    out.mkdir(parents=True)
    notes = []
    compose = root / "deploy/compose"
    if compose.is_dir() and not shutil.which("docker"):
        notes.append("docker not on PATH: no Compose render (Not covered; CI's compose.yml renders it)")
    for name, args, env in COMPOSE if compose.is_dir() and shutil.which("docker") else ():
        r = subprocess.run(["docker", "compose", "--env-file", env, *args, "config", "--format", "json"],
                           cwd=compose, capture_output=True, text=True)
        text = r.stdout if r.returncode == 0 else f"render failed ({r.returncode}): {r.stderr}"
        (out / f"compose-{name}.json").write_text(text.replace(str(root), "<root>"))
    # A render carries a mounted file's path, never its text (audit 2026-11-03, D3): the files the
    # stack mounts join the diff as they are.
    for f in sorted(p for pat in ("Caddyfile*", "*.toml", "*.sh", ".env*") for p in compose.glob(pat)) if compose.is_dir() else ():
        shutil.copy(f, out / f"mounted-{f.name}")
    if shutil.which("helm"):
        chart = root / "deploy/helm/meada"
        # The overlays render in the stack their headers name, as scripts/helm-check does.
        stacks = {"values-identity": ["values-console", "values-identity"],
                  "values-workspaces": ["values-console", "values-identity", "values-workspaces"]}
        for values in sorted(chart.glob("values*.yaml")):
            files = [f for n in stacks.get(values.stem, [values.stem]) for f in ("-f", str(chart / f"{n}.yaml"))]
            r = subprocess.run(["helm", "template", "check", str(chart), *files], capture_output=True, text=True)
            text = r.stdout if r.returncode == 0 else f"render failed ({r.returncode}): {r.stderr}"
            (out / f"helm-{values.stem}.yaml").write_text(text.replace(str(root), "<root>"))
    else:
        notes.append("helm not on PATH: no chart render (Not covered; CI's helm.yml renders it)")
    return notes


def cmd_deploy(a):
    """The deploy renders of the base and of this working tree; the base is the answer key."""
    run = Path(a.run).resolve()
    src = run / "deploy-base-src"
    git("worktree", "add", "-q", "--detach", str(src), a.base, cwd=ROOT)
    try:
        notes = renders(src, run / "deploy" / "base")
    finally:
        git("worktree", "remove", "--force", str(src), cwd=ROOT)
    renders(ROOT, run / "deploy" / "head")
    lines, differences = [], 0
    for head in sorted((run / "deploy" / "head").iterdir()):
        base = run / "deploy" / "base" / head.name
        b = base.read_text().splitlines() if base.exists() else []
        h = head.read_text().splitlines()
        if "render failed" in (h[:1] or [""])[0]:
            differences += 1
            lines.append(f"{head.name}: head render FAILED\n```\n{chr(10).join(h[:20])}\n```")
        elif b != h:
            differences += 1
            diff = list(difflib.unified_diff(b, h, "base", "head", lineterm="", n=2))
            lines.append(f"{head.name}: differs ({len(diff)} diff lines)\n```diff\n" + "\n".join(diff[:80]) + "\n```")
        else:
            lines.append(f"{head.name}: identical")
    verdict = "checks passed" if not differences else f"differences found: {differences}"
    (run / "deploy.md").write_text(f"# prove.py deploy\n\n{verdict}\n\n" + "\n".join(notes + lines) + "\n")
    print(verdict)
    print("\n".join(notes + [l.splitlines()[0] for l in lines]))
    print(f"full report: {run}/deploy.md")
    sys.exit(1 if differences else 0)


def cmd_selftest(_):
    """The comparison logic on two small exports: run after editing this script."""
    base = {"format": 1, "symbols": [{"id": "r::a.rs::f"}, {"id": "r::a.rs::g"}],
            "calls": [{"id": "c1", "target": None, "level": None}, {"id": "c2", "target": "r::a.rs::g", "level": "exact"},
                      {"id": "c3", "target": None, "level": "ambiguous", "candidates": ["x", "y"]}],
            "links": [], "communities": [[1]]}
    head = json.loads(json.dumps(base))
    head["calls"][0].update(target="r::a.rs::f", level="exact", span={"path": "a.rs", "start": 0})
    head["calls"][1].update(target=None, level=None)
    head["calls"][2]["candidates"] = ["x", "y", "z"]
    head["symbols"][0]["span"] = {"path": "a.rs", "start": 0}
    lines, sample = export_diff(base, head)
    text = "\n".join(lines)
    assert "calls: +0 -0 ~3" in text, text
    assert "level: None -> exact: 1" in text and "level: exact -> None: 1" in text, text
    assert "target: None -> set: 1" in text and "target: set -> None: 1" in text, text
    assert "candidates: [2 items] -> [3 items]: 1" in text, text
    assert "communities" not in text and len(sample) == 0, (text, sample)  # no repo: nothing rendered
    assert export_diff(base, json.loads(json.dumps(base))) == ([], [])
    assert spread(list(range(100)), 4) == [0, 25, 50, 75] and spread([1, 2], 10) == [1, 2]
    assert short("target", "r::x") == "set" and short("candidates", [1]) == "[1 items]"
    q = auto_queries({"calls": [{"target": "r::k.kt::p.Flow.test(Duration?,String?)"}], "providers": []})
    assert q[0] == {"tool": "search", "arguments": {"query": "test"}}, q[0]
    assert q[-1]["arguments"] == {"symbol": "zz_prove_missing"}
    assert q[-2]["arguments"]["kind"] == "zz_prove_kind" and q[3]["arguments"]["budget"] == 300, q
    assert {"tool": "context", "arguments": {"symbol": "r::k.kt::p.Flow.test(Duration?,String?)", "budget": 300}} in q
    assert auto_queries({"calls": [], "providers": []})[-1]["arguments"] == {"symbol": "zz_prove_missing"}
    import tempfile
    with tempfile.TemporaryDirectory() as d:
        f = Path(d) / "a.rs"
        f.write_bytes(b"fn foo() {}\nfn fooBar() { foo() }\n")
        got = rename_in_place(Path(d), {"symbols": [{"id": "r::a.rs::fooBar", "span": {"path": "a.rs"}},
                                                     {"id": "r::a.rs::foo", "span": {"path": "a.rs"}}]})
        assert got and got[2] == "a.rs: foo -> foQ" and got[1] == b"fn foo() {}\nfn fooBar() { foo() }\n", got
        assert got[3] == "r::a.rs::foo" and got[4] == "r::a.rs::foQ", got
        assert f.read_bytes() == b"fn foQ() {}\nfn fooBar() { foQ() }\n", f.read_bytes()
        assert rename_in_place(Path(d), {"symbols": [{"id": "r::a.rs::ab", "span": {"path": "a.rs"}}]}) is None
        assert rename_in_place(Path(d), {"symbols": [{"id": "r::other::fooBar", "span": {"path": "a.rs"}}]}) is None
    e = b'{\n  "format": 1,\n  "repository": "r",\n  "commit": "abc",\n  "dirty": false,\n  "files": []\n}\n'
    assert body(e) == b'{\n  "format": 1,\n  "repository": "r",\n  "files": []\n}\n', body(e)
    assert body(e) == body(e.replace(b'  "commit": "abc",\n  "dirty": false,\n', b"")) and quoted("r::a") == b'"r::a"'
    print("selftest passed")


# A7 config values: the oracle reads what ADR 0025 and #642's user stories 6 to 13 say Meada
# reads, on its own (PyYAML's composer and Python's json), and never looks at Meada's tables.
LOCKFILES = {"package-lock.json", "npm-shrinkwrap.json", "pnpm-lock.yaml", "packages.lock.json"}
HIDDEN_CI = {".github", ".gitlab-ci.yml", ".gitlab", ".circleci", ".buildkite", ".drone.yml"}
CONFIG_MAX_BYTES, VALUE_MAX, DOCUMENTS_MAX = 1 << 20, 1024, 1000
PLAIN_KEY = re.compile(r"[A-Za-z_][A-Za-z0-9_]*\Z")
CORE_NULL = re.compile(r"(~|null|Null|NULL|)\Z")
CORE_BOOL = re.compile(r"(true|True|TRUE|false|False|FALSE)\Z")
CORE_NUMBER = re.compile(r"([-+]?[0-9]+|0o[0-7]+|0x[0-9a-fA-F]+|[-+]?(\.[0-9]+|[0-9]+(\.[0-9]*)?)([eE][-+]?[0-9]+)?"
                         r"|[-+]?\.(inf|Inf|INF)|\.(nan|NaN|NAN))\Z")
STATUS_LINE = re.compile(r"config files: (\d+) read, (\d+) skipped \(lockfiles: (\d+), unreadable: (\d+)\); "
                         r"values: (\d+), redacted: (\d+)")


def rooted_patterns(repo, names=(".meadaignore", ".ignore")):
    """Every `.meadaignore` and `.ignore` of the tree as one excludes file of root-relative
    patterns, under gitignore's rules: a pattern with a slash before its end is anchored at its
    file's directory, one without matches at any depth below it."""
    out = []
    for f in sorted(x for n in names for x in repo.rglob(n) if ".git" not in x.parts and ".meada" not in x.parts):
        d = f.parent.relative_to(repo).as_posix()
        for line in f.read_text(errors="replace").splitlines():
            neg, pat = ("!", line[1:]) if line.startswith("!") else ("", line)
            if not pat.strip() or pat.startswith("#"):
                continue
            anchored = "/" in pat.rstrip("/")
            pat = pat.lstrip("/")
            out.append(neg + ("/" + (d + "/" if d != "." else "") + pat if anchored else
                              ("/" + d + "/**/" + pat if d != "." else pat)))
    return out


def config_candidates(repo):
    """The YAML and JSON files the walk visits (story 6), the hidden CI paths (story 9), minus
    `.gitignore`, `.ignore` and `.meadaignore` at any depth (story 8)."""
    listed = git("ls-files", "-co", "--exclude-standard", "-z", cwd=repo).split("\0")
    excludes = repo.parent / f".{repo.name}.zz-prove-excludes"
    excludes.write_text("\n".join(rooted_patterns(repo)) + "\n")
    r = subprocess.run(["git", "-c", f"core.excludesFile={excludes}", "check-ignore", "--no-index", "--stdin", "-z"], cwd=repo,
                       input="\0".join(listed), capture_output=True, text=True)
    excludes.unlink()
    if r.returncode not in (0, 1):  # 1: nothing ignored; anything else is a broken oracle, never "nothing ignored"
        raise RuntimeError(f"git check-ignore failed in {repo}: {r.stderr.strip()}")
    ignored = set(r.stdout.split("\0"))
    out, hidden = [], []
    for rel in sorted(p for p in listed if p and p not in ignored):
        parts = rel.split("/")
        if not parts[-1].endswith((".yaml", ".yml", ".json")):
            continue
        if any(p.startswith(".") for p in parts) and parts[0] not in HIDDEN_CI:
            hidden.append(rel)
            continue
        if any(p in ("node_modules", "Pods") for p in parts[:-1]) or "Carthage/Checkouts/" in rel:
            continue
        if any(p in ("bin", "obj") and any((repo.joinpath(*parts[:i])).glob("*.csproj"))
               for i, p in enumerate(parts[:-1])):
            continue
        if any((repo.joinpath(*parts[:i + 1])).is_symlink() for i in range(len(parts))):
            continue
        out.append(rel)
    untracked_ignored = git("ls-files", "-o", "-i", "--exclude-standard", "-z", cwd=repo).split("\0")
    left_out = hidden + sorted(p for p in ignored | set(untracked_ignored) if p.endswith((".yaml", ".yml", ".json")))
    return out, [p for p in left_out if ".meada/" not in p]


class AliasRef:
    """An alias where it is used: its line is where the key paths through it sit (story 3)."""

    def __init__(self, node, line):
        self.node, self.line = node, line


class AliasLoader(yaml.SafeLoader):
    def compose_scalar_node(self, anchor):
        explicit = self.peek_event().tag not in (None, "!")  # `!!str 123`, `!Ref x`: written, not resolved
        node = super().compose_scalar_node(anchor)
        node.explicit = explicit
        return node

    def compose_node(self, parent, index):
        if self.check_event(yaml.AliasEvent):
            line = self.peek_event().start_mark.line + 1
            return AliasRef(super().compose_node(parent, index), line)
        return super().compose_node(parent, index)


def key_step(key):
    return "." + key if PLAIN_KEY.match(key) else "[" + json.dumps(key, ensure_ascii=False) + "]"


def scalar_kind(node):
    """YAML 1.2 core schema on a plain scalar; quoted and block scalars are strings. A block
    scalar's line is its first line of text, one past its `|` or `>`."""
    if node.style is not None:
        return "string"
    if getattr(node, "explicit", False):  # `!!str`, `!!int`, `!!bool`, `!!null`; a custom tag (`!Ref`) keeps its text
        tag = node.tag.rsplit(":", 1)[-1]
        return {"int": "number", "float": "number", "bool": "boolean", "null": None}.get(tag, "string")
    v = node.value
    return None if CORE_NULL.match(v) else "boolean" if CORE_BOOL.match(v) else "number" if CORE_NUMBER.match(v) else "string"


def is_secret(node):
    node = node.node if isinstance(node, AliasRef) else node
    if not isinstance(node, yaml.MappingNode):
        return False
    keys = {k.value: v for k, v in node.value if isinstance(k, yaml.ScalarNode)}
    return "apiVersion" in keys and isinstance(keys.get("kind"), yaml.ScalarNode) and keys["kind"].value == "Secret"


def walk_values(node, path, rows, secret=False, depth=0, root=True, line=None):
    """Every non-null scalar under `node`. A Kubernetes `Secret` (a document, or an item of a
    List's `items`) redacts what its `data` and `stringData` hold (story 13)."""
    if depth > 200:
        raise ValueError("nesting past 200")
    if isinstance(node, AliasRef):
        walk_values(node.node, path, rows, secret, depth + 1, root, line or node.line)
    elif isinstance(node, yaml.ScalarNode):
        kind = scalar_kind(node)
        if kind:
            # PyYAML leaves an escaped pair (`\ud83d\ude00`) as two surrogates; a lone one is no text
            value = node.value.encode("utf-16", "surrogatepass").decode("utf-16")
            rows.append({"key": path or ".", "value": None if secret else value, "kind": kind,
                         "line": line or node.start_mark.line + 1 + (node.style in ("|", ">"))})
    elif isinstance(node, yaml.SequenceNode):
        for i, item in enumerate(node.value):
            walk_values(item, f"{path}[{i}]", rows, secret, depth + 1, root=root == "items", line=line)
    elif isinstance(node, yaml.MappingNode):
        holder = root is True and is_secret(node)
        for k, v in node.value:
            k = k.node if isinstance(k, AliasRef) else k
            key = k.value if isinstance(k, yaml.ScalarNode) else str(k.value)
            step = (path + key_step(key)) if path or not PLAIN_KEY.match(key) else key
            walk_values(v, step, rows, secret or (holder and key in ("data", "stringData")), depth + 1,
                        root="items" if root is True and key == "items" else False, line=line)


def read_config(repo, rel, lockfiles=True):
    """One file's outcome: ("lockfile"), ("unreadable", why) or ("read", rows)."""
    path = repo / rel
    if lockfiles and path.name in LOCKFILES:
        return ("lockfile",)
    data = path.read_bytes()
    if len(data) > CONFIG_MAX_BYTES:
        return ("unreadable", f"larger than {CONFIG_MAX_BYTES} bytes")
    try:
        text = data.decode("utf-8")
        if rel.endswith(".json"):
            json.loads(text)  # JSON's own rules: a JSONC file with comments is unreadable
        docs = list(yaml.compose_all(text.replace("\t", " ") if rel.endswith(".json") else text, Loader=AliasLoader))
        if len(docs) > DOCUMENTS_MAX:
            return ("unreadable", f"more than {DOCUMENTS_MAX} documents")
        rows = []
        for i, doc in enumerate(docs):
            walk_values(doc, f"[{i}]" if len(docs) > 1 else "", rows)
    except (UnicodeError, ValueError, yaml.YAMLError) as e:
        return ("unreadable", str(e).splitlines()[0][:120])
    for r in rows:
        r["path"] = rel
        r["cut"] = r["value"] is not None and len(r["value"].encode()) > VALUE_MAX
        if r["cut"]:
            r["value"] = r["value"].encode()[:VALUE_MAX].decode("utf-8", "ignore")
    return ("read", rows)


def shown(r):
    if r["value"] is None:
        return "redacted"
    if r["kind"] == "string":
        return json.dumps(r["value"], ensure_ascii=False) + (" (cut)" if r["cut"] else "")
    return r["value"]


def same_shown(expected_row, got):
    """Numbers and booleans compare by value: Meada keeps a number as parsed (`1e3` is `1000.0`)."""
    want = shown(expected_row)
    if want == got or expected_row["kind"] == "string" or expected_row["value"] is None:
        return want == got
    if expected_row["kind"] == "boolean":
        return want.lower() == got.lower()
    v = expected_row["value"].lower()
    if re.fullmatch(r"[-+]?\.(inf|nan)", v):
        return got.lower().lstrip("+") in (v.lstrip("+"), v.lstrip("+").lstrip(".")) or got.lower() in ("inf", "-inf", "nan", "infinity", "-infinity")
    try:
        exact = int(v, 0) if v.lstrip("+-").startswith(("0x", "0o")) else int(v) if re.fullmatch(r"[-+]?[0-9]+", v) else None
        if exact is not None and re.fullmatch(r"-?[0-9]+", got):
            return int(got) == exact  # no float: an integer past 2^53 must not pass by rounding
        return float(got) == float(exact if exact is not None else v)
    except ValueError:
        return want == got


def value_rank(r, q):
    k, v = r["key"].lower(), (r["value"] or "").lower() if r["value"] is not None else None
    if k == q or v == q:
        return 0
    if k.startswith(q) or (v is not None and v.startswith(q)):
        return 1
    if q in k or (v is not None and q in v):
        return 2
    return None  # a redacted value never matches by its text (story 13)


def expected_hits(rows, query):
    q = query.lower()
    hits = [(value_rank(r, q), r) for r in rows]
    return [r for rank, r in sorted(((k, r) for k, r in hits if k is not None),
                                     key=lambda t: (t[0], t[1]["path"], t[1]["line"], t[1]["key"]))]


def parse_hit(line):
    repo, rest = line.split(" ", 1)
    place, rest = rest.split(" ", 1)
    key, got = rest.split(" = ", 1)
    path, _, num = place.rpartition(":")
    return repo, path, int(num), key, got


def check_value_answer(identity, rows, query, text, budget_cut=False):
    """Problems of one `search kind: value` answer against the oracle: the same hits, in order."""
    want = expected_hits(rows, query)
    lines = [l for l in text.splitlines() if l.strip()]
    note = re.search(r"cut after (\d+) of (\d+) lines", lines[-1]) if lines else None
    if note:
        lines = lines[:-1]
    if not want:
        ok = len(lines) == 1 and lines[0].lower() == f"no matches for `{query}`".lower()
        return [] if ok else [f"expected no matches, got {lines[:3]}"]
    problems = []
    if budget_cut:
        if not note or int(note.group(2)) != len(want):
            problems.append(f"budget: expected a cut note over {len(want)} lines, got {lines[-1:] if not note else note.group(0)}")
    elif note:
        problems.append(f"cut at the default budget: {note.group(0)}; raise the query's budget")
    if not budget_cut and len(lines) != len(want):
        problems.append(f"{len(lines)} hits, oracle {len(want)}")
    for i, line in enumerate(lines):
        try:
            repo, path, num, key, got = parse_hit(line)
        except ValueError:
            problems.append(f"line {i}: not a value line: {line[:160]}")
            continue
        if i >= len(want):
            problems.append(f"line {i}: extra hit {line[:160]}")
            continue
        w = want[i]
        if (repo, path, num, key) != (identity, w["path"], w["line"], w["key"]) or not same_shown(w, got):
            problems.append(f"line {i}: got {line[:200]!r}, oracle {identity} {w['path']}:{w['line']} {w['key']} = {shown(w)[:120]}")
        if len(problems) > 6:
            problems.append("...")
            break
    return problems


def value_queries(rows, outside_rows, n):
    """Exact values, key paths, prefixes, an upper-cased query, redacted and cut rows, values only
    files left out hold, and one query nothing holds; spread over the files."""
    def askable(r):
        v = r["value"]
        return v is not None and r["kind"] == "string" and not r.get("cut") and 3 <= len(v) <= 120 and v == v.strip() and "\n" not in v

    plain = [r for r in rows if askable(r)]
    by_file = {}
    for r in plain:
        by_file.setdefault(r["path"], []).append(r)
    picked = spread([rs[len(rs) // 2] for _, rs in sorted(by_file.items())], n)
    qs = []
    for r in picked:
        v = r["value"]
        qs += [("exact", v), ("key path", r["key"]), ("prefix", v[:max(3, len(v) // 2)])]
        if len(v) >= 6:
            qs.append(("substring", v[1:-1]))
        parent = re.sub(r"(\.[^.\[]+|\[[^]]*\])$", "", r["key"])
        if parent and parent != r["key"]:
            qs.append(("parent key path", parent))
    if picked:
        qs.append(("case", picked[0]["value"].upper()))
    qs += [("redacted", r["key"]) for r in [r for r in rows if r["value"] is None][:3]]
    qs += [("cut", r["key"]) for r in [r for r in rows if r["cut"]][:2]]
    seen = {r["value"].lower() for r in rows if r["value"] is not None}
    qs += [("left out", r["value"]) for r in outside_rows if askable(r) and r["value"].lower() not in seen][:4]
    qs.append(("nothing", "zz_prove_no_value_here"))
    return list(dict.fromkeys(qs))


def mcp_answers(bin, repo, queries, log):
    mcp = Stdio(bin, repo, log)
    tools = mcp.call("tools/list", {}).get("result", {}).get("tools")
    answers, seconds = [], []
    for q in queries:
        t = time.monotonic()
        answers.append(answer(mcp.call("tools/call", {"name": q["tool"], "arguments": q["arguments"]})))
        seconds.append(round(time.monotonic() - t, 3))
    mcp.close()
    return tools, answers, seconds


def cmd_values(a):
    os.environ["LANG"] = "C.UTF-8"  # every answer below is read as English: status, `no matches`, refusals
    run = Path(a.run).resolve()
    run.mkdir(parents=True, exist_ok=True)
    report, differences, blocked = [], 0, False
    for name in a.repos.split(","):
        src = run / "work" / "src" / name
        if not src.exists():
            fetch(name, src)
        rows, outside_rows, outcomes = [], [], {}
        files, outside = config_candidates(src)
        for rel in files:
            outcomes[rel] = read_config(src, rel)
            if outcomes[rel][0] == "read":
                rows += outcomes[rel][1]
        for rel in outside + [f for f, o in outcomes.items() if o[0] == "lockfile"]:  # values that must stay out
            o = read_config(src, rel, lockfiles=False) if (src / rel).is_file() else ("gone",)
            outside_rows += o[1] if o[0] == "read" else []
        read = [f for f, o in outcomes.items() if o[0] == "read"]
        lock = [f for f, o in outcomes.items() if o[0] == "lockfile"]
        bad = {f: o[1] for f, o in outcomes.items() if o[0] == "unreadable"}
        want_status = (len(read), len(lock) + len(bad), len(lock), len(bad), len(rows),
                       sum(r["value"] is None for r in rows))
        recs = {}
        for side, bin in (("head", a.head_bin), ("base", a.base_bin)):
            repo = run / "work" / side / name
            out = run / side / name
            shutil.rmtree(repo, ignore_errors=True)
            shutil.copytree(src, repo, symlinks=True)
            out.mkdir(parents=True, exist_ok=True)
            bin = Path(bin).resolve()
            env = os.environ
            rec = {"version": subprocess.check_output([bin, "--version"], text=True).strip()}
            rec["index"] = timed([bin, "index"], repo, out / "index.out", out / "index.err")
            rec["export"] = timed([bin, "export"], repo, out / "export1.json", out / "export.err")
            st = subprocess.run([bin, "status"], cwd=repo, capture_output=True, text=True, env=env)
            (out / "status.txt").write_text(st.stdout + st.stderr)
            rec["status"], rec["status_exit"] = st.stdout, st.returncode
            if rec["index"]["exit"] or rec["export"]["exit"]:
                rec["failed"] = f"index or export exited non-zero, see {out}"
            recs[side] = rec
        h, b = recs["head"], recs["base"]
        report.append(f"## {name}\n\n{b['version']} (base) vs {h['version']} (head)\n")
        if h.get("failed") or b.get("failed"):
            blocked = True
            report.append(f"BLOCKED: base {b.get('failed')}; head {h.get('failed')}\n")
            continue
        identity = json.loads((run / "head" / name / "export1.json").read_text())["repository"]
        report.append(f"Oracle: {len(files)} config files ({len(read)} read, {len(lock)} lockfiles, {len(bad)} unreadable), "
                      f"{len(rows)} values, {want_status[5]} redacted, {sum(r['cut'] for r in rows)} cut; "
                      f"{len(outside)} YAML or JSON files left out (hidden or ignored)")
        if bad:
            report.append("  unreadable by the oracle:\n" + "\n".join(f"    {f}: {why}" for f, why in list(bad.items())[:15]))
        m = STATUS_LINE.search(h["status"])
        got_status = tuple(int(x) for x in m.groups()) if m else None
        if got_status != (want_status if files else None):
            differences += 1
            report.append(f"meada status: DIFFERS: head {m.group(0) if m else 'no config line'}; oracle "
                          f"{'%d read, %d skipped (lockfiles: %d, unreadable: %d); values: %d, redacted: %d' % want_status if files else 'no config line'}")
        else:
            report.append(f"meada status: config line {'equal to the oracle' if files else 'absent, as the oracle (no config file)'}")
        others = [[l for l in r["status"].splitlines() if not STATUS_LINE.search(l)] for r in (h, b)]
        if others[0] != others[1] or h["status_exit"] != b["status_exit"]:
            differences += 1
            diff = difflib.unified_diff(b["status"].splitlines(), h["status"].splitlines(), "base", "head", lineterm="", n=1)
            report.append("meada status without the config line: DIFFERS from base\n```\n" + "\n".join(diff) + "\n```")
        else:
            report.append("meada status without the config line: equal to base")
        same_export = body((run / "base" / name / "export1.json").read_bytes()) == body((run / "head" / name / "export1.json").read_bytes())
        differences += not same_export
        report.append(f"meada export: {'byte-equal to base (no config value in it, story 17)' if same_export else 'DIFFERS from base'}")
        picked = value_queries(rows, outside_rows, a.sample)
        queries = [{"tool": "search", "label": label, "arguments": {"query": q, "kind": "value", "budget": 200000}} for label, q in picked]
        big = max(range(len(picked)), key=lambda i: len(expected_hits(rows, picked[i][1])), default=None)
        if big is not None and len(expected_hits(rows, picked[big][1])) > 3:
            queries.append({"tool": "search", "label": "budget", "arguments": {"query": picked[big][1], "kind": "value", "budget": 120}})
        fallback = [q for label, q in picked if label in ("exact", "nothing")][:3]
        # Symbol names too: values added to a default search a symbol answers show only there
        # (audit 2026-11-04, M6: caught on one entry by luck of a value equal to a symbol name).
        symbols = json.loads((run / "head" / name / "export1.json").read_text()).get("symbols", [])
        fallback += sorted({short_name(x["id"]) for x in symbols if len(short_name(x["id"])) >= 3})[:3]
        queries += [{"tool": "search", "label": "default search", "arguments": {"query": q, "budget": 200000}} for q in fallback]
        queries.append({"tool": "search", "label": "unknown kind", "arguments": {"query": picked[0][1] if picked else "x", "kind": "values"}})
        try:
            h["tools"], h["answers"], secs = mcp_answers(Path(a.head_bin).resolve(), run / "work" / "head" / name, queries, run / "head" / name / "mcp.err")
            _, b["answers"], _ = mcp_answers(Path(a.base_bin).resolve(), run / "work" / "base" / name, queries, run / "base" / name / "mcp.err")
        except Exception as e:
            blocked = True
            report.append(f"BLOCKED: mcp stdio {e!r}")
            continue
        h["queries"] = queries
        (run / "head" / name / "values.json").write_text(json.dumps({"queries": queries, "answers": h["answers"], "seconds": secs}, indent=1, ensure_ascii=False))
        lines, fails = [], 0
        for i, q in enumerate(queries):
            args, ans = q["arguments"], h["answers"][i]
            label = q["label"]
            if args.get("kind") == "value":
                problems = check_value_answer(identity, rows, args["query"], ans, budget_cut=label == "budget")
            elif "kind" not in args:  # story 5: as base when a symbol or contract matches, else the value hits
                base_ans = b["answers"][i]
                if not base_ans.startswith("no matches"):
                    problems = [] if ans == base_ans else ["differs from base, which found a symbol or contract"]
                else:
                    problems = check_value_answer(identity, rows, args["query"], ans)
            else:
                listed = ans.rsplit(", not `", 1)[0]  # the refusal echoes the unknown kind (`values`) after the list
                problems = [] if ans.startswith(("TOOL ERROR", "ERROR")) and "`value`" in listed else [f"not refused or `value` not listed: {ans[:200]}"]
            fails += bool(problems)
            lines.append(f"  [{'FAIL' if problems else 'PASS'}] {label} {json.dumps(args, ensure_ascii=False)[:160]} ({secs[i]} s)"
                         + "".join(f"\n      {p}" for p in problems))
        differences += fails
        report.append(f"Value queries over stdio: {len(queries) - fails}/{len(queries)} equal to the oracle; slowest "
                      f"{max(secs)} s (story 16 has no number: the checker judges)\n" + "\n".join(lines))
        if a.http:
            line, failed = http_parity(Path(a.head_bin).resolve(), name, run / "work" / "head" / name, run, h)
            differences += failed
            report.append("MCP HTTP vs stdio (head):\n" + line)
        report.append("")
    verdict = "BLOCKED: a run failed" if blocked else ("checks passed" if not differences else f"differences found: {differences}")
    (run / "values.md").write_text(f"# prove.py values\n\n{verdict}\n\n" + "\n".join(report) + "\n")
    print(verdict)
    print(f"full report: {run}/values.md")
    sys.exit(2 if blocked else 1 if differences else 0)


def cmd_iac(a):
    """A7 IaC nodes (#649): the oracle's nodes and references against `search` kind `iac`,
    `context` and `trace` both ways over stdio (and HTTP with `--http`); the base answers the
    default search, `meada status` and the export, which the change leaves alone."""
    sys.dont_write_bytecode = True  # no __pycache__ left in the skill's folder
    import tf_oracle as tf  # needs python-hcl2: see SKILL.md's Gotchas
    os.environ["LANG"] = "C.UTF-8"
    run = Path(a.run).resolve()
    run.mkdir(parents=True, exist_ok=True)
    report, differences, blocked = [], 0, False
    footer = re.compile(r"chain: (exact|none) \u00b7 0 hops \u00b7 1 repos \u00b7 \d+ tokens\Z")
    for name in a.repos.split(","):
        src = run / "work" / "src" / name
        if not src.exists():
            fetch(name, src)
        nodes, references, unreadable = tf.evaluate(src)
        recs = {}
        for side, bin in (("head", a.head_bin), ("base", a.base_bin)):
            repo, out = run / "work" / side / name, run / side / name
            shutil.rmtree(repo, ignore_errors=True)
            shutil.copytree(src, repo, symlinks=True)
            out.mkdir(parents=True, exist_ok=True)
            bin = Path(bin).resolve()
            rec = {"version": subprocess.check_output([bin, "--version"], text=True).strip()}
            rec["index"] = timed([bin, "index"], repo, out / "index.out", out / "index.err")
            rec["export"] = timed([bin, "export"], repo, out / "export1.json", out / "export.err")
            st = subprocess.run([bin, "status"], cwd=repo, capture_output=True, text=True)
            rec["status"] = st.stdout + st.stderr
            if rec["index"]["exit"] or rec["export"]["exit"]:
                rec["failed"] = f"index or export exited non-zero, see {out}"
            recs[side] = rec
        h, b = recs["head"], recs["base"]
        report.append(f"## {name}\n\n{b['version']} (base) vs {h['version']} (head)\n")
        if h.get("failed") or b.get("failed"):
            blocked = True
            report.append(f"BLOCKED: base {b.get('failed')}; head {h.get('failed')}\n")
            continue
        identity = json.loads((run / "head" / name / "export1.json").read_text())["repository"]
        roots = sorted({n["root"] for n in nodes})
        # #650: a root with a `count` or `for_each` the oracle cannot evaluate is left out of the
        # comparison (its lines dropped on both sides), a node whose name it cannot evaluate
        # matches any name; both are counted, never hidden.
        unsure = sorted({n["root"] for n in nodes if n.get("root_unchecked")})
        names_unchecked = sum(1 for n in nodes if n.get("unchecked"))
        report.append(f"Oracle (#650 names): {names_unchecked} names unchecked, roots left out: {unsure}")
        drop = lambda text: "".join(l + "\n" for l in text.splitlines() if not any(l.endswith(f"root {r})") for r in unsure))
        nodes = [n for n in nodes if n["root"] not in unsure]
        report.append(f"Oracle: {len(nodes)} nodes in {len(roots)} roots ({', '.join(roots[:8])}{', ...' if len(roots) > 8 else ''}), "
                      f"{len(references)} references; unreadable: {[f for f, _ in unreadable]}")
        same = body((run / "base" / name / "export1.json").read_bytes()) == body((run / "head" / name / "export1.json").read_bytes())
        differences += not same
        report.append(f"meada export: {'byte-equal to base (no IaC node in it)' if same else 'DIFFERS from base'}")
        differences += h["status"] != b["status"]
        report.append(f"meada status: {'equal to base' if h['status'] == b['status'] else 'DIFFERS from base'}")
        # The sample: nodes with references both ways first, then the rest, spread over the list.
        linked = {r[1] for r in references} & {r[2] for r in references}
        ordered = sorted(nodes, key=lambda n: (n["address"] not in linked, n["path"], n["line"], n["address"], n["root"]))
        ordered = [n for n in ordered if not n.get("unchecked")]
        sample = spread([n for n in ordered if n["address"] in linked], a.sample // 2) + spread(ordered, a.sample - a.sample // 2)
        sample = list({(n["root"], n["address"]): n for n in sample}.values())
        counts = {}
        for n in nodes:
            counts[n["address"]] = counts.get(n["address"], 0) + 1
        queries = []
        search = lambda label, q, **kw: queries.append({"tool": "search", "label": label, "arguments": {"query": q, **kw}})
        for n in sample:
            search("address", n["address"], kind="iac", budget=200000)
            search("prefix", n["address"][:5], kind="iac", budget=200000)
            if n["name"]:
                search("physical name", n["name"], kind="iac", budget=200000)
            passed = f"{n['address']} (root {n['root']})"
            queries.append({"tool": "context", "label": "context", "node": n, "arguments": {"symbol": passed, "budget": 200000}})
            for d in ("down", "up"):
                queries.append({"tool": "trace", "label": f"trace {d}", "node": n, "arguments": {"from": passed, "direction": d, "budget": 200000}})
        for t in sorted({n["type"] for n in nodes})[:3]:
            search("type", t, kind="iac", budget=200000)
        shared = sorted(k for k, c in counts.items() if c > 1)[:2]
        for addr in shared:
            queries.append({"tool": "context", "label": "several", "arguments": {"symbol": addr, "budget": 200000}})
        search("nothing", "zz_prove_no_node", kind="iac", budget=200000)
        for n in sample[:3]:
            search("default search", n["address"], budget=200000)
        search("unknown kind", "x", kind="iacs")
        # The code keeps its answers: a symbol named like a node (`a7-iac`'s `zz_code.py`) answers
        # `context` and `trace` as on the base (the prover's finding on 09f2aae, 2026-11-04).
        symbols = json.loads((run / "head" / name / "export1.json").read_text()).get("symbols", [])
        for sym in sorted({x["id"].rsplit("::", 1)[-1] for x in symbols}, key=lambda n: ("." not in n, n))[:3]:
            queries.append({"tool": "context", "label": "code answer", "arguments": {"symbol": sym, "budget": 200000}})
            queries.append({"tool": "trace", "label": "code answer", "arguments": {"from": sym, "direction": "up", "budget": 200000}})
        wire = [{k: v for k, v in q.items() if k != "node"} for q in queries]
        try:
            h["tools"], h["answers"], secs = mcp_answers(Path(a.head_bin).resolve(), run / "work" / "head" / name, wire, run / "head" / name / "mcp.err")
            _, b["answers"], _ = mcp_answers(Path(a.base_bin).resolve(), run / "work" / "base" / name, wire, run / "base" / name / "mcp.err")
        except Exception as e:
            blocked = True
            report.append(f"BLOCKED: mcp stdio {e!r}")
            continue
        h["queries"] = wire
        (run / "head" / name / "iac.json").write_text(json.dumps({"queries": wire, "answers": h["answers"], "seconds": secs}, indent=1, ensure_ascii=False))
        lines, fails = [], 0
        for i, q in enumerate(queries):
            got, args, label = h["answers"][i], q["arguments"], q["label"]
            problems = []
            if q["tool"] == "search" and args.get("kind") == "iac":
                hits = tf.search_hits(identity, nodes, args["query"])
                want = "".join(l + "\n" for l in hits) if hits else f"no matches for `{args['query']}`\n"
                if unsure:
                    got = drop(got) or f"no matches for `{args['query']}`\n"
                if tf.UNCHECKED_NAME in want:
                    any_name = r'(, (?:name|pattern) "(?:[^"\\]|\\.)*")?'
                    pattern = "".join(re.escape(l).replace(re.escape(tf.UNCHECKED_NAME), any_name) + "\n" for l in hits)
                    if re.fullmatch(pattern, got):
                        got = want
                if got != want:
                    problems.append("differs from the oracle:\n" + "\n".join(difflib.unified_diff(want.splitlines(), got.splitlines(), "oracle", "head", lineterm="", n=1)))
            elif label == "code answer":
                if got != b["answers"][i]:
                    problems.append("differs from base: a symbol's answer must not change")
            elif q["tool"] == "context" and label == "several":
                if not got.startswith(f"`{args['symbol']}` fits {counts[args['symbol']]} IaC nodes; pass one of:\n"):
                    problems.append(f"not a list of {counts[args['symbol']]} nodes: {got[:200]}")
            elif q["tool"] == "context":
                want = tf.context_lines(identity, nodes, references, q["node"])
                got_lines = got.splitlines()
                head_part = got_lines[:len(want)]
                if head_part != want:
                    problems.append("differs from the oracle:\n" + "\n".join(difflib.unified_diff(want, head_part, "oracle", "head", lineterm="", n=1)))
                rest = got_lines[len(want):]
                values_ok = rest == ["values: none"] or (rest[:1] == ["values:"] and all(l.startswith(f"{identity} {q['node']['path']}:") for l in rest[1:]))
                if not values_ok:
                    problems.append(f"values section not of the node's file: {rest[:3]}")
            elif q["tool"] == "trace":
                want = tf.trace_lines(identity, nodes, references, q["node"], args["direction"] == "down")
                got_lines = got.splitlines()
                if got_lines[:-1] != want:
                    problems.append("differs from the oracle:\n" + "\n".join(difflib.unified_diff(want, got_lines[:-1], "oracle", "head", lineterm="", n=1)))
                m = footer.match(got_lines[-1] if got_lines else "")
                if not m or (m.group(1) == "none") != (len(want) == 1):
                    problems.append(f"footer: {got_lines[-1:] }")
            elif "kind" not in args:
                if got != b["answers"][i]:
                    problems.append("differs from base: the default search must leave IaC nodes out")
            else:
                listed = got.rsplit(", not `", 1)[0]
                if not (got.startswith(("TOOL ERROR", "ERROR")) and "`iac`" in listed):
                    problems.append(f"not refused or `iac` not listed: {got[:200]}")
            fails += bool(problems)
            lines.append(f"  [{'FAIL' if problems else 'PASS'}] {label} {json.dumps(args, ensure_ascii=False)[:150]} ({secs[i]} s)"
                         + "".join("\n      " + p.replace("\n", "\n      ") for p in problems))
        differences += fails
        report.append(f"IaC queries over stdio: {len(queries) - fails}/{len(queries)} equal to the oracle (or base); slowest {max(secs)} s\n" + "\n".join(lines))
        if a.http:
            line, failed = http_parity(Path(a.head_bin).resolve(), name, run / "work" / "head" / name, run, h)
            differences += failed
            report.append("MCP HTTP vs stdio (head):\n" + line)
        report.append("")
    verdict = "BLOCKED: a run failed" if blocked else ("checks passed" if not differences else f"differences found: {differences}")
    (run / "iac.md").write_text(f"# prove.py iac\n\n{verdict}\n\n" + "\n".join(report) + "\n")
    print(verdict)
    print(f"full report: {run}/iac.md")
    sys.exit(2 if blocked else 1 if differences else 0)


def cmd_clean(a):
    run = Path(a.run).resolve()
    for pid in run.glob("http/*/server.pid"):
        try:
            os.kill(int(pid.read_text()), 15)
        except ProcessLookupError:
            pass
    shutil.rmtree(run / "work", ignore_errors=True)
    for idx in run.glob("http/*/indexes"):
        shutil.rmtree(idx, ignore_errors=True)
    print(f"removed {run}/work and the HTTP indexes; evidence stays in {run}")


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run")
    r.add_argument("--base-bin", required=True)
    r.add_argument("--head-bin", required=True)
    r.add_argument("--repos", required=True, help="comma-separated corpus.tsv names, or all")
    r.add_argument("--run", required=True)
    r.add_argument("--queries", help="JSON lines {tool, arguments} used instead of the automatic ones")
    r.add_argument("--sample", type=int, default=10)
    r.add_argument("--http", action="store_true")
    r.add_argument("--overlay", help="a directory copied over each entry and committed before indexing (a probe)")
    b = sub.add_parser("rebless")
    b.add_argument("--base", required=True)
    b.add_argument("--run", required=True)
    d = sub.add_parser("deploy")
    d.add_argument("--base", required=True)
    d.add_argument("--run", required=True)
    v = sub.add_parser("values")
    v.add_argument("--base-bin", required=True)
    v.add_argument("--head-bin", required=True)
    v.add_argument("--repos", required=True, help="comma-separated corpus.tsv or infra.tsv names")
    v.add_argument("--run", required=True)
    v.add_argument("--sample", type=int, default=8, help="files sampled for value queries per entry")
    v.add_argument("--http", action="store_true")
    i = sub.add_parser("iac")
    i.add_argument("--base-bin", required=True)
    i.add_argument("--head-bin", required=True)
    i.add_argument("--repos", required=True, help="comma-separated infra.tsv names")
    i.add_argument("--run", required=True)
    i.add_argument("--sample", type=int, default=8, help="nodes sampled for context and trace per entry")
    i.add_argument("--http", action="store_true")
    c = sub.add_parser("clean")
    c.add_argument("run")
    sub.add_parser("selftest")
    a = p.parse_args()
    {"run": cmd_run, "rebless": cmd_rebless, "deploy": cmd_deploy, "clean": cmd_clean, "values": cmd_values, "iac": cmd_iac, "selftest": cmd_selftest}[a.cmd](a)


if __name__ == "__main__":
    main()
