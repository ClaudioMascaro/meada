"""Replay the public smoke scorer without building Meada."""

from copy import deepcopy
import json
from pathlib import Path
import subprocess
from tempfile import TemporaryDirectory
from types import SimpleNamespace
from zipfile import ZipFile

from proof_result import BLOCKED, DIFFERENCE, ORACLE_FAILED, TIMEOUT
from smoke_stack import (APP, HTTP, KOTLIN_SHADOW, LIB, ORDERS, ORDER_CANDIDATES, QUEUE, QUEUE_NODE,
                         ROUTE, SHARED, SHARED_HOST, TYPED_FIXTURE, Smoke, provision_ok,
                         score_http, score_kotlin, sqs)


def rig_at(path):
    return Smoke(SimpleNamespace(run=path, slice='resolution', base_bin=None, head_bin=None,
                                 old_bin=None, middle_bin=None, source_dir=None))


def site_id(case):
    path, symbol, call, _ = case
    return f'{APP}::orders/clients/{path}::{symbol}::{call}'


def set_site(data, site, contract, via, links=(), kind=None, candidates=()):
    for table, field in [('consumers', 'site'), ('links', 'consumer'), ('open_items', 'call')]:
        data[table] = [row for row in data.get(table, []) if row[field] != site]
    data['consumers'].append({'site': site, 'contract': contract, 'via': via})
    data['links'].extend({'consumer': site, 'provider': ORDERS[repo], 'contract': contract,
                          'level': level, 'reason': reason} for repo, level, reason in links)
    if kind:
        data['open_items'].append({'call': site, 'kind': kind, 'candidates': list(candidates)})


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


def replay(root, prefix, broken, kotlin):
    fixed = deepcopy(broken)
    for case in HTTP:
        if case[-1] in ('#948', '#949'):
            set_site(fixed, site_id(case), None, None, kind='dynamic-url', candidates=ORDER_CANDIDATES)
        if case[-1] == '#810':
            set_configured(fixed, site_id(case))
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
    set_site(opened, overridden, None, None, kind='dynamic-url')
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


def selftest(archive=None):
    with TemporaryDirectory() as directory:
        root = Path(directory)
        replay(root, 'synthetic', synthetic(), unresolved_kotlin())
        if archive:
            with ZipFile(archive) as zipfile:
                captures = 'flows/smoke-resolution/commands/'
                for slot in ('base', 'head'):
                    replay(root, f'archived-{slot}', json.loads(zipfile.read(captures + slot + '/app.out')),
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
    print('smoke-stack selftest: revision outcomes, exact site joins, Kotlin keys and exit codes passed')
