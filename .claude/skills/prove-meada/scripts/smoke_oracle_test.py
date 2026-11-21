"""Replay the public smoke scorer without building Meada."""

from copy import deepcopy
import json
from pathlib import Path
import subprocess
from tempfile import TemporaryDirectory
from types import SimpleNamespace
from unittest.mock import patch
from zipfile import ZipFile

from proof_result import BLOCKED, DIFFERENCE, ORACLE_FAILED, TIMEOUT
from smoke_stack import (APP, EXPANDED, HTTP, KOTLIN_SHADOW, LIB, NAMES, ORDERS, ORDER_CANDIDATES,
                         ORIGINS, PINS, QUEUE, QUEUE_NODE, ROUTE, SHARED, SHARED_HOST, TYPED_FIXTURE,
                         Smoke, endpoint_observation, git, http_cases, overridden_order,
                         provision_ok, score_http, score_kotlin, sqs)


def rig_at(path):
    return Smoke(SimpleNamespace(run=path, slice='resolution', base_bin=None, head_bin=None,
                                 old_bin=None, middle_bin=None, source_dir=None))


def site_id(case):
    path, symbol, call, _ = case
    return f'{APP}::orders/clients/{path}::{symbol}::{call}'


def set_site(data, site, contract, via, links=(), kind=None, candidates=(), limit=None):
    for table, field in [('consumers', 'site'), ('links', 'consumer'), ('open_items', 'call')]:
        data[table] = [row for row in data.get(table, []) if row[field] != site]
    data['consumers'].append({'site': site, 'contract': contract, 'via': via})
    data['links'].extend({'consumer': site, 'provider': ORDERS[repo], 'contract': contract,
                          'level': level, 'reason': reason} for repo, level, reason in links)
    if kind:
        item = {'call': site, 'kind': kind, 'candidates': list(candidates)}
        if limit is not None:
            item['limit'] = limit
        data['open_items'].append(item)


def set_configured(data, site):
    set_site(data, site, ROUTE, 'env:SMOKE_ORDERS_URL',
             [(LIB, 'inferred', f'base URL from SMOKE_ORDERS_URL, a base_url_env of peer {LIB}')])


def resolved_kotlin():
    data = {'consumers': [], 'links': [], 'calls': []}
    prefix = f'example.com/proof/kotlin-shadow::{TYPED_FIXTURE}::example.jvm.'
    for symbol, route in KOTLIN_SHADOW.values():
        uri = f'{prefix}{symbol}::(call_expression).uri#0'
        data['consumers'].append({'site': uri, 'contract': f'http::GET::{route}', 'via': 'host:typed.example.com'})
        data['calls'].extend([{'id': uri},
                              {'id': f'{prefix}{symbol}::make#0', 'level': 'exact', 'target': f'{prefix}make()'}])
    return data


def unresolved_kotlin():
    data = resolved_kotlin()
    data['open_items'] = [{'call': row['site'], 'kind': 'dynamic-url', 'candidates': []}
                          for row in data['consumers']]
    for row in data['consumers']:
        row.update(contract=None, via=None)
    return data


def synthetic():
    data = {}
    for case in HTTP:
        set_configured(data, site_id(case))
    for case in HTTP:
        if case[-1] == '#810':
            set_site(data, site_id(case), None, None, kind='dynamic-url', candidates=ORDER_CANDIDATES)
    data['providers'], data['provisions'] = [], []
    for name in ('publishOrder', 'publishLiteral', 'publishSelected'):
        site = f'{APP}::orders/events/publisher.ts::{name}::SendMessageCommand#0'
        known = name != 'publishSelected'
        data['providers'].append({'site': site, 'contract': QUEUE if known else 'channel::sqs'})
        if known:
            data['provisions'].append({'side': site, 'contract': QUEUE, 'provisioner': QUEUE_NODE, 'level': 'exact'})
    return data


