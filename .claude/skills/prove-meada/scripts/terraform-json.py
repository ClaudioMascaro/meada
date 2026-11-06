#!/usr/bin/env python3
"""Terraform/OpenTofu JSON acceptance, answer key: #642, #649 and ADR 0025.

Neutral inputs follow HCL's JSON specification, including repeated section keys. A script pass
is evidence for an independent checker; all-corpus/base and console checks remain separate.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import time
from prove import Http, Stdio, answer, free_port, git

IDENTITY = 'example.com/json-proof/infra'
SECRET = 'json-proof-sensitive-canary'
TFVARS_SECRET = 'json-proof-tfvars-canary'


def write(root, path, text):
    target = root / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(text)


def dump(root, path, value):
    write(root, path, json.dumps(value, indent=2, ensure_ascii=False) + '\n')


def located_nodes(text, trace=False):
    lines = text.splitlines()
    if trace:
        if not lines or not lines[-1].startswith('chain: '):
            return ['unexpected trace: ' + text]
        lines = lines[:-1]
    nodes = []
    for line in lines:
        fields = line.partition(':')[2].split()
        nodes.append(fields[1] if len(fields) >= 2 and fields[0].isdigit() else 'unexpected: ' + line)
    return sorted(nodes)


def seed(repo):
    repo.mkdir(parents=True)
    git('init', '-q', cwd=repo)
    git('remote', 'add', 'origin', f'https://{IDENTITY}.git', cwd=repo)
    dump(repo, 'main.tf.json', {
        'variable': {'stage': {'default': 'dev'}},
        'locals': {'prefix': '${upper(var.stage)}'},
        'resource': [
            {'aws_sqs_queue': {'orders': {'name': '${lower(local.prefix)}-orders'}}},
            {'aws_sns_topic': {'events': {'name': 'events'}}},
            {'aws_s3_bucket': [{'archive': [{'bucket': 'json-archive'}]}]},
            {'aws_sns_topic_subscription': {'route': {'protocol': 'sqs', 'topic_arn': '${aws_sns_topic.events.arn}', 'endpoint': '${aws_sqs_queue.orders.arn}'}}},
            {'null_resource': {'counted': {'count': '${2}'}, 'literal': {'triggers': {'text': 'aws_sqs_queue.orders.arn'}}, 'after': {'depends_on': ['aws_sqs_queue.orders', 'module.child1']}}},
        ],
        'data': {'aws_sqs_queue': {'outside': {'name': 'outside'}}},
        'module': {'child1': {'source': './child', 'name': '${local.prefix}'}, 'child2': {'source': './child', 'name': 'fixed'}},
    })
    dump(repo, 'child/main.tofu.json', {'variable': {'name': {}}, 'resource': {'aws_s3_bucket': {'this': {'bucket': '${lower(var.name)}-module'}}}})
    dump(repo, 'open.tofu.json', {'resource': {'aws_s3_bucket': {'tofu': {'bucket': 'tofu-archive'}}}})
    dump(repo, 'shadow.tf.json', {'resource': {'null_resource': {'terraform_shadow': {}}}})
    dump(repo, 'shadow.tofu.json', {'resource': {'null_resource': {'tofu_shadow': {}}}})
    dump(repo, 'ordinary.json', {'resource': {'null_resource': {'ordinary_json': {}}}})
    write(repo, 'native.tf', 'resource "null_resource" "native" {}\n')
    write(repo, 'repeat.tf.json', '{"resource":{"null_resource":{"first":{}}},"resource":{"null_resource":{"second":{}}}}\n')
    write(repo, 'secrets.tf.json', '{"variable":[{"password":[{"default":"' + SECRET + '","sensitive":true}]}]}\n')
    dump(repo, 'terraform.tfvars.json', {'password': TFVARS_SECRET})
    write(repo, 'meada.toml', '[[resource]]\nkind = "json-proof"\nid = "tofu"\nrole = "read"\nvia = ["open.tofu.json::aws_s3_bucket.tofu"]\n')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--head-bin', required=True)
    parser.add_argument('--run', required=True)
    parser.add_argument('--http', action='store_true')
    args = parser.parse_args()
    binary = str(Path(args.head_bin).resolve())
    run = Path(args.run).resolve()
    run.mkdir(parents=True, exist_ok=False)
    repo = run / 'input'
    seed(repo)
    dump(run, 'identity.json', {'binary': binary, 'sha256': hashlib.sha256(Path(binary).read_bytes()).hexdigest()})
    checks = []

    def check(name, passed, evidence):
        checks.append({'name': name, 'passed': bool(passed), 'evidence': evidence})
        dump(run, 'checks.json', checks)

    def command(root, label, *words):
        result = subprocess.run([binary, *words], cwd=root, env={**os.environ, 'LANG': 'en_US.UTF-8'}, capture_output=True, text=True, timeout=120)
        write(run, label + '.stdout', result.stdout)
        write(run, label + '.stderr', result.stderr)
        dump(run, label + '.exit.json', result.returncode)
        result.check_returncode()
        return result.stdout

    initial = command(repo, 'initial-export', 'export')
    exported = json.loads(initial)
    provisioners = [
        ('channel::sns::events', 'main.tf.json', 'aws_sns_topic.events'),
        ('channel::sqs::dev-orders', 'main.tf.json', 'aws_sqs_queue.orders'),
        ('resource::s3::dev-module', 'child/main.tofu.json', 'module.child1.aws_s3_bucket.this'),
        ('resource::s3::fixed-module', 'child/main.tofu.json', 'module.child2.aws_s3_bucket.this'),
        ('resource::s3::json-archive', 'main.tf.json', 'aws_s3_bucket.archive'),
        ('resource::s3::tofu-archive', 'open.tofu.json', 'aws_s3_bucket.tofu'),
    ]
    expected = [{'contract': c, 'node': f'{IDENTITY}::{path}::{address}', 'root': '.'} for c, path, address in provisioners]
    check('complete provisioner set including array blocks and two module calls', exported.get('provisioners') == expected, exported.get('provisioners'))
    subscription = [{'source': 'channel::sns::events', 'target': 'channel::sqs::dev-orders', 'node': f'{IDENTITY}::main.tf.json::aws_sns_topic_subscription.route', 'root': '.'}]
    check('complete subscription set', exported.get('subscriptions') == subscription, exported.get('subscriptions'))
    check('definitions do not use themselves', not exported.get('uses'), exported.get('uses'))
    check('deterministic whole export', initial == command(repo, 'repeat-export', 'export'), 'whole bytes')
    status = command(repo, 'initial-status', 'status')
    check('JSON via is live', '`via`' not in status, status)
    index = (repo / '.meada/index.db').read_bytes()
    check('sensitive values absent from the index', all(v.encode() not in index for v in [SECRET, TFVARS_SECRET]), 'invented canaries only')
    node_addresses = sorted([
        'aws_sqs_queue.orders', 'aws_sns_topic.events', 'aws_s3_bucket.archive', 'aws_sns_topic_subscription.route',
        'null_resource.counted[0]', 'null_resource.counted[1]', 'null_resource.literal', 'null_resource.after',
        'data.aws_sqs_queue.outside', 'module.child1', 'module.child2', 'module.child1.aws_s3_bucket.this',
        'module.child2.aws_s3_bucket.this', 'aws_s3_bucket.tofu', 'null_resource.tofu_shadow', 'null_resource.native',
        'null_resource.first', 'null_resource.second',
    ])
    queries = [('search', {'query': '.', 'kind': 'iac'}), ('context', {'symbol': 'aws_sqs_queue.orders'}),
               ('trace', {'from': 'null_resource.after', 'direction': 'down'}), ('trace', {'from': 'aws_sqs_queue.orders', 'direction': 'up'}),
               ('trace', {'from': 'null_resource.literal', 'direction': 'down'})]
    queries += [('search', {'query': value, 'kind': 'value'}) for value in [SECRET, TFVARS_SECRET]]
    queries = [(tool, {**params, 'budget': 200000}) for tool, params in queries]
    dump(run, 'oracle.json', {'provisioners': expected, 'subscriptions': subscription, 'node_addresses': node_addresses})
    dump(run, 'queries.json', queries)
    client = Stdio(binary, repo, run / 'stdio.stderr')
    try:
        replies = [answer(client.call('tools/call', {'name': tool, 'arguments': params})) for tool, params in queries]
        dump(run, 'stdio.answers.json', replies)
        actual = located_nodes(replies[0])
        check('complete node set with shadowing, repeated keys and ordinary-JSON refusal', actual == node_addresses, replies[0])
        check('evaluated overridable name', 'name: "dev-orders"' in replies[1] and 'var.stage' in replies[1], replies[1])
        queue_line = next(n for n, line in enumerate((repo / 'main.tf.json').read_text().splitlines(), 1) if line.strip() == '"orders": {')
        check('JSON block span starts at its label and covers its body', replies[1].startswith(f'{IDENTITY} main.tf.json:{queue_line}-{queue_line + 2} aws_sqs_queue.orders\n'), replies[1])
        check('complete static references down', located_nodes(replies[2], trace=True) == ['aws_sqs_queue.orders', 'module.child1', 'null_resource.after'], replies[2])
        check('complete static references up', located_nodes(replies[3], trace=True) == ['aws_sns_topic_subscription.route', 'aws_sqs_queue.orders', 'null_resource.after'], replies[3])
        check('plain strings create no reference', located_nodes(replies[4], trace=True) == ['null_resource.literal'], replies[4])
        check('sensitive values absent from search', replies[-2:] == [f'no matches for `{v}`\n' for v in [SECRET, TFVARS_SECRET]], replies[-2:])
        before = (repo / 'main.tf.json').read_text()
        write(repo, 'main.tf.json', before.replace('"dev"', '"prd"'))
        refreshed = answer(client.call('tools/call', {'name': queries[1][0], 'arguments': queries[1][1]}))
        check('warm same-size content refresh', 'name: "prd-orders"' in refreshed and 'name: "dev-orders"' not in refreshed, refreshed)
        warm = command(repo, 'edited-warm-export', 'export')
        cold = run / 'cold' / 'input'
        shutil.copytree(repo, cold, ignore=shutil.ignore_patterns('.meada'))
        check('warm export equals cold whole export', warm == command(cold, 'edited-cold-export', 'export'), 'whole bytes')
        write(repo, 'main.tf.json', before)
        check('whole export restored', command(repo, 'restored-export', 'export') == initial, 'whole bytes')
        source = (repo / 'open.tofu.json').read_text()
        (repo / 'open.tofu.json').unlink()
        deleted = command(repo, 'deleted-source-status', 'status')
        check('deleted JSON source makes via stale', 'open.tofu.json::aws_s3_bucket.tofu' in deleted and 'matches no symbol' in deleted, deleted)
        write(repo, 'open.tofu.json', source)
        check('JSON source restore', command(repo, 'source-restored-export', 'export') == initial, 'whole bytes')
    finally:
        client.close()
    if args.http:
        folder = run / 'http'
        folder.mkdir()
        write(folder, 'workspace.toml', 'index_dir = "indexes"\n\n[[repository]]\nrepo = "' + IDENTITY + '"\npath = ' + json.dumps(str(repo)) + '\n')
        command(folder, 'http/index', 'index', '--workspace', 'workspace.toml')
        port = free_port()
        with open(folder / 'server.log', 'w') as log:
            server = subprocess.Popen([binary, 'mcp', '--listen', f'127.0.0.1:{port}', '--workspace', 'workspace.toml'], cwd=folder, stdin=subprocess.DEVNULL, stdout=log, stderr=log)
            try:
                for _ in range(150):
                    if server.poll() is not None:
                        raise RuntimeError('HTTP server exited')
                    try:
                        socket.create_connection(('127.0.0.1', port), timeout=.2).close()
                        break
                    except OSError:
                        time.sleep(.1)
                http = Http(f'http://127.0.0.1:{port}/{IDENTITY}')
                responses = [answer(http.call('tools/call', {'name': tool, 'arguments': params})) for tool, params in queries]
                dump(folder, 'answers.json', responses)
                check('HTTP whole-answer parity', responses == replies, len(responses))
            finally:
                server.terminate()
                server.wait(timeout=20)
    failed = [row['name'] for row in checks if not row['passed']]
    write(run, 'receipt.md', '# Terraform JSON checks\n\nAnswer keys: #642, #649, ADR 0025 and HCL JSON syntax.\n\n' + ('Checks passed; independent verdict pending.\n' if not failed else 'BROKEN checks:\n' + '\n'.join('- ' + name for name in failed) + '\n'))
    print(json.dumps({'passed': len(checks) - len(failed), 'failed': len(failed)}))
    return bool(failed)


if __name__ == '__main__':
    raise SystemExit(main())
