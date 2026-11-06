#!/usr/bin/env python3
"""Independent public-surface probe for #658, #642 and ADR 0025/0026.

This file deliberately imports only the existing transport mechanics. Its answer key is
maintainer prose and AWS identifier syntax, never the changed type table or fixtures.
Exit 0: checks passed (human still judges proof); 1: BROKEN; 2: BLOCKED.
"""
import argparse
import json
import os
from pathlib import Path
import re
import socket
import shutil
import subprocess
import traceback
import time
from prove import git, free_port, Http, answer, mcp_answers

REMOTE = 'example.com/zz-prove-aws/'
SECRET = 'zz-prove-secure-must-never-appear'
# Terraform type, physical-name attribute, contract prefix, literal, ARN.
CASES = [
    ('aws_sns_topic', 'name', 'channel::sns', 'signal', 'arn:aws:sns:eu-west-1:123456789012:signal'),
    ('aws_cloudwatch_event_bus', 'name', 'channel::eventbridge', 'hub', 'arn:aws:events:eu-west-1:123456789012:event-bus/hub'),
    ('aws_kinesis_stream', 'name', 'channel::kinesis', 'flow', 'arn:aws:kinesis:eu-west-1:123456789012:stream/flow'),
    ('aws_dynamodb_table', 'name', 'resource::dynamodb', 'ledger', 'arn:aws:dynamodb:eu-west-1:123456789012:table/ledger'),
    ('aws_db_instance', 'identifier', 'resource::rds', 'dbnode', 'arn:aws:rds:eu-west-1:123456789012:db:dbnode'),
    ('aws_rds_cluster', 'cluster_identifier', 'resource::rds', 'dbgroup', 'arn:aws:rds:eu-west-1:123456789012:cluster:dbgroup'),
    ('aws_elasticache_cluster', 'cluster_id', 'resource::elasticache', 'cacheone', 'arn:aws:elasticache:eu-west-1:123456789012:cluster:cacheone'),
    ('aws_elasticache_replication_group', 'replication_group_id', 'resource::elasticache', 'cachegroup', 'arn:aws:elasticache:eu-west-1:123456789012:replicationgroup:cachegroup'),
    ('aws_ssm_parameter', 'name', 'resource::ssm', '/service/token', 'arn:aws:ssm:eu-west-1:123456789012:parameter/service/token'),
    ('aws_secretsmanager_secret', 'name', 'resource::secretsmanager', 'service/login', 'arn:aws:secretsmanager:eu-west-1:123456789012:secret:service/login-AbCdEf'),
]


def write(root, path, value):
    target = root / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(value)


def block(typ, label, body):
    return f'resource "{typ}" "{label}" {{\n{body}\n}}\n'


def orphan_set_matches(text, expected):
    """#657 format, exact set AND row count; reject the superseded reporter too."""
    lines = [line for line in text.splitlines() if line.strip().startswith('orphan ')]
    got = {(m.group(1), m.group(2)) for line in lines
           if (m := re.search(r'^\s*orphan (\S+): (no use|no provisioner);', line))}
    return (got == set(expected.items()) and len(lines) == len(expected)
            and ': provisioned without use' not in text
            and ': no provisioner in the workspace' not in text)


def expected_orphans():
    # Derived solely from the generated neutral source: each AWS group has one
    # explicit unused provisioner and missing use. The default EventBridge source
    # intentionally has no bus provisioner. All generated SQS queues have uses.
    expected = {}
    for prefix in dict.fromkeys(case[2] for case in CASES):
        kind = prefix.split('::')[1]
        expected[f'{prefix}::unused-{kind}'] = 'no use'
        expected[f'{prefix}::missing-{kind}'] = 'no provisioner'
    expected['channel::eventbridge::default'] = 'no provisioner'
    return expected


def document(broker, names):
    return json.dumps({'asyncapi': '2.6.0', 'info': {'title': 'Independent AWS producer', 'version': '1'},
                       'servers': {'aws': {'url': 'localhost', 'protocol': broker}},
                       'channels': {name: {'subscribe': {'message': {'payload': {'type': 'object'}}}}
                                    for name in names}}, indent=2)