def corrected(broken):
    fixed = deepcopy(broken)
    for case in HTTP:
        if case[-1] in ('#948', '#949'):
            helper = 'sharedBase' if case[-1] == '#948' else 'shared_base'
            set_site(fixed, site_id(case), None, None, kind='dynamic-url', candidates=ORDER_CANDIDATES,
                     limit=f'base from the return value of {helper}')
        if case[-1] == '#810':
            set_configured(fixed, site_id(case))
    return fixed


def replay(root, prefix, broken, kotlin):
    fixed = corrected(broken)
    good, bad = (fixed, resolved_kotlin()), (broken, kotlin)
    for base_name, base in [('broken', bad), ('healthy', good)]:
        for head_name, head in [('broken', bad), ('healthy', good)]:
            rig = rig_at(root / f'{prefix}-{base_name}-{head_name}')
            for revision, (app, fixture) in [('base', base), ('head', head)]:
                score_http(rig, revision, app)
                sqs(rig, app, revision)
                score_kotlin(rig, revision, fixture)
            assert sum(rig.scored['base'].values()) == (9 if base_name == 'broken' else 13)
            assert sum(rig.scored['head'].values()) == (9 if head_name == 'broken' else 13)
            expected = ORACLE_FAILED if head_name == 'broken' else DIFFERENCE if base_name == 'broken' else 0
            assert rig.finish() == expected
            ledger = json.loads((rig.run / 'transitions.json').read_text())
            changed = [row['transition'] for row in ledger.values() if row['transition'] != 'stable']
            assert changed == ([{'broken': 'still rejected', 'healthy': 'recovered'}[head_name]] * 6
                               if base_name == 'broken' else ['regression'] * 6 if head_name == 'broken' else [])
            print(f'{prefix}: {base_name} → {head_name}: exit {expected}')

    configured, overridden = map(site_id, HTTP[:2])
    configured_key = 'HTTP go/client.go SendConfigured'
    override_key = 'HTTP go/client.go SendShared #948'

    def fails(data, key, name, accepted=False):
        rig = rig_at(root / f'{prefix}-{name}')
        score_http(rig, 'head', data)
        assert rig.scored['head'][key] == accepted, name
        print(f'{prefix}: {name}: {"accepted" if accepted else "rejected"}')

    def change(data, table, field, edit):
        out = deepcopy(data)
        row, = [row for row in out[table] if row[field] == configured]
        edit(out[table], row)
        return out

    fails(change(broken, 'consumers', 'site', lambda rows, row: rows.remove(row)),
          configured_key, 'link-without-consumer')
    fails(change(broken, 'consumers', 'site', lambda _, row: row.update(via='env:UNRELATED')),
          configured_key, 'contradictory-provenance')
    neighbor = configured.replace('::SendConfigured::', '::SendConfiguredCopy::')
    moved = change(broken, 'consumers', 'site', lambda _, row: row.update(site=neighbor))
    fails(change(moved, 'links', 'consumer', lambda _, row: row.update(consumer=neighbor)),
          configured_key, 'neighboring-symbol')
    moved = change(broken, 'links', 'consumer', lambda _, row: row.update(provider=ORDERS[SHARED]))
    fails(moved, configured_key, 'swapped-provider')
    opened = deepcopy(broken)
    set_site(opened, overridden, None, None)
    fails(opened, override_key, 'dynamic-without-item')
    set_site(opened, overridden, None, None, kind='dynamic-url', candidates=ORDER_CANDIDATES,
             limit='base from the return value of sharedBase')
    fails(opened, override_key, 'dynamic-with-item', accepted=True)
    links = [(repo, 'ambiguous', f'host unresolved; served by {repo}') for repo in ORDERS]
    for kind, candidates, name in [
        ('dynamic-url', [], 'ambiguous-with-dynamic-item'),
        ('ambiguous-link', [], 'ambiguous-with-empty-candidates'),
        ('ambiguous-link', [ORDER_CANDIDATES[0]] * 2, 'ambiguous-with-duplicate-candidate'),
        ('ambiguous-link', ORDER_CANDIDATES, 'ambiguous-with-both-candidates'),
    ]:
        set_site(opened, overridden, ROUTE, None, links, kind, candidates)
        fails(opened, override_key, name, accepted=name == 'ambiguous-with-both-candidates')
    set_site(opened, overridden, ROUTE, SHARED_HOST,
             [(SHARED, 'exact', f'host shared.smoke.example is served by {SHARED}')])
    fails(opened, override_key, 'observed-shared-host', accepted=True)
    del opened['open_items']
    fails(opened, configured_key, 'configured-without-optional-array', accepted=True)


