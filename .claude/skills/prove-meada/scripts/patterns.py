#!/usr/bin/env python3
"""#674 runtime flow, keyed to #674/#642 and ADR 0026, never feature goldens.

The implementation agent supplied this driver; final proof requires an independent checker.
"""
import argparse
import hashlib
import json
from pathlib import Path
import socket
import subprocess
import time
from prove import Http, answer, free_port, git, mcp_answers


def write(root, name, content):
    path = root / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--head-bin', required=True)
    parser.add_argument('--run', required=True)
    parser.add_argument('--http', action='store_true')
    args = parser.parse_args()
    binary = str(Path(args.head_bin).resolve())
    run = Path(args.run).resolve()
    run.mkdir(parents=True, exist_ok=False)
    infra, app = run / 'work/infra', run / 'work/app'
    prefix = 'example.com/anchored-proof/'
    for root, name, peer in [(infra, 'infra', 'app'), (app, 'app', 'infra')]:
        root.mkdir(parents=True)
        git('init', '-q', cwd=root)
        git('remote', 'add', 'origin', f'https://{prefix}{name}.git', cwd=root)
        write(root, 'meada.toml', f'[[peer]]\nrepo = "{prefix}{peer}"\n')
    # `minimum` has the three letters or digits a pattern's literal part needs; `short`'s two and a
    # separator are no evidence, so it links to nothing (#711, #782).
    write(infra, 'main.tf', '''resource "aws_sqs_queue" "messages" { name = "${terraform.workspace}-notifications" }
resource "aws_s3_bucket" "ledger" { bucket = "ledger-${terraform.workspace}" }
resource "aws_s3_bucket" "minimum" { bucket = "xyz-${terraform.workspace}" }
resource "aws_s3_bucket" "short" { bucket = "xy-${terraform.workspace}" }
resource "aws_s3_bucket" "inside" { bucket = "led${terraform.workspace}ger" }
resource "aws_s3_bucket" "both" { bucket = "both-${terraform.workspace}" }
''')
    with (app / 'meada.toml').open('a') as stream:
        for name in ['ledger-dev', 'ledger-prod', 'xyz-dev', 'xy-dev', 'ledger', 'both-*']:
            stream.write(f'\n[[resource]]\nkind = "s3"\nid = "{name}"\nrole = "read"\n')
    write(app, 'send.py', '''import boto3
client = boto3.client("sqs")
def send():
    client.send_message(QueueUrl="https://sqs.eu-west-1.amazonaws.com/123456789012/west-notifications", MessageBody="x")
''')
    write(app, '.github/workflows/image.yml', '''jobs:
  build:
    steps:
      - uses: docker/build-push-action@v6
        with:
          tags: ghcr.io/anchored/worker-${{ matrix.region }}:v1
''')
    write(infra, 'compose.yaml', 'services:\n  worker:\n    image: ghcr.io/anchored/worker-west:v1\n')
    for root in [infra, app]:
        git('add', '.', cwd=root)
        git('commit', '-qm', 'anchored proof inputs', cwd=root)
    inventory = {str(p.relative_to(run)): hashlib.sha256(p.read_bytes()).hexdigest()
                 for root in [infra, app] for p in root.rglob('*')
                 if p.is_file() and '.git' not in p.parts}
    write(run, 'inputs.json', json.dumps(inventory, indent=2))

    def cli(label, command, root=infra, stdin=''):
        result = subprocess.run([binary, *command], cwd=root, input=stdin,
                                capture_output=True, text=True, timeout=90)
        write(run, label+'.stdout', result.stdout)
        write(run, label+'.stderr', result.stderr)
        assert result.returncode == 0, result.stderr
        return result.stdout

    before = json.loads(cli('before', ['export']))
    expected = {('channel::sqs::west-notifications', 'inferred'),
                ('resource::s3::ledger-dev', 'ambiguous'),
                ('resource::s3::ledger-prod', 'ambiguous'),
                ('resource::s3::xyz-dev', 'inferred')}
    assert len(before['provisions']) == 4, before['provisions']
    assert {(l['contract'], l['level']) for l in before['provisions']} == expected
    assert len(before['links']) == 1 and before['links'][0]['level'] == 'inferred'
    listing = cli('list', ['decide', '--list'])
    assert 'ledger-dev' in listing and 'ledger-prod' in listing, listing
    cli('pick', ['decide'], stdin='1\n')
    decision = (infra / 'meada.decisions.toml').read_text()
    write(run, 'decision.toml', decision)
    assert 'by = "human"' in decision
    after = json.loads(cli('decided', ['export']))
    assert len(after['provisions']) == 3
    assert {(l['contract'], l['level']) for l in after['provisions']} == {
        ('channel::sqs::west-notifications', 'inferred'),
        ('resource::s3::ledger-dev', 'decided'), ('resource::s3::xyz-dev', 'inferred')}
    assert 'stale_decisions' not in after
    queries = [
        {'tool': 'context', 'arguments': {'symbol': 'aws_s3_bucket.ledger', 'budget': 12000}},
        {'tool': 'trace', 'arguments': {'from': 'aws_s3_bucket.ledger', 'direction': 'down', 'budget': 12000}},
        {'tool': 'search', 'arguments': {'query': 'ledger', 'kind': 'ambiguous', 'budget': 12000}},
    ]
    _, texts, _ = mcp_answers(binary, infra, queries, run / 'stdio.stderr')
    write(run, 'stdio.json', json.dumps(texts, indent=2))
    assert all('decided by human' in text for text in texts[:2]), texts
    if args.http:
        manifest = 'index_dir = "indexes"\n'
        for root, name in [(infra, 'infra'), (app, 'app')]:
            manifest += f'\n[[repository]]\nrepo = "{prefix}{name}"\npath = "{root}"\n'
        write(run, 'workspace.toml', manifest)
        cli('index', ['index', '--workspace', 'workspace.toml'], run)
        port = free_port()
        with (run / 'http.stdout').open('w') as out, (run / 'http.stderr').open('w') as err:
            server = subprocess.Popen([binary, 'mcp', '--listen', f'127.0.0.1:{port}', '--workspace', 'workspace.toml'], cwd=run, stdout=out, stderr=err)
            try:
                for _ in range(100):
                    try:
                        socket.create_connection(('127.0.0.1', port), timeout=1).close()
                        break
                    except OSError:
                        time.sleep(.1)
                client = Http(f'http://127.0.0.1:{port}/{prefix}infra')
                http = [answer(client.call('tools/call', {'name': q['tool'], 'arguments': q['arguments']})) for q in queries]
                write(run, 'http.json', json.dumps(http, indent=2))
                assert http == texts
            finally:
                server.terminate()
                server.wait(timeout=30)
    names = (app / 'meada.toml').read_text()
    write(app, 'meada.toml', names.replace('ledger-dev', 'ledger-new'))
    stale = json.loads(cli('stale', ['export']))
    assert len(stale['stale_decisions']) == 1, stale
    assert len(stale['provisions']) == 4
    assert sum(l['level'] == 'ambiguous' for l in stale['provisions']) == 2
    write(app, 'meada.toml', names)
    (infra / 'meada.decisions.toml').unlink()
    restored = json.loads(cli('restored', ['export']))
    assert restored['provisions'] == before['provisions']
    # The unresolved side can also be an image builder or a resource use.
    write(infra, 'compose.yaml', 'services:\n  west:\n    image: ghcr.io/anchored/worker-west:v1\n  east:\n    image: ghcr.io/anchored/worker-east:v1\n')
    images = json.loads(cli('image-ambiguous', ['export']))
    assert len(images['links']) == 2 and all(l['level'] == 'ambiguous' for l in images['links'])
    listing = cli('image-list', ['decide', '--list'], app)
    assert 'worker-east' in listing and 'worker-west' in listing, listing
    cli('image-pick', ['decide'], app, '1\n')
    images = json.loads(cli('image-decided', ['export']))
    assert len(images['links']) == 1
    assert images['links'][0]['contract'] == 'image::ghcr.io/anchored/worker-east'
    assert images['links'][0]['level'] == 'decided'
    assert images['links'][0]['by'] == 'human'
    image_status = cli('image-status', ['status'])
    assert 'orphan image::ghcr.io/anchored/worker-west: no builder' in image_status, image_status
    write(infra, 'main.tf', 'resource "aws_s3_bucket" "east" { bucket = "invoices-east" }\nresource "aws_s3_bucket" "west" { bucket = "invoices-west" }\n')
    write(app, 'use.yaml', 'bucket: invoices-*\n')
    write(app, 'meada.toml', f'[[peer]]\nrepo = "{prefix}infra"\n[[resource]]\nkind = "s3"\nid = "invoices-*"\nrole = "read"\nvia = ["use.yaml#bucket"]\n')
    reverse = json.loads(cli('reverse', ['export']))
    assert len(reverse['provisions']) == 2 and all(l['level'] == 'ambiguous' for l in reverse['provisions'])
    listing = cli('reverse-list', ['decide', '--list'], app)
    assert 'invoices-east' in listing and 'invoices-west' in listing, listing
    assert 'use.yaml:1' in listing and 'resource::s3::invoices-* at use.yaml#bucket' in listing, listing
    cli('reverse-pick', ['decide'], app, '2\n')
    reverse = json.loads(cli('reverse-decided', ['export']))
    assert len(reverse['provisions']) == 1
    assert reverse['provisions'][0]['level'] == 'decided'
    assert reverse['provisions'][0]['side'] == f'{prefix}app::use.yaml#bucket'
    assert reverse['provisions'][0]['provisioner'].endswith('aws_s3_bucket.west')
    write(run, 'checks.md', 'PASS: complete provision sets, both pattern orientations, image builder choice, channel/resource/image anchors, short/interior/pattern-pattern negatives, CLI human choice, stale target, restore, MCP context/trace' + (' and HTTP parity' if args.http else '') + '.\nChecker: same agent driver; independent interpretation required.\n')


if __name__ == '__main__':
    main()
