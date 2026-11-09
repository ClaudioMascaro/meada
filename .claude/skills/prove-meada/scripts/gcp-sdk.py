#!/usr/bin/env python3
"""Neutral #668/#642 SDK probe. Official operation inventory: references/gcp-sdk.md.

The source and oracle are written before any product answer. Only public transport
mechanics are reused. A script pass is input to the checker's verdict, never that verdict.
"""
import argparse
from collections import Counter
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import time

from prove import Http, answer, free_port, git, mcp_answers

REMOTE = 'example.com/zz-prove-gcp-sdk/'
EXT = {'js': 'js', 'ts': 'ts', 'python': 'py', 'java': 'java', 'go': 'go', 'csharp': 'cs'}
PREFIX = {'publish': 'channel::pubsub', 'receive': 'channel::pubsub',
          'write': 'resource::gcs', 'read': 'resource::gcs',
          'secret': 'resource::gcpsecretmanager'}
ROLE = {'publish': 'providers', 'receive': 'consumers', 'write': 'providers',
        'read': 'consumers', 'secret': 'consumers'}


def write(root, name, content):
    target = root / name
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(content)


def saved(root, name, value):
    write(root, name, json.dumps(value, indent=2) + '\n')


def names(language, operation):
    if operation in ('publish', 'receive'):
        return 'signal-' + language
    return ('objects-' if operation in ('write', 'read') else 'credential-') + language