def replay_open_items(root, prefix, fixed):
    for case in (HTTP[1], HTTP[3]):
        path, symbol, _, issue = case
        site, key = site_id(case), f'HTTP {path} {symbol} {issue}'
        for name in ('original', 'empty-candidates', 'absent-limit', 'wrong-helper',
                     'one-candidate', 'absent-item'):
            data = deepcopy(fixed)
            items = data['open_items']
            row, = [row for row in items if row['call'] == site]
            if name == 'empty-candidates':
                row['candidates'] = []
            elif name == 'absent-limit':
                del row['limit']
            elif name == 'wrong-helper':
                row['limit'] = 'base from the return value of NO_SUCH_HELPER'
            elif name == 'one-candidate':
                row['candidates'] = row['candidates'][:1]
            elif name == 'absent-item':
                items.remove(row)
            rig = rig_at(root / f'{prefix}-queue-{symbol}-{name}')
            score_http(rig, 'head', data)
            sqs(rig, data, 'head')
            accepted = name == 'original'
            assert sum(rig.scored['head'].values()) == 12 + accepted
            assert rig.scored['head'][key] == accepted
            assert rig.finish() == (0 if accepted else ORACLE_FAILED)
            print(f'{prefix} {symbol} {name}: {12 + accepted}/13')

    no_route = {'consumers': [], 'links': []}
    set_site(no_route, 'different-path', None, None, kind='dynamic-url',
             limit='base from the return value of other')
    assert overridden_order(endpoint_observation(no_route, 'different-path'), 'other', [])


def mcp_fixture():
    app_locations = '''
orders/clients/csharp/OrdersClient.cs:11
orders/clients/go/client.go:20
orders/clients/java/OrdersClient.java:11
orders/clients/kotlin/Orders.kt:7
orders/clients/python/client.py:30
orders/clients/rust/src/lib.rs:8
orders/clients/swift/Orders.swift:13
orders/clients/typescript/client.ts:6
orders/evidence/client.go:13
orders/evidence/client.go:20
orders/evidence/client.go:24
orders/evidence/client.go:31
orders/evidence/client.go:39
orders/httpkey/client.go:17
orders/neutral/client.go:17
orders/oskey/client.go:17
'''.split()
    search = '\n'.join([f'{APP} {location} {ROUTE}' for location in app_locations]
                       + [f'{repo} openapi.yaml (documented only) {ROUTE}' for repo in (LIB, SHARED)])
    context = f'''{ROUTE}
  {LIB} provider openapi.yaml (documented only)
  {SHARED} provider openapi.yaml (documented only)
  16 linked consumers omitted. See trace with direction "up".
'''
    trace = f'''{APP} .github/workflows/ci.yml:32 jobs["through-lib"]
  →[exact: @main (indexed cb746ac)] {LIB} .github/workflows/check.yml:1 .
    →[exact: @main (indexed 0aabcee)] {SHARED} .github/workflows/build.yml:1 .
chain: exact · 2 hops · 3 repos · 220 tokens
'''
    return [search, context, trace]


