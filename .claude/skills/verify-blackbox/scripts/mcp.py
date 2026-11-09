#!/usr/bin/env python3
"""Usage: mcp.py <meada-binary> <repo-dir> <tool|--list> [json-args]"""
import json, subprocess, sys
binary, repo, tool = sys.argv[1], sys.argv[2], sys.argv[3]
args = json.loads(sys.argv[4]) if len(sys.argv) > 4 else {}
p = subprocess.Popen([binary, "mcp"], cwd=repo, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
def send(m): p.stdin.write(json.dumps(m) + "\n"); p.stdin.flush()
def recv(i):
    while True:
        line = p.stdout.readline()
        if not line: sys.exit("server closed")
        m = json.loads(line)
        if m.get("id") == i: return m
send({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "v", "version": "0"}}})
recv(1)
send({"jsonrpc": "2.0", "method": "notifications/initialized"})
if tool == "--list":
    send({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
    print(json.dumps(recv(2)["result"], indent=1))
else:
    send({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": tool, "arguments": args}})
    r = recv(2)
    res = r.get("result") or r
    for c in res.get("content", []) if isinstance(res, dict) else []:
        print(c.get("text", c))
    if "content" not in res: print(json.dumps(res, indent=1))
p.stdin.close(); p.wait(timeout=30)