def source(language, operation, expression, label, mode='literal'):
    """SDK sample spellings, with no feature source, fixture or normalizer imports."""
    if language in ('js', 'ts'):
        params = 'Bus, Store, Secrets' if mode == 'hidden' else ''
        setup, action = {
            'publish': ('const conduit = new Bus().topic(NAME);', 'conduit.publishMessage({data});'),
            'receive': ('const conduit = new Bus().subscription(NAME);', 'conduit.on("message", handler);'),
            'write': ('const conduit = new Store().bucket(NAME).file(unknownObject);', 'conduit.save(data);'),
            'read': ('const conduit = new Store().bucket(NAME).file(unknownObject);', 'conduit.download();'),
            'secret': ('const conduit = new Secrets();', 'conduit.accessSecretVersion({name: NAME});'),
        }[operation]
        extras = ''
        if mode == 'namehop':
            setup = 'const conduit = opaque.makeClient();'
            extras = 'class Factory { makeClient() { return new Bus().topic("forbidden-hop"); } }\n'
        if mode == 'reassigned':
            setup = setup.replace('const conduit', 'let conduit') + ' conduit = opaque;'
        return ("import {PubSub as Bus} from '@google-cloud/pubsub';\n"
                "import {Storage as Store} from '@google-cloud/storage';\n"
                "import {SecretManagerServiceClient as Secrets} from '@google-cloud/secret-manager';\n"
                + extras + f'export function {label}({params}) {{ {setup} {action} }}\n').replace('NAME', expression)
    if language == 'python':
        params = 'bus, store, secrets' if mode == 'hidden' else ''
        setup, action = {
            'publish': ('conduit = bus.PublisherClient()', 'conduit.publish(NAME, data)'),
            'receive': ('conduit = bus.SubscriberClient()', 'conduit.subscribe(NAME, callback=handler)'),
            'write': ('conduit = store.Client().bucket(NAME).blob(unknown_object)', 'conduit.upload_from_string(data)'),
            'read': ('conduit = store.Client().bucket(NAME).blob(unknown_object)', 'conduit.download_as_bytes()'),
            'secret': ('conduit = secrets.SecretManagerServiceClient()', 'conduit.access_secret_version(request={"name": NAME})'),
        }[operation]
        extras = ''
        if mode == 'namehop':
            setup = 'conduit = opaque.make_client()'
            extras = 'class Factory:\n    def make_client(self):\n        return bus.PublisherClient()\n'
        if mode == 'reassigned':
            setup += '\n    conduit = opaque'
        return ('from google.cloud import pubsub_v1 as bus\nfrom google.cloud import storage as store\n'
                'from google.cloud import secretmanager as secrets\nimport os\n' + extras
                + f'def {label}({params}):\n    {setup}\n    {action}\n').replace('NAME', expression)
    if language == 'java':
        params = ('Publisher Publisher, Subscriber Subscriber, StorageOptions StorageOptions, '
                  'SecretManagerServiceClient SecretManagerServiceClient') if mode == 'hidden' else ''
        setup, action = {
            'publish': ('var conduit = Publisher.newBuilder(NAME).build();', 'conduit.publish(data);'),
            'receive': ('var conduit = Subscriber.newBuilder(NAME, handler).build();', 'conduit.startAsync();'),
            'write': ('var conduit = StorageOptions.newBuilder().build().getService();', 'conduit.create(BlobInfo.newBuilder(BlobId.of(NAME, unknownObject)).build(), data);'),
            'read': ('var conduit = StorageOptions.newBuilder().build().getService();', 'conduit.readAllBytes(NAME, unknownObject);'),
            'secret': ('var conduit = SecretManagerServiceClient.create();', 'conduit.accessSecretVersion(NAME);'),
        }[operation]
        extras = ''
        if mode == 'namehop':
            setup = 'var opaque = opaqueReceiver(); var conduit = opaque.makeClient();'
            extras = 'Publisher makeClient() { return Publisher.newBuilder("forbidden-hop").build(); }\n'
        if mode == 'reassigned':
            setup += ' conduit = opaque;'
        return ('import com.google.cloud.pubsub.v1.Publisher;\nimport com.google.cloud.pubsub.v1.Subscriber;\n'
                'import com.google.cloud.storage.StorageOptions;\nimport com.google.cloud.storage.BlobId;\n'
                'import com.google.cloud.storage.BlobInfo;\nimport com.google.cloud.secretmanager.v1.SecretManagerServiceClient;\n'
                + f'class {label} {{\n{extras}void probe({params}) {{ {setup} {action} }}\n}}\n').replace('NAME', expression)
    if language == 'go':
        params = 'bus any, store any, secrets any' if mode == 'hidden' else ''
        setup, action = {
            'publish': ('root, err := bus.NewClient(ctx, "p"); conduit := root.Publisher(NAME);', 'conduit.Publish(ctx, message)'),
            'receive': ('root, err := bus.NewClient(ctx, "p"); conduit := root.Subscriber(NAME);', 'conduit.Receive(ctx, handler)'),
            'write': ('root, err := store.NewClient(ctx); conduit := root.Bucket(NAME).Object(unknownObject);', 'conduit.NewWriter(ctx)'),
            'read': ('root, err := store.NewClient(ctx); conduit := root.Bucket(NAME).Object(unknownObject);', 'conduit.NewReader(ctx)'),
            'secret': ('conduit, err := secrets.NewClient(ctx);', 'conduit.AccessSecretVersion(ctx, &secretpb.AccessSecretVersionRequest{Name: NAME})'),
        }[operation]
        extras = ''
        if mode == 'namehop':
            setup = 'conduit := opaque.makeClient();'
            extras = 'type Factory struct{}\nfunc (f Factory) makeClient() *bus.Publisher { root, err := bus.NewClient(ctx, "p"); return root.Publisher("forbidden-hop") }\n'
        if mode == 'reassigned':
            setup += ' conduit = opaque;'
        return ('package neutral\nimport (bus "cloud.google.com/go/pubsub/v2"; store "cloud.google.com/go/storage"; '
                'secrets "cloud.google.com/go/secretmanager/apiv1"; secretpb "cloud.google.com/go/secretmanager/apiv1/secretmanagerpb"; "os")\n'
                + extras + f'func {label}({params}) {{ {setup} {action} }}\n').replace('NAME', expression)
    params = ('PublisherClient PublisherClient, SubscriberClient SubscriberClient, StorageClient StorageClient, '
              'SecretManagerServiceClient SecretManagerServiceClient') if mode == 'hidden' else ''
    setup, action = {
        'publish': ('var conduit = await PublisherClient.CreateAsync(NAME);', 'await conduit.PublishAsync(data);'),
        'receive': ('var conduit = await SubscriberClient.CreateAsync(NAME);', 'await conduit.StartAsync(handler);'),
        'write': ('var conduit = StorageClient.Create();', 'conduit.UploadObject(NAME, unknownObject, null, stream);'),
        'read': ('var conduit = StorageClient.Create();', 'conduit.DownloadObject(NAME, unknownObject, stream);'),
        'secret': ('var conduit = SecretManagerServiceClient.Create();', 'conduit.AccessSecretVersion(NAME);'),
    }[operation]
    extras = ''
    if mode == 'namehop':
        setup = 'var conduit = await opaque.MakeClient();'
        extras = 'async Task<PublisherClient> MakeClient() { return await PublisherClient.CreateAsync("forbidden-hop"); }\n'
    if mode == 'reassigned':
        setup += ' conduit = opaque;'
    return ('using System;\nusing Google.Cloud.PubSub.V1;\nusing Google.Cloud.Storage.V1;\nusing Google.Cloud.SecretManager.V1;\n'
            + f'class {label} {{\n{extras}async Task Probe({params}) {{ {setup} {action} }}\n}}\n').replace('NAME', expression)


