#!/usr/bin/env python3
"""The console surface of prove-meada, over the repositories a `prove.py run` left in RUN.

  console.py start --bin H --run RUN --ui-dir ui/dist [--accounts]
  console.py api --run RUN
  console.py stop --run RUN

`start` writes a bootstrap manifest of every head repository of RUN, indexes it, starts
`meada ui` detached on a free loopback port and waits until `/api/v1/config` answers. Without
`--accounts` the console admits every request as a viewer (the operator's proxy case). With it,
`[postgres]` reads `MEADA_TEST_POSTGRES_URL` and a schema of this run, and `[accounts]` signs in
`admin` with a password derived from the run's signing key, never written. `api` checks the console's API against the head's MCP
stdio answers and `meada export` bytes. Exit 0: checks passed, 1: differences, 2: BLOCKED.
"""

import argparse
import base64
import hmac
import json
import os
import shutil
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path


def http(url, method="GET", body=None, headers=None):
    req = urllib.request.Request(url, body if body is None else json.dumps(body).encode(), headers or {}, method=method)
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            return r.status, r.read()
    except urllib.error.HTTPError as e:
        return e.code, e.read()


def repos(run):
    out = []
    for export in sorted(run.glob("head/*/export1.json")):
        name = export.parent.name
        identity = json.loads(export.read_text())["repository"]
        out.append((name, identity, run / "work" / "head" / name))
    return out


def admin_password(d):
    """The console's admin password, derived from the run's signing key and never written: whoever
    can read private.pem can already mint a session. console-pages.mjs derives the same value."""
    key = (d / "private.pem").read_bytes()
    return base64.urlsafe_b64encode(hmac.new(key, b"prove-meada admin", "sha256").digest()[:18]).decode()


def cmd_start(a):
    run = Path(a.run).resolve()
    d = run / "console"
    if (d / "ui.pid").exists():
        sys.exit(f"a console of this run may still be up: console.py stop --run {run}")
    d.mkdir(parents=True, exist_ok=True)
    os.chmod(d, 0o700)  # it holds the signing key
    ui_dir = Path(a.ui_dir).resolve()
    if not (ui_dir / "meada-ui.json").exists():
        sys.exit(f"BLOCKED: {ui_dir}/meada-ui.json missing: build the front end (references/console.md)")
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        port = s.getsockname()[1]
    url = f"http://127.0.0.1:{port}"
    text = 'index_dir = "indexes"\n'
    for _, identity, path in repos(run):
        text += f'\n[[repository]]\nrepo = "{identity}"\npath = "{path}"\n'
    text += f'\n[ui]\ndir = "{ui_dir}"\npublic_url = "{url}"\n'
    env = dict(os.environ)
    if a.accounts:
        if not os.environ.get("MEADA_TEST_POSTGRES_URL"):
            sys.exit("BLOCKED: --accounts needs MEADA_TEST_POSTGRES_URL (a Postgres 15+ the run may write a schema in)")
        subprocess.run(["openssl", "genpkey", "-algorithm", "ed25519", "-out", d / "private.pem"], check=True, capture_output=True)
        subprocess.run(["openssl", "pkey", "-in", d / "private.pem", "-pubout", "-out", d / "public.pem"], check=True, capture_output=True)
        os.chmod(d / "private.pem", 0o600)
        env["PROVE_ADMIN_PASSWORD"] = admin_password(d)
        text += (f'\n[postgres]\nurl_env = "MEADA_TEST_POSTGRES_URL"\nschema = "zz_prove_{run.name.replace("-", "_")[:40]}"\n'
                 '\n[accounts]\npublic_key_file = "public.pem"\nprivate_key_file = "private.pem"\n'
                 'admin_password_env = "PROVE_ADMIN_PASSWORD"\n')
    (d / "meada.workspace.toml").write_text(text)
    idx = subprocess.run([a.bin, "index", "--workspace", "meada.workspace.toml"], cwd=d, capture_output=True, text=True)
    (d / "index.err").write_text(idx.stderr)
    if idx.returncode:
        sys.exit(f"BLOCKED: meada index --workspace exited {idx.returncode}, see {d}/index.err")
    log = open(d / "ui.log", "wb")
    p = subprocess.Popen([a.bin, "ui", "--listen", f"127.0.0.1:{port}", "--workspace", "meada.workspace.toml"], cwd=d,
                         env=env, stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)
    (d / "ui.pid").write_text(str(p.pid))
    (d / "url").write_text(url)
    for _ in range(150):
        if p.poll() is not None:
            sys.exit(f"BLOCKED: meada ui exited {p.returncode}, see {d}/ui.log")
        try:
            status, body = http(url + "/api/v1/config")
            if status == 200:
                print(f"console up at {url} (pid {p.pid}); config {body.decode()[:200]}")
                return
        except OSError:
            pass
        time.sleep(0.2)
    sys.exit(f"BLOCKED: no answer from {url}/api/v1/config in 30 s, see {d}/ui.log")


