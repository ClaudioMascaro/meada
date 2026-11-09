#!/usr/bin/env python3
"""#671 CLI/MCP probe; answer keys are #671/#642, ADR0025/26 and official CR schemas.

All names are independently generated here. No product table, feature fixture or golden is
read. A passing script is an implementer receipt, not an independent proof verdict.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import socket
import subprocess
import time

from prove import Http, answer, free_port, git, mcp_answers
from provisions import write

REMOTE = 'example.com/custom-iac-proof/'
EXTERNAL = 'metadata.annotations.crossplane.io/external-name'
# Explicit, bounded official inventory: group, kind, identity path, canonical contract kind.
ROWS = [
 ('kafka.strimzi.io', 'KafkaTopic', 'spec.topicName', 'channel::kafka'),
 ('rabbitmq.com', 'Queue', 'spec.name', 'channel::amqp'),
 ('sqs.services.k8s.aws', 'Queue', 'spec.queueName', 'channel::sqs'),
 ('s3.services.k8s.aws', 'Bucket', 'spec.name', 'resource::s3'),
 ('sns.services.k8s.aws', 'Topic', 'spec.name', 'channel::sns'),
 ('eventbridge.services.k8s.aws', 'EventBus', 'spec.name', 'channel::eventbridge'),
 ('kinesis.services.k8s.aws', 'Stream', 'spec.name', 'channel::kinesis'),
 ('dynamodb.services.k8s.aws', 'Table', 'spec.tableName', 'resource::dynamodb'),
 ('rds.services.k8s.aws', 'DBInstance', 'spec.dbInstanceIdentifier', 'resource::rds'),
 ('rds.services.k8s.aws', 'DBCluster', 'spec.dbClusterIdentifier', 'resource::rds'),
 ('elasticache.services.k8s.aws', 'CacheCluster', 'spec.cacheClusterID', 'resource::elasticache'),
 ('elasticache.services.k8s.aws', 'ReplicationGroup', 'spec.replicationGroupID', 'resource::elasticache'),
 ('ssm.services.k8s.aws', 'Parameter', 'spec.name', 'resource::ssm'),
 ('secretsmanager.services.k8s.aws', 'Secret', 'spec.name', 'resource::secretsmanager'),
 ('pubsub.cnrm.cloud.google.com', 'PubSubTopic', 'spec.resourceID', 'channel::pubsub'),
 ('pubsub.cnrm.cloud.google.com', 'PubSubSubscription', 'spec.resourceID', 'channel::pubsub'),
 ('storage.cnrm.cloud.google.com', 'StorageBucket', 'spec.resourceID', 'resource::gcs'),
 ('sql.cnrm.cloud.google.com', 'SQLInstance', 'spec.resourceID', 'resource::cloudsql'),
 ('sql.cnrm.cloud.google.com', 'SQLDatabase', 'spec.resourceID', 'resource::cloudsql'),
 ('secretmanager.cnrm.cloud.google.com', 'SecretManagerSecret', 'spec.resourceID', 'resource::gcpsecretmanager'),
 ('sqs.aws.upbound.io', 'Queue', EXTERNAL, 'channel::sqs'),
 ('s3.aws.upbound.io', 'Bucket', EXTERNAL, 'resource::s3'),
 ('sns.aws.upbound.io', 'Topic', EXTERNAL, 'channel::sns'),
 ('cloudwatchevents.aws.upbound.io', 'Bus', EXTERNAL, 'channel::eventbridge'),
 ('kinesis.aws.upbound.io', 'Stream', EXTERNAL, 'channel::kinesis'),
 ('dynamodb.aws.upbound.io', 'Table', EXTERNAL, 'resource::dynamodb'),
 ('rds.aws.upbound.io', 'Instance', EXTERNAL, 'resource::rds'),
 ('elasticache.aws.upbound.io', 'Cluster', EXTERNAL, 'resource::elasticache'),
 ('ssm.aws.upbound.io', 'Parameter', EXTERNAL, 'resource::ssm'),
 ('secretsmanager.aws.upbound.io', 'Secret', EXTERNAL, 'resource::secretsmanager'),
 ('pubsub.gcp.upbound.io', 'Topic', EXTERNAL, 'channel::pubsub'),
 ('pubsub.gcp.upbound.io', 'Subscription', EXTERNAL, 'channel::pubsub'),
 ('storage.gcp.upbound.io', 'Bucket', EXTERNAL, 'resource::gcs'),
 ('sql.gcp.upbound.io', 'DatabaseInstance', EXTERNAL, 'resource::cloudsql'),
 ('sql.gcp.upbound.io', 'Database', EXTERNAL, 'resource::cloudsql'),
 ('secretmanager.gcp.upbound.io', 'Secret', EXTERNAL, 'resource::gcpsecretmanager'),
 ('servicebus.azure.upbound.io', 'Queue', EXTERNAL, 'channel::servicebus'),
 ('servicebus.azure.upbound.io', 'Topic', EXTERNAL, 'channel::servicebus'),
 ('servicebus.azure.upbound.io', 'Subscription', EXTERNAL, 'channel::servicebus'),
 ('eventhub.azure.upbound.io', 'EventHub', EXTERNAL, 'channel::eventhubs'),
 ('storage.azure.upbound.io', 'Account', EXTERNAL, 'resource::azurestorage'),
 ('storage.azure.upbound.io', 'Container', EXTERNAL, 'resource::azureblob'),
 ('keyvault.azure.upbound.io', 'Secret', EXTERNAL, 'resource::keyvault'),
]


def saved(run, name, data):
    (run / name).write_text(json.dumps(data, indent=2) + '\n')


def capture(binary, repo, run, label, *arguments, success=True):
    result = subprocess.run([binary, *arguments], cwd=repo, text=True, capture_output=True, timeout=120)
    (run / (label+'.out')).write_text(result.stdout)
    (run / (label+'.err')).write_text(result.stderr)
    (run / (label+'.exit')).write_text(str(result.returncode)+'\n')
    assert (result.returncode == 0) == success, result.stderr
    return json.loads(result.stdout) if success and arguments == ('export',) else result.stdout+result.stderr


def repository(run, name):
    repo = run / 'work' / name
    repo.mkdir(parents=True)
    git('init', '-q', cwd=repo)
    git('remote', 'add', 'origin', f'https://{REMOTE}{name}.git', cwd=repo)
    return repo


def inventory(repo):
    expected, contexts = [], []
    for i, (group, kind, field, target) in enumerate(ROWS):
        name = f'probe671-{i:02}'
        if group == 'storage.azure.upbound.io' and kind == 'Account': name = f'probe671{i:02}'
        version = 'v1alpha1' if group.endswith('.services.k8s.aws') else 'v1beta1'
        if group == 'kafka.strimzi.io': version = 'v1'
        if (group, kind) in {('kinesis.aws.upbound.io', 'Stream'), ('dynamodb.aws.upbound.io', 'Table'), ('s3.aws.upbound.io', 'Bucket'), ('pubsub.gcp.upbound.io', 'Topic'), ('pubsub.gcp.upbound.io', 'Subscription'), ('storage.gcp.upbound.io', 'Bucket'), ('sql.gcp.upbound.io', 'DatabaseInstance'), ('secretmanager.gcp.upbound.io', 'Secret'), ('servicebus.azure.upbound.io', 'Subscription'), ('storage.azure.upbound.io', 'Account'), ('eventhub.azure.upbound.io', 'EventHub')}: version = 'v1beta2'
        if (group, kind) == ('rds.aws.upbound.io', 'Instance'): version = 'v1beta3'
        obj = {'apiVersion': group+'/'+version, 'kind': kind, 'metadata': {'name': f'object-{i:02}'}, 'spec': {}}
        contract_name = name
        if group == 'servicebus.azure.upbound.io' and kind == 'Subscription':
            value = f'/subscriptions/proof/resourceGroups/core/providers/Microsoft.ServiceBus/namespaces/bus/topics/audit/subscriptions/{name}'
            contract_name = 'subscription:audit/'+name
        else:
            value = name
        if field == EXTERNAL:
            obj['metadata']['annotations'] = {'crossplane.io/external-name': value}
        else:
            obj['spec'][field.split('.')[1]] = value
        if target == 'resource::cloudsql' and kind in ['SQLDatabase', 'Database']:
            if kind == 'SQLDatabase': obj['spec']['instanceRef'] = {'external': 'proof-instance'}
            else: obj['spec']['forProvider'] = {'instance': 'proof-instance'}
            contract_name = 'proof-instance/'+name
        if target == 'channel::pubsub' and kind in ['PubSubSubscription', 'Subscription']:
            contract_name = 'subscriptions/'+name
        path = f'inventory/case-{i:02}.json'
        write(repo, path, json.dumps(obj))
        address = f'{kind}.{group}/object-{i:02}'
        expected.append((target+'::'+contract_name, REMOTE+'inventory::'+path+'::'+address, 'inventory'))
        contexts.append((address, value, target+'::'+contract_name))
    write(repo, 'inventory/unknown.json', json.dumps({'apiVersion': 'neighbor.example.com/v1', 'kind': 'Queue', 'metadata': {'name': 'searchable-unmapped'}, 'spec': {'name': 'never-a-side'}}))
    return sorted(expected), contexts


def provisioners(value):
    return sorted((p['contract'], p['node'], p['root']) for p in value.get('provisioners', []))


def transports(binary, repos, primary, queries, run, http):
    tools, texts, _ = mcp_answers(binary, primary, queries, run/'stdio.err')
    saved(run, 'queries.json', queries)
    saved(run, 'stdio.json', texts)
    if http:
        manifest = 'index_dir = "indexes"\n'
        for name, repo in repos.items():
            manifest += f'\n[[repository]]\nrepo = "{REMOTE}{name}"\npath = "{repo}"\n'
        write(run, 'workspace.toml', manifest)
        with (run/'index.out').open('w') as out, (run/'index.err').open('w') as err:
            subprocess.run([binary, 'index', '--workspace', 'workspace.toml'], cwd=run, check=True, stdout=out, stderr=err)
        port = free_port()
        with (run/'http.out').open('w') as out, (run/'http.err').open('w') as err:
            server = subprocess.Popen([binary, 'mcp', '--listen', f'127.0.0.1:{port}', '--workspace', 'workspace.toml'], cwd=run, stdout=out, stderr=err)
            try:
                for _ in range(100):
                    try:
                        socket.create_connection(('127.0.0.1', port), timeout=1).close()
                        break
                    except OSError: time.sleep(.1)
                client = Http(f'http://127.0.0.1:{port}/{REMOTE}{primary.name}')
                assert client.call('tools/list', {})['result']['tools'] == tools
                replies = [answer(client.call('tools/call', {'name': q['tool'], 'arguments': q['arguments']})) for q in queries]
                saved(run, 'http.json', replies)
                assert replies == texts
            finally:
                server.terminate()
                server.wait(timeout=30)
    return texts


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--head-bin', required=True)
    p.add_argument('--base-bin')
    p.add_argument('--run', required=True)
    p.add_argument('--http', action='store_true')
    args = p.parse_args()
    binary = str(Path(args.head_bin).resolve())
    run = Path(args.run).resolve()
    run.mkdir(parents=True, exist_ok=False)
    saved(run, 'inputs.json', {'head_sha256': hashlib.sha256(Path(binary).read_bytes()).hexdigest(), 'answer_keys': ['#671', '#642', 'ADR0025', 'ADR0026'], 'inventory_count': len(ROWS)})
    inv = repository(run, 'inventory')
    expected, contexts = inventory(inv)
    saved(run, 'inventory-oracle.json', expected)
    value = capture(binary, inv, run, 'inventory', 'export')
    assert provisioners(value) == expected, value
    for field in ['providers', 'consumers', 'links', 'provisions']:
        assert not value.get(field), (field, value)
    if args.base_bin:
        base = capture(str(Path(args.base_bin).resolve()), inv, run, 'base-inventory', 'export')
        assert not base.get('provisioners'), base
    queries = [{'tool': 'context', 'arguments': {'symbol': address, 'budget': 12000}} for address, _, _ in contexts]
    queries.append({'tool': 'search', 'arguments': {'query': 'searchable-unmapped', 'kind': 'iac', 'budget': 12000}})
    invrun = run/'inventory-transports'; invrun.mkdir()
    texts = transports(binary, {'inventory': inv}, inv, queries, invrun, args.http)
    for text, (_, physical, contract) in zip(texts, contexts):
        assert physical in text and contract in text, text
    assert 'Queue.neighbor.example.com' in texts[-1], texts[-1]
    infra = repository(run, 'infra')
    config = '''[[iac.module]]
source_prefix = "git::https://example.com/private/modules"
entry = "terraform:aws_sqs_queue"
input = "queue_name"
[[iac.module]]
source = "git::https://example.com/private/modules/priority"
entry = "terraform:aws_sns_topic"
input = "settings.name"
[[iac.custom_resource]]
group = "platform.example.com"
kind = "MessageQueue"
entry = "terraform:aws_sqs_queue"
key_path = "spec.target"
'''
    write(infra, 'meada.toml', config)
    tf = '''variable "stage" { default = "blue" }
module "private" {
 source = "git::https://example.com/private/modules/queue"
 queue_name = "private-${var.stage}"
}
module "priority" {
 source = "git::https://example.com/private/modules/priority"
 settings = { name = "urgent" }
}
module "neighbor" {
 source = "git::https://example.com/private/modules-other/queue"
 queue_name = "never-neighbor"
}
module "queue" {
 source = "terraform-aws-modules/sqs/aws"
 name = "registry-jobs"
}
module "topic" {
 source = "terraform-aws-modules/sns/aws"
 name = "registry-events"
 fifo_topic = true
}
module "bucket" {
 source = "terraform-aws-modules/s3-bucket/aws"
 bucket = "registry-assets"
}
module "table" {
 source = "terraform-aws-modules/dynamodb-table/aws"
 name = "registry-ledger"
}
module "function" {
 source = "terraform-aws-modules/lambda/aws"
 function_name = "processor"
 package_type = "Image"
 image_uri = "registry.example.com/probe671/function:1"
 environment_variables = { JOBS = "registry-jobs" }
}
module "service" {
 source = "terraform-aws-modules/ecs/aws//modules/service"
 name = "processor-service"
 container_definitions = { worker = { image = "registry.example.com/probe671/service:1", environment = [{ name = "JOBS", value = "private-blue" }] }, off = { create = false, image = "registry.example.com/probe671/service:1", environment = [{name = "JOBS", value = "never-disabled"}] } }
}
module "off" {
 source = "terraform-aws-modules/sqs/aws"
 name = "never-off"
 create = false
}
module "zero" {
 source = "terraform-aws-modules/sqs/aws"
 name = "never-zero"
 count = 0
}
module "maybe" {
 source = "terraform-aws-modules/sqs/aws"
 name = "never-maybe"
 create = data.external.unread.result
}
module "unknown" {
 source = "terraform-aws-modules/sqs/aws"
 name = data.external.unread.result
}
module "prefix" {
 source = "terraform-aws-modules/sqs/aws"
 name = "prefix"
 use_name_prefix = true
}
'''
    write(infra, 'main.tf', tf)
    custom = {'apiVersion': 'platform.example.com/v1', 'kind': 'MessageQueue', 'metadata': {'name': 'custom-object'}, 'spec': {'target': 'private-custom'}}
    write(infra, 'custom.json', json.dumps(custom))
    module_expected = [('channel::sqs::private-blue', 'module.private'), ('channel::sns::urgent', 'module.priority'), ('channel::sqs::registry-jobs', 'module.queue'), ('channel::sns::registry-events.fifo', 'module.topic'), ('resource::s3::registry-assets', 'module.bucket'), ('resource::dynamodb::registry-ledger', 'module.table')]
    wanted = sorted([(c, REMOTE+'infra::main.tf::'+a, '.') for c,a in module_expected]+[('channel::sqs::private-custom', REMOTE+'infra::custom.json::MessageQueue.platform.example.com/custom-object', '.')])
    baseline = capture(binary, infra, run, 'modules', 'export')
    assert provisioners(baseline) == wanted, baseline
    assert [(c['contract'], c['site']) for c in baseline.get('consumers', [])] == [('image::registry.example.com/probe671/function', REMOTE+'infra::main.tf::module.function'), ('image::registry.example.com/probe671/service', REMOTE+'infra::main.tf::module.service')], baseline
    tf += '''module "source_function" {
 source = "terraform-aws-modules/lambda/aws"
 function_name = "local-source"
 source_path = "code"
 handler = "handler.processJob"
}
module "package_function" {
 source = "terraform-aws-modules/lambda/aws"
 function_name = "opaque-package"
 create_package = false
 local_existing_package = "bundle/function.zip"
}
module "list_source" {
 source = "terraform-aws-modules/lambda/aws"
 function_name = "never-list"
 source_path = ["code"]
}
module "parent_source" {
 source = "terraform-aws-modules/lambda/aws"
 function_name = "never-parent"
 source_path = "../outside"
}
module "unread_package" {
 source = "terraform-aws-modules/lambda/aws"
 function_name = "never-package"
 source_path = "code"
 create_package = data.external.unread.result
}
'''
    write(infra, 'main.tf', tf)
    code = 'export function processJob() { return "done"; }\n'
    write(infra, 'code/handler.ts', code)
    write(infra, 'bundle/function.zip', 'opaque package; do not extract or execute\n')
    def executions(value):
        return sorted((x['workload'].rsplit('::', 1)[1], x['path'], x['level']) for x in value.get('executions', []))
    paths = capture(binary, infra, run, 'code-paths', 'export')
    expected_paths = [('module.package_function', 'bundle/function.zip', 'exact'), ('module.source_function', 'code', 'exact')]
    assert executions(paths) == expected_paths, paths
    shutil.rmtree(infra/'code')
    assert executions(capture(binary, infra, run, 'source-removed', 'export')) == expected_paths[:1]
    outside = run/'outside'; outside.mkdir()
    write(outside, 'handler.ts', code)
    (infra/'code').symlink_to(outside, target_is_directory=True)
    assert executions(capture(binary, infra, run, 'source-symlink', 'export')) == expected_paths[:1]
    (infra/'code').unlink()
    write(infra, 'code/handler.ts', code)
    (infra/'bundle/function.zip').unlink()
    assert executions(capture(binary, infra, run, 'package-removed', 'export')) == expected_paths[1:]
    write(infra, 'bundle/function.zip', 'opaque package restored\n')
    assert executions(capture(binary, infra, run, 'paths-restored', 'export')) == expected_paths
    apps = {}
    for name, target in [('function', 'registry-jobs'), ('service', 'private-blue')]:
        app = repository(run, name); apps[name] = app
        write(app, 'meada.toml', f'[[peer]]\nrepo = "{REMOTE}infra"\n')
        write(app, '.github/workflows/build.yaml', f'jobs:\n  image:\n    steps:\n      - uses: docker/build-push-action@v6\n        with:\n          tags: registry.example.com/probe671/{name}:1\n')
        write(app, 'worker.ts', '''import { SQSClient, SendMessageCommand } from '@aws-sdk/client-sqs';
const client = new SQSClient({});
export function enqueue() { return client.send(new SendMessageCommand({QueueUrl: process.env.JOBS})); }
''')
    peers = ''.join(f'[[peer]]\nrepo = "{REMOTE}{name}"\n' for name in apps)
    write(infra, 'meada.toml', peers+config)
    for name, target in [('function', 'registry-jobs'), ('service', 'private-blue')]:
        appvalue = capture(binary, apps[name], run, name+'-environment', 'export')
        assert sorted(x['contract'] for x in appvalue.get('providers', [])) == ['channel::sqs::'+target, 'image::registry.example.com/probe671/'+name], appvalue
        assert all('JOBS' in x.get('via', '') for x in appvalue['providers'] if x['contract'].startswith('channel::')), appvalue
    # A real operator binding must route a real publish; a same-named Exchange is not a queue.
    objects = [
        {'kind':'Queue','metadata':{'name':'queue-object'},'spec':{'name':'operator-jobs'}},
        {'kind':'Exchange','metadata':{'name':'exchange-object'},'spec':{'name':'operator-events','type':'topic'}},
        {'kind':'Binding','metadata':{'name':'binding-object'},'spec':{'source':'operator-events','destination':'operator-jobs','destinationType':'queue','routingKey':'job.*'}},
        {'kind':'Binding','metadata':{'name':'unread-binding'},'spec':{'source':'missing','destination':'operator-jobs','destinationType':'queue','routingKey':'job.*'}},
    ]
    for i,obj in enumerate(objects):
        obj['apiVersion']='rabbitmq.com/v1beta1'
        obj['spec']['rabbitmqClusterReference']={'name':'broker'}
        write(infra, f'rabbit-{i}.json', json.dumps(obj))
    write(infra, 'rabbit.ts', '''import amqp from 'amqplib';
export async function produce() { const connection = await amqp.connect('amqp://localhost'); const channel = await connection.createChannel(); channel.publish('operator-events', 'job.created', Buffer.from('x')); }
export async function receive() { const connection = await amqp.connect('amqp://localhost'); const channel = await connection.createChannel(); channel.consume('operator-jobs', message => console.log(message)); }
''')
    routed = capture(binary, infra, run, 'routed', 'export')
    rabbitrow = ('channel::amqp::operator-jobs', REMOTE+'infra::rabbit-0.json::Queue.rabbitmq.com/queue-object', '.')
    assert provisioners(routed) == sorted(wanted+[rabbitrow]), routed
    assert [(b['contract'], b['exchange'], b['pattern']) for b in routed.get('bindings', [])] == [('channel::amqp::operator-jobs', 'operator-events', 'job.*')], routed
    assert sorted((l['contract'], l['level']) for l in routed.get('links', [])) == [('channel::amqp::operator-jobs', 'inferred'), ('image::registry.example.com/probe671/function', 'exact'), ('image::registry.example.com/probe671/service', 'exact')], routed
    assert len(routed.get('provisions', [])) == 4, routed
    queries = [
        {'tool':'context','arguments':{'symbol':'module.private','budget':12000}},
        {'tool':'context','arguments':{'symbol':'Binding.rabbitmq.com/binding-object','budget':12000}},
        {'tool':'trace','arguments':{'from':'produce','direction':'up','budget':12000}},
        {'tool':'trace','arguments':{'from':'Queue.rabbitmq.com/queue-object','direction':'down','budget':12000}},
        {'tool':'context','arguments':{'symbol':'module.function','budget':12000}},
    ]
    route_run=run/'module-transports'; route_run.mkdir()
    texts=transports(binary, {'infra':infra,**apps}, infra, queries, route_run, args.http)
    for text, needle in zip(texts, ['var.stage', 'Exchange.rabbitmq.com/exchange-object', 'Queue.rabbitmq.com/queue-object', 'receive', 'registry.example.com/probe671/function']): assert needle in text, text
    capture(binary, infra, run, 'status', 'status')
    write(infra, 'meada.toml', peers)
    removed=capture(binary, infra, run, 'mapping-removed', 'export')
    assert provisioners(removed)==sorted([r for r in wanted if '::registry-' in r[0]]+[rabbitrow]), removed
    write(infra, 'meada.toml', peers+config)
    assert provisioners(capture(binary, infra, run, 'mapping-restored', 'export'))==provisioners(routed)
    write(infra, 'meada.toml', peers+config.replace('terraform:aws_sqs_queue','terraform:not_a_type',1))
    error=capture(binary, infra, run, 'unknown-entry', 'export', success=False)
    assert 'terraform:not_a_type' in error and 'meada.toml' in error, error
    write(infra, 'meada.toml', peers+config)
    saved(run, 'checks.json', {'checks':'passed','independent_verdict':False,'answer_keys':['#671','#642','ADR0025/26'],'surfaces':['CLI','MCP stdio']+(['MCP HTTP'] if args.http else [])})


if __name__ == '__main__': main()
