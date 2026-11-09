#!/usr/bin/env python3
"""Index every fixture under tests/fixtures/ as its own repository with two Meada binaries and
compare what they answer.

Usage: fixtures.py <prev-binary> <after-binary> <out-dir> [substring]  |  fixtures.py --selftest

Each fixture is copied into <out-dir>/repo, committed, indexed, exported and asked `status` by
each binary, then deleted. Prints one line per fixture: `ok`, or what differs (the export minus
`commit`, `status`, a non-zero index exit or stderr). Writes only under <out-dir>.
"""
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

FIXTURES = Path(__file__).resolve().parents[4] / "tests/fixtures"


def run(binary, repo, *args):
    p = subprocess.run([str(binary), *args], cwd=repo, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                       text=True, timeout=600)
    return p.returncode, p.stdout, p.stderr


def answers(binary, repo):
    shutil.rmtree(repo / ".meada", ignore_errors=True)
    code, _, err = run(binary, repo, "index")
    _, export, export_err = run(binary, repo, "export")
    _, status, _ = run(binary, repo, "status")
    try:
        export = json.loads(export)
        export.pop("commit", None)
    except ValueError:
        pass
    return {"index": code, "stderr": (err + export_err).strip()[:200], "export": export, "status": status}


def difference(prev, after):
    """What differs between two `answers`, as short words; empty when nothing does."""
    found = [k for k in ("export", "status") if prev[k] != after[k]]
    found += [f"index exit {after['index']}"] if after["index"] else []
    found += [f"stderr {after['stderr']!r}"] if after["stderr"] else []
    return found


def main(prev, after, out, only=""):
    out = Path(out).resolve()
    repo = out / "repo"
    for fixture in sorted(p for p in FIXTURES.iterdir() if p.is_dir() and only in p.name):
        shutil.rmtree(repo, ignore_errors=True)
        shutil.copytree(fixture, repo, symlinks=True)
        for args in (["init", "-q", "-b", "main"], ["remote", "add", "origin", f"https://example.com/fx/{fixture.name}.git"],
                     ["add", "-A"], ["-c", "user.email=v@v", "-c", "user.name=v", "commit", "-qm", "fixture"]):
            subprocess.run(["git", *args], cwd=repo, check=True, stdout=subprocess.DEVNULL)
        found = difference(answers(prev, repo), answers(after, repo))
        print(fixture.name, "ok" if not found else "DIFF " + ", ".join(found), flush=True)
    shutil.rmtree(repo, ignore_errors=True)


def selftest():
    same = {"index": 0, "stderr": "", "export": {"a": 1}, "status": "s"}
    assert difference(same, dict(same)) == []
    assert difference(same, dict(same, export={"a": 2}, index=1)) == ["export", "index exit 1"]
    assert FIXTURES.is_dir(), FIXTURES
    with tempfile.TemporaryDirectory():
        pass
    print("fixtures.py selftest passed")


if __name__ == "__main__":
    if sys.argv[1:] == ["--selftest"]:
        selftest()
    elif len(sys.argv) in (4, 5):
        main(*sys.argv[1:])
    else:
        sys.exit(__doc__)
