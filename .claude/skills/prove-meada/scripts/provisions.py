#!/usr/bin/env python3
"""Drive #656 through CLI and both MCP transports. Answer key: #656 and ADR 0026.

provisions.py --head-bin BIN --run DIR
The probe is authored independently of the feature fixtures/type table; evidence stays in DIR.
"""
import argparse
import json
import os
from pathlib import Path
import socket
import subprocess
import time
from prove import git, free_port, Http, answer, mcp_answers


def write(root, path, text):
    p = root / path
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(text)


def export(binary, repo):
    return json.loads(subprocess.check_output([binary, 'export'], cwd=repo, text=True))


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--head-bin', required=True)
    p.add_argument('--run', required=True)
    a = p.parse_args()
    binary = str(Path(a.head_bin).resolve())
    run = Path(a.run).resolve()
    run.mkdir(parents=True, exist_ok=False)
    infra, app = run / 'work/infra', run / 'work/app'
    for root, identity in [(infra, 'infra'), (app, 'app')]:
        root.mkdir(parents=True)
        git('init', '-q', cwd=root)
        git('remote', 'add', 'origin', f'https://example.com/zz-prove/{identity}.git', cwd=root)
    write(infra, 'meada.toml', '[[peer]]\nrepo = "example.com/zz-prove/app"\n')
    write(app, 'meada.toml', '''[[peer]]
repo = "example.com/zz-prove/infra"
[[resource]]
kind = "s3"
id = "receipts"
role = "write"
via = ["app.py::send"]
[[resource]]
kind = "s3"
id = "receipts"
role = "read"
''')
    write(app, 'app.py', '''import boto3
queue = boto3.client("sqs")
def send():
    queue.send_message(QueueUrl="https://sqs.eu-west-1.amazonaws.com/123456789012/jobs", MessageBody="x")
def receive():
    return queue.receive_message(QueueUrl="https://sqs.eu-west-1.amazonaws.com/123456789012/jobs")
''')
    write(infra, 'envs/dev/main.tf', '''resource "aws_sqs_queue" "jobs" { name = "jobs" }
resource "aws_s3_bucket" "receipts" { bucket = "receipts" }
data "aws_sqs_queue" "existing" { name = "jobs" }
resource "aws_sqs_queue" "unknown" { name = terraform.workspace }
''')
    write(infra, 'envs/prod/main.tf', '''variable "queue" { default = "jobs" }
resource "aws_sqs_queue" "jobs" { name = var.queue }
resource "aws_s3_bucket" "receipts" { bucket = "receipts" }
''')
    for root in [infra, app]:
        git('add', '.', cwd=root)
        git('commit', '-qm', 'provisioning probe', cwd=root)
    results = {name: export(binary, root) for name, root in [('infra', infra), ('app', app)]}
    for name, data in results.items():
        (run / f'{name}.json').write_text(json.dumps(data, indent=2))
        assert data['format'] == 1
        assert len(data['provisions']) == 8, data['provisions']
        expected = {
            ('example.com/zz-prove/app::app.py::send::queue.send_message#0', 'channel::sqs::jobs'),
            ('example.com/zz-prove/app::app.py::receive', 'channel::sqs::jobs'),
            ('example.com/zz-prove/app::app.py::send', 'resource::s3::receipts'),
            ('example.com/zz-prove/app', 'resource::s3::receipts'),
        }
        assert {(l['side'], l['contract']) for l in data['provisions']} == expected
        for link in data['provisions']:
            expected_level = ('inferred' if link['root'] == 'envs/prod' else 'exact') \
                if link['contract'].startswith('channel::') else 'declared'
            assert link['level'] == expected_level, link
            if expected_level == 'inferred':
                assert 'var.queue (default)' in link['reason'], link
        assert len(data['provisions']) == len({(l['side'], l['provisioner'], l['root']) for l in data['provisions']})
    assert len(results['infra']['provisioners']) == 4
    assert len(results['infra']['iac_nodes']) == 4
    queries = [
        {'tool': 'context', 'arguments': {'symbol': 'aws_sqs_queue.jobs (root envs/dev)', 'budget': 8000}},
        {'tool': 'context', 'arguments': {'symbol': 'aws_s3_bucket.receipts (root envs/prod)', 'budget': 8000}},
        {'tool': 'context', 'arguments': {'symbol': 'send', 'budget': 8000}},
        {'tool': 'trace', 'arguments': {'from': 'aws_sqs_queue.jobs (root envs/dev)', 'direction': 'down', 'budget': 8000}},
        {'tool': 'trace', 'arguments': {'from': 'send', 'direction': 'up', 'budget': 8000}},
        {'tool': 'trace', 'arguments': {'from': 'receive', 'direction': 'up', 'budget': 8000}},
    ]
    tools, texts, _ = mcp_answers(binary, infra, queries, run / 'stdio.err')
    (run / 'stdio.json').write_text(json.dumps(texts, indent=2))
    assert 'producer app.py:' in texts[0] and 'consumer app.py:' in texts[0], texts[0]
    assert 'writer app.py:' in texts[1] and 'reader (repository-level)' in texts[1], texts[1]
    assert 'provisioners:' in texts[2] and '(root envs/prod)' in texts[2], texts[2]
    assert all('provisions ' in t for t in texts[3:]), texts[3:]
    manifest = 'index_dir = "indexes"\n'
    for name, root in [('infra', infra), ('app', app)]:
        manifest += f'\n[[repository]]\nrepo = "example.com/zz-prove/{name}"\npath = "{root}"\n'
    write(run, 'workspace.toml', manifest)
    subprocess.run([binary, 'index', '--workspace', 'workspace.toml'], cwd=run, check=True,
                   stdout=open(run / 'index.out', 'w'), stderr=open(run / 'index.err', 'w'))
    port = free_port()
    server = subprocess.Popen([binary, 'mcp', '--listen', f'127.0.0.1:{port}', '--workspace', 'workspace.toml'],
                              cwd=run, stdout=open(run / 'http.out', 'w'), stderr=open(run / 'http.err', 'w'))
    try:
        for _ in range(100):
            try:
                socket.create_connection(('127.0.0.1', port), timeout=1).close()
                break
            except OSError:
                time.sleep(.1)
        client = Http(f'http://127.0.0.1:{port}/example.com/zz-prove/infra')
        assert client.call('tools/list', {})['result']['tools'] == tools
        http = [answer(client.call('tools/call', {'name': q['tool'], 'arguments': q['arguments']})) for q in queries]
        (run / 'http.json').write_text(json.dumps(http, indent=2))
        assert http == texts
    finally:
        server.terminate()
        server.wait(timeout=30)
    # A same-size variable edit must remove just the prod queue's links, then restore them.
    path = infra / 'envs/prod/main.tf'
    original = path.read_text()
    path.write_text(original.replace('"jobs" }', '"gone" }', 1))
    changed = export(binary, infra)
    (run / 'changed.json').write_text(json.dumps(changed, indent=2))
    assert len(changed['provisions']) == 6
    assert not any(l['root'] == 'envs/prod' and l['contract'] == 'channel::sqs::jobs'
                   for l in changed['provisions'])
    path.write_text(original)
    restored = export(binary, infra)
    assert restored['provisions'] == results['infra']['provisions']
    # ADR 0026: whole-segment patterns, literal evidence, multiple distinct names. The literal
    # part needs three letters or digits; a separator is no evidence (#711, #782).
    write(infra, 'patterns/main.tf', '''resource "aws_s3_bucket" "several" { bucket = "receipts-${terraform.workspace}" }
resource "aws_s3_bucket" "minimum" { bucket = "abc-${terraform.workspace}" }
resource "aws_s3_bucket" "short" { bucket = "ab-${terraform.workspace}" }
resource "aws_s3_bucket" "inside" { bucket = "rece${terraform.workspace}ipts" }
''')
    with (app / 'meada.toml').open('a') as f:
        for name in ['receipts-prod', 'receipts-stage', 'abc-prod', 'ab-prod']:
            f.write(f'\n[[resource]]\nkind = "s3"\nid = "{name}"\nrole = "read"\n')
    patterns = export(binary, infra)
    (run / 'patterns.json').write_text(json.dumps(patterns, indent=2))
    reached = [l for l in patterns['provisions'] if l['root'] == 'patterns']
    assert len(reached) == 3, reached
    assert {(l['contract'], l['level']) for l in reached} == {
        ('resource::s3::receipts-prod', 'ambiguous'),
        ('resource::s3::receipts-stage', 'ambiguous'),
        ('resource::s3::abc-prod', 'inferred'),
    }, reached
    _, pattern_texts, _ = mcp_answers(binary, infra, [
        {'tool': 'context', 'arguments': {'symbol': 'aws_s3_bucket.several', 'budget': 8000}},
        {'tool': 'trace', 'arguments': {'from': 'aws_s3_bucket.minimum',
                                     'direction': 'down', 'budget': 8000}},
    ], run / 'patterns.err')
    (run / 'patterns-stdio.json').write_text(json.dumps(pattern_texts, indent=2))
    assert all(n in pattern_texts[0] for n in ['receipts-prod', 'receipts-stage'])
    assert 'provisions resource::s3::abc-prod [inferred:' in pattern_texts[1]
    (run / 'provisions.md').write_text('PASS: 8 links in each export, 4 provisioners, exact/declared/inferred levels, '
        'all sides, 6 stdio/HTTP answers equal, same-size edit removes 2 links and restore returns all 8; '
        '3 pattern links, ambiguity by distinct literal names, short/interior patterns refused.\n')
    print('checks passed; evidence:', run)


if __name__ == '__main__':
    main()
