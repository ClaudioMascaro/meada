#!/usr/bin/env python3
"""Independent #659 public-surface checks, with #642 and base ADR 0026 as oracle.

Inputs and expected semantic rows are written before the first application call.
Only transport helpers are reused; feature fixtures, goldens and implementation are not read.
Exit 0 means checks passed, 1 observed mismatch, 2 inaccessible surface/checker failure.
The independent checker must inspect the evidence before writing a PROVED receipt.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import socket
import subprocess
import time
import traceback
from prove import git, free_port, Http, answer, mcp_answers
from console import http, query_equal

REMOTE = 'example.com/neutral-sdk/'
# SDK service, JS constructor stem, Terraform type/name field, contract kind/name.
SERVICES = {
    'sns': ('SNS', 'aws_sns_topic', 'name', 'channel::sns', 'dispatch'),
    'events': ('EventBridge', 'aws_cloudwatch_event_bus', 'name', 'channel::eventbridge', 'audit-bus'),
    'kinesis': ('Kinesis', 'aws_kinesis_stream', 'name', 'channel::kinesis', 'audit-stream'),
    's3': ('S3', 'aws_s3_bucket', 'bucket', 'resource::s3', 'audit-bucket'),
    'dynamodb': ('DynamoDB', 'aws_dynamodb_table', 'name', 'resource::dynamodb', 'audit-table'),
    'ssm': ('SSM', 'aws_ssm_parameter', 'name', 'resource::ssm', '/audit/setting'),
    'secretsmanager': ('SecretsManager', 'aws_secretsmanager_secret', 'name', 'resource::secretsmanager', 'audit/credential'),
}
# Independently selected AWS low-level operation/argument matrix, not a feature fixture.
OPERATIONS = [
    ('sns', 'Publish', 'publish', 'TopicArn', 'providers'),
    ('events', 'PutEvents', 'put_events', 'EventBusName', 'providers'),
    ('kinesis', 'PutRecord', 'put_record', 'StreamName', 'providers'),
    ('kinesis', 'PutRecords', 'put_records', 'StreamName', 'providers'),
    ('s3', 'PutObject', 'put_object', 'Bucket', 'providers'),
    ('s3', 'GetObject', 'get_object', 'Bucket', 'consumers'),
    ('dynamodb', 'PutItem', 'put_item', 'TableName', 'providers'),
    ('dynamodb', 'UpdateItem', 'update_item', 'TableName', 'providers'),
    ('dynamodb', 'DeleteItem', 'delete_item', 'TableName', 'providers'),
    ('dynamodb', 'BatchWriteItem', 'batch_write_item', 'RequestItems', 'providers'),
    ('dynamodb', 'TransactWriteItems', 'transact_write_items', 'TransactItems', 'providers'),
    ('dynamodb', 'TransactWriteItems', 'condition_check', 'ConditionCheck', 'consumers'),
    ('dynamodb', 'GetItem', 'get_item', 'TableName', 'consumers'),
    ('dynamodb', 'BatchGetItem', 'batch_get_item', 'RequestItems', 'consumers'),
    ('dynamodb', 'Query', 'query', 'TableName', 'consumers'),
    ('dynamodb', 'Scan', 'scan', 'TableName', 'consumers'),
    ('dynamodb', 'TransactGetItems', 'transact_get_items', 'TransactItems', 'consumers'),
    ('ssm', 'GetParameter', 'get_parameter', 'Name', 'consumers'),
    ('ssm', 'GetParameters', 'get_parameters', 'Names', 'consumers'),
    ('ssm', 'GetParametersByPath', 'get_parameters_by_path', 'Path', 'consumers'),
    ('secretsmanager', 'GetSecretValue', 'get_secret_value', 'SecretId', 'consumers'),
    ('secretsmanager', 'BatchGetSecretValue', 'batch_get_secret_value', 'SecretIdList', 'consumers'),
]


def write(root, relative, value):
    target = root / relative
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(value)


def dump(root, relative, value):
    write(root, relative, json.dumps(value, indent=2) + '\n')


def digest(path):
    value = hashlib.sha256()
    with open(path, 'rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            value.update(chunk)
    return value.hexdigest()


def arguments(service, operation, key, value, language):
    if service == 'sns':
        name = value if value == '__ENVIRONMENT_NAME__' else 'arn:aws:sns:eu-west-1:123456789012:' + value
        body = {'TopicArn': name, 'Message': 'audit'}
    elif service == 'events':
        body = {'Entries': [{'EventBusName': value, 'Source': 'audit', 'DetailType': 'entry', 'Detail': '{}'}]}
    elif key == 'RequestItems':
        body = {key: {value: [{'PutRequest': {'Item': {'id': {'S': 'x'}}}}]
                      if operation == 'BatchWriteItem' else {'Keys': [{'id': {'S': 'x'}}]}}}
    elif key == 'ConditionCheck':
        body = {'TransactItems': [{'ConditionCheck': {'TableName': value, 'Key': {'id': {'S': 'x'}},
                                                    'ConditionExpression': 'attribute_exists(id)'}}]}
    elif key == 'TransactItems':
        writer = operation == 'TransactWriteItems'
        body = {key: [{'Put' if writer else 'Get': {'TableName': value,
                      'Item' if writer else 'Key': {'id': {'S': 'x'}}}}]}
    else:
        body = {key: [value] if key in ['Names', 'SecretIdList'] else value}
        if service == 'kinesis':
            body.update({'Data': 'audit', 'PartitionKey': 'audit'} if operation == 'PutRecord'
                        else {'Records': [{'Data': 'audit', 'PartitionKey': 'audit'}]})
        elif service == 's3':
            body['Key'] = 'entry.json'
            if operation == 'PutObject':
                body['Body'] = '{}'
        elif service == 'dynamodb' and operation in ['PutItem', 'UpdateItem', 'DeleteItem', 'GetItem']:
            body['Item' if operation == 'PutItem' else 'Key'] = {'id': {'S': 'x'}}
            if operation == 'UpdateItem':
                body['UpdateExpression'] = 'SET audit = :value'
                body['ExpressionAttributeValues'] = {':value': {'S': 'x'}}
        elif service == 'dynamodb' and operation == 'Query':
            body['KeyConditionExpression'] = 'id = :value'
            body['ExpressionAttributeValues'] = {':value': {'S': 'x'}}
    if language == 'py':
        return ', '.join(f'{k}={repr(v)}' for k, v in body.items())
    return json.dumps(body)


def seed(run):
    infra, app, peer, control = [run / 'work' / name for name in ['infra', 'app', 'peer', 'control']]
    for name, root in [('infra', infra), ('app', app), ('peer', peer), ('control', control)]:
        root.mkdir(parents=True)
        git('init', '-q', cwd=root)
        git('remote', 'add', 'origin', f'https://{REMOTE}{name}.git', cwd=root)
    for name, root in [('infra', infra), ('app', app), ('peer', peer)]:
        write(root, 'meada.toml', ''.join(f'[[peer]]\nrepo = "{REMOTE}{other}"\n'
                                         for other in ['infra', 'app', 'peer'] if other != name))
    tf = ''
    for service, (_, typ, attr, _, name) in SERVICES.items():
        tf += f'resource "{typ}" "{service}" {{ {attr} = {json.dumps(name)} }}\n'
    write(infra, 'main.tf', tf)
    expected, refused = [], []
    for language in ['ts', 'js', 'py']:
        code = 'import boto3 as cloud\nimport os\n' if language == 'py' else ''
        for service, (stem, _, _, _, _) in SERVICES.items():
            if language == 'py':
                code += f'{service}_client = cloud.client({service!r})\n'
            else:
                package = {'events': 'eventbridge', 'secretsmanager': 'secrets-manager'}.get(service, service)
                commands = ', '.join(dict.fromkeys(op + 'Command as C' + op for svc, op, *_ in OPERATIONS if svc == service))
                code += f'import {{ {stem}Client as Origin{stem}, {commands} }} from "@aws-sdk/client-{package}";\n'
                code += f'const {service}_client = new Origin{stem}({{}});\n'
        for service, operation, pyop, key, role in OPERATIONS:
            stem, _, _, prefix, name = SERVICES[service]
            symbol = f'{language}_{pyop}'
            args = arguments(service, operation, key, name, language)
            method = 'transact_write_items' if pyop == 'condition_check' else pyop
            call = f'{service}_client.{method}({args})' if language == 'py' else f'{service}_client.send(new C{operation}({args}))'
            code += f'\ndef {symbol}():\n    return {call}\n' if language == 'py' else f'\nexport function {symbol}() {{ return {call}; }}\n'
            unresolved = operation == 'GetParametersByPath'
            expected.append({'file': f'app.{language}', 'symbol': symbol, 'role': role,
                             'contract': prefix if unresolved else f'{prefix}::{name}',
                             'operation': operation, 'environment': None, 'unresolved': unresolved, 'via': 'sdk:aws'})
        # Each service has a literal sibling and an unresolved environment side.
        for service in SERVICES:
            service, operation, pyop, key, role = next(o for o in OPERATIONS if o[0] == service and o[3] not in ['RequestItems', 'TransactItems'])
            prefix = SERVICES[service][3]
            variable, marker = f'NEUTRAL_{language.upper()}_{service.upper()}', '__ENVIRONMENT_NAME__'
            args = arguments(service, operation, key, marker, language)
            expression = f'os.environ[{variable!r}]' if language == 'py' else f'process.env.{variable}'
            args = args.replace(repr(marker) if language == 'py' else json.dumps(marker), expression)
            symbol = f'{language}_env_{service}'
            call = f'{service}_client.{pyop}({args})' if language == 'py' else f'{service}_client.send(new C{operation}({args}))'
            code += f'\ndef {symbol}():\n    return {call}\n' if language == 'py' else f'\nexport function {symbol}() {{ return {call}; }}\n'
            expected.append({'file': f'app.{language}', 'symbol': symbol, 'role': role,
                             'contract': prefix, 'operation': operation, 'environment': variable, 'unresolved': True})
            expected[-1]['via'] = 'env:' + variable
        # Origin and hidden-name refusals: all contain a real supported operation/name.
        if language == 'py':
            controls = {
                'parameter': 'def py_refuse_parameter(s3_client):\n    return s3_client.get_object(Bucket="audit-bucket")\n',
                'assignment': 'def py_refuse_assignment():\n    s3_client = unrelated\n    return s3_client.get_object(Bucket="audit-bucket")\n',
                'tuple': 'def py_refuse_tuple():\n    s3_client, other = pair\n    return s3_client.get_object(Bucket="audit-bucket")\n',
                'loop': 'def py_refuse_loop():\n    for s3_client in clients:\n        s3_client.get_object(Bucket="audit-bucket")\n',
                'with': 'def py_refuse_with():\n    with resource as s3_client:\n        s3_client.get_object(Bucket="audit-bucket")\n',
                'except': 'def py_refuse_except():\n    try:\n        action()\n    except Exception as s3_client:\n        s3_client.get_object(Bucket="audit-bucket")\n',
                'import': 'def py_refuse_import():\n    import unrelated as s3_client\n    return s3_client.get_object(Bucket="audit-bucket")\n',
                'walrus': 'def py_refuse_walrus():\n    if (s3_client := unrelated):\n        return s3_client.get_object(Bucket="audit-bucket")\n',
                'constructor': 'def py_refuse_constructor(cloud):\n    local = cloud.client("s3")\n    return local.get_object(Bucket="audit-bucket")\n',
                'hop': 'class Factory:\n    def make_client(self):\n        return cloud.client("s3")\ndef py_refuse_hop(receiver):\n    distant = receiver.make_client()\n    return distant.get_object(Bucket="audit-bucket")\n',
                'unknown': 'def py_refuse_unknown():\n    local = unrelated.client("s3")\n    return local.get_object(Bucket="audit-bucket")\n',
                'wrong_service': 'def py_refuse_wrong_service():\n    return s3_client.publish(TopicArn="arn:aws:sns:eu-west-1:123456789012:dispatch", Message="audit")\n',
            }
        else:
            controls = {
                'parameter': f'export function {language}_refuse_parameter(s3_client) {{ return s3_client.send(new CGetObject({{Bucket: "audit-bucket"}})); }}\n',
                'catch': f'export function {language}_refuse_catch() {{ try {{ action(); }} catch (s3_client) {{ s3_client.send(new CGetObject({{Bucket: "audit-bucket"}})); }} }}\n',
                'object': f'export function {language}_refuse_object(value) {{ const {{ s3_client }} = value; return s3_client.send(new CGetObject({{Bucket: "audit-bucket"}})); }}\n',
                'array': f'export function {language}_refuse_array(value) {{ const [s3_client] = value; return s3_client.send(new CGetObject({{Bucket: "audit-bucket"}})); }}\n',
                'constructor': f'export function {language}_refuse_constructor(OriginS3) {{ const local = new OriginS3({{}}); return local.send(new CGetObject({{Bucket: "audit-bucket"}})); }}\n',
                'command': f'export function {language}_refuse_command(CGetObject) {{ return s3_client.send(new CGetObject({{Bucket: "audit-bucket"}})); }}\n',
                'command_catch': f'export function {language}_refuse_command_catch() {{ try {{ action(); }} catch (CGetObject) {{ s3_client.send(new CGetObject({{Bucket: "audit-bucket"}})); }} }}\n',
                'command_object': f'export function {language}_refuse_command_object(value) {{ const {{ CGetObject }} = value; return s3_client.send(new CGetObject({{Bucket: "audit-bucket"}})); }}\n',
                'hop': f'class Factory {{ make_client() {{ return new OriginS3({{}}); }} }}\nexport function {language}_refuse_hop(receiver) {{ const distant = receiver.make_client(); return distant.send(new CGetObject({{Bucket: "audit-bucket"}})); }}\n',
                'unknown': f'export function {language}_refuse_unknown() {{ const local = new UnknownS3({{}}); return local.send(new CGetObject({{Bucket: "audit-bucket"}})); }}\n',
                'wrong_service': f'export function {language}_refuse_wrong_service() {{ return sns_client.send(new CGetObject({{Bucket: "audit-bucket"}})); }}\n',
            }
        for name, text in controls.items():
            code += '\n' + text
            refused.append({'file': f'app.{language}', 'symbol': f'{language}_refuse_{name}'})
        write(app, f'app.{language}', code)
    write(peer, 'app.py', 'import boto3\ns3_client = boto3.client("s3")\ndef peer_put_object():\n    return s3_client.put_object(Bucket="audit-bucket", Key="peer.json", Body="{}")\n')
    expected.append({'repository': 'peer', 'file': 'app.py', 'symbol': 'peer_put_object', 'role': 'providers',
                     'contract': 'resource::s3::audit-bucket', 'operation': 'PutObject',
                     'environment': None, 'unresolved': False, 'via': 'sdk:aws'})
    write(control, 'app.py', 'import boto3\nqueue = boto3.client("sqs")\ndef send_job():\n    queue.send_message(QueueUrl="https://sqs.eu-west-1.amazonaws.com/123456789012/jobs", MessageBody="x")\ndef read_job():\n    return queue.receive_message(QueueUrl="https://sqs.eu-west-1.amazonaws.com/123456789012/jobs")\n')
    write(control, 'app.ts', 'import { SQSClient, SendMessageCommand } from "@aws-sdk/client-sqs";\nconst q = new SQSClient({});\nexport function send_ts() { return q.send(new SendMessageCommand({QueueUrl:"https://sqs.eu-west-1.amazonaws.com/123456789012/jobs",MessageBody:"x"})); }\n')
    inventory = []
    for root in [infra, app, peer, control]:
        for path in sorted(root.iterdir()):
            if path.is_file():
                inventory.append({'path': str(path.relative_to(run)), 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()})
        git('add', '.', cwd=root)
        git('commit', '-qm', 'neutral public SDK probe', cwd=root)
    dump(run, 'oracle.json', {'authority': '#659/#642, base ADR 0026, independently selected AWS low-level argument forms',
                             'expected': expected, 'refused': refused, 'services': SERVICES, 'inputs': inventory})
    return infra, app, peer, control, expected, refused


class Probe:
    def __init__(self, run):
        self.run, self.checks = run, []

    def check(self, name, passed, detail=''):
        self.checks.append({'check': name, 'passed': bool(passed), 'detail': detail})
        dump(self.run, 'checks.json', self.checks)
        print(('PASS ' if passed else 'FAIL ') + name, flush=True)

    def command(self, binary, cwd, label, *args):
        result = subprocess.run([binary, *args], cwd=cwd, capture_output=True, timeout=120)
        path = self.run / label
        write(self.run, label + '.out', result.stdout.decode())
        write(self.run, label + '.err', result.stderr.decode())
        write(self.run, label + '.exit', str(result.returncode))
        if result.returncode:
            raise RuntimeError(f'{label} exit {result.returncode}: {result.stderr.decode()}')
        return result.stdout


def semantic_sides(data):
    return [(role, side) for role in ['providers', 'consumers'] for side in data[role]]


def belongs(site, row):
    prefix = f'{REMOTE}{row.get("repository", "app")}::{row["file"]}::{row["symbol"]}'
    return site == prefix or site.startswith(prefix + '::')


def inspect(probe, data, expected, refused):
    sides, mapped = semantic_sides(data), []
    for row in expected:
        found = [(role, side) for role, side in sides if belongs(side['site'], row)]
        valid = (len(found) == 1 and found[0][0] == row['role'] and found[0][1]['contract'] == row['contract']
                 and found[0][1].get('via') == row['via'])
        probe.check(f'{row["file"]} {row["symbol"]}: one {row["role"]} side with correct identity', valid, found)
        if valid:
            mapped.append((row, found[0][1]['site']))
    probe.check('complete SDK side count, including unresolved environment forms', len(sides) == len(expected),
                {'actual': len(sides), 'expected': len(expected)})
    for row in refused:
        found = [(role, side) for role, side in sides if belongs(side['site'], row)]
        probe.check(f'{row["file"]} {row["symbol"]}: origin/hidden name refused', not found, found)
    resolved = [(row, site) for row, site in mapped if not row['unresolved']]
    expected_provisions = {(site, row['contract']) for row, site in resolved}
    provisions = data.get('provisions', [])
    probe.check('complete provisioning link set and no unresolved/refused provision',
                len(provisions) == len(resolved) and {(p['side'], p['contract']) for p in provisions} == expected_provisions,
                {'actual': len(provisions), 'expected': len(resolved)})
    probe.check('every provisioning link exact and attached to the matching IaC node', all(
        p['level'] == 'exact' and any(p['contract'] == f'{prefix}::{name}' and
        p['provisioner'] == f'{REMOTE}infra::main.tf::{typ}.{service}' and p['root'] == '.'
        for service, (_, typ, _, prefix, name) in SERVICES.items()) for p in provisions), provisions)
    expected_links = {(consumer, provider, c['contract']) for c, consumer in resolved for p, provider in resolved
                      if c['role'] == 'consumers' and p['role'] == 'providers' and c['contract'] == p['contract']}
    links = data['links']
    probe.check('complete reader/writer link set, correct direction and exact levels', len(links) == len(expected_links)
                and {(l['consumer'], l['provider'], l['contract']) for l in links} == expected_links
                and all(l['level'] == 'exact' for l in links), {'actual': len(links), 'expected': len(expected_links)})
    return mapped


def wait_server(server, port):
    for _ in range(150):
        if server.poll() is not None:
            raise RuntimeError(f'server exited {server.returncode}')
        try:
            socket.create_connection(('127.0.0.1', port), timeout=1).close()
            return
        except OSError:
            time.sleep(.1)
    raise RuntimeError('server did not become ready')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--head-bin', required=True)
    parser.add_argument('--base-bin', required=True)
    parser.add_argument('--run', required=True)
    parser.add_argument('--ui-bin')
    parser.add_argument('--ui-dir')
    args = parser.parse_args()
    os.environ['LANG'] = 'C.UTF-8'
    run = Path(args.run).resolve()
    run.mkdir(parents=True, exist_ok=False)
    probe = Probe(run)
    binary, base = str(Path(args.head_bin).resolve()), str(Path(args.base_bin).resolve())
    try:
        infra, app, peer, control, expected, refused = seed(run)
        # ADR 0025/0026 forbid process-environment resolution. Explicit values that
        # would match real provisioners make an accidental runtime lookup observable.
        for row in expected:
            if row['environment']:
                service = row['symbol'].split('_env_', 1)[1]
                name = SERVICES[service][4]
                os.environ[row['environment']] = ('arn:aws:sns:eu-west-1:123456789012:' + name
                                                 if service == 'sns' else name)
        dump(run, 'identity.json', {'head': binary, 'base': base,
             'head_sha256': digest(binary), 'base_sha256': digest(base),
             'process_environment_probe': 'all 21 NEUTRAL_* variables explicitly set to matching physical names; expected unresolved'})
        probe.command(binary, run, 'head-version', '--version')
        probe.command(base, run, 'base-version', '--version')
        head = {name: json.loads(probe.command(binary, root, f'head-{name}-export', 'export'))
                for name, root in [('infra', infra), ('app', app), ('peer', peer)]}
        probe.check('export format remains 1', all(data['format'] == 1 for data in head.values()))
        combined = dict(head['app'])
        for role in ['providers', 'consumers']:
            combined[role] = head['app'][role] + head['peer'][role] + head['infra'][role]
        # Exports project links touching their repository. Infra owns every
        # provisioner; app owns all readers. These give complete global arrays.
        combined['provisions'] = head['infra'].get('provisions', [])
        mapped = inspect(probe, combined, expected, refused)
        for name in ['app', 'peer', 'infra']:
            identity = REMOTE + name
            for role in ['providers', 'consumers']:
                probe.check(f'{name}: complete {role} array contains only owned sites',
                            all(side['site'].split('::')[0] == identity for side in head[name][role]), head[name][role])
            for key, endpoints in [('links', ['consumer', 'provider']), ('provisions', ['side', 'provisioner'])]:
                complete = combined[key]
                wanted = [link for link in complete if any(link[field].split('::')[0] == identity for field in endpoints)]
                probe.check(f'{name}: exact complete repository projection of {key}', head[name].get(key, []) == wanted,
                            {'actual': len(head[name].get(key, [])), 'expected': len(wanted)})
        for language in ['ts', 'js', 'py']:
            hops = [call for call in head['app']['calls'] if f'::{language}_refuse_hop::' in call['id'] and 'receiver.make_client' in call['id']]
            probe.check(language + ': hop refusal actually exercises inferred/ambiguous name resolution',
                        len(hops) == 1 and hops[0].get('level') in ['inferred', 'ambiguous'], hops)
        provisioners = head['infra'].get('provisioners', [])
        expected_provisioners = {(f'{prefix}::{name}', f'{REMOTE}infra::main.tf::{typ}.{service}')
                                for service, (_, typ, _, prefix, name) in SERVICES.items()}
        probe.check('complete seven provisioner identities', len(provisioners) == 7 and
                    {(p['contract'], p['node']) for p in provisioners} == expected_provisioners, provisioners)
        status = probe.command(binary, app, 'head-app-status', 'status').decode()
        probe.check('all literal uses have provisioners, no orphan status row', not re.search(r'^\s*orphan ', status, re.M), status)
        queries = []
        for row, _ in mapped:
            queries.append({'tool': 'context', 'arguments': {'symbol': row['symbol'], 'budget': 16000}})
        for service, (_, typ, _, _, _) in SERVICES.items():
            address = f'{typ}.{service}'
            queries += [{'tool': 'context', 'arguments': {'symbol': address, 'budget': 16000}},
                        {'tool': 'trace', 'arguments': {'from': address, 'direction': 'down',
                                                       'budget': 32000 if service == 'dynamodb' else 16000}}]
        literal_sites = [(row['symbol'], site) for row, site in mapped if not row['unresolved']]
        queries += [{'tool': 'trace', 'arguments': {'from': site, 'direction': 'up', 'budget': 16000}}
                    for _, site in literal_sites]
        queries += [{'tool': 'search', 'arguments': {'query': 'NEUTRAL_no_such_contract', 'kind': 'resource', 'budget': 1000}},
                    {'tool': 'search', 'arguments': {'query': 'sdk', 'kind': 'not-a-kind', 'budget': 1000}}]
        dump(run, 'queries.json', queries)
        tools, texts, _ = mcp_answers(binary, app, queries, run / 'stdio.err')
        dump(run, 'stdio.json', texts)
        for i, (row, _) in enumerate(mapped):
            text = texts[i]
            if row['environment']:
                probe.check(row['symbol'] + ': context names unresolved environment variable',
                            row['environment'] in text and 'unresolved' in text, text)
            elif row['unresolved']:
                probe.check(row['symbol'] + ': hierarchy is an unresolved reader, never a same-named parameter',
                            'unresolved' in text, text)
            else:
                probe.check(row['symbol'] + ': context shows identity and provisioner',
                            row['contract'] in text and 'provisioner' in text, text)
        offset = len(mapped)
        for i, (service, (_, typ, _, prefix, name)) in enumerate(SERVICES.items()):
            context, trace = texts[offset + 2 * i:offset + 2 * i + 2]
            symbols = [row['symbol'] for row, _ in mapped if row['contract'] == f'{prefix}::{name}']
            probe.check(service + ': IaC context and downward trace reach every literal SDK side',
                        f'{prefix}::{name}' in context and all(symbol in context and symbol in trace for symbol in symbols),
                        {'context': context, 'trace': trace})
        offset += 2 * len(SERVICES)
        for i, (symbol, _) in enumerate(literal_sites):
            probe.check(symbol + ': upward trace reaches provisioning link', 'provisions ' in texts[offset + i], texts[offset + i])
        # Existing SQS/code-only flows answer to the authorized base on identical files.
        controls = [{'tool': 'context', 'arguments': {'symbol': symbol, 'budget': 8000}}
                    for symbol in ['send_job', 'read_job', 'send_ts']]
        controls += [{'tool': 'trace', 'arguments': {'from': 'read_job', 'direction': 'up', 'budget': 8000}}]
        old_bytes = probe.command(base, control, 'base-control-export', 'export')
        old_status = probe.command(base, control, 'base-control-status', 'status')
        old_tools, old_texts, _ = mcp_answers(base, control, controls, run / 'base-control-stdio.err')
        new_bytes = probe.command(binary, control, 'head-control-export', 'export')
        new_status = probe.command(binary, control, 'head-control-status', 'status')
        new_tools, new_texts, _ = mcp_answers(binary, control, controls, run / 'head-control-stdio.err')
        dump(run, 'base-control-stdio.json', old_texts)
        dump(run, 'head-control-stdio.json', new_texts)
        probe.check('existing code-only SQS export byte parity', old_bytes == new_bytes)
        probe.check('existing code-only SQS status byte parity', old_status == new_status)
        probe.check('existing SQS MCP answers and tool schemas byte parity', old_texts == new_texts and old_tools == new_tools == tools)
        manifest = 'index_dir = "indexes"\n'
        for name, root in [('infra', infra), ('app', app), ('peer', peer)]:
            manifest += f'\n[[repository]]\nrepo = "{REMOTE}{name}"\npath = "{root}"\n'
        write(run, 'workspace.toml', manifest)
        probe.command(binary, run, 'workspace-index', 'index', '--workspace', 'workspace.toml')
        port = free_port()
        server = subprocess.Popen([binary, 'mcp', '--listen', f'127.0.0.1:{port}', '--workspace', 'workspace.toml'],
                                  cwd=run, stdin=subprocess.DEVNULL, stdout=open(run / 'http.out', 'w'),
                                  stderr=open(run / 'http.err', 'w'))
        write(run, 'http.pid', str(server.pid))
        try:
            wait_server(server, port)
            client = Http(f'http://127.0.0.1:{port}/{REMOTE}app')
            http_tools = client.call('tools/list', {})['result']['tools']
            answers = [answer(client.call('tools/call', {'name': q['tool'], 'arguments': q['arguments']})) for q in queries]
            dump(run, 'http.json', answers)
            probe.check('complete MCP HTTP/stdin answer and error parity', answers == texts and http_tools == tools)
        finally:
            server.terminate()
            server.wait(timeout=30)
            write(run, 'http.stopped', str(server.returncode))
        if args.ui_bin and args.ui_dir:
            console(probe, args, run, manifest, queries, texts)
        else:
            probe.check('console API and loaded graph supplied for A7 surface', False, 'requires --ui-bin and --ui-dir')
        failed = [check for check in probe.checks if not check['passed']]
        dump(run, 'result.json', {'result': 'checks passed' if not failed else 'BROKEN', 'failed': len(failed), 'checks': len(probe.checks)})
        return 1 if failed else 0
    except Exception:
        write(run, 'blocked.txt', traceback.format_exc())
        print(traceback.format_exc(), flush=True)
        return 2


def console(probe, args, run, manifest, queries, texts):
    binary, ui = str(Path(args.ui_bin).resolve()), str(Path(args.ui_dir).resolve())
    dump(run, 'console-identity.json', {'binary': binary, 'sha256': digest(binary), 'ui_dir': ui,
                                     'ui_manifest': json.loads(Path(ui, 'meada-ui.json').read_text())})
    probe.command(binary, run, 'console-version', '--version')
    port = free_port()
    url = f'http://127.0.0.1:{port}'
    write(run, 'console.workspace.toml', manifest + f'\n[ui]\ndir = "{ui}"\npublic_url = "{url}"\n')
    probe.command(binary, run, 'console-index', 'index', '--workspace', 'console.workspace.toml')
    server = subprocess.Popen([binary, 'ui', '--listen', f'127.0.0.1:{port}', '--workspace', 'console.workspace.toml'],
                              cwd=run, stdin=subprocess.DEVNULL, stdout=open(run / 'console.out', 'w'),
                              stderr=open(run / 'console.err', 'w'))
    write(run, 'console.pid', str(server.pid))
    write(run, 'console.url', url)
    try:
        wait_server(server, port)
        status, body = http(url + '/api/v1/config')
        dump(run, 'console-config.json', {'status': status, 'body': body.decode()})
        probe.check('console usable: config HTTP 200', status == 200)
        for i, (query, expected) in enumerate(zip(queries, texts)):
            status, body = http(url + '/api/v1/query', 'POST', {'repository': REMOTE + 'app', 'generation': None,
                                      'tool': query['tool'], 'arguments': query['arguments']},
                                {'Origin': url, 'Content-Type': 'application/json'})
            dump(run, f'console-responses/{i}.json', {'query': query, 'status': status, 'body': body.decode()})
            probe.check(f'console query {i}: MCP parity including exact refusal protocol', query_equal(status, json.loads(body), expected))
        for repository in ['app', 'infra', 'peer']:
            status, body = http(f'{url}/api/v1/repositories/{REMOTE}{repository}/export')
            write(run, f'console-{repository}-export.json', body.decode())
            cli = probe.command(binary, run, f'console-{repository}-cli-export', 'export', '--workspace', 'console.workspace.toml', '--repo', REMOTE + repository)
            probe.check(f'console {repository} complete export bytes equal CLI on identical manifest', status == 200 and body == cli)
        script = Path(__file__).with_name('aws-sdk-graph.mjs')
        result = subprocess.run(['node', str(script), str(run)], capture_output=True, timeout=120)
        write(run, 'console-graph.out', result.stdout.decode())
        write(run, 'console-graph.err', result.stderr.decode())
        if not (run / 'console-graph-result.json').exists():
            raise RuntimeError('browser checker did not finish; see console-graph.err')
        probe.check('loaded console graph: complete contract, side, provisioner and provision tables', result.returncode == 0, result.stdout.decode() + result.stderr.decode())
    finally:
        server.terminate()
        server.wait(timeout=30)
        write(run, 'console.stopped', str(server.returncode))


if __name__ == '__main__':
    raise SystemExit(main())
