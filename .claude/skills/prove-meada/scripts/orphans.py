#!/usr/bin/env python3
"""Drive #657 on neutral inputs; answer keys are the issue and base ADR 0026.

No feature-test fixtures or product matching code are imported. The generated files and
their SHA256 inventory stay with every raw CLI/MCP response, even when a check fails.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import socket
import subprocess
import time
from prove import Http, Stdio, answer, free_port, git


NO_PROVIDER = 'no provisioner in the workspace'
NO_USE = 'no use in the workspace'


def write(root, path, text):
    target = root / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(text)


def declare(kind, name, role='read'):
    return f'\n[[resource]]\nkind = "{kind}"\nid = "{name}"\nrole = "{role}"\n'


def orphan_set_matches(text, expected):
    lines = [l for l in text.splitlines() if l.strip().startswith('orphan ')]
    got = {(m.group(1), m.group(2)) for line in lines
           if (m := re.search(r'orphan (\S+): (no use|no provisioner);', line))}
    return got == set(expected.items()) and len(lines) == len(expected)


def queue_code(names, amqp=False):
    text = 'import boto3\nclient = boto3.client("sqs")\n'
    for n, name in enumerate(names):
        text += (f'def emit_{n}():\n    client.send_message(QueueUrl="https://sqs.eu-central-1.amazonaws.com/'
                 f'123456789012/{name}", MessageBody="probe")\n')
    if amqp:
        text += ('import pika\nconnection = pika.BlockingConnection()\nchannel = connection.channel()\n'
                 'def handle(body):\n    return body\n'
                 'def listen():\n    channel.basic_consume(queue="parcel-live", on_message_callback=handle)\n')
    return text


def seed(root):
    """Names chosen independently from the feature fixtures; staged, never committed."""
    repos = {}
    for name in ['mixed-infra', 'mixed-app', 'code-only', 'iac-only', 'isolation', 'patterns']:
        repo = root / name
        repo.mkdir(parents=True)
        git('init', '-q', cwd=repo)
        git('remote', 'add', 'origin', f'https://example.com/orphan-proof/{name}.git', cwd=repo)
        repos[name] = repo
    write(repos['mixed-infra'], 'meada.toml', '[[peer]]\nrepo = "example.com/orphan-proof/mixed-app"\n')
    write(repos['mixed-app'], 'meada.toml', '[[peer]]\nrepo = "example.com/orphan-proof/mixed-infra"\n')
    infra = ('resource "aws_sqs_queue" "active" { name = "parcel-live" }\n'
             'resource "aws_sqs_queue" "idle" { name = "parcel-idle" }\n')
    write(repos['mixed-infra'], 'regional/main.tf', infra)
    write(repos['mixed-app'], 'worker.py', queue_code(['parcel-live', 'parcel-lost']))
    write(repos['code-only'], 'worker.py', queue_code(['parcel-live']))
    write(repos['code-only'], 'meada.toml', declare('s3', 'archive-live', 'write'))
    write(repos['iac-only'], 'regional/main.tf', infra)
    # Same spelling across different kind/broker groups is not matching evidence.
    write(repos['isolation'], 'main.tf',
          'resource "aws_sqs_queue" "queue" { name = "parcel-live" }\n'
          'resource "aws_s3_bucket" "bucket" { bucket = "archive-live" }\n')
    write(repos['isolation'], 'worker.py', queue_code([], amqp=True))
    write(repos['isolation'], 'meada.toml', declare('gcs', 'archive-live') + declare('sqs', 'parcel-live'))
    # Multiple roots with the same literal are normal. Patterns require whole segments
    # and three literal characters; their lack of a link is itself orphan evidence.
    for env in ['east', 'west']:
        write(repos['patterns'], f'{env}/main.tf',
              'resource "aws_s3_bucket" "archive" { bucket = "archive-live" }\n')
    write(repos['patterns'], 'patterns/main.tf',
          'resource "aws_s3_bucket" "bounded" { bucket = "archive-${terraform.workspace}" }\n'
          'resource "aws_s3_bucket" "unused" { bucket = "reserve-${terraform.workspace}" }\n'
          'resource "aws_s3_bucket" "interior" { bucket = "arc${terraform.workspace}hive" }\n'
          'resource "aws_s3_bucket" "short" { bucket = "a-${terraform.workspace}" }\n')
    write(repos['patterns'], 'meada.toml', ''.join(declare('s3', n) for n in
          ['archive-live', 'archive-cold', 'archive', 'a-live']))
    for repo in repos.values():
        git('add', '.', cwd=repo)
    inventory = {}
    for path in sorted(root.rglob('*')):
        if path.is_file() and '.git' not in path.parts:
            inventory[str(path.relative_to(root))] = hashlib.sha256(path.read_bytes()).hexdigest()
    return repos, inventory


class Probe:
    def __init__(self, args):
        self.run = Path(args.run).resolve()
        self.run.mkdir(parents=True, exist_ok=False)
        self.base, self.head = map(lambda p: str(Path(p).resolve()), [args.base_bin, args.head_bin])
        self.failures, self.steps = [], []
        self.http = args.http

    def check(self, label, ok, detail=''):
        self.steps.append({'check': label, 'pass': bool(ok), 'detail': detail})
        if not ok:
            self.failures.append(label)
        (self.run / 'checks.json').write_text(json.dumps(self.steps, indent=2))

    def command(self, binary, cwd, args, label):
        p = subprocess.run([binary, *args], cwd=cwd, text=True, capture_output=True, timeout=120)
        write(self.run, label + '.out', p.stdout)
        write(self.run, label + '.err', p.stderr)
        write(self.run, label + '.exit', str(p.returncode))
        if p.returncode:
            raise RuntimeError(f'{label}: exit {p.returncode}; see captured stderr')
        return p.stdout

    def queries(self, name):
        qs = []
        def add(tool, **args):
            qs.append({'tool': tool, 'arguments': dict(args, budget=200000)})
        if name in ['mixed-infra', 'mixed-app', 'code-only', 'iac-only']:
            for value in ['parcel-live', 'parcel-idle', 'parcel-lost']:
                add('search', query=value, kind='contract')
                add('search', query=value)
                add('context', symbol=f'channel::sqs::{value}')
            if name != 'code-only':
                add('search', query='aws_sqs_queue', kind='iac')
                add('context', symbol='aws_sqs_queue.idle (root regional)')
            else:
                add('context', symbol='emit_0')
                add('search', query='archive-live', kind='contract')
        elif name == 'isolation':
            for kind, value in [('channel::amqp', 'parcel-live'), ('channel::sqs', 'parcel-live'),
                                ('resource::s3', 'archive-live'), ('resource::gcs', 'archive-live'),
                                ('resource::sqs', 'parcel-live')]:
                add('context', symbol=f'{kind}::{value}')
                add('search', query=f'{kind}::{value}', kind='contract')
            add('context', symbol='aws_s3_bucket.bucket')
            add('context', symbol='aws_sqs_queue.queue')
        else:
            for root in ['east', 'west']:
                add('context', symbol=f'aws_s3_bucket.archive (root {root})')
            for node in ['bounded', 'unused', 'interior', 'short']:
                add('context', symbol=f'aws_s3_bucket.{node}')
            for value in ['archive-live', 'archive-cold', 'archive', 'a-live']:
                add('context', symbol=f'resource::s3::{value}')
            add('search', query='archive', kind='contract')
        add('search', query='zz_orphan_proof_missing', kind='contract')
        return qs

    def drive_queries(self, binary, repo, qs, label):
        client = Stdio(binary, repo, self.run / f'{label}.err')
        try:
            tools = client.call('tools/list', {})['result']['tools']
            texts = [answer(client.call('tools/call', {'name': q['tool'], 'arguments': q['arguments']})) for q in qs]
            write(self.run, label + '.json', json.dumps({'queries': qs, 'answers': texts, 'tools': tools}, indent=2))
            return tools, texts
        finally:
            client.close()

    def parity(self, repo_map, name, qs, expected, tools, label):
        manifest = 'index_dir = "indexes"\n'
        for repo_name, repo in repo_map.items():
            manifest += f'\n[[repository]]\nrepo = "example.com/orphan-proof/{repo_name}"\npath = "{repo}"\n'
        folder = self.run / label
        folder.mkdir()
        write(folder, 'workspace.toml', manifest)
        self.command(self.head, folder, ['index', '--workspace', 'workspace.toml'], label + '/index')
        port = free_port()
        log = open(folder / 'server.log', 'w')
        process = subprocess.Popen([self.head, 'mcp', '--listen', f'127.0.0.1:{port}', '--workspace', 'workspace.toml'],
                                   cwd=folder, stdin=subprocess.DEVNULL, stdout=log, stderr=log)
        write(folder, 'server.pid', str(process.pid))
        try:
            for _ in range(100):
                if process.poll() is not None:
                    raise RuntimeError(f'{label}: HTTP server exited {process.returncode}')
                try:
                    socket.create_connection(('127.0.0.1', port), timeout=.2).close()
                    break
                except OSError:
                    time.sleep(.1)
            client = Http(f'http://127.0.0.1:{port}/example.com/orphan-proof/{name}')
            got_tools = client.call('tools/list', {})['result']['tools']
            texts = [answer(client.call('tools/call', {'name': q['tool'], 'arguments': q['arguments']})) for q in qs]
            write(folder, 'answers.json', json.dumps(texts, indent=2))
            self.check(label + ' transport parity', texts == expected and got_tools == tools)
        finally:
            process.terminate()
            process.wait(timeout=30)
            log.close()
            write(folder, 'stopped', str(process.returncode))

    def run_checks(self):
        src, inventory = seed(self.run / 'input')
        write(self.run, 'input.sha256.json', json.dumps(inventory, indent=2))
        versions = {s: subprocess.check_output([b, '--version'], text=True).strip()
                    for s, b in [('base', self.base), ('head', self.head)]}
        write(self.run, 'identity.json', json.dumps({'binaries': {'base': self.base, 'head': self.head}, 'versions': versions}, indent=2))
        all_repos = {}
        for side, binary in [('base', self.base), ('head', self.head)]:
            root = self.run / side / 'work'
            shutil.copytree(self.run / 'input', root)
            all_repos[side] = {name: root / name for name in src}
            for name, repo in all_repos[side].items():
                self.command(binary, repo, ['export'], f'{side}/{name}/export')
                self.command(binary, repo, ['status'], f'{side}/{name}/status')
                qs = self.queries(name)
                tools, texts = self.drive_queries(binary, repo, qs, f'{side}/{name}/stdio')
                if side == 'head' and self.http:
                    members = {n: all_repos[side][n] for n in
                               (['mixed-infra', 'mixed-app'] if name.startswith('mixed-') else [name])}
                    self.parity(members, name, qs, texts, tools, f'http-{name}')
        for name in src:
            b = (self.run / f'base/{name}/status.out').read_text()
            h = (self.run / f'head/{name}/status.out').read_text()
            lines = [l for l in h.splitlines() if l.strip().startswith('orphan ')]
            expected = {'channel::sqs::parcel-idle': 'no use', 'channel::sqs::parcel-lost': 'no provisioner'} if name.startswith('mixed-') else {}
            if name == 'patterns':
                # The exact base does not create provisioner sides for unsupported
                # interior/short patterns. A supported unused pattern is a provisioner.
                expected = {'resource::s3::reserve-*': 'no use',
                            'resource::s3::archive': 'no provisioner', 'resource::s3::a-live': 'no provisioner'}
            self.check(name + ' exact orphan set', orphan_set_matches(h, expected), repr(lines))
            without = ''.join(l + '\n' for l in h.splitlines() if not l.strip().startswith('orphan '))
            self.check(name + ' whole status baseline parity',
                       (h if self.base_has_orphans else without) == b,
                       'The current tier already has #657: compare complete status.' if self.base_has_orphans else
                       'Only the exact new orphan lines may differ; existing declared-resource diagnostics remain independent.')
            be = json.loads((self.run / f'base/{name}/export.out').read_text())
            he = json.loads((self.run / f'head/{name}/export.out').read_text())
            self.check(name + ' export parity', be == he)
            qs = self.queries(name)
            texts = json.loads((self.run / f'head/{name}/stdio.json').read_text())['answers']
            if name == 'code-only':
                for i, q in enumerate(qs):
                    if q['arguments'].get('query') in ['parcel-live', 'archive-live'] or q['arguments'].get('symbol') in ['channel::sqs::parcel-live', 'emit_0']:
                        self.check(f'{name} marker {i}', NO_PROVIDER in texts[i], texts[i])
                self.check('code-only no opposite marker', not any(NO_USE in t for t in texts))
            if name == 'iac-only':
                for i, q in enumerate(qs):
                    if q['arguments'].get('query') in ['parcel-live', 'parcel-idle', 'aws_sqs_queue'] or q['arguments'].get('symbol') in ['channel::sqs::parcel-live', 'channel::sqs::parcel-idle', 'aws_sqs_queue.idle (root regional)']:
                        self.check(f'{name} marker {i}', NO_USE in texts[i], texts[i])
                self.check('iac-only no opposite marker', not any(NO_PROVIDER in t for t in texts))
            if name.startswith('mixed-'):
                self.check(name + ' mixed groups no one-sided marker', not any(NO_PROVIDER in t or NO_USE in t for t in texts))
            if name == 'isolation':
                for i, q in enumerate(qs[:-1]):
                    key = q['arguments'].get('symbol', q['arguments'].get('query', ''))
                    wanted = NO_PROVIDER if ('::amqp::' in key or '::gcs::' in key
                                             or key.startswith('resource::sqs::')) else NO_USE
                    opposite = NO_USE if wanted == NO_PROVIDER else NO_PROVIDER
                    self.check(f'isolation marker {i}', wanted in texts[i] and opposite not in texts[i], texts[i])
            if name == 'patterns':
                self.check('multiple roots not orphaned', all(NO_USE not in t for t in texts[:2]))
                self.check('bounded pattern used', NO_USE not in texts[2] and 'archive-cold' in texts[2])
        self.negative_controls()
        # Visibility: a server manifest containing ONLY the app has no provisioner even
        # though its explicitly declared peer lives beside it on disk. Compare against
        # an isolated local copy with its peer absent, retaining configuration,
        # identity and source locations (including any missing-peer note).
        head = all_repos['head']
        isolated = self.run / 'visibility-local-copy' / 'mixed-app'
        shutil.copytree(self.run / 'input/mixed-app', isolated)
        qs = self.queries('mixed-app')
        tools, texts = self.drive_queries(self.head, isolated, qs, 'visibility-local')
        self.check('visibility isolated side marker', NO_PROVIDER in texts[0])
        if self.http:
            self.parity({'mixed-app': head['mixed-app']}, 'mixed-app', qs, texts, tools, 'visibility-http')
        # Refresh: both directions, on a persistent stdio server, after same-size edits.
        repo = head['mixed-infra']
        path = repo / 'regional/main.tf'
        original = path.read_text()
        q = {'name': 'context', 'arguments': {'symbol': 'channel::sqs::parcel-lost', 'budget': 200000}}
        client = Stdio(self.head, repo, self.run / 'refresh.err')
        try:
            before = answer(client.call('tools/call', q))
            path.write_text(original.replace('parcel-idle', 'parcel-lost'))
            after = answer(client.call('tools/call', q))
            status = self.command(self.head, repo, ['status'], 'refresh-changed/status')
            path.write_text(original)
            restored = answer(client.call('tools/call', q))
            returned = self.command(self.head, repo, ['status'], 'refresh-restored/status')
            write(self.run, 'refresh.json', json.dumps({'query': q, 'before': before, 'after': after, 'restored': restored}, indent=2))
            self.check('refresh gains provisioner', 'provisioner' not in before and 'provisioner' in after and before != after)
            self.check('refresh removes orphan pair', 'orphan ' not in status)
            self.check('refresh restores query', restored == before)
            self.check('refresh restores status', returned == (self.run / 'head/mixed-infra/status.out').read_text())
        finally:
            path.write_text(original)
            client.close()
        report = ['# #657 orphan checks', '', f'{len(self.steps) - len(self.failures)}/{len(self.steps)} checks passed.', '']
        report += [f"- {'PASS' if s['pass'] else 'FAIL'} {s['check']}" for s in self.steps]
        write(self.run, 'orphans.md', '\n'.join(report) + '\n')
        print('\n'.join(report[:3]))
        return int(bool(self.failures))

    def cross_kind_only(self):
        """Focused addendum, without repeating the six completed scenarios."""
        source = self.run / 'input/isolation'
        source.mkdir(parents=True)
        git('init', '-q', cwd=source)
        git('remote', 'add', 'origin', 'https://example.com/orphan-proof/isolation.git', cwd=source)
        write(source, 'main.tf', 'resource "aws_sqs_queue" "queue" { name = "parcel-live" }\n')
        write(source, 'meada.toml', declare('sqs', 'parcel-live'))
        git('add', '.', cwd=source)
        inventory = {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                     for p in source.iterdir() if p.is_file()}
        write(self.run, 'input.sha256.json', json.dumps(inventory, indent=2))
        queries = []
        for prefix in ['channel::sqs', 'resource::sqs']:
            contract = prefix + '::parcel-live'
            queries += [{'tool': 'context', 'arguments': {'symbol': contract, 'budget': 200000}},
                        {'tool': 'search', 'arguments': {'query': contract, 'kind': 'contract', 'budget': 200000}}]
        captures = {}
        for side, binary in [('base', self.base), ('head', self.head)]:
            repo = self.run / side / 'work/isolation'
            shutil.copytree(source, repo)
            exported = self.command(binary, repo, ['export'], f'{side}/isolation/export')
            status = self.command(binary, repo, ['status'], f'{side}/isolation/status')
            tools, texts = self.drive_queries(binary, repo, queries, f'{side}/isolation/stdio')
            captures[side] = (json.loads(exported), status, texts)
            if side == 'head' and self.http:
                self.parity({'isolation': repo}, 'isolation', queries, texts, tools, 'http-isolation')
        self.check('cross-kind export parity', captures['base'][0] == captures['head'][0])
        self.check('cross-kind whole status parity', captures['base'][1] == captures['head'][1])
        self.check('cross-kind no orphan status lines', orphan_set_matches(captures['head'][1], {}))
        for i, text in enumerate(captures['head'][2]):
            wanted, opposite = (NO_USE, NO_PROVIDER) if i < 2 else (NO_PROVIDER, NO_USE)
            self.check(f'cross-kind marker {i}', wanted in text and opposite not in text, text)
        write(self.run, 'orphans.md', '\n'.join([
            '# #657 cross-kind same-token addendum', '',
            f'{len(self.steps) - len(self.failures)}/{len(self.steps)} checks passed.', '',
            *[f"- {'PASS' if s['pass'] else 'FAIL'} {s['check']}" for s in self.steps], '']))
        print((self.run / 'orphans.md').read_text())
        return int(bool(self.failures))

    def negative_controls(self):
        """Retain rejected mutations of actual captures; never alter the app or answer key."""
        directory = self.run / 'negative-controls'
        directory.mkdir()
        base = json.loads((self.run / 'base/code-only/export.out').read_text())
        mutated = json.loads((self.run / 'head/code-only/export.out').read_text())
        mutated['repository'] = 'example.com/orphan-proof/incorrect-identity'
        write(directory, 'export-wrong-identity.json', json.dumps(mutated, indent=2))
        export_rejected = base != mutated
        status = (self.run / 'head/mixed-infra/status.out').read_text()
        status += '  orphan channel::sqs::parcel-injected: no use; injected negative control\n'
        write(directory, 'status-extra-orphan.out', status)
        expected = {'channel::sqs::parcel-idle': 'no use', 'channel::sqs::parcel-lost': 'no provisioner'}
        status_rejected = not orphan_set_matches(status, expected)
        controls = [('export wrong repository identity', export_rejected, 'export-wrong-identity.json'),
                    ('whole status unexpected third orphan', status_rejected, 'status-extra-orphan.out')]
        if self.http:
            stdio = json.loads((self.run / 'head/iac-only/stdio.json').read_text())['answers']
            http = json.loads((self.run / 'http-iac-only/answers.json').read_text())
            http[0] = http[0].replace(NO_USE, 'removed marker by negative control')
            write(directory, 'http-dropped-marker.json', json.dumps(http, indent=2))
            controls.append(('HTTP marker removed', http != stdio, 'http-dropped-marker.json'))
        lines = ['# Verifier negative controls', '',
                 'These mutate saved real responses. FAIL below is the expected comparison result, not a product failure.', '']
        for name, rejected, evidence in controls:
            self.check('negative control detects ' + name, rejected, evidence)
            lines.append(f'- {name}: {"FAIL (expected rejection)" if rejected else "UNDETECTED"}; {evidence}')
        write(directory, 'mutations.md', '\n'.join(lines) + '\n')


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--base-bin', required=True)
    p.add_argument('--head-bin', required=True)
    p.add_argument('--run', required=True)
    p.add_argument('--http', action='store_true')
    p.add_argument('--base-has-orphans', action='store_true',
                   help='baseline already includes #657; require complete status parity')
    p.add_argument('--cross-kind-only', action='store_true', help='focused same-token cross-kind addendum only')
    args = p.parse_args()
    os.environ['LANG'] = 'C.UTF-8'
    probe = Probe(args)
    probe.base_has_orphans = args.base_has_orphans
    try:
        return probe.cross_kind_only() if args.cross_kind_only else probe.run_checks()
    except Exception as e:
        write(probe.run, 'blocked.txt', f'{type(e).__name__}: {e}\n')
        raise


if __name__ == '__main__':
    raise SystemExit(main())
