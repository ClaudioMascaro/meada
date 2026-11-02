#!/usr/bin/env python3
"""Scripted checks of prove-meada: drive two meada binaries over the same inputs and compare.

  prove.py run --base-bin B --head-bin H --repos meada,click --run DIR [--queries F] [--http] [--overlay D]
  prove.py rebless --base SHA --run DIR
  prove.py deploy --base SHA --run DIR
  prove.py clean DIR

`run` indexes each corpus repository twice from cold with each binary (determinism and the
budget), exports it, refreshes after an mtime touch, drives `meada mcp` over stdio with the same
queries, and with `--http` serves the head's indexes over streamable HTTP for a parity check. It
prints "checks passed" or the differences found: the verdict is the checker's, never this script's.
Exit 0: no difference, 1: differences or budget overruns, 2: a run failed (BLOCKED).
"""

import argparse
import difflib
import json
import os
import shutil
import socket
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

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
OTHER = ("files", "bindings", "publishes", "dependencies", "communities", "processes")
FIXED_GIT = {
    "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_AUTHOR_NAME": "prove-meada", "GIT_AUTHOR_EMAIL": "prove@example.com", "GIT_AUTHOR_DATE": "2026-01-01T00:00:00Z",
    "GIT_COMMITTER_NAME": "prove-meada", "GIT_COMMITTER_EMAIL": "prove@example.com", "GIT_COMMITTER_DATE": "2026-01-01T00:00:00Z",
}


def git(*args, cwd, **kw):
    return subprocess.run(["git", *args], cwd=cwd, env={**os.environ, **FIXED_GIT}, check=True,
                          capture_output=True, text=True, **kw).stdout


def corpus():
    rows = {}
    for line in (HERE / "corpus.tsv").read_text().splitlines():
        if line and not line.startswith("#"):
            name, packs, url, sha = line.split("\t")
            rows[name] = (packs.split(","), url, sha)
    return rows


def fetch(name, into):
    """A clean git checkout of corpus entry `name` at `into`."""
    _, url, sha = corpus()[name]
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


def auto_queries(export):
    """Three symbols with the most resolved callers, each through every tool."""
    count = {}
    for c in export.get("calls", []):
        if c.get("target"):
            count[c["target"]] = count.get(c["target"], 0) + 1
    top = sorted(count, key=lambda s: (-count[s], s))[:3]
    queries = []
    for sym in top:
        name = sym.rsplit("::", 1)[-1].split("(")[0].rsplit(".", 1)[-1]  # Kotlin and Swift signatures carry dots
        queries += [{"tool": "search", "arguments": {"query": name}},
                    {"tool": "context", "arguments": {"symbol": sym}},
                    {"tool": "trace", "arguments": {"from": sym}}]
    contracts = sorted({p["contract"] for p in export.get("providers", [])})
    if contracts:
        queries.append({"tool": "search", "arguments": {"query": contracts[0], "kind": "contract"}})
    # Error parity: an unknown symbol must be refused the same way by both binaries.
    queries.append({"tool": "context", "arguments": {"symbol": "zz_prove_missing"}})
    return queries


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
    if not json.loads((out / "export1.json").read_text()).get("symbols"):
        rec["failed"] = f"the export of {repo} has no symbol: an empty index proves nothing"
        (out / "record.json").write_text(json.dumps(rec, indent=1, sort_keys=True))
        return rec
    rec["deterministic"] = (out / "export1.json").read_bytes() == (out / "export2.json").read_bytes()
    tracked = git("ls-files", cwd=repo).splitlines()
    if tracked:
        os.utime(repo / tracked[0])  # same bytes, new mtime: the freshness path must give the same export
        rec["refresh"] = timed([bin, "export"], repo, out / "export3.json", out / "export3.err")
        rec["refresh_equal"] = (out / "export3.json").read_bytes() == (out / "export1.json").read_bytes()
    rec["export_bytes"] = (out / "export1.json").stat().st_size
    for n in ("export2.json", "export3.json"):  # kept only when they differ: an export can be 28 MB
        if (out / n).exists() and (out / n).read_bytes() == (out / "export1.json").read_bytes():
            (out / n).unlink()
    if not queries_file.exists():
        export = json.loads((out / "export1.json").read_text())
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
        det = [r["deterministic"] and r.get("refresh_equal", True) for r in (b, h)]
        differences += det.count(False)
        report.append(f"Determinism: cold twice equal base {b['deterministic']}, head {h['deterministic']}; "
                      f"refresh equal base {b.get('refresh_equal')}, head {h.get('refresh_equal')}"
                      + ("" if all(det) else "  NOT DETERMINISTIC"))
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
    for name, args, env in COMPOSE if compose.is_dir() else ():
        r = subprocess.run(["docker", "compose", "--env-file", env, *args, "config", "--format", "json"],
                           cwd=compose, capture_output=True, text=True)
        text = r.stdout if r.returncode == 0 else f"render failed ({r.returncode}): {r.stderr}"
        (out / f"compose-{name}.json").write_text(text.replace(str(root), "<root>"))
    if shutil.which("helm"):
        chart = root / "deploy/helm/meada"
        for values in sorted(chart.glob("values*.yaml")):
            r = subprocess.run(["helm", "template", "check", str(chart), "-f", str(values)], capture_output=True, text=True)
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
    print("selftest passed")


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
    c = sub.add_parser("clean")
    c.add_argument("run")
    sub.add_parser("selftest")
    a = p.parse_args()
    {"run": cmd_run, "rebless": cmd_rebless, "deploy": cmd_deploy, "clean": cmd_clean, "selftest": cmd_selftest}[a.cmd](a)


if __name__ == "__main__":
    main()