def seed(run):
    oracle, roots = [], {}
    for language, extension in EXT.items():
        root = run / 'work' / language
        root.mkdir(parents=True)
        roots[language] = root
        git('init', '-q', cwd=root)
        git('remote', 'add', 'origin', f'https://{REMOTE}{language}.git', cwd=root)
        for operation in PREFIX:
            name = names(language, operation)
            for mode in ['literal', 'env', 'hidden'] + (['namehop', 'reassigned'] if operation == 'publish' else []):
                label = operation.capitalize() + mode.capitalize()
                variable = f'PROOF_{language}_{operation}'.upper()
                literal = (f'projects/p/{"topics" if operation == "publish" else "subscriptions"}/{name}'
                           if operation in ('publish', 'receive') else
                           f'projects/p/secrets/{name}/versions/latest' if operation == 'secret' else name)
                env = {'js': f'process.env.{variable}', 'ts': f'process.env.{variable}',
                       'python': f'os.environ["{variable}"]', 'java': f'System.getenv("{variable}")',
                       'go': f'os.Getenv("{variable}")', 'csharp': f'Environment.GetEnvironmentVariable("{variable}")'}[language]
                expression = env if mode == 'env' else json.dumps(literal)
                filename = label + '.' + extension
                write(root, filename, source(language, operation, expression, label, mode))
                prefix = PREFIX[operation]
                physical = ('subscriptions/' if operation == 'receive' else '') + name
                contract = prefix if mode == 'env' else prefix + '::' + physical
                oracle.append(dict(language=language, file=filename, operation=operation, mode=mode,
                                   role=ROLE[operation], contract=contract, variable=variable,
                                   accepted=mode in ('literal', 'env')))
        topic, bucket, secret = [names(language, op) for op in ('publish', 'write', 'secret')]
        write(root, 'main.tf', f'''resource "google_pubsub_topic" "signal" {{ name = "{topic}" }}
resource "google_pubsub_subscription" "signal" {{
  name = "{topic}"
  topic = google_pubsub_topic.signal.id
  filter = "attributes.kind = \\"signal\\""
}}
resource "google_storage_bucket" "objects" {{ name = "{bucket}" }}
resource "google_secret_manager_secret" "credential" {{ secret_id = "{secret}" }}
''')
        git('add', '.', cwd=root)
        git('commit', '-qm', 'neutral GCP SDK proof input', cwd=root)
    saved(run, 'oracle.json', oracle)
    saved(run, 'inputs.sha256.json', {
        str(p.relative_to(run)): hashlib.sha256(p.read_bytes()).hexdigest()
        for root in roots.values() for p in root.rglob('*') if p.is_file() and '.git' not in p.parts})
    return roots, oracle


def capture(binary, root, run, label, *arguments):
    environment = os.environ.copy()
    environment.update({f'PROOF_{language}_{op}'.upper(): names(language, op)
                        for language in EXT for op in PREFIX})
    result = subprocess.run([binary, *arguments], cwd=root, env=environment,
                            text=True, capture_output=True, timeout=120)
    write(run, label + '.out', result.stdout)
    write(run, label + '.err', result.stderr)
    write(run, label + '.exit', str(result.returncode) + '\n')
    assert result.returncode == 0, result.stderr
    return result.stdout


def judge(data, rows):
    for role in ['providers', 'consumers']:
        expected = Counter(r['contract'] for r in rows if r['role'] == role and r['accepted'])
        assert Counter(s['contract'] for s in data[role]) == expected, (role, data[role], expected)
    for row in rows:
        sides = [s for s in data[row['role']] if '::' + row['file'] + '::' in s['site']]
        assert len(sides) == int(row['accepted']), (row, sides)
        if sides:
            assert sides[0]['contract'] == row['contract'], (row, sides)
            if row['mode'] == 'env':
                assert sides[0]['via'] == 'env:' + row['variable'], (row, sides)
        if row['mode'] == 'namehop':
            assert any(c['span']['path'] == row['file']
                       and ('opaque.make' in c['id'] or 'opaque.Make' in c['id'])
                       and c['level'] in ('inferred', 'ambiguous')
                       for c in data['calls']), ('name-matched control did not exercise the seam', row)
    assert len(data.get('provisions', [])) == 5, data.get('provisions')
    assert all(p['level'] == 'exact' for p in data['provisions']), data['provisions']
    assert len(data['links']) == 2, data['links']
    assert sorted(l['level'] for l in data['links']) == ['exact', 'inferred'], data['links']
    assert all('filter' in l['reason'] for l in data['links'] if l['level'] == 'inferred'), data['links']
    assert len(data['subscriptions']) == 1, data['subscriptions']
    assert 'pending-workload-env:' not in json.dumps(data)


