#!/usr/bin/env python3
"""Drive #667 through CLI and both MCP transports. Answer key: #667/#642 and ADR 0026.

Generated names differ from feature fixtures. No product reader, fixture or golden supplies
expected answers. A script pass records checks, never an independent proof verdict.
"""
import argparse
import hashlib
import json
import socket
import subprocess
import time
from collections import Counter
from pathlib import Path

from prove import Http, answer, free_port, git, mcp_answers
from provisions import write

REMOTE = 'example.com/gcp-proof/'


def saved(run, label, data):
    (run / label).write_text(json.dumps(data, indent=2) + '\n')


def capture(binary, repo, run, label, *arguments):
    result = subprocess.run([binary, *arguments], cwd=repo, text=True, capture_output=True,
                            timeout=120)
    (run / f'{label}.out').write_text(result.stdout)
    (run / f'{label}.err').write_text(result.stderr)
    (run / f'{label}.exit').write_text(str(result.returncode) + '\n')
    assert result.returncode == 0, result.stderr
    return result.stdout


def container(image, name='worker', extra=''):
    return f'''containers {{
      name = "{name}"
      image = {image}
      env {{
        name = "TASKS"
        value = "tasks"
      }}
      {extra}
    }}'''


def seed(run):
    infra, app = run / 'work/infra', run / 'work/app'
    for name, root, peer in [('infra', infra, 'app'), ('app', app, 'infra')]:
        root.mkdir(parents=True)
        git('init', '-q', cwd=root)
        git('remote', 'add', 'origin', f'https://{REMOTE}{name}.git', cwd=root)
        write(root, 'meada.toml', f'[[peer]]\nrepo = "{REMOTE}{peer}"\n')
    write(infra, 'main.tf', '''variable "database_instance" { default = "core-db" }
resource "google_pubsub_topic" "notice" { name = "notices" }
resource "google_pubsub_subscription" "notice" {
  name = "notices"
  topic = google_pubsub_topic.notice.id
  filter = "attributes.kind = \\"notice\\""
}
resource "google_storage_bucket" "assets" { name = "vault-assets" }
resource "google_storage_bucket" "idle" { name = "unused-assets" }
resource "google_sql_database_instance" "core" { name = var.database_instance }
resource "google_sql_database_instance" "audit" { name = "audit-db" }
resource "google_sql_database" "core" {
  name = "ledger"
  instance = google_sql_database_instance.core.name
}
resource "google_sql_database" "audit" {
  name = "ledger"
  instance = google_sql_database_instance.audit.id
}
resource "google_secret_manager_secret" "key" { secret_id = "api-token" }
resource "aws_sqs_queue" "tasks" { name = "tasks" }
resource "google_sql_database" "unknown" {
  name = "ledger"
  instance = data.external.unknown.result
}
''')
    secret = '''env {
        name = "SECRET"
        value_from {
          secret_key_ref {
          name = "api-token"
          key = "latest"
          }
        }
      }'''
    v1 = container('"registry.example.com/proof/worker:1"', extra=secret)
    other = container('"registry.example.com/proof/other:1"', 'other',
                      'env {\n name = "SIDE"\n value = "other-container"\n }')
    write(infra, 'run.tf', f'''variable "image" {{ default = "registry.example.com/proof/worker:2" }}
resource "google_cloud_run_service" "v1" {{
  name = "legacy"
  template {{
    spec {{
      {v1}\n{other}
    }}
  }}
}}
resource "google_cloud_run_v2_service" "v2" {{
  name = "current"
  template {{
    {container('var.image')}
  }}
}}
resource "google_cloud_run_v2_job" "job" {{
  name = "batch"
  template {{
    template {{
      {container(chr(34) + 'registry.example.com/proof/worker:1' + chr(34))}
    }}
  }}
}}
''')
    write(app, '.github/workflows/image.yml', '''jobs:
  image:
    steps:
      - uses: docker/build-push-action@v6
        with:
          tags: registry.example.com/proof/worker:1
''')
    write(app, 'worker.ts', '''import { SQSClient, SendMessageCommand } from '@aws-sdk/client-sqs';
const sqs = new SQSClient({});
export function send() { return sqs.send(new SendMessageCommand({QueueUrl: process.env.TASKS})); }
export function secret() { return sqs.send(new SendMessageCommand({QueueUrl: process.env.SECRET})); }
export function side() { return sqs.send(new SendMessageCommand({QueueUrl: process.env.SIDE})); }
''')
    settings = {'exact': {
        'topic': 'projects/proof/topics/notices',
        'subscription': 'projects/proof/subscriptions/notices',
        'bucket': 'gs://vault-assets/path?opaque#object',
        'secret': 'projects/proof/secrets/api-token/versions/latest'},
        'typed': {'pubsub_topic': 'notices', 'pubsub_subscription': 'notices',
                  'gcs_bucket': 'vault-assets', 'cloudsql_instance': 'core-db',
                  'cloudsql_database': 'core-db/ledger', 'gcpsecretmanager_secret': 'api-token'},
        'controls': {'equal': 'notices', 'bad': 'projects/proof/topics/notices/extra',
                     'wrong': 'projects/proof/subscriptions/notices?query'}}
    write(app, 'settings.json', json.dumps(settings))
    manifest = (app / 'meada.toml').read_text() + '\n[serve]\n'
    for instance in ['core-db', 'audit-db']:
        manifest += f'\n[[resource]]\nkind = "cloudsql"\nid = "{instance}/ledger"\nrole = "read"\n'
    write(app, 'meada.toml', manifest)
    write(app, 'asyncapi.json', json.dumps({
        'asyncapi': '2.6.0', 'servers': {'gcp': {'url': 'localhost', 'protocol': 'pubsub'}},
        'channels': {'notices': {'subscribe': {}}, 'subscriptions/notices': {'publish': {}}}}))
    for root in [infra, app]:
        git('add', '.', cwd=root)
        git('commit', '-qm', 'GCP proof input', cwd=root)
    saved(run, 'input-sha256.json', {
        str(p.relative_to(run)): hashlib.sha256(p.read_bytes()).hexdigest()
        for root in [infra, app] for p in root.rglob('*') if p.is_file() and '.git' not in p.parts})
    return infra, app


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--head-bin', required=True)
    parser.add_argument('--run', required=True)
    args = parser.parse_args()
    binary, run = str(Path(args.head_bin).resolve()), Path(args.run).resolve()
    run.mkdir(parents=True, exist_ok=False)
    infra, app = seed(run)
    data = json.loads(capture(binary, infra, run, 'infra-export', 'export'))
    expected = {
        'google_pubsub_topic.notice': 'channel::pubsub::notices',
        'google_pubsub_subscription.notice': 'channel::pubsub::subscriptions/notices',
        'google_storage_bucket.assets': 'resource::gcs::vault-assets',
        'google_storage_bucket.idle': 'resource::gcs::unused-assets',
        'google_sql_database_instance.core': 'resource::cloudsql::core-db',
        'google_sql_database_instance.audit': 'resource::cloudsql::audit-db',
        'google_sql_database.core': 'resource::cloudsql::core-db/ledger',
        'google_sql_database.audit': 'resource::cloudsql::audit-db/ledger',
        'google_secret_manager_secret.key': 'resource::gcpsecretmanager::api-token',
        'aws_sqs_queue.tasks': 'channel::sqs::tasks',
    }
    actual = [(p['node'].split('::')[-1], p['contract']) for p in data.get('provisioners', [])]
    assert len(actual) == len(expected) and dict(actual) == expected, actual
    routes = data['subscriptions']
    assert len(routes) == 1 and routes[0]['source'] == 'channel::pubsub::notices' and routes[0]['target'] == 'channel::pubsub::subscriptions/notices', routes
    app_data = json.loads(capture(binary, app, run, 'app-export', 'export'))
    expected_uses = {
        ('settings.json#exact.topic', 'channel::pubsub::notices'),
        ('settings.json#exact.subscription', 'channel::pubsub::subscriptions/notices'),
        ('settings.json#exact.bucket', 'resource::gcs::vault-assets'),
        ('settings.json#exact.secret', 'resource::gcpsecretmanager::api-token'),
        ('settings.json#typed.pubsub_topic', 'channel::pubsub::notices'),
        ('settings.json#typed.pubsub_subscription', 'channel::pubsub::subscriptions/notices'),
        ('settings.json#typed.gcs_bucket', 'resource::gcs::vault-assets'),
        ('settings.json#typed.cloudsql_instance', 'resource::cloudsql::core-db'),
        ('settings.json#typed.cloudsql_database', 'resource::cloudsql::core-db/ledger'),
        ('settings.json#typed.gcpsecretmanager_secret', 'resource::gcpsecretmanager::api-token'),
    }
    actual_uses = [(u['at'], u['contract']) for u in app_data['uses']]
    assert len(actual_uses) == len(expected_uses) and set(actual_uses) == expected_uses, actual_uses
    for p in app_data['provisions']:
        if p.get('at', '').startswith('settings.json#'):
            assert p['level'] == ('inferred' if '#typed.' in p['at'] else 'exact'), p
        if p['contract'] == 'resource::cloudsql::core-db/ledger':
            assert p['level'] == 'inferred' and 'var.database_instance (default)' in p['reason'], p
    links = app_data['links']
    assert len(links) == 4, links
    route = [l for l in links if l['contract'].startswith('channel::')]
    assert len(route) == 1 and route[0]['level'] == 'inferred' and 'filter' in route[0]['reason'], links
    providers = app_data['providers']
    assert Counter(p['contract'] for p in providers) == Counter({
        'channel::sqs::tasks': 1, 'channel::sqs': 2, 'channel::pubsub::notices': 1,
        'image::registry.example.com/proof/worker': 1}), providers
    for p in providers:
        if '::send::' in p['site']:
            assert p['contract'] == 'channel::sqs::tasks' and 'google_cloud_run' in p['via'], p
        elif '::secret::' in p['site'] or '::side::' in p['site']:
            assert p['contract'] == 'channel::sqs', p
    status = capture(binary, infra, run, 'status', 'status')
    assert 'orphan resource::gcs::unused-assets: no use;' in status, status
    queries = [
        {'tool': 'search', 'arguments': {'query': 'google_cloud_run', 'kind': 'iac', 'budget': 12000}},
        {'tool': 'context', 'arguments': {'symbol': 'google_pubsub_subscription.notice', 'budget': 12000}},
        {'tool': 'context', 'arguments': {'symbol': 'google_cloud_run_service.v1', 'budget': 12000}},
        {'tool': 'context', 'arguments': {'symbol': 'google_cloud_run_v2_service.v2', 'budget': 12000}},
        {'tool': 'context', 'arguments': {'symbol': 'google_cloud_run_v2_job.job', 'budget': 12000}},
        {'tool': 'trace', 'arguments': {'from': 'google_cloud_run_v2_job.job', 'direction': 'down', 'budget': 16000}},
        {'tool': 'trace', 'arguments': {'from': 'send', 'direction': 'up', 'budget': 16000}},
    ]
    tools, texts, _ = mcp_answers(binary, infra, queries, run / 'stdio.err')
    saved(run, 'stdio.json', texts)
    assert all(name in texts[0] for name in ['google_cloud_run_service.v1', 'google_cloud_run_v2_service.v2', 'google_cloud_run_v2_job.job']), texts[0]
    assert 'name: "notices"' in texts[1] and 'subscriptions/notices' in texts[1], texts[1]
    assert 'SECRET' in texts[2] and 'secret' in texts[2], texts[2]
    assert all('image::registry.example.com/proof/worker' in text for text in texts[2:5]), texts
    assert 'worker.ts' in texts[5] and 'google_cloud_run_v2_job.job' in texts[6], texts
    manifest = 'index_dir = "indexes"\n' + ''.join(f'\n[[repository]]\nrepo = "{REMOTE}{name}"\npath = "{root}"\n' for name, root in [('infra', infra), ('app', app)])
    write(run, 'workspace.toml', manifest)
    capture(binary, run, run, 'index', 'index', '--workspace', 'workspace.toml')
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
            client = Http(f'http://127.0.0.1:{port}/{REMOTE}infra')
            assert client.call('tools/list', {})['result']['tools'] == tools
            replies = [answer(client.call('tools/call', {'name': q['tool'], 'arguments': q['arguments']})) for q in queries]
            saved(run, 'http.json', replies)
            assert replies == texts
        finally:
            server.terminate()
            server.wait(timeout=30)
    source = infra / 'main.tf'
    original = source.read_text()
    source.write_text('\n'.join(line for line in original.splitlines() if not line.strip().startswith('filter =')) + '\n')
    plain = json.loads(capture(binary, app, run, 'plain-export', 'export'))
    assert [l['level'] for l in plain['links'] if l['contract'].startswith('channel::')] == ['exact']
    source.write_text(original)
    assert json.loads(capture(binary, app, run, 'restored-export', 'export')) == app_data
    saved(run, 'checks.json', {'checks': 'passed', 'answer_key': '#667/#642; ADR 0026', 'surfaces': ['CLI', 'MCP stdio', 'MCP HTTP'], 'independent': False})


if __name__ == '__main__':
    main()
