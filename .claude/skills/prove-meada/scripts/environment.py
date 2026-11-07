#!/usr/bin/env python3
"""#663 through CLI and MCP. Answer key: #663/#642 and ADR 0005, never fixture goldens."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import time
from prove import git, free_port, Http, answer, mcp_answers
from provisions import write, export


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--head-bin', required=True)
    parser.add_argument('--run', required=True)
    args = parser.parse_args()
    os.environ["TASK_QUEUE"] = "https://sqs.eu-west-1.amazonaws.com/111111111111/process-value-must-not-win"
    os.environ["LOCAL_QUEUE"] = "process-value-must-not-win"
    binary = str(Path(args.head_bin).resolve())
    run = Path(args.run).resolve()
    run.mkdir(parents=True, exist_ok=False)
    app, infra = run / 'work/app', run / 'work/infra'
    for root, name, peer in [(app, 'app', 'infra'), (infra, 'infra', 'app')]:
        root.mkdir(parents=True)
        git('init', '-q', cwd=root)
        git('remote', 'add', 'origin', f'https://example.com/env-proof/{name}.git', cwd=root)
        write(root, 'meada.toml', f'[[peer]]\nrepo = "example.com/env-proof/{peer}"\n')
    write(app, '.github/workflows/publish.yaml', '''jobs:
  publish:
    steps:
      - uses: docker/build-push-action@v6
        with:
          tags: registry.example.com/proof/sender:1
''')
    write(app, 'sender.ts', '''import { SQSClient, SendMessageCommand } from '@aws-sdk/client-sqs';
const client = new SQSClient({});
export function dispatch() {
  return client.send(new SendMessageCommand({QueueUrl: process.env.TASK_QUEUE, MessageBody: 'x'}));
}
export function inherited() {
  return client.send(new SendMessageCommand({QueueUrl: process.env.FROM_QUEUE, MessageBody: 'x'}));
}
export function outside() {
  return client.send(new SendMessageCommand({QueueUrl: process.env.LOCAL_QUEUE, MessageBody: 'x'}));
}
export function secretFrom() {
  return client.send(new SendMessageCommand({QueueUrl: process.env.SECRET_FROM, MessageBody: 'x'}));
}
export function unrelated() {
  return client.send(new SendMessageCommand({QueueUrl: process.env.OTHER_QUEUE, MessageBody: 'x'}));
}
export function secret() {
  return client.send(new SendMessageCommand({QueueUrl: process.env.SECRET_QUEUE, MessageBody: 'x'}));
}
''')
    write(app, 'local/local.ts', "import { SQSClient, SendMessageCommand } from '@aws-sdk/client-sqs';\nconst client = new SQSClient({});\nexport function local() { return client.send(new SendMessageCommand({QueueUrl: process.env.LOCAL_QUEUE, MessageBody: 'x'})); }\n")
    write(app, 'compose.yaml', "services:\n  local:\n    build: ./local\n    environment:\n      LOCAL_QUEUE: ${LOCAL_QUEUE:-https://sqs.eu-west-1.amazonaws.com/111111111111/tasks}\n")
    write(infra, 'queues.tf' , 'resource "aws_sqs_queue" "tasks" { name = "tasks" }\n')
    write(infra, 'workloads.yaml', '''apiVersion: v1
kind: ConfigMap
metadata:
  name: endpoints
  namespace: proof
data:
  url: https://sqs.eu-west-1.amazonaws.com/111111111111/tasks
  FROM_QUEUE: https://sqs.eu-west-1.amazonaws.com/111111111111/tasks
---
apiVersion: v1
kind: Secret
metadata:
  name: hidden
  namespace: proof
stringData:
  SECRET_FROM: secret-value-must-stay-hidden
  url: secret-value-must-stay-hidden
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: sender
  namespace: proof
spec:
  template:
    spec:
      containers:
        - name: main
          image: registry.example.com/proof/sender:1
          envFrom:
            - configMapRef: {name: endpoints}
            - secretRef: {name: hidden}
          env:
            - name: TASK_QUEUE
              valueFrom:
                configMapKeyRef: {name: endpoints, key: url}
            - name: SECRET_QUEUE
              valueFrom:
                secretKeyRef: {name: hidden, key: url}
        - name: other
          image: registry.example.com/proof/unrelated:1
          env:
            - {name: OTHER_QUEUE, value: tasks}
---
apiVersion: v1
kind: Pod
metadata:
  name: unrelated
  namespace: proof
spec:
  containers:
    - name: other
      image: registry.example.com/proof/unrelated:1
      env:
        - {name: OTHER_QUEUE, value: tasks}
''')
    inventory = {}
    for root in (app, infra):
        for path in root.rglob('*'):
            if path.is_file() and '.git' not in path.parts:
                inventory[str(path.relative_to(run))] = hashlib.sha256(path.read_bytes()).hexdigest()
        git('add', '.', cwd=root)
        git('commit', '-qm', 'workload environment proof', cwd=root)
    (run / 'input-sha256.json').write_text(json.dumps(inventory, indent=2))
    data = export(binary, app)
    (run / 'app.json').write_text(json.dumps(data, indent=2))
    producers = [p for p in data['providers'] if p['contract'].startswith('channel::')]
    assert len(producers) == 7, producers
    resolved = ('::dispatch::', '::inherited::', '::local::')
    assert all(p['contract'] == ('channel::sqs::tasks' if any(token in p['site'] for token in resolved) else 'channel::sqs') for p in producers), producers
    links = data.get('provisions', [])
    expected = {
        'example.com/env-proof/app::sender.ts::dispatch::SendMessageCommand#0',
        'example.com/env-proof/app::sender.ts::inherited::SendMessageCommand#0',
        'example.com/env-proof/app::local/local.ts::local::SendMessageCommand#0',
    }
    assert len(links) == 3 and {p['side'] for p in links} == expected, links
    assert all(p['level'] == 'inferred' and p['contract'] == 'channel::sqs::tasks'
               and p['provisioner'] == 'example.com/env-proof/infra::queues.tf::aws_sqs_queue.tasks'
               and p['root'] == '.' for p in links), links
    dispatch = next(p for p in links if '::dispatch::' in p['side'])
    assert 'Deployment.apps/proof/sender' in dispatch['reason'] and 'TASK_QUEUE' in dispatch['reason'], dispatch
    assert any('services.local' in p['reason'] and 'LOCAL_QUEUE' in p['reason'] for p in links), links
    queries = [
        {'tool': 'context', 'arguments': {'symbol': 'Deployment.apps/proof/sender', 'budget': 8000}},
        {'tool': 'context', 'arguments': {'symbol': 'dispatch', 'budget': 8000}},
        {'tool': 'trace', 'arguments': {'from': 'aws_sqs_queue.tasks', 'direction': 'down', 'budget': 8000}},
    ]
    tools, texts, _ = mcp_answers(binary, infra, queries, run / 'stdio.err')
    (run / 'stdio.json').write_text(json.dumps(texts, indent=2))
    assert all(token in texts[0] for token in ['TASK_QUEUE', 'ConfigMap', 'FROM_QUEUE', 'envFrom', 'SECRET_QUEUE', 'SECRET_FROM', 'redacted']), texts[0]
    assert 'secret-value-must-stay-hidden' not in texts[0], texts[0]
    assert 'inferred' in texts[1] and 'TASK_QUEUE' in texts[1], texts[1]
    assert 'sender.ts' in texts[2], texts[2]
    manifest = 'index_dir = "indexes"\n'
    for root, name in [(infra, 'infra'), (app, 'app')]:
        manifest += f'\n[[repository]]\nrepo = "example.com/env-proof/{name}"\npath = "{root}"\n'
    write(run, 'workspace.toml', manifest)
    with (run / 'index.out').open('w') as out, (run / 'index.err').open('w') as err:
        subprocess.run([binary, 'index', '--workspace', 'workspace.toml'], cwd=run, check=True, stdout=out, stderr=err)
    port = free_port()
    with (run / 'http.out').open('w') as out, (run / 'http.err').open('w') as err:
        server = subprocess.Popen([binary, 'mcp', '--listen', f'127.0.0.1:{port}', '--workspace', 'workspace.toml'], cwd=run, stdout=out, stderr=err)
        try:
            for _ in range(100):
                try:
                    socket.create_connection(('127.0.0.1', port), timeout=1).close()
                    break
                except OSError:
                    time.sleep(.1)
            client = Http(f'http://127.0.0.1:{port}/example.com/env-proof/infra')
            assert client.call('tools/list', {})['result']['tools'] == tools
            replies = [answer(client.call('tools/call', {'name': q['tool'], 'arguments': q['arguments']})) for q in queries]
            (run / 'http.json').write_text(json.dumps(replies, indent=2))
            assert replies == texts
        finally:
            server.terminate()
            server.wait(timeout=30)
    path = infra / 'workloads.yaml'
    original = path.read_text()
    path.write_text(original.replace('/111111111111/tasks', '/111111111111/other', 1))
    changed = export(binary, app)
    (run / 'changed.json').write_text(json.dumps(changed, indent=2))
    assert any(p['contract'] == 'channel::sqs::other' and '::dispatch::' in p['site'] for p in changed['providers'])
    assert not any('::dispatch::' in p['side'] for p in changed.get('provisions', []))
    path.write_text(original)
    assert export(binary, app)['providers'] == data['providers']
    write(app, 'compose.override.yaml', 'services:\n  local:\n    environment:\n      - LOCAL_QUEUE\n')
    overridden = export(binary, app)
    (run / 'compose-override.json').write_text(json.dumps(overridden, indent=2))
    assert all(p['contract'] == 'channel::sqs' for p in overridden['providers'] if '::local::' in p['site']), overridden
    (run / 'checks.json').write_text(json.dumps({'checks': 'passed', 'answer_key': '#663/#642; ADR 0005', 'surfaces': ['CLI', 'MCP stdio', 'MCP HTTP']}, indent=2))


if __name__ == '__main__':
    main()