def replay_mcp(root, prefix, data, captured):
    data = deepcopy(data)
    if 'calls' not in data:
        data['calls'] = [
            {'id': site_id((path, symbol, 'ordersBase#0', issue)), 'level': 'exact',
             'target': f'{APP}::orders/clients/{path}::{symbol.rsplit(".", 1)[0]}.ordersBase()'}
            for path, symbol, _, issue in HTTP if issue == '#810'
        ]
    search, context, trace = captured
    search_key, context_key, trace_key = 'MCP POST /orders search', 'MCP POST /orders context', 'MCP CI two-hop trace'
    for name, answers, rejected in [
        ('original', captured, None),
        ('empty-search', ['', context, trace], search_key),
        ('unrelated-context', [search, 'unrelated context', trace], context_key),
        ('wrong-trace', [search, context, 'unrelated trace'], trace_key),
        ('search-no-shared', [search.replace(f'{SHARED} openapi.yaml (documented only) {ROUTE}', ''),
                              context, trace], search_key),
        ('search-no-configured-go', [search.replace(f'{APP} orders/clients/go/client.go:20 {ROUTE}', ''),
                                     context, trace], search_key),
        ('context-no-lib', [search, context.replace(f'{LIB} provider openapi.yaml (documented only)', ''),
                            trace], context_key),
        ('context-no-guidance', [search, context.replace(' See trace with direction "up".', ''), trace], context_key),
        ('context-wrong-count', [search, context.replace('16 linked', '15 linked'), trace], context_key),
    ]:
        rig = rig_at(root / f'{prefix}-mcp-{name}')
        rig.revision = 'head'
        rig.mcp = lambda *_: answers
        http_cases(rig, 'head', None, data)
        assert len(rig.scored['head']) == sum(rig.scored['head'].values()) == 13
        assert [row['key'] for row in rig.checks if row['result'] == 'fail'] == ([rejected] if rejected else [])
        expected = ORACLE_FAILED if rejected else 0
        assert rig.finish() == expected
        print(f'{prefix} MCP {name}: exit {expected}; export 13/13')


def replay_missing_pin(root):
    name, sha = NAMES[0], EXPANDED[0]
    bare = root / 'empty.git'
    cache = root / 'empty-cache'
    cache.mkdir()
    (cache / name).mkdir()
    git('init', '-q', '--bare', str(bare), cwd=root)
    git('init', '-q', cwd=cache / name)
    git('remote', 'add', 'origin', ORIGINS[name], cwd=cache / name)
    for variant in ('fetch', 'cache'):
        rig = rig_at(root / f'missing-{variant}')
        if variant == 'fetch':
            count = int(rig.env.get('GIT_CONFIG_COUNT', '0'))
            rig.env.update({f'GIT_CONFIG_KEY_{count}': f'url.{bare.as_uri()}.insteadOf',
                            f'GIT_CONFIG_VALUE_{count}': ORIGINS[name], 'GIT_CONFIG_COUNT': str(count + 1),
                            'GIT_ALLOW_PROTOCOL': 'file'})
            evidence = f'fetch/{name}-{sha}'
        else:
            rig.source, rig.capture_sources = cache, False
            evidence = f'checkout/base-expanded/{name}-{sha}'
        rig.phase('resolution-base', lambda r: r.clone('base-expanded', 'expanded'), revision='base')
        check, = rig.checks
        assert (check['result'], check['revision']) == ('blocked', 'base')
        assert rig.finish() == BLOCKED
        stderr = (rig.run / 'commands' / (evidence + '.err')).read_text().strip()
        report = (rig.run / check['evidence']).read_text()
        assert stderr and stderr in report and sha in report and ORIGINS[name] in report
        print(f'{variant} missing pin: blocked / 2; {stderr}')
    git('commit', '-q', '--allow-empty', '-m', 'Present pin', cwd=cache / name)
    present = git('rev-parse', 'HEAD', cwd=cache / name).strip()
    (cache / name / '.git/index.lock').touch()
    rig = rig_at(root / 'failed-checkout-with-object')
    rig.source, rig.capture_sources = cache, False
    with patch.dict(PINS, expanded=(present, *EXPANDED[1:])):
        rig.phase('resolution-base', lambda r: r.clone('base-expanded', 'expanded'), revision='base')
    check, = rig.checks
    assert check['result'] == 'error' and rig.finish() == 1
    stderr = (rig.run / f'commands/checkout/base-expanded/{name}-{present}.err').read_text()
    assert 'index.lock' in stderr
    print('cache checkout with present pin and locked index: error / 1')