def seed(run):
    infra, app = run / 'work/infra', run / 'work/app'
    for name, root in [('infra', infra), ('app', app)]:
        root.mkdir(parents=True)
        git('init', '-q', cwd=root)
        git('remote', 'add', 'origin', f'https://{REMOTE}{name}.git', cwd=root)
    write(infra, 'meada.toml', f'[[peer]]\nrepo = "{REMOTE}app"\n')
    manifest = '[serve]\ndocuments = ["sns.asyncapi.json", "eventbridge.asyncapi.json", "kinesis.asyncapi.json"]\n'\
        + f'[[peer]]\nrepo = "{REMOTE}infra"\n'
    code = 'import boto3\nqueue = boto3.client("sqs")\n'
    tf, expected, bindings = '', {}, []
    channel_names = {kind: [] for kind in ['sns', 'eventbridge', 'kinesis']}
    # Two provisioners per identity exercise ARN extraction independently of name matching.
    for i, (typ, attr, prefix, name, arn) in enumerate(CASES):
        contract = f'{prefix}::{name}'
        for label, value in [('literal', name), ('arn', arn)]:
            address = f'{typ}.{label}'
            extra = '\n  type = "SecureString"\n  value = "' + SECRET + '"' if typ == 'aws_ssm_parameter' else ''
            tf += block(typ, label, f'  {attr} = {json.dumps(value)}' + extra)
            expected[address] = contract
        if prefix.startswith('channel'):
            channel_names[prefix.split('::')[1]].append(name)
        else:
            kind = prefix.split('::')[1]
            symbol = f'read_case_{i}'
            code += f'\ndef {symbol}():\n    return "reader-{i}"\n'
            manifest += f'\n[[resource]]\nkind = "{kind}"\nid = "{name}"\nrole = "read"\nvia = ["app.py::{symbol}"]\n'
    # Every new kind has a provisioned-but-unused and a used-but-unprovisioned name.
    for prefix in dict.fromkeys(case[2] for case in CASES):
        typ, attr, _, _, _ = next(case for case in CASES if case[2] == prefix)
        kind = prefix.split('::')[1]
        name = 'unused-' + kind
        tf += block(typ, 'orphan', f'  {attr} = "{name}"')
        expected[f'{typ}.orphan'] = f'{prefix}::{name}'
        if prefix.startswith('channel'):
            channel_names[kind].append('missing-' + kind)
        else:
            manifest += f'\n[[resource]]\nkind = "{kind}"\nid = "missing-{kind}"\nrole = "read"\n'
    # Refused forms remain searchable IaC nodes, but hold no provisioned contract.
    tf += 'data "aws_sns_topic" "read_only" { name = "signal" }\n'
    tf += block('aws_unrecognized_service', 'unknown', '  name = "signal"')
    tf += block('aws_sns_topic', 'wrong_service', '  name = "arn:aws:sqs:eu-west-1:123456789012:signal"')
    tf += block('aws_dynamodb_table', 'wrong_service', '  name = "arn:aws:s3:::ledger"')
    tf += block('aws_kinesis_stream', 'unresolved', '  name = terraform.workspace')
    # Filtered and unfiltered subscriptions must preserve source, consumer and level.
    queues = ['sns_open', 'sns_filter', 'sns_refs', 'bus_open', 'bus_filter', 'bus_default', 'bus_other',
              'bad_protocol', 'bad_topic', 'bad_rule', 'bad_bus', 'bad_target', 'timer_only', 'ambiguous_rule']
    for queue in queues:
        tf += block('aws_sqs_queue', queue, f'  name = "{queue}"')
        expected[f'aws_sqs_queue.{queue}'] = f'channel::sqs::{queue}'
        code += f'\ndef receive_{queue}():\n    return queue.receive_message(QueueUrl="https://sqs.eu-west-1.amazonaws.com/123456789012/{queue}")\n'
    for label, queue, filtered, refs in [('open', 'sns_open', False, False),
                                          ('filter', 'sns_filter', True, False),
                                          ('refs', 'sns_refs', False, True)]:
        topic = 'aws_sns_topic.literal.arn' if refs else '"arn:aws:sns:eu-west-1:123456789012:signal"'
        endpoint = f'aws_sqs_queue.{queue}.arn' if refs else f'"arn:aws:sqs:eu-west-1:123456789012:{queue}"'
        body = f'  topic_arn = {topic}\n  protocol = "sqs"\n  endpoint = {endpoint}'
        if filtered:
            body += '\n  filter_policy = "{\\"severity\\":[\\"high\\"]}"'
        address = 'aws_sns_topic_subscription.' + label
        tf += block('aws_sns_topic_subscription', label, body)
        bindings.append((f'channel::sns::signal', f'channel::sqs::{queue}', address,
                         'inferred' if filtered else 'exact', 'sns.asyncapi.json'))
    # A second bus with the SAME physical rule name must select its own producer.
    # This is a positive counterpart to the missing-other-bus refusal below.
    tf += block('aws_cloudwatch_event_bus', 'distinct_bus', '  name = "edge"')
    expected['aws_cloudwatch_event_bus.distinct_bus'] = 'channel::eventbridge::edge'
    channel_names['eventbridge'].append('edge')
    for label, queue, bus, filtered in [('open', 'bus_open', 'hub', False),
                                      ('filter', 'bus_filter', 'hub', True),
                                      ('default', 'bus_default', None, False),
                                      ('other', 'bus_other', 'edge', False)]:
        # A no-pattern/no-schedule rule is a synthetic unfiltered representation.
        rule_name = 'rule-open' if label == 'other' else f'rule-{label}'
        body = f'  name = "{rule_name}"'
        if bus:
            body += '\n  event_bus_name = ' + json.dumps(bus)
        if filtered:
            body += '\n  event_pattern = "{\\"source\\":[\\"zz\\"]}"'
        tf += block('aws_cloudwatch_event_rule', label, body)
        target = f'  rule = aws_cloudwatch_event_rule.{label}.name\n  arn = "arn:aws:sqs:eu-west-1:123456789012:{queue}"'
        if bus:
            target += '\n  event_bus_name = ' + json.dumps(bus)
        tf += block('aws_cloudwatch_event_target', label, target)
        bindings.append((f'channel::eventbridge::{bus or "default"}', f'channel::sqs::{queue}',
                         f'aws_cloudwatch_event_target.{label}', 'inferred' if filtered else 'exact',
                         'eventbridge.asyncapi.json'))
    tf += block('aws_sns_topic_subscription', 'bad_protocol', '  topic_arn = "arn:aws:sns:eu-west-1:123456789012:signal"\n  protocol = "https"\n  endpoint = "arn:aws:sqs:eu-west-1:123456789012:bad_protocol"')
    tf += block('aws_sns_topic_subscription', 'bad_topic', '  topic_arn = "arn:aws:sqs:eu-west-1:123456789012:signal"\n  protocol = "sqs"\n  endpoint = "arn:aws:sqs:eu-west-1:123456789012:bad_topic"')
    tf += block('aws_cloudwatch_event_target', 'bad_rule', '  rule = "no-such-rule"\n  event_bus_name = "hub"\n  arn = "arn:aws:sqs:eu-west-1:123456789012:bad_rule"')
    tf += block('aws_cloudwatch_event_target', 'bad_bus', '  rule = "rule-open"\n  event_bus_name = "other-bus"\n  arn = "arn:aws:sqs:eu-west-1:123456789012:bad_bus"')
    tf += block('aws_cloudwatch_event_target', 'bad_target', '  rule = "rule-open"\n  event_bus_name = "hub"\n  arn = "arn:aws:s3:::bad_target"')
    tf += block('aws_cloudwatch_event_rule', 'timer_only', '  name = "timer-only"\n  event_bus_name = "hub"\n  schedule_expression = "rate(5 minutes)"')
    tf += block('aws_cloudwatch_event_target', 'timer_only', '  rule = aws_cloudwatch_event_rule.timer_only.name\n  event_bus_name = "hub"\n  arn = "arn:aws:sqs:eu-west-1:123456789012:timer_only"')
    # Two declarations with the same bus/name make a literal target ambiguous.
    # Certain or absent forbids selecting the first declaration arbitrarily.
    for label in ['ambiguous_first', 'ambiguous_second']:
        tf += block('aws_cloudwatch_event_rule', label, '  name = "rule-ambiguous"\n  event_bus_name = "hub"')
    tf += block('aws_cloudwatch_event_target', 'ambiguous_rule', '  rule = "rule-ambiguous"\n  event_bus_name = "hub"\n  arn = "arn:aws:sqs:eu-west-1:123456789012:ambiguous_rule"')
    channel_names['eventbridge'].append('default')
    write(infra, 'main.tofu', tf)
    # OpenTofu overrides a same-basename .tf. A stale or duplicate .tf must not provision.
    write(infra, 'main.tf', block('aws_sns_topic', 'shadowed', '  name = "shadowed-must-not-provision"'))
    write(app, 'meada.toml', manifest)
    write(app, 'app.py', code)
    for broker, names in channel_names.items():
        write(app, f'{broker}.asyncapi.json', document(broker, names))
    for root in [infra, app]:
        git('add', '.', cwd=root)
        git('commit', '-qm', 'independent AWS public-surface probe', cwd=root)
    return infra, app, expected, bindings


