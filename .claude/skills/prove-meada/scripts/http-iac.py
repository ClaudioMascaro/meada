#!/usr/bin/env python3
"""#665 through CLI and both MCP transports; answer key: #665/#642 and ADR 0026."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import time
from prove import Http, answer, free_port, git, mcp_answers
from provisions import export, write


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--head-bin', required=True)
    parser.add_argument('--base-bin')
    parser.add_argument('--run', required=True)
    args = parser.parse_args()
    binary = str(Path(args.head_bin).resolve())
    run = Path(args.run).resolve()
    run.mkdir(parents=True, exist_ok=False)
    # The binaries this evidence came from, as iac-http-routes.py records them.
    identity = lambda path: {'path': path, 'sha256': hashlib.sha256(Path(path).read_bytes()).hexdigest(),
                             'version': subprocess.check_output([path, '--version'], text=True).strip()}
    (run / 'binary.json').write_text(json.dumps({**identity(binary), **(
        {'base': identity(str(Path(args.base_bin).resolve()))} if args.base_bin else {})}, indent=2))
    os.environ['BILLING_URL'] = 'https://process-environment-must-stay-unread.invalid'
    app, infra, server = [run / 'work' / name for name in ('app', 'infra', 'server')]
    for root in (app, infra, server):
        root.mkdir(parents=True)
        git('init', '-q', cwd=root)
        git('remote', 'add', 'origin', f'https://example.com/http-proof/{root.name}.git', cwd=root)
        write(root, 'meada.toml', ''.join(f'[[peer]]\nrepo = "example.com/http-proof/{name}"\n' for name in ('app', 'infra', 'server') if name != root.name))
    for root in (app, server):
        write(root, '.github/workflows/publish.yml', f'jobs:\n  publish:\n    steps:\n      - uses: docker/build-push-action@v6\n        with:\n          tags: registry.example.com/proof/{root.name}:1\n')
    with (server / 'meada.toml').open('a') as file:
        file.write('[serve]\nhosts = ["billing.example.com"]\n')
    write(server, 'app/bills/route.ts', 'export function GET() { return Response.json({ok: true}); }\n')
    source = '''import { Config } from '@backstage/config';
export function fromWorkload() { return fetch(`${process.env.BILLING_URL}/bills`); }
export function publicCall() { return fetch('https://public.example.com/api/bills'); }
export function directCall() { return fetch('https://billing.example.com/bills'); }
export function rewriteCall() { return fetch('https://rewrite.example.com/api/bills'); }
export function unreadLonger() { return fetch('https://public.example.com/api/private/bills'); }
export function wrongNamespace() { return fetch('http://billing.elsewhere.svc/bills'); }
export function missingService() { return fetch('http://absent/bills'); }
export function fromSecret() { return fetch(`${process.env.SECRET_URL}/bills`); }
export function fromConfig() { return fetch('/api/proxy/billing/bills'); }
'''
    hosts = ['billing', 'billing.workshop', 'billing.workshop.svc', 'billing.workshop.svc.cluster.local']
    for index, host in enumerate(hosts + [f'{host}:8080' for host in hosts]):
        source += f"export function dns{index}() {{ return fetch('http://{host}/bills'); }}\n"
    write(app, 'client.ts', source)
    write(app, 'app-config.yaml', "proxy:\n  endpoints:\n    '/billing':\n      target: http://billing.workshop.svc:8080\n")
    write(app, 'local/client.ts', 'export function localCall() { return fetch(`${process.env.LOCAL_URL}/stock`); }\n')
    write(app, 'inventory/server.ts', "import express from 'express';\nconst app = express();\napp.get('/stock', (req, res) => res.json({ok: true}));\n")
    write(app, 'outside/server.ts', "import express from 'express';\nconst app = express();\napp.get('/stock', (req, res) => res.json({wrong: true}));\n")
    write(app, 'compose.yaml', 'services:\n  local:\n    build: ./local\n    environment:\n      LOCAL_URL: http://inventory:7000\n  inventory:\n    build: ./inventory\n')
    write(infra, 'workloads.yaml', '''apiVersion: v1
kind: Secret
metadata: {name: hidden, namespace: workshop}
stringData: {url: http://billing:8080}
---
apiVersion: apps/v1
kind: Deployment
metadata: {name: web, namespace: workshop}
spec:
  template:
    spec:
      containers:
        - name: main
          image: registry.example.com/proof/app:1
          env:
            - {name: BILLING_URL, value: 'http://billing:8080'}
            - name: SECRET_URL
              valueFrom:
                secretKeyRef: {name: hidden, key: url}
---
apiVersion: v1
kind: Service
metadata: {name: billing, namespace: workshop}
spec:
  selector: {app: bills, stage: production}
---
apiVersion: apps/v1
kind: Deployment
metadata: {name: bills, namespace: workshop}
spec:
  template:
    metadata: {labels: {app: bills, stage: production}}
    spec:
      containers: [{name: main, image: 'registry.example.com/proof/server:1'}]
---
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata: {name: public, namespace: workshop}
spec:
  rules:
    - host: public.example.com
      http:
        paths:
          - {path: /api, pathType: Prefix, backend: {service: {name: billing, port: {number: 8080}}}}
          - {path: /api/private, pathType: Prefix, backend: {service: {name: absent, port: {number: 8080}}}}
---
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: rewrite
  namespace: workshop
  annotations: {nginx.ingress.kubernetes.io/rewrite-target: /}
spec:
  rules:
    - host: rewrite.example.com
      http:
        paths:
          - {path: /api, pathType: Prefix, backend: {service: {name: billing, port: {number: 8080}}}}
''')
    inputs = {}
    for root in (app, infra, server):
        for path in root.rglob('*'):
            if path.is_file() and '.git' not in path.parts:
                inputs[str(path.relative_to(run))] = hashlib.sha256(path.read_bytes()).hexdigest()
        git('add', '.', cwd=root)
        git('commit', '-qm', 'neutral HTTP infrastructure source', cwd=root)
    (run / 'input-sha256.json').write_text(json.dumps(inputs, indent=2))
    data = export(binary, app)
    (run / 'export.json').write_text(json.dumps(data, indent=2))
    links = data['links']
    expected = {'fromWorkload', 'publicCall', 'directCall', 'fromConfig', 'localCall'} | {f'dns{i}' for i in range(8)}
    http = [link for link in links if link['contract'].startswith('http::')]
    assert len(links) == 14 and len(http) == 13, links
    assert {link['consumer'].split('::')[-2] for link in http} == expected, http
    for link in http:
        name = link['consumer'].split('::')[-2]
        assert link['provider'] == ('example.com/http-proof/app::inventory/server.ts::GET /stock' if name == 'localCall' else 'example.com/http-proof/server::app/bills/route.ts::GET'), link
        assert link['level'] == ('exact' if name == 'directCall' else 'inferred'), link
        if name != 'directCall':
            assert ('services.inventory' if name == 'localCall' else 'Service/workshop/billing') in link['reason'], link
    source_links = {(link['consumer'], link['provider'], link['reason'], link['level']) for link in http}
    hops = []
    for process in data.get('processes', []):
        for step in process['steps']:
            if step.get('via') == 'link':
                hops.append(step)
                assert any(consumer.startswith(process['entry_point'] + '::') and provider == step['node'] and reason == step['reason'] and level == step['level'] for consumer, provider, reason, level in source_links), process
    assert len(hops) == 13, hops
    if args.base_bin:
        baseline = export(str(Path(args.base_bin).resolve()), app)
        (run / 'baseline.json').write_text(json.dumps(baseline, indent=2))
        direct = lambda value: [link for link in value['links'] if '::directCall::' in link['consumer']]
        assert direct(data) == direct(baseline)
        # Processes walk links too: compare their direct-host control and check new hops
        # against the same complete source-derived link set, keeping every other field exact.
        assert {k:v for k,v in data.items() if k not in {'links', 'processes'}} == {k:v for k,v in baseline.items() if k not in {'links', 'processes'}}
        direct_process = lambda value: [p for p in value.get('processes', []) if p['entry_point'].endswith('::directCall')]
        assert direct_process(data) == direct_process(baseline)
    queries = [
        {'tool': 'context', 'arguments': {'symbol': 'fromWorkload', 'budget': 10000}},
        {'tool': 'context', 'arguments': {'symbol': 'publicCall', 'budget': 10000}},
        {'tool': 'trace', 'arguments': {'from': 'publicCall', 'direction': 'down', 'budget': 10000}},
        {'tool': 'search', 'arguments': {'query': '/bills', 'kind': 'route', 'budget': 10000}},
    ]
    tools, texts, _ = mcp_answers(binary, app, queries, run / 'stdio.err')
    (run / 'stdio.json').write_text(json.dumps(texts, indent=2))
    assert all(token in texts[0] for token in ['Service/workshop/billing', 'BILLING_URL', 'inferred']), texts[0]
    assert 'Ingress.networking.k8s.io/workshop/public' in texts[1] and 'app/bills/route.ts' in texts[2], texts
    manifest = 'index_dir = "indexes"\n' + ''.join(f'\n[[repository]]\nrepo = "example.com/http-proof/{root.name}"\npath = "{root}"\n' for root in (app, infra, server))
    write(run, 'workspace.toml', manifest)
    subprocess.run([binary, 'index', '--workspace', 'workspace.toml'], cwd=run, check=True, stdout=(run/'index.out').open('w'), stderr=(run/'index.err').open('w'))
    port = free_port()
    server_process = subprocess.Popen([binary, 'mcp', '--listen', f'127.0.0.1:{port}', '--workspace', 'workspace.toml'], cwd=run, stdout=(run/'http.out').open('w'), stderr=(run/'http.err').open('w'))
    try:
        for _ in range(100):
            try:
                socket.create_connection(('127.0.0.1', port), timeout=1).close()
                break
            except OSError:
                time.sleep(.1)
        client = Http(f'http://127.0.0.1:{port}/example.com/http-proof/app')
        assert client.call('tools/list', {})['result']['tools'] == tools
        replies = [answer(client.call('tools/call', {'name': q['tool'], 'arguments': q['arguments']})) for q in queries]
        (run / 'http.json').write_text(json.dumps(replies, indent=2))
        assert replies == texts
        # Delete every source of new routing facts, keeping code for unchanged display locations.
        for path in (infra/'workloads.yaml', app/'compose.yaml', app/'app-config.yaml'):
            path.unlink()
        deleted = [answer(client.call('tools/call', {'name': q['tool'], 'arguments': q['arguments']})) for q in queries]
        (run / 'source-deleted-http.json').write_text(json.dumps(deleted, indent=2))
        assert deleted == texts, deleted
        readonly = json.loads(subprocess.check_output([binary, 'export', '--workspace', 'workspace.toml', '--repo', 'example.com/http-proof/app'], cwd=run, text=True))
        (run / 'source-deleted-export.json').write_text(json.dumps(readonly, indent=2))
        assert readonly['links'] == data['links']
    finally:
        server_process.terminate()
        server_process.wait(timeout=30)
    (run / 'checks.json').write_text(json.dumps({'checks': 'passed', 'answer_key': '#665/#642; ADR 0026', 'all_links': len(links), 'http_links': len(http), 'process_hops': len(hops), 'base_parity': bool(args.base_bin), 'surfaces': ['CLI', 'MCP stdio', 'MCP HTTP with source facts deleted']}, indent=2))
    print('checks passed; evidence:', run)


if __name__ == '__main__':
    main()