def cmd_api(a):
    run = Path(a.run).resolve()
    d = run / "console"
    url = (d / "url").read_text()
    origin = {"Origin": url, "Content-Type": "application/json"}
    lines, differences = [], 0
    token = None
    if (d / "private.pem").exists():
        password = admin_password(d)
        status, body = http(url + "/api/v1/login", "POST", {"username": "admin", "password": password + "x"}, origin)
        lines.append(f"login with a wrong password: {status}" + ("" if status in (401, 403) else "  UNEXPECTED, expected 401 or 403"))
        differences += status not in (401, 403)
        status, body = http(url + "/api/v1/login", "POST", {"username": "admin", "password": password}, {"Content-Type": "application/json"})
        lines.append(f"login without Origin: {status}" + ("" if status == 403 else "  UNEXPECTED, expected 403"))
        differences += status != 403
        status, body = http(url + "/api/v1/login", "POST", {"username": "admin", "password": password}, origin)
        reply = json.loads(body or b"{}")
        token = reply.get("token")
        lines.append(f"login as admin: {status}, token {'issued' if token else 'MISSING'}, fields {sorted(reply)}")
        if status != 200 or not token:
            print("BLOCKED: admin cannot sign in\n" + "\n".join(lines))
            sys.exit(2)
        origin["Authorization"] = f"Bearer {token}"
        status, body = http(url + "/api/v1/session", headers={"Authorization": f"Bearer {token}"})
        lines.append(f"session: {status} {body.decode()[:160]}")
        status, _ = http(url + "/api/v1/manifest")
        lines.append(f"manifest without a token: {status}" + ("" if status == 401 else "  UNEXPECTED, expected 401"))
        differences += status != 401
    for name, identity, _ in repos(run):
        rec = json.loads((run / "head" / name / "record.json").read_text())
        same = 0
        for i, q in enumerate(rec["queries"]):
            status, body = http(url + "/api/v1/query", "POST",
                                {"repository": identity, "generation": None, "tool": q["tool"], "arguments": q["arguments"]}, origin)
            text = json.loads(body).get("text") if status == 200 else f"HTTP {status} {body.decode()[:200]}"
            if text == rec["answers"][i]:
                same += 1
            else:
                differences += 1
                lines.append(f"{name} query {i} {q['tool']}: differs from MCP stdio: {text[:200]!r}")
        lines.append(f"{name}: {same}/{len(rec['queries'])} query answers byte-equal to MCP stdio")
        status, body = http(f"{url}/api/v1/repositories/{identity}/export", headers={k: v for k, v in origin.items() if k == "Authorization"})
        exported = (run / "head" / name / "export1.json").read_bytes()
        (d / f"{name}.export.json").write_bytes(body)
        equal = status == 200 and body == exported
        differences += not equal
        lines.append(f"{name}: export {status}, {'byte-equal to' if equal else 'DIFFERS from'} meada export ({d}/{name}.export.json)")
    verdict = "checks passed" if not differences else f"differences found: {differences}"
    (d / "api.md").write_text(f"# console.py api\n\n{verdict}\n\n" + "\n".join(f"- {l}" for l in lines) + "\n")
    print(verdict)
    print("\n".join(lines[:40]))
    sys.exit(1 if differences else 0)


def cmd_stop(a):
    run = Path(a.run).resolve()
    d = run / "console"
    pid = d / "ui.pid"
    if pid.exists():
        try:
            os.killpg(int(pid.read_text()), 15)
        except ProcessLookupError:
            pass
        pid.unlink()
    shutil.rmtree(d / "indexes", ignore_errors=True)
    (d / "private.pem").unlink(missing_ok=True)
    print(f"console of {run} stopped; the Postgres schema zz_prove_* of an --accounts run stays: drop it by name")


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("start")
    s.add_argument("--bin", required=True)
    s.add_argument("--run", required=True)
    s.add_argument("--ui-dir", required=True)
    s.add_argument("--accounts", action="store_true")
    for name in ("api", "stop"):
        sub.add_parser(name).add_argument("--run", required=True)
    a = p.parse_args()
    {"start": cmd_start, "api": cmd_api, "stop": cmd_stop}[a.cmd](a)


if __name__ == "__main__":
    main()