class Probe:
    def __init__(self, run):
        self.run, self.checks = run, []

    def check(self, name, passed, detail=''):
        self.checks.append({'check': name, 'passed': bool(passed), 'detail': str(detail)})
        (self.run / 'checks.json').write_text(json.dumps(self.checks, indent=2))
        print(('PASS ' if passed else 'FAIL ') + name, flush=True)

    def command(self, binary, root, label, *args):
        p = subprocess.run([binary, *args], cwd=root, text=True, capture_output=True, timeout=120)
        write(self.run, label + '.out', p.stdout)
        write(self.run, label + '.err', p.stderr)
        if p.returncode:
            raise RuntimeError(f'{label}: exit {p.returncode}: {p.stderr}')
        return p.stdout


def json_values(binary, run, probe):
    """Focused #658 follow-up: secret markers apply to both JSON IaC spellings."""
    root = run / 'work/json-values'
    root.mkdir(parents=True)
    identity = REMOTE + 'json-values'
    git('init', '-q', cwd=root)
    git('remote', 'add', 'origin', f'https://{identity}.git', cwd=root)
    rows, secrets = [], []
    for suffix in ['tf', 'tofu']:
        resources = {}
        for kind, typ in [('secure', 'SecureString'), ('unknown', '${var.unresolved_type}'), ('plain', 'String')]:
            label = suffix + '_' + kind
            value = 'zz-json-' + label + '-sentinel'
            resources[label] = {'name': '/proof/json/' + label, 'type': typ, 'value': value}
            if kind != 'plain':
                secrets.append(value)
        filename = f'probe-{suffix}.{suffix}.json'
        text = json.dumps({'resource': {'aws_ssm_parameter': resources}}, indent=2) + '\n'
        write(root, filename, text)
        for label, attrs in resources.items():
            line = next(i for i, line in enumerate(text.splitlines(), 1) if '"value": ' + json.dumps(attrs['value']) in line)
            key = 'resource.aws_ssm_parameter.' + label + '.value'
            hidden = attrs['type'] != 'String'
            shown = 'redacted' if hidden else json.dumps(attrs['value'])
            rows.append({'file': filename, 'key': key, 'line': line, 'hidden': hidden,
                         'value': attrs['value'], 'expected': f'{identity} {filename}:{line} {key} = {shown}\n'})
    write(run, 'oracle.json', json.dumps(rows, indent=2))
    git('add', '.', cwd=root)
    git('commit', '-qm', 'independent JSON SSM marker probe', cwd=root)
    data = json.loads(probe.command(binary, root, 'export', 'export'))
    status = probe.command(binary, root, 'status', 'status')
    probe.check('JSON export format stays 1', data.get('format') == 1)
    probe.check('JSON two files and four redactions counted',
                'config files: 2 read' in status and 'redacted: 4\n' in status, status)
    queries = []
    for row in rows:
        queries.append({'tool': 'search', 'arguments': {'query': row['key'], 'kind': 'value', 'budget': 30000}})
        queries.append({'tool': 'search', 'arguments': {'query': row['value'], 'kind': 'value', 'budget': 30000}})
    write(run, 'queries.json', json.dumps(queries, indent=2))
    tools, texts, _ = mcp_answers(binary, root, queries, run / 'stdio.err')
    write(run, 'stdio.json', json.dumps(texts, indent=2))
    for i, row in enumerate(rows):
        probe.check('JSON exact key answer ' + row['key'], texts[2*i] == row['expected'], texts[2*i])
        expected_value = f'no matches for `{row["value"]}`\n' if row['hidden'] else row['expected']
        probe.check('JSON exact value answer ' + row['key'], texts[2*i+1] == expected_value, texts[2*i+1])
    probe.check('JSON no secret in export or key answers',
                all(secret not in json.dumps(data) and all(secret not in t for t in texts[::2]) for secret in secrets))
    write(run, 'workspace.toml', f'index_dir = "indexes"\n[[repository]]\nrepo = "{identity}"\npath = "{root}"\n')
    probe.command(binary, run, 'index', 'index', '--workspace', 'workspace.toml')
    port = free_port()
    server = subprocess.Popen([binary, 'mcp', '--listen', f'127.0.0.1:{port}', '--workspace', 'workspace.toml'],
                              cwd=run, stdin=subprocess.DEVNULL, stdout=open(run / 'http.out', 'w'), stderr=open(run / 'http.err', 'w'))
    write(run, 'http.pid', str(server.pid))
    try:
        for _ in range(100):
            try:
                socket.create_connection(('127.0.0.1', port), timeout=.2).close()
                break
            except OSError:
                time.sleep(.1)
        client = Http(f'http://127.0.0.1:{port}/{identity}')
        probe.check('JSON HTTP tools parity', client.call('tools/list', {})['result']['tools'] == tools)
        http = [answer(client.call('tools/call', {'name': q['tool'], 'arguments': q['arguments']})) for q in queries]
        write(run, 'http.json', json.dumps(http, indent=2))
        probe.check('JSON HTTP all answers byte equal', http == texts, f'{len(queries)} answers')
    finally:
        server.terminate()
        server.wait(timeout=30)
        write(run, 'http.stopped', str(server.returncode))
    failed = [c for c in probe.checks if not c['passed']]
    verdict = 'BROKEN' if failed else 'CHECKS PASSED; independent checker must judge PROVED'
    write(run, 'receipt.md', '# Focused JSON SSM marker proof\n\n' + verdict + '\n\n'
          'Checker: independent AWS proof agent. New flow: #658 SecureString criterion, ADR0025 JSON format coverage; '
          'unresolved type follows the coordinating review requirement to redact when SecureString cannot be ruled out. '
          'HTTP parity uses stdio too. No fixture or feature code was read to author the oracle.\n\n'
          f'{len(probe.checks)} checks; {len(failed)} failures. oracle.json precedes all Meada answers. '
          'Raw CLI: export/status.out; MCP: queries.json, stdio.json, http.json.\n\n'
          'This is a new-format focused supplement, not a fourth full AWS attempt. Existing broad AWS '
          'and corpus evidence remain separately scoped.\n\n'
          + '\n'.join('- ' + c['check'] + ': ' + c['detail'] for c in failed) + '\n')
    return bool(failed)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--head-bin', required=True)
    parser.add_argument('--base-bin')
    parser.add_argument('--json-values', action='store_true', help='only the focused Terraform/OpenTofu JSON SSM marker flow')
    parser.add_argument('--run', required=True)
    args = parser.parse_args()
    if not args.json_values and not args.base_bin:
        parser.error('--base-bin is required unless --json-values is selected')
    run = Path(args.run).resolve()
    run.mkdir(parents=True, exist_ok=False)
    probe = Probe(run)
    head = str(Path(args.head_bin).resolve())
    base = str(Path(args.base_bin).resolve()) if args.base_bin else None
    os.environ['LANG'] = 'C.UTF-8'
    try:
        if args.json_values:
            probe.command(head, run, 'head-identity', '--version')
            return json_values(head, run, probe)
        for label, binary in [('head', head), ('base', base)]:
            probe.command(binary, run, label + '-identity', '--version')
        infra, app, expected, bindings = seed(run)
        write(run, 'oracle.json', json.dumps({'provisioners': expected, 'subscriptions': bindings,
                                             'orphans': expected_orphans()}, indent=2))
        data = json.loads(probe.command(head, infra, 'export', 'export'))
        app_data = json.loads(probe.command(head, app, 'app-export', 'export'))
        status = probe.command(head, infra, 'status', 'status')
        probe.check('format stays 1', data.get('format') == 1)
        got = {(n['node'].rsplit('::', 1)[-1], n['contract']) for n in data.get('provisioners', [])}
        probe.check('all literal/ARN kinds, orphan fixtures and no refused provisioners', got == set(expected.items()) and len(data.get('provisioners', [])) == len(expected),
                    {'missing': sorted(set(expected.items()) - got), 'extra': sorted(got - set(expected.items()))})
        provisions = data.get('provisions', [])
        used = {address: contract for address, contract in expected.items() if '.orphan' not in address}
        probe.check('complete provisioning link count', len(provisions) == len(used), provisions)
        for address, contract in used.items():
            links = [l for l in provisions if l['contract'] == contract and l['provisioner'].endswith('::' + address)]
            if contract.startswith('resource::'):
                case = next(i for i, c in enumerate(CASES) if contract == f'{c[2]}::{c[3]}')
                side = f'{REMOTE}app::app.py::read_case_{case}'
                level = 'declared'
            elif contract.startswith('channel::sqs::'):
                side = f'{REMOTE}app::app.py::receive_{contract.rsplit("::", 1)[-1]}'
                level = 'exact'
            else:
                _, broker, name = contract.split('::')
                side = f'{REMOTE}app::{broker}.asyncapi.json::{broker} send {name}'
                level = 'exact'
            probe.check('provisions ' + address,
                        len(links) == 1 and links[0]['side'] == side and links[0]['level'] == level
                        and links[0]['root'] == '.', links)

        for prefix in dict.fromkeys(c[2] for c in CASES):
            kind = prefix.split('::')[1]
            probe.check('status unused ' + kind, 'unused-' + kind in status, status)
            probe.check('status missing ' + kind, 'missing-' + kind in status, status)
        probe.check('complete scope-aware orphan set and count', orphan_set_matches(status, expected_orphans()), status)
        subs = data.get('subscriptions', [])
        actual_pairs = {(s['source'], s['target']) for s in subs}
        expected_pairs = {(s, t) for s, t, _, _, _ in bindings}
        probe.check('subscription identities and refusal controls', actual_pairs == expected_pairs and len(subs) == len(bindings),
                    {'expected': sorted(expected_pairs), 'actual': sorted(actual_pairs)})
        probe.check('complete routed consumer link count', len(app_data.get('links', [])) == len(bindings))
        for source, target, address, level, doc in bindings:
            queue = target.rsplit('::', 1)[-1]
            links = [l for l in app_data.get('links', []) if l['consumer'].endswith('::receive_' + queue)]
            subscription_names = [address]
            if address.startswith('aws_cloudwatch_event_target.'):
                subscription_names.append(address.replace('aws_cloudwatch_event_target.', 'aws_cloudwatch_event_rule.'))
            probe.check('subscription consumer routing ' + queue,
                        len(links) == 1
                        and links[0]['provider'] == f'{REMOTE}app::{doc}::{source.split("::")[1]} send {source.rsplit("::", 1)[-1]}'
                        and links[0]['level'] == level
                        and any(name in links[0]['reason'] for name in subscription_names), links)
        for queue in ['bad_protocol', 'bad_topic', 'bad_rule', 'bad_bus', 'bad_target', 'timer_only', 'ambiguous_rule']:
            probe.check('no link for ' + queue,
                        not any(l['consumer'].endswith('::receive_' + queue) for l in app_data.get('links', [])))
        queries = []
        def query(tool, **arguments):
            arguments['budget'] = 30000
            queries.append({'tool': tool, 'arguments': arguments})
        for address, contract in expected.items():
            if '.orphan' not in address:
                query('context', symbol=address)
                query('trace', **{'from': address, 'direction': 'down'})
        for i, case in enumerate(CASES):
            if case[2].startswith('resource'):
                query('context', symbol=f'read_case_{i}')
                query('trace', **{'from': f'read_case_{i}', 'direction': 'up'})
        for source in dict.fromkeys(b[0] for b in bindings):
            query('context', symbol=source)
            query('trace', **{'from': source, 'direction': 'down'})
        for _, target, _, _, _ in bindings:
            queue = target.rsplit('::', 1)[-1]
            query('context', symbol='receive_' + queue)
            query('trace', **{'from': 'receive_' + queue, 'direction': 'up'})
        query('context', symbol='aws_unrecognized_service.unknown')
        query('search', query='zz_no_aws_match', kind='iac')
        query('search', query='signal', kind='unknown-aws-kind')
        query('search', query='resource.aws_ssm_parameter.literal.value', kind='value')
        query('search', query=SECRET, kind='value')
        write(run, 'queries.json', json.dumps(queries, indent=2))
        tools, texts, _ = mcp_answers(head, infra, queries, run / 'stdio.err')
        write(run, 'stdio.json', json.dumps(texts, indent=2))
        for q, text in zip(queries, texts):
            a = q['arguments']
            if q['tool'] == 'context' and a['symbol'] in expected:
                contract = expected[a['symbol']]
                role = 'consumer' if contract.startswith('channel::sqs::') else ('producer' if contract.startswith('channel::') else 'reader')
                probe.check('context contract ' + a['symbol'], contract in text and role in text, text)
            elif q['tool'] == 'trace' and a['from'] in expected:
                probe.check('down provisions ' + a['from'], 'provisions ' in text and 'app' in text, text)
            elif q['tool'] == 'trace' and a['from'].startswith('read_case_'):
                probe.check('up provisions ' + a['from'], 'provisions ' in text and 'main.tofu' in text, text)
            elif q['tool'] == 'trace' and a['from'].startswith(('channel::sns::', 'channel::eventbridge::')):
                queues = [b[1].rsplit('::', 1)[-1] for b in bindings if b[0] == a['from']]
                probe.check('source channel down ' + a['from'], all('receive_' + queue in text for queue in queues), text)
            elif q['tool'] == 'trace' and a['from'].startswith('receive_'):
                queue = a['from'].removeprefix('receive_')
                binding = next(b for b in bindings if b[1].endswith('::' + queue))
                probe.check('up subscription ' + queue, binding[4] in text and binding[3] in text and 'main.tofu' in text, text)
        probe.check('unknown node remains searchable', 'aws_unrecognized_service' in texts[-5])
        probe.check('unknown search is empty', 'no matches' in texts[-4].lower(), texts[-4])
        probe.check('wrong kind refused', ('ERROR' in texts[-3] or 'unknown' in texts[-3].lower()) and 'iac' in texts[-3], texts[-3])
        probe.check('SecureString key shows redacted', 'redacted' in texts[-2] and SECRET not in texts[-2], texts[-2])
        probe.check('SecureString cannot be searched by value', texts[-1] == f'no matches for `{SECRET}`\n', texts[-1])
        probe.check('SecureString absent from all surface output', SECRET not in json.dumps(data) and SECRET not in json.dumps(app_data) and SECRET not in '\n'.join(texts[:-1]))
        workspace = 'index_dir = "indexes"\n'
        for name, root in [('infra', infra), ('app', app)]:
            workspace += f'\n[[repository]]\nrepo = "{REMOTE}{name}"\npath = "{root}"\n'
        write(run, 'workspace.toml', workspace)
        probe.command(head, run, 'index', 'index', '--workspace', 'workspace.toml')
        port = free_port()
        server = subprocess.Popen([head, 'mcp', '--listen', f'127.0.0.1:{port}', '--workspace', 'workspace.toml'],
                                  cwd=run, stdin=subprocess.DEVNULL, stdout=open(run / 'http.out', 'w'), stderr=open(run / 'http.err', 'w'))
        write(run, 'http.pid', str(server.pid))
        try:
            for _ in range(100):
                try:
                    socket.create_connection(('127.0.0.1', port), timeout=.2).close()
                    break
                except OSError:
                    time.sleep(.1)
            client = Http(f'http://127.0.0.1:{port}/{REMOTE}infra')
            probe.check('HTTP tool list parity', client.call('tools/list', {})['result']['tools'] == tools)
            http = [answer(client.call('tools/call', {'name': q['tool'], 'arguments': q['arguments']})) for q in queries]
            write(run, 'http.json', json.dumps(http, indent=2))
            probe.check('HTTP all answers byte equal', http == texts, f'{len(queries)} answers')
        finally:
            server.terminate()
            server.wait(timeout=30)
            write(run, 'http.stopped', str(server.returncode))
        # Same-size content edit: rename both topic and routes, keep app unchanged.
        path = infra / 'main.tofu'
        original = path.read_text()
        path.write_text(original.replace('signal', 'silent'))
        changed = json.loads(probe.command(head, infra, 'refresh', 'export'))
        probe.check('warm refresh removes original topic provisioners',
                    not any(p['contract'] == 'channel::sns::signal' for p in changed.get('provisioners', [])))
        changed_app = json.loads(probe.command(head, app, 'app-refresh', 'export'))
        probe.check('warm refresh removes routed source producers',
                    not any(l['consumer'].endswith(('::receive_sns_open', '::receive_sns_filter', '::receive_sns_refs'))
                            for l in changed_app.get('links', [])))
        path.write_text(original)
        restored = json.loads(probe.command(head, infra, 'restore', 'export'))
        restored_app = json.loads(probe.command(head, app, 'app-restore', 'export'))
        fields = ['provisioners', 'provisions', 'subscriptions', 'links']
        probe.check('restore round trip', all(restored.get(f) == data.get(f) for f in fields)
                    and all(restored_app.get(f) == app_data.get(f) for f in fields))
        # Orphan output requires BOTH a use and a provisioner of that broker/kind in scope.
        for name, source in [('only-provisioners', infra), ('only-uses', app)]:
            isolated = run / 'work' / name
            shutil.copytree(source, isolated, ignore=shutil.ignore_patterns('.git', '.meada'))
            manifest = isolated / 'meada.toml'
            text = manifest.read_text()
            text = text.replace(f'[[peer]]\nrepo = "{REMOTE}{"app" if source == infra else "infra"}"\n', '')
            manifest.write_text(text)
            git('init', '-q', cwd=isolated)
            git('add', '.', cwd=isolated)
            git('commit', '-qm', 'isolated orphan gate probe', cwd=isolated)
            isolated_export = json.loads(probe.command(head, isolated, name + '-export', 'export'))
            present = (bool(isolated_export.get('provisioners'))
                       and not isolated_export.get('providers') and not isolated_export.get('consumers')) if source == infra else (
                           bool(isolated_export.get('consumers') and isolated_export.get('providers'))
                           and not isolated_export.get('provisioners'))
            probe.check('orphan gate known rows ' + name, present, 'A no-row or not-indexed status cannot prove the gate.')
            isolated_status = probe.command(head, isolated, name + '-status', 'status')
            probe.check('orphan gate ' + name, 'not indexed' not in isolated_status
                        and orphan_set_matches(isolated_status, {}), isolated_status)
        # Unchanged code-only flow is driven on the same neutral repository with both binaries.
        clean = run / 'work/plain'
        clean.mkdir()
        git('init', '-q', cwd=clean)
        write(clean, 'plain.py', 'def independent_plain():\n    return 42\n')
        git('add', '.', cwd=clean)
        git('commit', '-qm', 'code-only parity input', cwd=clean)
        old = json.loads(probe.command(base, clean, 'base-plain', 'export'))
        old_status = probe.command(base, clean, 'base-plain-status', 'status')
        new = json.loads(probe.command(head, clean, 'head-plain', 'export'))
        new_status = probe.command(head, clean, 'head-plain-status', 'status')
        probe.check('code-only export base parity', old == new)
        probe.check('code-only status base parity', old_status == new_status)
        oq = [{'tool': 'context', 'arguments': {'symbol': 'independent_plain', 'budget': 8000}},
              {'tool': 'trace', 'arguments': {'from': 'independent_plain', 'direction': 'up', 'budget': 8000}}]
        _, bt, _ = mcp_answers(base, clean, oq, run / 'base-plain-stdio.err')
        _, ht, _ = mcp_answers(head, clean, oq, run / 'head-plain-stdio.err')
        write(run, 'plain-stdio.json', json.dumps({'base': bt, 'head': ht}, indent=2))
        probe.check('code-only MCP base parity', bt == ht)
        failed = [c for c in probe.checks if not c['passed']]
        verdict = 'BROKEN' if failed else 'CHECKS PASSED; independent checker must judge PROVED'
        write(run, 'receipt.md', f'# AWS #658 proof\n\n{verdict}\n\nChecker: independent AWS proof agent.\n'
              'New flows: #658, #642, base ADR0025/0026 (rung 4). HTTP: stdio parity (rung 1). '
              'Existing code-only flow: base binary (rung 1).\n\n'
              'Review correction before the first run: exact EventBridge fixtures omit both pattern and schedule '
              '(synthetic unfiltered representation); timer-only rules have their own negative control because '
              'a schedule does not route arbitrary bus messages. Exact/inferred assertions remain unchanged.\n\n'
              'Verifier retry corrections: consumer links are read from the consumer repository export '
              '(exports are repository-scoped), SQS context requires its consumer role, and the secret '
              'absence check excludes only the search request echoed by its no-matches answer. '
              'The first attempt was BLOCKED by sandbox socket permission; HTTP requires loopback access. '
              'The second attempt asked base status after HEAD had overwritten the shared index; '
              'each status now runs immediately after its own version indexes the repository. '
              'Isolated orphan scopes are explicitly indexed and show known rows before status is checked.\n\n'
              f'{len(probe.checks)} checks; {len(failed)} failures. Raw checks: checks.json; '
              'CLI: export/status/refresh/restore.out; MCP: queries.json, stdio.json, http.json.\n\n'
              'Not covered: cloud SDK additions (separate tickets), console rendering, grants; '
              'root checker runs full shared-code corpus parity and docs/release checks.\n\n'
              + '\n'.join('- ' + c['check'] + ': ' + c['detail'] for c in failed) + '\n')
        return bool(failed)
    except Exception:
        write(run, 'blocked.txt', traceback.format_exc())
        write(run, 'receipt.md', '# AWS #658 proof\n\nBLOCKED: verifier or surface failed; see blocked.txt and raw evidence.\n')
        print(traceback.format_exc(), flush=True)
        return 2


if __name__ == '__main__':
    raise SystemExit(main())