def selftest(archive=None):
    with TemporaryDirectory() as directory:
        root = Path(directory)
        fixture = synthetic()
        replay(root, 'synthetic', fixture, unresolved_kotlin())
        fixed = corrected(fixture)
        replay_open_items(root, 'synthetic', fixed)
        replay_mcp(root, 'synthetic', fixed, mcp_fixture())
        replay_missing_pin(root)
        if archive:
            with ZipFile(archive) as zipfile:
                flow = 'flows/smoke-resolution/'
                captures = flow + 'commands/'
                scores = json.loads(zipfile.read(flow + 'denominator.json'))['checks']
                for slot in ('base', 'head'):
                    data = json.loads(zipfile.read(captures + slot + '/app.out'))
                    assert len(scores[slot]) == 13
                    passed = sum(scores[slot].values())
                    if passed == 13:
                        replay_open_items(root, f'archived-{slot}', data)
                        mcp = json.loads(zipfile.read(flow + f'mcp/{slot}/answers.json'))
                        replay_mcp(root, f'archived-{slot}', data, mcp['answers'])
                    else:
                        assert passed == 9
                        replay(root, f'archived-{slot}', data,
                               json.loads(zipfile.read(captures + 'kotlin-shadow/' + slot + '.out')))
        for error, code in [(FileNotFoundError(), BLOCKED), (RuntimeError(), 1),
                            (subprocess.TimeoutExpired('probe', 1), TIMEOUT)]:
            rig = rig_at(root / f'operational-{code}')

            def fail(_):
                raise error

            rig.phase('base-input', fail, revision='base')
            assert rig.finish() == code
        rig = rig_at(root / 'lifecycle-parity')
        rig.parity.compare('healthy status', 'healthy\nextra presentation\n', 'healthy\n')
        rig.parity.compare('continuous decider', 'saved\n', 'committed\n')
        assert rig.finish() == DIFFERENCE
        rig.check('candidate broken-lib identity and cause', 'lib: no such table: symbols', 'lib')
        assert rig.finish() == ORACLE_FAILED
        exact = [{'level': 'exact', 'contract': QUEUE, 'provisioner': QUEUE_NODE}]
        assert provision_ok(exact, True) and not provision_ok(exact, False)
        rig = rig_at(root / 'sqs-site-and-runtime')
        sqs_data = synthetic()
        for table, identity in [('providers', 'site'), ('provisions', 'side')]:
            for row in sqs_data[table]:
                row[identity] = row[identity].replace('::publishOrder::', '::publishOrderCopy::')
        choice = next(row['site'] for row in sqs_data['providers'] if '::publishSelected::' in row['site'])
        sqs_data['provisions'].append(dict(exact[0], side=choice, level='inferred'))
        sqs(rig, sqs_data, 'head')
        assert rig.scored['head'] == {'SQS publishOrder': False, 'SQS publishLiteral': True,
                                      'SQS publishSelected': False}
        rig = rig_at(root / 'kotlin-omission-must-keep-code-call')
        kotlin = resolved_kotlin()
        omitted = kotlin['consumers'].pop(0)
        score_kotlin(rig, 'base', kotlin)
        kotlin['calls'] = [row for row in kotlin['calls'] if row['id'] != omitted['site']]
        score_kotlin(rig, 'head', kotlin)
        assert [row['result'] for row in rig.checks] == ['pass', 'pass', 'fail', 'pass']
    print('smoke-stack selftest: revision outcomes, site joins, MCP, Kotlin and input failures passed')
