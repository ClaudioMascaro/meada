#!/usr/bin/env python3
"""Drive #672's alert contract through CLI and both MCP transports.

Answer keys: #672, #642 and ADR 0026, read by the checker at run time. This driver creates
neutral inputs, imports only the existing transport mechanics, and preserves raw evidence.
A successful run reports checks passed, never the independent checker's verdict.
"""
import argparse
import hashlib
import json
from pathlib import Path
import socket
import subprocess
import time
from prove import git, free_port, Http, answer, mcp_answers


def write(root, path, text):
    target = root / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(text)


def command(binary, repo, run, name, *args):
    result = subprocess.run([binary, *args], cwd=repo, capture_output=True, text=True, timeout=90)
    write(run, name + '.out', result.stdout)
    write(run, name + '.err', result.stderr)
    write(run, name + '.exit', str(result.returncode))
    result.check_returncode()
    return result.stdout


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--head-bin', required=True)
    parser.add_argument('--run', required=True)
    parser.add_argument('--http', action='store_true')
    args = parser.parse_args()
    binary = str(Path(args.head_bin).resolve())
    run = Path(args.run).resolve()
    run.mkdir(parents=True, exist_ok=False)
    rules, routing, app = [run / 'work' / name for name in ['rules', 'routing', 'app']]
    identities = ['example.com/alert-proof/' + p.name for p in [rules, routing, app]]
    for root, identity in zip([rules, routing, app], identities):
        root.mkdir(parents=True)
        git('init', '-q', cwd=root)
        git('remote', 'add', 'origin', 'https://' + identity + '.git', cwd=root)
        write(root, 'meada.toml', ''.join(f'[[peer]]\nrepo = "{other}"\n' for other in identities if other != identity))
    write(rules, 'signals.yaml', '''groups:
  - name: checkout
    rules:
      - alert: CheckoutDelay
        expr: latency_seconds > 10
        labels:
          support: squad-cedar
          severity: critical
      - alert: OtherTeam
        expr: errors_total > 2
        labels:
          support: squad-maple
''')
    write(routing, 'dispatch.yaml', '''route:
  receiver: fallback
  routes:
    - receiver: cedar-oncall
      continue: true
      match: {support: squad-cedar, severity: critical}
    - receiver: squads
      match_re: {support: "squad-.*"}
receivers:
  - name: fallback
  - name: cedar-oncall
  - name: squads
''')
    write(app, 'values.yaml', '''groups:
  - name: unrendered
    rules:
      - alert: HelmOnly
        expr: up == 0
        labels: {support: squad-cedar}
''')
    for root in [rules, routing, app]:
        git('add', '.', cwd=root)
        git('commit', '-qm', 'neutral alert proof', cwd=root)
    inventory = {str(p.relative_to(run)): hashlib.sha256(p.read_bytes()).hexdigest()
                 for root in [rules, routing, app] for p in root.rglob('*')
                 if p.is_file() and '.git' not in p.parts}
    write(run, 'inputs.sha256.json', json.dumps(inventory, indent=2, sort_keys=True))
    command(binary, rules, run, 'version', '--version')
    exports = {root.name: json.loads(command(binary, root, run, root.name, 'export')) for root in [rules, routing, app]}
    providers = exports['rules']['providers']
    assert sorted(p['contract'] for p in providers) == ['alert::alertname::CheckoutDelay', 'alert::alertname::OtherTeam', 'alert::severity::critical', 'alert::support::squad-cedar', 'alert::support::squad-maple'], providers
    assert exports['app']['providers'] == [], exports['app']
    assert len(exports['routing']['consumers']) == 3, exports['routing']
    links = exports['routing']['links']
    expected = sorted([
        ('alert::support::squad-cedar', 'exact'),
        ('alert::severity::critical', 'exact'),
        ('alert::support::squad-.*', 'inferred'),
        ('alert::support::squad-.*', 'inferred'),
    ])
    assert sorted((l['contract'], l['level']) for l in links) == expected, links
    assert exports['rules']['links'] == links, exports
    queries = [
        {'tool': 'search', 'arguments': {'query': 'squad-cedar', 'kind': 'alert', 'budget': 8000}},
        {'tool': 'context', 'arguments': {'symbol': 'dispatch.yaml::route.routes[0]', 'budget': 8000}},
        {'tool': 'trace', 'arguments': {'from': 'signals.yaml::groups[0].rules[0]', 'direction': 'down', 'budget': 8000}},
        {'tool': 'trace', 'arguments': {'from': 'dispatch.yaml::route.routes[0]', 'direction': 'up', 'budget': 8000}},
    ]
    tools, texts, _ = mcp_answers(binary, routing, queries, run / 'stdio.err')
    write(run, 'queries.json', json.dumps(queries, indent=2))
    write(run, 'stdio.json', json.dumps(texts, indent=2))
    assert 'alert::support::squad-cedar' in texts[0], texts[0]
    assert 'receiver: cedar-oncall' in texts[1], texts[1]
    assert 'dispatch.yaml' in texts[2] and 'inferred' in texts[2], texts[2]
    reverse_rows = texts[3].splitlines()
    assert reverse_rows[:2] == [
        'example.com/alert-proof/routing dispatch.yaml:4 route.routes[0]',
        '  →[exact: alert::severity::critical; matchers "severity"="critical", "support"="squad-cedar"; receiver: cedar-oncall] example.com/alert-proof/rules signals.yaml:4 groups[0].rules[0]',
    ], texts[3]
    assert reverse_rows[2].startswith('chain: exact · 1 hops · 2 repos · '), texts[3]
    if args.http:
        manifest = 'index_dir = "indexes"\n' + ''.join(f'[[repository]]\nrepo = "{i}"\npath = "{r}"\n' for r, i in zip([routing, rules, app], [identities[1], identities[0], identities[2]]))
        write(run, 'workspace.toml', manifest)
        command(binary, run, run, 'index', 'index', '--workspace', 'workspace.toml')
        port = free_port()
        with open(run / 'http.out', 'w') as out, open(run / 'http.err', 'w') as err:
            server = subprocess.Popen([binary, 'mcp', '--listen', f'127.0.0.1:{port}', '--workspace', 'workspace.toml'], cwd=run, stdin=subprocess.DEVNULL, stdout=out, stderr=err)
            try:
                for _ in range(100):
                    try:
                        socket.create_connection(('127.0.0.1', port), timeout=1).close()
                        break
                    except OSError:
                        if server.poll() is not None:
                            raise RuntimeError('HTTP server exited before readiness')
                        time.sleep(.1)
                client = Http(f'http://127.0.0.1:{port}/{identities[1]}')
                assert client.call('tools/list', {})['result']['tools'] == tools
                http = [answer(client.call('tools/call', {'name': q['tool'], 'arguments': q['arguments']})) for q in queries]
                write(run, 'http.json', json.dumps(http, indent=2))
                assert http == texts, http
            finally:
                server.terminate()
                server.wait(timeout=30)
    original = (rules / 'signals.yaml').read_text()
    (rules / 'signals.yaml').write_text(original.replace('critical', 'warning '))
    changed = json.loads(command(binary, routing, run, 'changed', 'export'))
    assert len(changed['links']) == 2 and all(l['level'] == 'inferred' for l in changed['links']), changed
    status = command(binary, routing, run, 'orphans', 'status')
    assert 'no emitter' in status, status
    (rules / 'signals.yaml').write_text(original)
    restored = json.loads(command(binary, routing, run, 'restored', 'export'))
    assert restored['links'] == links, restored
    write(run, 'checks.txt', 'CLI export/status, emitter and matcher sets, exact/regex levels, receiver context, both trace directions, Helm exclusion, refresh and restoration passed.\n' + ('HTTP parity passed.\n' if args.http else 'HTTP not requested.\n'))
    print(run / 'checks.txt')


if __name__ == '__main__':
    main()