def transports(binary, root, language, data, run, http):
    row = next(p for p in data['providers'] if p['contract'] == 'channel::pubsub::' + names(language, 'publish'))
    symbol = row['site'].rsplit('::', 1)[0]
    queries = [
        {'tool': 'search', 'arguments': {'query': 'signal-' + language, 'kind': 'contract', 'budget': 12000}},
        {'tool': 'context', 'arguments': {'symbol': symbol, 'budget': 20000}},
        {'tool': 'trace', 'arguments': {'from': symbol, 'direction': 'up', 'budget': 20000}},
        {'tool': 'search', 'arguments': {'query': 'zz_prove_missing_gcp', 'budget': 300}},
    ]
    tools, texts, _ = mcp_answers(binary, root, queries, run / (language + '.stdio.err'))
    saved(run, language + '.queries.json', queries)
    saved(run, language + '.stdio.json', texts)
    assert all(texts) and 'channel::pubsub::' in texts[0], texts
    assert names(language, 'publish') in texts[1], texts[1]
    assert 'google_pubsub_topic.signal' in texts[2], texts[2]
    if not http:
        return
    manifest = run / (language + '.workspace.toml')
    write(run, manifest.name, f'index_dir = "{language}-indexes"\n[[repository]]\nrepo = "{REMOTE}{language}"\npath = "{root}"\n')
    capture(binary, run, run, language + '.index', 'index', '--workspace', str(manifest))
    port = free_port()
    with (run / (language + '.http.out')).open('w') as out, (run / (language + '.http.err')).open('w') as err:
        server = subprocess.Popen([binary, 'mcp', '--listen', f'127.0.0.1:{port}', '--workspace', str(manifest)], cwd=run, stdout=out, stderr=err)
        try:
            for _ in range(100):
                try:
                    socket.create_connection(('127.0.0.1', port), timeout=1).close()
                    break
                except OSError:
                    time.sleep(.1)
            client = Http(f'http://127.0.0.1:{port}/{REMOTE}{language}')
            assert client.call('tools/list', {})['result']['tools'] == tools
            replies = [answer(client.call('tools/call', {'name': q['tool'], 'arguments': q['arguments']})) for q in queries]
            saved(run, language + '.http.json', replies)
            assert replies == texts
        finally:
            server.terminate()
            server.wait(timeout=30)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--head-bin')
    parser.add_argument('--run', required=True)
    parser.add_argument('--http', action='store_true')
    parser.add_argument('--seed-only', action='store_true')
    args = parser.parse_args()
    run = Path(args.run).resolve()
    run.mkdir(parents=True, exist_ok=False)
    roots, oracle = seed(run)
    if args.seed_only:
        subprocess.run([sys.executable, str(Path(__file__).with_name('gcp-sdk-supplement.py')),
                        '--run', str(run / 'supplement'), '--seed-only'], check=True)
        return
    assert args.head_bin, '--head-bin is required except with --seed-only'
    binary = str(Path(args.head_bin).resolve())
    for language, root in roots.items():
        data = json.loads(capture(binary, root, run, language + '.export', 'export'))
        judge(data, [r for r in oracle if r['language'] == language])
        transports(binary, root, language, data, run, args.http)
        path = root / ('PublishLiteral.' + EXT[language])
        original = path.read_text()
        renamed = names(language, 'publish').replace('signal-', 'mutate-')
        path.write_text(original.replace(names(language, 'publish'), renamed))
        changed = json.loads(capture(binary, root, run, language + '.changed', 'export'))
        assert Counter(p['contract'] for p in changed['providers']) != Counter(p['contract'] for p in data['providers'])
        assert any(p['contract'] == 'channel::pubsub::' + renamed for p in changed['providers'])
        path.write_text(original)
        restored = json.loads(capture(binary, root, run, language + '.restored', 'export'))
        assert restored == data
    subprocess.run([sys.executable, str(Path(__file__).with_name('gcp-sdk-supplement.py')),
                    '--head-bin', binary, '--run', str(run / 'supplement'),
                    *(['--http'] if args.http else [])], check=True)
    saved(run, 'checks.json', {'checks': 'passed', 'answer_key': '#668/#642; official SDK inventory; base ADR 0026',
                              'independent': False, 'http': args.http, 'groups': list(EXT)})
    print('GCP SDK checks passed; independent checker judges scope and verdict')


if __name__ == '__main__':
    main()
