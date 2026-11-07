#!/usr/bin/env python3
"""Bounded evidence capture using existing transports and frozen source phases.

Query files must be independently derived from source before invoking the binary.
Only transport/refresh equality, exact restoration and explicit privacy constraints
are automated. CAPTURED is not semantic acceptance or PROVED. Run in public CI.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time


def save(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n")


def source_hash(path):
    data = os.readlink(path).encode() if path.is_symlink() else path.read_bytes()
    return hashlib.sha256(data).hexdigest()


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--head-bin", required=True)
    source = p.add_mutually_exclusive_group(required=True)
    source.add_argument("--entry", choices=["kube-prometheus", "boutique", "crossplane-aws", "sam-rust", "terraform-sqs", "a7-sls"])
    source.add_argument("--source", help="frozen independent neutral files; source key names their Git origin")
    p.add_argument("--name", help="required with --source")
    p.add_argument("--queries", required=True, help="source-authored JSONL {tool, arguments}, never product-generated")
    p.add_argument("--source-key", required=True, help="pre-answer source hashes, not a product snapshot")
    p.add_argument("--phase-key", help="frozen source_edits and response privacy expectations")
    p.add_argument("--run", required=True)
    a = p.parse_args()
    os.environ["LANG"] = "C.UTF-8"
    root = Path(subprocess.check_output(["git", "rev-parse", "--show-toplevel"], text=True).strip())
    scripts = root / ".claude/skills/prove-meada/scripts"
    spec = importlib.util.spec_from_file_location("prove_capture_recipe", scripts / "prove.py")
    prove = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(prove)
    binary = str(Path(a.head_bin).resolve())
    queries_path = Path(a.queries).resolve()
    queries = [json.loads(line) for line in queries_path.read_text().splitlines() if line.strip()]
    if not queries or not all(set(q) == {"tool", "arguments"} for q in queries):
        p.error("a nonempty independently prepared query file is required")
    name = a.entry or a.name
    if not name or "/" in name or name in (".", ".."):
        p.error("use a plain capture name")
    run = Path(a.run).resolve()
    run.mkdir(parents=True, exist_ok=False)
    src = run / "work" / "src" / name
    src.parent.mkdir(parents=True)
    key_path = Path(a.source_key).resolve()
    key = json.loads(key_path.read_text())
    if a.entry and a.entry in key:
        key = key[a.entry]  # the frozen generic manifest separates its repository scopes
    if a.entry:
        prove.fetch(a.entry, src)
        source_pin = {"entry": a.entry, "binding": prove.infra()[a.entry]}
    else:
        if not key.get("origin"):
            raise RuntimeError("neutral source key must name the independently chosen Git origin")
        shutil.copytree(Path(a.source).resolve(), src, symlinks=True, ignore=shutil.ignore_patterns(".git"))
        meta = key.get("fixture_git", {})
        prove.git("-c", "init.defaultBranch=" + meta.get("branch", "main"), "init", "-q", cwd=src)
        prove.git("remote", "add", "origin", key["origin"], cwd=src)
        prove.git("add", "-A", cwd=src)
        env = {**os.environ, **prove.FIXED_GIT}
        env.update(key.get("fixed_git", {}))
        if meta:
            env.update(GIT_AUTHOR_NAME=meta["author_name"], GIT_COMMITTER_NAME=meta.get("committer_name", meta["author_name"]),
                       GIT_AUTHOR_EMAIL=meta["author_email"], GIT_COMMITTER_EMAIL=meta.get("committer_email", meta["author_email"]),
                       GIT_AUTHOR_DATE=meta.get("author_date", meta.get("author_and_committer_date")),
                       GIT_COMMITTER_DATE=meta.get("committer_date", meta.get("author_and_committer_date")))
        subprocess.run(["git", "commit", "-q", "-m", key.get("commit_message", meta.get("commit_message", meta.get("message", "independent source-key input")))],
                       cwd=src, env=env, check=True, capture_output=True)
        if prove.git("rev-parse", "HEAD", cwd=src).strip() != meta.get("commit", key.get("commit")) \
                or meta.get("tree") and prove.git("rev-parse", "HEAD^{tree}", cwd=src).strip() != meta["tree"]:
            raise RuntimeError("reconstructed neutral Git commit/tree differs from the frozen key")
        source_pin = {"independent_neutral_source": str(Path(a.source).resolve())}
    if a.entry and key.get("commit") and prove.git("rev-parse", "HEAD", cwd=src).strip() != key["commit"]:
        raise RuntimeError("pinned input commit differs from the independent source key")
    source_files = key.get("files", {})
    expected = key.get("files_sha256") or {path: row["sha256"] for path, row in key.get("fixture_files", {}).items()} \
        or ({path: row.get("sha256", row.get("target_sha256")) if isinstance(row, dict) else row for path, row in source_files.items()}
            if isinstance(source_files, dict) else {row["path"]: row["sha256"] for row in source_files})
    if not expected:
        raise RuntimeError("source key must supply the complete declared input hashes")
    for path, expected_hash in expected.items():
        if source_hash(src / path) != expected_hash:
            raise RuntimeError("independent source bytes changed: " + path)
    tracked = prove.git("ls-files", cwd=src).splitlines()
    files = {path: hashlib.sha256((src / path).read_bytes()).hexdigest() for path in tracked if (src / path).is_file() and not (src / path).is_symlink()}
    symlinks = {path: {"target": os.readlink(src / path), "sha256": source_hash(src / path)} for path in tracked if (src / path).is_symlink()}
    save(run / "inputs.json", {"source": source_pin, "input_commit": prove.git("rev-parse", "HEAD", cwd=src).strip(), "files": files, "symlinks": symlinks, "binary_sha256": hashlib.sha256(Path(binary).read_bytes()).hexdigest(), "query_file_sha256": hashlib.sha256(queries_path.read_bytes()).hexdigest(), "source_key_sha256": hashlib.sha256(key_path.read_bytes()).hexdigest(), "phase_key_sha256": hashlib.sha256(Path(a.phase_key).read_bytes()).hexdigest() if a.phase_key else None, "product_source_commit": prove.git("rev-parse", "HEAD", cwd=root).strip(), "semantic_judgment": "PENDING"})
    repo = run / "work" / "head" / name
    repo.parent.mkdir(parents=True)
    shutil.copytree(src, repo, symlinks=True)
    phase_key = json.loads(Path(a.phase_key).read_text()) if a.phase_key else {}
    outcomes = []
    lifecycle_err = run / "lifecycle/mcp.err"
    lifecycle_err.parent.mkdir()

    def close(client):
        try:
            client.close()
        finally:
            if client.p.poll() is None:
                client.p.terminate()
                try:
                    client.p.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    client.p.kill()
                    client.p.wait(timeout=10)

    def capture(work, folder, phase, cold=False, client=None):
        out = folder / "head" / name
        out.mkdir(parents=True, exist_ok=True)
        def cli(label, cmd):
            save(out / (label + ".command.json"), [binary, *cmd])
            result = subprocess.run([binary, *cmd], cwd=work, capture_output=True, timeout=900)
            (out / (label + (".json" if label == "export1" else ".out"))).write_bytes(result.stdout)
            (out / (label + ".err")).write_bytes(result.stderr)
            (out / (label + ".exit")).write_text(str(result.returncode) + "\n")
            result.check_returncode()
        if cold:
            cli("index", ["index"])
        own_client = client is None
        error_path = out / "mcp.err" if own_client else lifecycle_err
        offset = error_path.stat().st_size if error_path.exists() and phase != "baseline" else 0
        if own_client:
            client = prove.Stdio(binary, work, error_path)
        responses, answers, seconds = [], [], []
        try:
            # The persistent reader sees edited source before any CLI refresh/export.
            for query in queries:
                start = time.monotonic()
                response = client.call("tools/call", {"name": query["tool"], "arguments": query["arguments"]})
                responses.append(response)
                answers.append(prove.answer(response))
                seconds.append(round(time.monotonic() - start, 3))
            tools_response = client.call("tools/list", {})
        finally:
            if own_client:
                close(client)
        (out / "mcp-phase.err").write_bytes(error_path.read_bytes()[offset:])
        record = {"bin": binary, "queries": queries, "initialize": client.info, "tools": tools_response.get("result", {}).get("tools"),
                  "tools_response": tools_response, "answers": answers, "responses": responses, "seconds": seconds}
        save(out / "record.json", record)
        cli("status", ["status"])
        cli("export1", ["export"])
        http_responses = []
        original_http = prove.Http
        class RecordedHttp(original_http):
            def call(self, method, params):
                response = super().call(method, params)
                http_responses.append({"method": method, "response": response})
                return response
        prove.Http = RecordedHttp
        try:
            line, failed = prove.http_parity(binary, name, work, folder, record)
        finally:
            prove.Http = original_http
            save(folder / "http" / name / "responses.json", http_responses)
        privacy = phase_key.get("privacy", {})
        forbidden = privacy.get("sensitivity_public_forbidden_substrings", []) if phase == "sensitivity_public" else \
            privacy.get("restored_forbidden_substrings", []) if phase == "restored" else \
            privacy.get("baseline_and_literal_changed_forbidden_substrings", [])
        payloads = json.dumps([client.info, responses, tools_response, http_responses], ensure_ascii=False)
        diagnostics = "\n".join(path.read_text(errors="replace") for path in out.iterdir()
                                if path.suffix in (".out", ".err") or path.name == "export1.json")
        diagnostics += "\n" + "\n".join(path.read_text(errors="replace") for path in (folder / "http" / name).iterdir()
                                       if path.suffix in (".out", ".err"))
        leaks = [text for text in forbidden if text in payloads or text in diagnostics]
        export_text = (out / "export1.json").read_text()
        leaks += [text for text in privacy.get("export_always_forbidden_substrings", []) if text in export_text]
        contexts = json.dumps([response for query, response in zip(queries, responses) if query["tool"] == "context"], ensure_ascii=False)
        leaks += [text for text in privacy.get("declared_target_context_no_scalar_payloads", []) if text in contexts]
        outcome = {"phase": phase, "raw": str(folder.relative_to(run)) or ".", "http_copies_stdio": line,
                   "transport_equal": not failed, "privacy_leaks": sorted(set(leaks)), "semantic_judgment": "PENDING"}
        outcomes.append(outcome)
        save(folder / "capture.json", outcome)
        print(line, flush=True)
        return {"tools": record["tools"], "answers": answers,
                "response_payloads": [{k: v for k, v in response.items() if k != "id"} for response in [client.info, tools_response, *responses]],
                "status": (out / "status.out").read_text(), "export": json.loads(export_text)}

    def compare(label, first, second):
        equal = {field: first[field] == second[field] for field in first}
        result = {"phase": label, "whole_payload_equal": equal, "passed": all(equal.values()),
                  "excluded_transport_envelope": "JSON-RPC id only; complete raw replies are retained"}
        outcomes.append(result)
        save(run / "comparisons" / (label + ".json"), result)

    # Index once, then keep the same MCP reader for every source-only phase.
    baseline_out = run / "head" / name
    baseline_out.mkdir(parents=True)
    save(baseline_out / "index.command.json", [binary, "index"])
    result = subprocess.run([binary, "index"], cwd=repo, capture_output=True, timeout=900)
    (baseline_out / "index.out").write_bytes(result.stdout)
    (baseline_out / "index.err").write_bytes(result.stderr)
    (baseline_out / "index.exit").write_text(str(result.returncode) + "\n")
    result.check_returncode()
    client = prove.Stdio(binary, repo, lifecycle_err)
    try:
        baseline = capture(repo, run, "baseline", client=client)
        for edit in phase_key.get("source_edits", []):
            if edit.get("starts_from") != "baseline":
                raise RuntimeError("each source edit must start from baseline")
            target = repo / edit["file"]
            original, stamp = target.read_bytes(), target.stat()
            replacement = edit["replace_exactly_once"]
            before, after = replacement["before"].encode(), replacement["after"].encode()
            if original.count(before) != 1 or edit.get("same_utf8_byte_count", True) and len(before) != len(after) \
                    or hashlib.sha256(original).hexdigest() != edit["sha256_before"]:
                raise RuntimeError("source edit anchor/hash/size changed")
            backup = run / "originals" / edit["phase"] / edit["file"]
            backup.parent.mkdir(parents=True)
            backup.write_bytes(original)
            try:
                changed = original.replace(before, after, 1)
                if hashlib.sha256(changed).hexdigest() != edit["sha256_after"]:
                    raise RuntimeError("source edit changed beyond the frozen key")
                target.write_bytes(changed)
                os.utime(target, ns=(stamp.st_atime_ns, stamp.st_mtime_ns))
                actual = target.stat()
                save(run / "phases" / edit["phase"] / "source-edit.json", {
                    "file": edit["file"], "bytes_before": len(original), "bytes_after": actual.st_size,
                    "sha256_before": source_hash(backup), "sha256_after": source_hash(target),
                    "mtime_before_ns": stamp.st_mtime_ns, "mtime_after_ns": actual.st_mtime_ns,
                    "first_reader": "existing persistent MCP session, before CLI export/status"})
                if actual.st_mtime_ns != stamp.st_mtime_ns or source_hash(target) != edit["sha256_after"]:
                    raise RuntimeError("source bytes/mtime differ before the warm query")
                warm = capture(repo, run / "phases" / edit["phase"] / "warm", edit["phase"], client=client)
                cold_repo = run / "work/cold" / edit["phase"] / name
                cold_repo.parent.mkdir(parents=True)
                shutil.copytree(src, cold_repo, symlinks=True)
                (cold_repo / edit["file"]).write_bytes(changed)
                os.utime(cold_repo / edit["file"], ns=(stamp.st_atime_ns, stamp.st_mtime_ns))
                if any(source_hash(repo / path) != source_hash(cold_repo / path) for path in expected):
                    raise RuntimeError("warm/cold source inputs differ")
                cold = capture(cold_repo, run / "phases" / edit["phase"] / "cold", edit["phase"], cold=True)
                compare(edit["phase"] + "-warm-cold", warm, cold)
            finally:
                shutil.copyfile(backup, target)
                os.utime(target, ns=(stamp.st_atime_ns, stamp.st_mtime_ns))
                cmp = subprocess.run(["cmp", str(backup), str(target)], capture_output=True)
                source_equal = all(source_hash(repo / path) == value for path, value in expected.items())
                restore = {"cmp_exit": cmp.returncode, "all_source_hashes_equal": source_equal,
                           "mtime_restored": target.stat().st_mtime_ns == stamp.st_mtime_ns,
                           "cmp_command": ["cmp", str(backup), str(target)],
                           "cmp_stdout": cmp.stdout.decode(errors="replace"), "cmp_stderr": cmp.stderr.decode(errors="replace")}
                save(run / "restoration" / (edit["phase"] + ".json"), restore)
                if cmp.returncode or not source_equal or not restore["mtime_restored"]:
                    raise RuntimeError("STOP: source fixture failed exact restore")
            restored = capture(repo, run / "phases" / edit["phase"] / "restored", "restored", client=client)
            compare(edit["phase"] + "-restored-baseline", restored, baseline)
            cold_restored_repo = run / "work/cold-restored" / edit["phase"] / name
            cold_restored_repo.parent.mkdir(parents=True)
            shutil.copytree(src, cold_restored_repo, symlinks=True)
            cold_restored = capture(cold_restored_repo, run / "phases" / edit["phase"] / "cold-restored", "restored", cold=True)
            compare(edit["phase"] + "-restored-warm-cold", restored, cold_restored)
        for phase, subset in phase_key.get("scope_phases", {}).items():
            if not isinstance(subset, dict) or "source_paths" not in subset:
                continue
            scope_repo = run / "work/scope" / phase / name
            scope_repo.parent.mkdir(parents=True)
            shutil.copytree(src, scope_repo, symlinks=True)
            keep = set(subset["source_paths"])
            if not keep <= set(expected):
                raise RuntimeError("scope phase names a source path outside the frozen key")
            for path in expected:
                if path not in keep:
                    (scope_repo / path).unlink()
            save(run / "phases" / phase / "source-scope.json", {"kept": sorted(keep), "removed": sorted(set(expected) - keep),
                 "sha256": {path: expected[path] for path in sorted(keep)}, "semantic_judgment": "PENDING"})
            capture(scope_repo, run / "phases" / phase, phase, cold=True)
    finally:
        close(client)
    failed = any(not item.get("transport_equal", True) or item.get("privacy_leaks") or item.get("passed") is False for item in outcomes)
    save(run / "capture.json", {"status": "CAPTURE_FAILED" if failed else "CAPTURED", "outcomes": outcomes,
                                "semantic_judgment": "PENDING_INDEPENDENT_SOURCE_COMPARISON"})
    return 1 if failed else 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, RuntimeError, KeyError, TypeError, subprocess.SubprocessError) as error:
        print("BLOCKED: source capture: " + str(error), file=sys.stderr)
        sys.exit(2)
