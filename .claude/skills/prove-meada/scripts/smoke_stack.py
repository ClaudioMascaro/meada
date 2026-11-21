#!/usr/bin/env python3
"""Drive the pinned public smoke workspaces through Meada's CLI and MCP."""
import argparse
from contextlib import closing
import hashlib
import itertools
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import sys
import traceback

from proof_result import BLOCKED, TIMEOUT, Comparisons, execution_exit, run_driver
from prove import ROOT, export_diff, git, mcp_answers

NAMES = tuple(f'meada-smoke-{name}' for name in ('app', 'lib', 'shared'))
ORIGINS = {name: f'https://github.com/ClaudioMascaro/{name}.git' for name in NAMES}
APP, LIB, SHARED = (f'github.com/claudiomascaro/{name}' for name in NAMES)
ROUTE = 'http::POST::/orders'
SHARED_HOST = 'host:shared.smoke.example'
ORDERS = {repo: f'{repo}::openapi.yaml::POST /orders' for repo in (LIB, SHARED)}
QUEUE = 'channel::sqs::smoke-orders'
QUEUE_NODE = f'{LIB}::queues.tf::aws_sqs_queue.orders'
ORIGINAL = ('280efe9d33217ac8750659b987877a004c963f32',
            'a36abd395d4a5fa07ddbd37ce23971a6aceab961',
            '51703923bd1f76c0b948db8afb1fa52fe86300c1')
EXPANDED = ('3d5c209a84d71c609f3495fd68a163100ca01fb7',
            'cb746acf3fef864145d4cedad8a21977414b3309',
            '0aabcee48fe9d2d2e401a514bcfd0f4af8984501')
PINS = {
    'original': ORIGINAL,
    'expanded': EXPANDED,
    'class-authority': ('ed13044ebad552f4dc8dd8ac4ab8fb35bbab8f3a', *EXPANDED[1:]),
    'published': ('fffac4a975ccb37b72abadb01a82159e3b6374f1', *EXPANDED[1:]),
    'linked': ('d4d76058f3dbe777854d05e8e0d781a37bee6cd1', *ORIGINAL[1:]),
    'malformed': ('ad48b4c17536a2c35102ae87c34fca85807eb700', *ORIGINAL[1:]),
}
KEY = Path(__file__).with_name('smoke-stack')


def save(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + '\n')


class Smoke:
    def __init__(self, args):
        self.slice = args.slice
        self.run = Path(args.run).resolve()
        self.run.mkdir(parents=True)
        self.work = self.run / 'work'
        self.work.mkdir()
        self.checks, self.checkouts = [], {}
        self.parity = Comparisons(self.run)
        self.revision = 'shared'
        self.env = {key: value for key, value in os.environ.items()
                    if not key.startswith(('LC_', 'MEADA_DECIDER_'))}
        self.env.update(LANG='C.UTF-8', LC_ALL='C')
        self.bins = {label: str(Path(path).resolve()) for label, path in
                     (('base', args.base_bin), ('head', args.head_bin), ('old', args.old_bin),
                      ('middle', args.middle_bin)) if path}
        self.source = Path(args.source_dir).resolve() if args.source_dir else self.work / 'sources'
        self.capture_sources = not args.source_dir
        self.fetched = set()
        self.scored = {}
        shutil.copy(KEY / 'README.md', self.run / 'answer-key.md')
        provenance = {}
        for slot, path in self.bins.items():
            with Path(path).open('rb') as binary:
                sha256 = hashlib.file_digest(binary, 'sha256').hexdigest()
            origin = Path(path + '.from')
            provenance[slot] = {'sha256': sha256, 'from': origin.read_text().strip() if origin.is_file() else None}
        save(self.run / 'inputs.json', {'driver_revision': git('rev-parse', 'HEAD', cwd=ROOT).strip(),
                                       'corpora': {kind: dict(zip(NAMES, pins)) for kind, pins in PINS.items()},
                                       'binaries': self.bins, 'binary_provenance': provenance})

    def check(self, name, expected, actual, *, matches=None, revision=None, key=None):
        passed = expected == actual if matches is None else matches
        self.record({'name': name, 'result': 'pass' if passed else 'fail',
                     'revision': revision or self.revision, 'key': key,
                     'expected': expected, 'actual': actual})
        return passed

    def record(self, check):
        check.setdefault('revision', self.revision)
        self.checks.append(check)
        save(self.run / 'checks.json', self.checks)
        print(f"{check['result'].upper()} {check['name']}", flush=True)

    def phase(self, name, action, *args, revision='shared'):
        self.revision = revision
        try:
            action(self, *args)
        except Exception as error:
            status = {BLOCKED: 'blocked', TIMEOUT: 'timeout'}.get(execution_exit(error), 'error')
            evidence = f'{name}.{status}.txt'
            (self.run / evidence).write_text(traceback.format_exc())
            self.record({'name': name, 'result': status, 'evidence': evidence})
        finally:
            self.revision = 'shared'

    def clone(self, label, kind):
        pins = PINS[kind]
        checkouts = {}
        for name, sha in zip(NAMES, pins):
            source = self.source / name
            if self.capture_sources and (name, sha) not in self.fetched:
                if not source.exists():
                    source.mkdir(parents=True)
                    git('init', '-q', cwd=source)
                    git('remote', 'add', 'origin', ORIGINS[name], cwd=source)
                result = self.command(f'fetch/{name}-{sha}', source,
                                      ['git', 'fetch', '-q', '--no-tags', '--depth=1', 'origin', sha],
                                      timeout=90, check=False)
                if result.returncode:
                    missing = (f'upload-pack: not our ref {sha}', f"couldn't find remote ref {sha}")
                    if any(line.startswith('fatal: ') and line.endswith(missing)
                           for line in result.stderr.splitlines()):
                        raise FileNotFoundError(f'{ORIGINS[name]} at {sha}: {result.stderr.strip()}')
                    result.check_returncode()
                self.fetched.add((name, sha))
            target = self.work / label / name
            checkouts[name] = self.checkout(label, source, target, sha, name)
            evidence = self.run / 'sources' / kind / f'{name}.tar'
            if not evidence.exists():
                evidence.parent.mkdir(parents=True, exist_ok=True)
                with evidence.open('wb') as archive:
                    subprocess.run(['git', 'archive', sha], cwd=target, stdout=archive, check=True, timeout=30)
        self.checkouts[label] = checkouts
        save(self.run / 'checkouts.json', self.checkouts)
        return tuple(self.work / label / name for name in NAMES)

    def checkout(self, label, source, target, sha, name):
        shutil.copytree(source, target, symlinks=True, ignore=shutil.ignore_patterns('.meada'))
        origin = git('config', '--get', 'remote.origin.url', cwd=target).strip()
        if origin != ORIGINS[name]:
            raise ValueError(f'unexpected origin at {target}: {origin}, need {ORIGINS[name]} at {sha}')
        step = f'checkout/{label}/{name}-{sha}'
        result = self.command(step, target, ['git', 'checkout', '-q', '--detach', sha], check=False)
        if result.returncode:
            # A checkout can fail with the object present (e.g. a worktree lock).
            probe = self.command(step + '-object', target, ['git', 'cat-file', '--batch-check'], stdin=sha + '\n')
            if probe.stdout == f'{sha} missing\n':
                raise FileNotFoundError(f'{origin} at {sha}: {result.stderr.strip()}')
            result.check_returncode()
        head = git('rev-parse', 'HEAD', cwd=target).strip()
        if head != sha or git('status', '--porcelain', cwd=target):
            raise ValueError(f'unexpected source at {target}: {origin}, {head}, need {sha}')
        return {'origin': origin, 'head': head, 'tree': git('rev-parse', 'HEAD^{tree}', cwd=target).strip()}

    def command(self, label, cwd, argv, *, check=True, stdin='', env=None, timeout=45):
        location = self.run / 'commands' / label
        location.parent.mkdir(parents=True, exist_ok=True)
        code, stdout, stderr = None, '', ''
        try:
            result = subprocess.run(argv, cwd=cwd, capture_output=True, text=True, input=stdin,
                                    env={**self.env, **(env or {})}, timeout=timeout)
            code, stdout, stderr = result.returncode, result.stdout, result.stderr
        except subprocess.TimeoutExpired as error:
            stdout = (error.stdout or b'').decode(errors='replace')
            stderr = (error.stderr or b'').decode(errors='replace')
            raise
        finally:
            location.with_name(location.name + '.out').write_text(stdout)
            location.with_name(location.name + '.err').write_text(stderr)
            save(location.with_name(location.name + '.json'), {'argv': argv, 'cwd': str(cwd), 'exit': code})
        if check and code:
            raise RuntimeError(f'{label}: exit {code}; see commands/{label}.err')
        return result

    def cli(self, label, root, binary, *argv, **kwargs):
        return self.command(label, root, [self.bins[binary], *argv], **kwargs)

    def export(self, label, root, binary, *argv):
        return json.loads(self.cli(label, root, binary, 'export', *argv).stdout)

    def mcp(self, label, root, binary, queries):
        directory = self.run / 'mcp' / label
        directory.mkdir(parents=True)
        tools, texts, _ = mcp_answers(self.bins[binary], root, queries, directory / 'stderr')
        save(directory / 'answers.json', {'queries': queries, 'tools': tools, 'answers': texts})
        return texts

    def score(self, binary, name, expected, actual, matches=None):
        self.scored.setdefault(binary, {})[name] = self.check(
            f'{binary} {name}', expected, actual, matches=matches, revision=binary, key=name)
        save(self.run / 'denominator.json', {'corpus': 'expanded', 'checks': self.scored,
                                           'totals': {revision: {'passed': sum(checks.values()),
                                                                 'checked': len(checks),
                                                                 'expected': 13 if self.slice == 'resolution' else 3}
                                                      for revision, checks in self.scored.items()}})

    def finish(self):
        statuses = ('pass', 'fail', 'blocked', 'timeout', 'error')
        counts = lambda rows: {status: sum(row['result'] == status for row in rows) for status in statuses}
        revisions = {slot: counts([row for row in self.checks if row['revision'] == slot])
                     for slot in ('base', 'head', 'shared')}
        paired = {}
        for row in self.checks:
            if row.get('key'):
                paired.setdefault(row['key'], {})[row['revision']] = row
        transitions = {('pass', 'pass'): 'stable', ('fail', 'pass'): 'recovered',
                       ('pass', 'fail'): 'regression', ('fail', 'fail'): 'still rejected'}
        for key, pair in paired.items():
            if 'base' in pair and 'head' in pair:
                pair['transition'] = transitions[pair['base']['result'], pair['head']['result']]
                self.parity.compare(key, pair['base']['actual'], pair['head']['actual'])
        save(self.run / 'transitions.json', paired)
        auxiliary = {}
        for prefix, group, expected in (('MCP ', 'http-mcp', 3), ('#962 ', 'python-class', 6)):
            for slot in ('base', 'head', 'old'):
                rows = [row for row in self.checks if row['revision'] == slot and
                        (row.get('key') or '').startswith(prefix)]
                if rows:
                    auxiliary.setdefault(group, {})[slot] = {'passed': counts(rows)['pass'],
                                                             'checked': len(rows), 'expected': expected}
        if auxiliary:
            save(self.run / 'auxiliary-denominators.json', auxiliary)
        combined = counts(self.checks)
        for status, code in (('timeout', TIMEOUT), ('blocked', BLOCKED), ('error', 1)):
            if combined[status]:
                break
        else:
            code = self.parity.exit_code(failed=bool(revisions['head']['fail'] or revisions['shared']['fail']))
        save(self.run / 'result.json', {'counts': combined, 'revisions': revisions,
                                       'differences': self.parity.differences, 'exit': code, 'verdict': 'UNPROVEN'})
        print(f'capture finished: {revisions}. Judge against answer-key.md.')
        return code


def rows_at(data, section, field, path, name):
    return [row for row in data[section]
            if f'::{path}::' in row[field] and name in row[field].split('::')[2]]


def endpoint_observation(data, site):
    return {'site': site,
            'consumers': [row for row in data['consumers'] if row['site'] == site],
            'links': [row for row in data['links'] if row['consumer'] == site],
            'items': [row for row in data.get('open_items', []) if row['call'] == site]}


def compare_export(rig, name, expected, actual):
    changed, _ = export_diff(expected, actual)
    rig.check(name, [], changed)


def configured_links(links, repo=LIB):
    return (len(links) == 1 and links[0]['provider'] == ORDERS[repo]
            and links[0]['contract'] == ROUTE and links[0]['level'] == 'inferred'
            and all(part in links[0]['reason'] for part in ('SMOKE_ORDERS_URL', 'base_url_env', repo)))


def consumer_is(observation, contract, via):
    return len(observation['consumers']) == 1 and all(
        field in row and row[field] == value
        for row in observation['consumers'] for field, value in [('contract', contract), ('via', via)])


def configured_order(observation, repo=LIB):
    return (consumer_is(observation, ROUTE, 'env:SMOKE_ORDERS_URL') and not observation['items']
            and configured_links(observation['links'], repo))


def decision_is(observation, kind, candidates):
    items = observation['items']
    return len(items) == 1 and (items[0].get('kind'), items[0].get('candidates')) == (kind, candidates)


ORDER_CANDIDATES = [f'{repo}::{ROUTE}' for repo in ORDERS]


def ambiguous_order(observation):
    links = observation['links']
    return (consumer_is(observation, ROUTE, None)
            and len(links) == 2 and {row['provider'] for row in links} == set(ORDERS.values())
            and all(row['contract'] == ROUTE and row['level'] == 'ambiguous' and row['reason'] for row in links)
            and decision_is(observation, 'ambiguous-link', ORDER_CANDIDATES))


def overridden_order(observation, helper, candidates=ORDER_CANDIDATES):
    links = observation['links']
    if not links:
        return (consumer_is(observation, None, None)
                and decision_is(observation, 'dynamic-url', candidates)
                and observation['items'][0].get('limit') == f'base from the return value of {helper}')
    if len(links) == 1:
        link = links[0]
        return (consumer_is(observation, ROUTE, SHARED_HOST) and not observation['items']
                and link['provider'] == ORDERS[SHARED] and link['contract'] == ROUTE
                and link['level'] in ('exact', 'inferred') and 'shared.smoke.example' in link['reason'])
    return ambiguous_order(observation)


HTTP = (
    ('go/client.go', 'SendConfigured', 'http.Post#0', None),
    ('go/client.go', 'SendShared', 'http.Post#0', '#948'),
    ('python/client.py', 'send_configured', 'requests.post#0', None),
    ('python/client.py', 'send_shared', 'requests.post#0', '#949'),
    ('java/OrdersClient.java', 'OrdersClient.sendOrder()', 'http.postForObject#0', '#810'),
    ('kotlin/Orders.kt', 'KotlinOrders.sendOrder()', 'http.postForObject#0', '#810'),
    ('csharp/OrdersClient.cs', 'OrdersClient.SendOrder()', 'http.PostAsync#0', None),
    ('rust/src/lib.rs', 'send_order', '(call_expression).post#0', None),
    ('swift/Orders.swift', 'sendOrder()', 'URLSession.shared.data#0', None),
    ('typescript/client.ts', 'sendOrder', 'fetch#0', None),
)
OVERRIDDEN = {'#948': 'sharedBase', '#949': 'shared_base'}


def client_site(path, symbol, call):
    return f'{APP}::orders/clients/{path}::{symbol}::{call}'


# The pinned sources' search locations, including clients added before the eight-pack
# corpus. The four version-sensitive requests have independent export source keys.
SEARCH_LINES = (20, 24, 30, 26, 11, 7, 11, 8, 13, 6)
STABLE_ORDERS = tuple(f'orders/evidence/client.go:{line}' for line in (13, 20, 24, 31, 39)) + tuple(
    f'orders/{name}/client.go:17' for name in ('httpkey', 'neutral', 'oskey'))


def provision_ok(rows, known):
    if not known:
        return not rows
    return (len(rows) == 1 and (rows[0]['contract'], rows[0]['provisioner'], rows[0]['level'])
            == (QUEUE, QUEUE_NODE, 'exact'))


def sqs(rig, data, binary, *, historical=False):
    for name, known in (('publishOrder', True), ('publishLiteral', True), ('publishSelected', False)):
        site = f'{APP}::orders/events/publisher.ts::{name}::SendMessageCommand#0'
        rows = {'providers': [row for row in data['providers'] if row['site'] == site],
                'provisions': [row for row in data['provisions'] if row['side'] == site]}
        expected = [QUEUE, QUEUE_NODE, 'exact'] if known else 'no selected queue at runtime choice'
        if historical:
            # The inherited SDK regression drops the single-return helper at this tier.
            known = name == 'publishLiteral'
        providers = rows['providers']
        matches = (len(providers) == 1 and providers[0]['contract'] == (QUEUE if known else 'channel::sqs')
                   and provision_ok(rows['provisions'], known)
                   and all(row['side'] == providers[0]['site'] for row in rows['provisions']))
        if historical:
            rig.check(f'68f1c0b {name} historical observation', [QUEUE, QUEUE_NODE, 'exact'] if known else
                      'channel::sqs with no selected queue', rows, matches=matches)
        else:
            rig.score(binary, f'SQS {name}', expected, rows, matches)


def jvm(rig, data, binary, variant, path, symbol, helper):
    site = f'{APP}::orders/clients/{path}::{symbol}::{helper}#0'
    calls = [row for row in data['calls'] if row['id'] == site]
    owner = symbol.rsplit('.', 1)[0]
    target = f'{APP}::orders/clients/{path}::{owner}.{helper}()'
    actual = [{'target': row['target'], 'level': row['level'], 'candidates': row.get('candidates', [])} for row in calls]
    rig.check(f'{binary} {variant} {owner}: private code hop', [{'target': target, 'level': 'exact', 'candidates': []}], actual)


def transport(rig, language, app=None, variant='pinned', overridden='shared'):
    if app is None:
        app, _, _ = rig.clone(f'transport-{language}', 'expanded')
    root = rig.work / f'record-{language}-{variant}'
    root.mkdir()
    url = 'https://lib.smoke.example'
    step = f'transport/{language}' if variant == 'pinned' else f'transport/{variant}'
    if language == 'go':
        shutil.copy(app / 'orders/clients/go/client.go', root)
        shutil.copy(KEY / 'transport_test.go', root)
        rig.command(step + '-version', root, ['go', 'version'])
        got = rig.command(step, root, ['go', 'test', '-v', 'client.go', 'transport_test.go'],
                          env={'GO111MODULE': 'off', 'GOTOOLCHAIN': 'local', 'GOCACHE': str(rig.work / 'go-cache'),
                               'SMOKE_ORDERS_URL': url}, timeout=150)
        records = [json.loads(line[len('PROBE '):]) for line in got.stdout.splitlines() if line.startswith('PROBE ')]
        callers = ('SendShared', 'SendConfigured')
    else:
        source = app / 'orders/clients/python/client.py'
        got = rig.command(step, root, [sys.executable, str(KEY / 'transport_record.py'), str(source)],
                          env={'SMOKE_ORDERS_URL': url, 'SMOKE_OVERRIDE': '1'})
        records = json.loads(got.stdout)
        callers = ('send_shared', 'send_configured')
    rig.check(f'{language} {variant} transport receives overridden and ordinary URL', [
        {'caller': callers[0], 'method': 'POST', 'url': f'https://{overridden}.smoke.example/orders'},
        {'caller': callers[1], 'method': 'POST', 'url': url + '/orders'},
    ], records)


def override_controls(rig):
    app, _, _ = rig.clone('go-defer-controls', 'expanded')
    source = app / 'orders/clients/go/client.go'
    original = source.read_text()
    benign = original.replace('\t\tbase = "https://shared.smoke.example"\n', '')
    if benign == original:
        raise ValueError('Go benign-defer overlay did not change the deferred write')
    source.write_text(benign)
    (rig.run / 'go-benign-defer.go').write_text(benign)
    transport(rig, 'go', app, 'go-benign-defer', 'lib')
    data = rig.export('go-benign-defer/export', app, 'head')
    site = f'{APP}::orders/clients/go/client.go::SendShared::http.Post#0'
    got = endpoint_observation(data, site)
    rig.check('#948 benign defer retains configured peer', ORDERS[LIB], got, matches=configured_order(got))

    app, _, _ = rig.clone('python-rebinding-controls', 'expanded')
    source = app / 'orders/clients/python/client.py'
    original = source.read_text()
    plain = original.replace('@use_host("https://shared.smoke.example")\n', '')
    marker = '\ndef send_shared():'
    if plain == original or plain.count(marker) != 1:
        raise ValueError('Python rebinding overlay could not locate the original helper')
    replacement = 'replacement = use_host("https://shared.smoke.example")(shared_base)\n'
    site = f'{APP}::orders/clients/python/client.py::send_shared::requests.post#0'
    ordinary = f'{APP}::orders/clients/python/client.py::send_configured::requests.post#0'
    expected = None
    for variant, inserted, open_site in [
        ('plain', '', False),
        ('direct', replacement + 'shared_base = replacement\n', True),
        ('alias', replacement + 'alias = replacement\nshared_base = alias\n', True),
        ('conditional', replacement + 'if os.getenv("SMOKE_OVERRIDE"):\n    shared_base = replacement\n', True),
        ('unrelated', 'other = 1\nother = 2\n', False),
        ('restored', '', False),
    ]:
        changed = plain.replace(marker, f'\n{inserted}\n{marker}')
        source.write_text(changed if inserted else plain)
        label = f'python-{variant}'
        shutil.copyfile(source, rig.run / f'{label}.py')
        transport(rig, 'python', app, label, 'shared' if open_site else 'lib')
        data = rig.export(f'{label}/export', app, 'head')
        point, control = (endpoint_observation(data, name) for name in (site, ordinary))
        rig.check(f'#949 {variant} original helper binding',
                  'no selected lib peer' if open_site else ORDERS[LIB], point,
                  matches=overridden_order(point, 'shared_base') if open_site else configured_order(point))
        rig.check(f'#949 {variant} configured control', ORDERS[LIB], control, matches=configured_order(control))
        if variant == 'plain':
            expected = [point, control]
        if variant == 'restored':
            rig.check('#949 restored source restores both site observations', expected, [point, control])


def score_http(rig, binary, data):
    for path, symbol, call, issue in HTTP:
        evidence = endpoint_observation(data, client_site(path, symbol, call))
        expected = {'contract': ROUTE, 'provider': ORDERS[LIB],
                    'level': 'inferred', 'reason': 'via SMOKE_ORDERS_URL; lib is the declaring peer'}
        if issue in OVERRIDDEN:
            expected = {'correct provider': ORDERS[SHARED], 'via': SHARED_HOST,
                        'otherwise': {'candidates': ORDER_CANDIDATES,
                                      'limit': f'base from the return value of {OVERRIDDEN[issue]}'}}
        matches = overridden_order(evidence, OVERRIDDEN[issue]) if issue in OVERRIDDEN else configured_order(evidence)
        rig.score(binary, f'HTTP {path} {symbol}' + (f' {issue}' if issue else ''), expected, evidence, matches)


def score_http_mcp(rig, binary, data, texts):
    known = {row['site'] for row in data['consumers'] if row['contract'] == ROUTE}
    # Check by location. The two overridden calls and two JVM calls can legitimately
    # differ between slots. Their own export oracles decide those four rows.
    callers = list(STABLE_ORDERS)
    for (path, symbol, call, issue), line in zip(HTTP, SEARCH_LINES, strict=True):
        if not issue or client_site(path, symbol, call) in known:
            callers.append(f'orders/clients/{path}:{line}')
    search = sorted([f'{APP} {caller} {ROUTE}' for caller in callers]
                    + [f'{repo} openapi.yaml (documented only) {ROUTE}' for repo in ORDERS])
    context = [
        ROUTE, *(f'{repo} provider openapi.yaml (documented only)' for repo in ORDERS),
        f'{len(callers)} linked consumers omitted. See trace with direction "up".',
    ]
    trace = {
        'start': f'{APP} .github/workflows/ci.yml:32 jobs["through-lib"]',
        'hops': [
            f'→[exact: @main (indexed {EXPANDED[1][:7]})] {LIB} .github/workflows/check.yml:1 .',
            f'→[exact: @main (indexed {EXPANDED[2][:7]})] {SHARED} .github/workflows/build.yml:1 .',
        ],
        'chain': ['chain: exact · 2 hops · 3 repos'],
    }
    searched, contextual, traced = ([line.strip() for line in text.splitlines() if line.strip()] for text in texts)
    spans = {'start': traced[0] if traced else None,
             'hops': [line for line in traced if line.startswith('→[')],
             'chain': [line.rsplit(' · ', 1)[0] for line in traced if line.startswith('chain: ')]}
    for key, expected, actual in [
        ('MCP POST /orders search', search, sorted(searched)),
        ('MCP POST /orders context', context, contextual),
        ('MCP CI two-hop trace', trace, spans),
    ]:
        rig.check(f'{binary} {key}', expected, actual, revision=binary, key=key)


def http_cases(rig, binary, app, data):
    score_http(rig, binary, data)
    for path, symbol, _, issue in HTTP:
        if issue == '#810':
            jvm(rig, data, binary, 'combined', path, symbol, 'ordersBase')
    sqs(rig, data, binary)
    mcp = [
        {'tool': 'search', 'arguments': {'query': '/orders', 'kind': 'route', 'budget': 4000}},
        {'tool': 'context', 'arguments': {'symbol': ROUTE, 'budget': 8000}},
        {'tool': 'trace', 'arguments': {'from': f'{APP}::.github/workflows/ci.yml::jobs["through-lib"]',
                                        'direction': 'down', 'budget': 8000}},
    ]
    score_http_mcp(rig, binary, data, rig.mcp(binary, app, binary, mcp))


def jvm_controls(rig):
    variants = (
        ('java-alone', 'kotlin/Orders.kt', None),
        ('kotlin-alone', 'java/OrdersClient.java', None),
        ('rename-java', None, 'java/OrdersClient.java'),
        ('rename-kotlin', None, 'kotlin/Orders.kt'),
    )
    for variant, drop, rename in variants:
        app, _, _ = rig.clone(variant, 'expanded')
        if drop:
            (app / 'orders/clients' / drop).unlink()
        if rename:
            source = app / 'orders/clients' / rename
            source.write_text(source.read_text().replace('ordersBase', 'ordersBasePrivateProbe'))
        rig.cli(variant + '/index', app, 'head', 'index')
        data = rig.export(variant + '/export', app, 'head')
        for path, symbol, call, issue in HTTP:
            if issue != '#810' or path == drop:
                continue
            jvm(rig, data, 'head', variant, path, symbol, 'ordersBasePrivateProbe' if path == rename else 'ordersBase')
            got = endpoint_observation(data, f'{APP}::orders/clients/{path}::{symbol}::{call}')
            rig.check(f'#810 {variant} {symbol} HTTP provider', ORDERS[LIB], got, matches=configured_order(got))


def peer_controls(rig, app, lib, baseline):
    wanted = [(ORDERS[LIB], 'ambiguous'), (ORDERS[SHARED], 'ambiguous')]
    relative = endpoint_observation(baseline, f'{APP}::orders/evidence/client.go::Relative::http.Post#0')
    rig.check('authority-free /orders has two alternatives', wanted, relative, matches=ambiguous_order(relative))
    for name, symbol in [('Archive', 'write'), ('Choose', 'writeConditional'), ('Current', 'Current')]:
        file = 'orders/mutation/store.go'
        rows = [row for row in rows_at(baseline, 'providers', 'site', file, symbol)
                if row['site'].split('::')[2] == symbol]
        named_orders = [row for row in rows if row['contract'] == 'resource::dynamodb::orders']
        if name != 'Current':
            target = f'{APP}::{file}::{symbol}'
            calls = [row for row in rows_at(baseline, 'calls', 'id', file, name) if row['target'] == target]
            rig.check(f'#891 {name} reaches {symbol} exactly', 'exact',
                      [row['level'] for row in calls], matches=len(calls) == 1 and calls[0]['level'] == 'exact')
        rig.check(f'#891 {name} orders writer', 'orders' if name == 'Current' else 'no unconditional orders writer',
                  rows, matches=len(rows) == 1 and len(named_orders) == (1 if name == 'Current' else 0))
    config = app / 'meada.toml'
    original = config.read_text()
    blocks = original.split('[[peer]]')
    config.write_text(blocks[0] + ''.join('[[peer]]' + block for block in reversed(blocks[1:])))
    reordered = rig.export('peers/reordered', app, 'head')
    rig.check('peer order preserves whole app links and candidates',
              [baseline['links'], baseline['open_items']], [reordered['links'], reordered['open_items']])
    declaration = 'base_url_env = ["SMOKE_ORDERS_URL"]'
    reassigned = original.replace(declaration, '').replace('path = "../meada-smoke-shared"',
                    f'path = "../meada-smoke-shared"\n{declaration}')
    config.write_text(reassigned)
    other = rig.export('peers/reassigned', app, 'head')
    outcome = endpoint_observation(other, f'{APP}::orders/evidence/client.go::Client.ViaMethod::http.Post#0')
    rig.check('provider changes with the declared authority', ORDERS[SHARED], outcome,
              matches=configured_order(outcome, SHARED))
    config.write_text(original)
    route = lib / 'openapi.yaml'
    saved = route.read_bytes()
    edited = saved.replace(b'  /orders:', b'  /orders-renamed:')
    if edited == saved:
        raise ValueError('route edit did not touch the declared path')
    route.write_bytes(edited)
    changed = rig.export('peers/edited-warm', app, 'head')
    copy, cold_lib, _ = rig.clone('edited-cold', 'published')
    (cold_lib / 'openapi.yaml').write_bytes(edited)
    rig.cli('peers/edited-cold-index', copy, 'head', 'index')
    cold = rig.export('peers/edited-cold', copy, 'head')
    rig.check('peer edit warm/cold links and candidates', [cold['links'], cold['open_items']],
              [changed['links'], changed['open_items']])
    no_route = [row for row in changed['links'] if row['provider'] == ORDERS[LIB]]
    rig.check('edited lib no longer provides POST /orders', [], no_route)
    route.write_bytes(saved)
    restored = rig.export('peers/restored', app, 'head')
    rig.check('restored peer links and candidates', [baseline['links'], baseline['open_items']],
              [restored['links'], restored['open_items']])
    save(rig.run / 'peer-mutations.json', {'meada.toml': [original, reassigned], 'route': [saved.decode(), edited.decode()]})


def ci_keys(rig, data):
    job = f'{LIB}::.github/workflows/ci.yml::jobs.test.steps'
    install, program = f'{job}[2]', f'{job}[3]'
    sides = [row for row in data['consumers'] if row['site'] == install and (row['contract'] or '').startswith('package::')]
    rig.check('#945 pip install . names no package', [], sides)
    links = [row for row in data['links'] if row['consumer'] == program]
    rig.check('#945 smoke-cli site links to its entry point at inferred',
              {'provider': f'{LIB}::src/smoke_cli/main.py::run', 'level': 'inferred'}, links,
              matches=len(links) == 1 and links[0]['provider'] == f'{LIB}::src/smoke_cli/main.py::run'
              and links[0]['level'] == 'inferred')


def resolution(rig, binary):
    app, lib, shared = rig.clone(binary + '-expanded', 'expanded')
    rig.cli(binary + '/index', app, binary, 'index')
    data = rig.export(binary + '/app', app, binary)
    lib_data = rig.export(binary + '/lib', lib, binary)
    rig.export(binary + '/shared', shared, binary)
    rig.cli(binary + '/status', app, binary, 'status')
    rig.cli(binary + '/list', app, binary, 'decide', '--list')
    http_cases(rig, binary, app, data)
    ci_keys(rig, lib_data)


def expanded_upgrade(rig):
    app = rig.work / 'base-expanded' / NAMES[0]
    expected = json.loads((rig.run / 'commands/head/app.out').read_text())
    sites = [f'{APP}::orders/clients/{path}::{symbol}::{call}' for path, symbol, call, _ in HTTP]
    before = git('status', '--porcelain', cwd=app)
    for stage in ('first-export', 'after-index', 'reopened'):
        if stage == 'after-index':
            rig.cli('expanded-upgrade/index', app, 'head', 'index')
        data = rig.export(f'expanded-upgrade/{stage}', app, 'head')
        rig.check(f'expanded binary swap {stage} preserves cold HTTP site evidence',
                  [endpoint_observation(expected, site) for site in sites],
                  [endpoint_observation(data, site) for site in sites])
        rig.check(f'expanded binary swap {stage} preserves cold SQS provisions',
                  expected['provisions'], data['provisions'])
        compare_export(rig, f'expanded binary swap {stage} whole export matches cold', expected, data)
    rig.check('expanded binary swap leaves input checkouts clean', ['', '', '', ''],
              [before, *(git('status', '--porcelain', cwd=app.parent / name) for name in NAMES)])


CLASS_PATH = 'python/class_probe.py'
CLASS_CALLERS = ('Changed.send', 'Ordinary.send')


def class_authority(rig, binary):
    app, _, _ = rig.clone(f'class-{binary}', 'class-authority')
    source = app / 'orders/clients' / CLASS_PATH
    decorated = source.read_bytes()
    original = decorated.replace(b'@install\n', b'')
    if decorated.count(b'@install\n') != 1:
        raise ValueError('the class fixture needs one @install decorator')
    rig.cli(f'class/{binary}/index', app, binary, 'index')
    cold = None
    for state, content, host in [('decorated', decorated, 'shared'), ('removed', original, 'lib'),
                                 ('restored', decorated, 'shared')]:
        if state != 'decorated':
            source.write_bytes(content)
        label = f'class/{binary}/{state}'
        variant = rig.run / 'class-sources' / f'{state}.py'
        variant.parent.mkdir(exist_ok=True)
        variant.write_bytes(content)
        records = json.loads(rig.command(label + '/runtime', app,
                                        [sys.executable, 'scripts/record-class-authority.py'],
                                        env={'SMOKE_ORDERS_URL': 'https://lib.smoke.example'}).stdout)
        data = rig.export(label + '/export', app, binary)
        runtime = [{'caller': caller, 'method': 'POST', 'url': f'https://{peer}.smoke.example/orders'}
                   for caller, peer in zip(CLASS_CALLERS, (host, 'lib'), strict=True)]
        rig.check(f'{binary} #962 {state}: recorder', runtime, records)
        for request, peer in zip(runtime, (host, 'lib'), strict=True):
            caller = request['caller']
            site = client_site(CLASS_PATH, caller, 'requests.post#0')
            evidence = endpoint_observation(data, site)
            key = f'#962 {state} {caller}'
            expected = {'runtime': request, 'provider': ORDERS[LIB], 'via': 'env:SMOKE_ORDERS_URL'}
            matches = configured_order(evidence)
            if peer == 'shared':
                helper = 'self.changed_origin'
                expected.update(provider=ORDERS[SHARED], via=SHARED_HOST,
                                otherwise={'candidates': ORDER_CANDIDATES,
                                           'limit': f'base from the return value of {helper}'})
                matches = overridden_order(evidence, helper)
            rig.check(f'{binary} {key}', expected, evidence, key=key, matches=matches and request in records)
        if state == 'decorated':
            cold = data
        if state == 'restored':
            compare_export(rig, f'{binary} #962 restoring the source restores the whole cold export', cold, data)


def class_index_version(app):
    with closing(sqlite3.connect((app / '.meada/index.db').as_uri() + '?mode=ro', uri=True)) as db:
        return db.execute('PRAGMA user_version').fetchone()[0]


def class_upgrade(rig):
    app, lib, shared = rig.clone('class-upgrade', 'class-authority')
    source = app / 'orders/clients' / CLASS_PATH
    old = 'old' if 'old' in rig.bins else 'base'
    rig.cli('class/upgrade/old-index', app, old, 'index')
    before = rig.export('class/upgrade/old-export', app, old)
    old_cold = json.loads((rig.run / f'commands/class/{old}/decorated/export.out').read_text())
    compare_export(rig, '#962 old index matches old cold export', old_cold, before)
    expected = json.loads((rig.run / 'commands/class/head/decorated/export.out').read_text())
    fingerprints = {'old': class_index_version(app)}
    source_hash = hashlib.sha256(source.read_bytes()).hexdigest()
    for stage in ('first-export', 'after-index', 'reopened'):
        if stage == 'after-index':
            rig.cli('class/upgrade/index', app, 'head', 'index')
        data = rig.export('class/upgrade/' + stage, app, 'head')
        compare_export(rig, f'#962 binary swap {stage} whole export matches cold', expected, data)
        fingerprints[stage] = class_index_version(app)
    fingerprints['cold'] = class_index_version(rig.work / 'class-head' / NAMES[0])
    rig.check('#962 first export refreshes the extraction fingerprint', fingerprints['cold'], fingerprints,
              matches=fingerprints['old'] != fingerprints['first-export'] and
              all(fingerprints[key] == fingerprints['cold'] for key in ('first-export', 'after-index', 'reopened')))
    rig.check('#962 binary swap preserves pinned source bytes', [source_hash, '', '', ''],
              [hashlib.sha256(source.read_bytes()).hexdigest(),
               *(git('status', '--porcelain', cwd=repo) for repo in (app, lib, shared))])


def published_controls(rig):
    app, lib, _ = rig.clone('published-head', 'published')
    rig.cli('published/index', app, 'head', 'index')
    data = rig.export('published/app', app, 'head')
    peer_controls(rig, app, lib, data)


def configuration_matrix(rig):
    """Three independently named routes per combination, with two callers per route."""
    matrix = []
    for number, (owner, dual, reversed_order, query, warm) in enumerate(itertools.product(
            (LIB, SHARED), (False, True), (False, True), (False, True), (False, True))):
        label = f'matrix/{number:02}'
        app, lib, shared = rig.clone(f'matrix-{number:02}', 'published')
        peers = {LIB: lib, SHARED: shared}
        if warm:
            rig.cli(label + '/before-index', app, 'head', 'index')
        config = []
        for peer in sorted(peers, reverse=reversed_order):
            text = f'[[peer]]\nrepo = "{peer}"\npath = "../{peers[peer].name}"\n'
            config.append(text + ('base_url_env = ["SMOKE_ORDERS_URL"]\n' if peer == owner else ''))
        (app / 'meada.toml').write_text('\n'.join(config))
        paths = [f'/probe-order-{number * 3 + offset:03}' for offset in range(3)]
        routes = ''.join(f"  {path}:\n    post:\n      responses: {{'200': {{description: ok}}}}\n" for path in paths)
        for peer, repo in peers.items():
            if dual or peer == owner:
                with (repo / 'openapi.yaml').open('a') as document:
                    document.write(routes)
        client = app / 'matrix.go'
        source = 'package matrix\nimport ("net/http"; "os")\n'
        suffix = '?variant=fixed' if query else ''
        for index, path in enumerate(paths):
            source += (f'func Configured{index}() {{ http.Post(os.Getenv("SMOKE_ORDERS_URL")+"{path}{suffix}", "", nil) }}\n'
                       f'func Relative{index}() {{ http.Post("{path}{suffix}", "", nil) }}\n')
        client.write_text(source)
        data = rig.export(label + '/app-export', app, 'head')
        matrix.append({'owner': owner, 'both_providers': dual, 'reverse_peer_order': reversed_order,
                       'query': suffix, 'warm': warm, 'paths': paths, 'file': label + '/app-export.out'})
        for index, path in enumerate(paths):
            contract = f'http::POST::{path}'
            level = 'ambiguous' if dual else 'inferred'
            for name, expected_peers in ((f'Configured{index}', [owner]),
                                         (f'Relative{index}', sorted(peers) if dual else [owner])):
                got = endpoint_observation(data, f'{APP}::matrix.go::{name}::http.Post#0')
                expected_level = 'inferred' if name.startswith('Configured') else level
                expected = [(contract, f'{peer}::openapi.yaml::POST {path}', expected_level)
                            for peer in expected_peers]
                links = got['links']
                candidates = [f'{peer}::{contract}' for peer in expected_peers] if expected_level == 'ambiguous' else []
                rig.check(f'{label} {name} {path}', {'links': expected, 'candidates': candidates}, got,
                          matches=consumer_is(got, contract, 'env:SMOKE_ORDERS_URL' if name.startswith('Configured') else None)
                          and sorted((row['contract'], row['provider'], row['level']) for row in links) == expected
                          and (decision_is(got, 'ambiguous-link', candidates) if candidates else not got['items'])
                          and all(row.get('reason') for row in links)
                          and (not name.startswith('Configured') or all('SMOKE_ORDERS_URL' in row['reason'] for row in links)))
    save(rig.run / 'matrix.json', {'configurations': matrix, 'routes': 96, 'callers': 192})


KOTLIN_SHADOW = {
    'shadowed': ('Shadowing.hidden()', '/persons/hidden'),
    'other_class': ('Typed.typed()', '/persons/typed'),
}
TYPED_FIXTURE = 'src/main/kotlin/example/jvm/Typed.kt'


def score_kotlin(rig, binary, data):
    observations = {}
    for label, (symbol, route) in KOTLIN_SHADOW.items():
        key = f'#952 {symbol}'
        prefix = f'example.com/proof/kotlin-shadow::{TYPED_FIXTURE}::example.jvm.'
        site = f'{prefix}{symbol}::(call_expression).uri#0'
        got = observations[label] = endpoint_observation(data, site)
        got['code_calls'] = [row for row in data['calls']
                             if row['id'] in (site, f'{prefix}{symbol}::make#0')]
        uri = [row for row in got['code_calls'] if row['id'] == site]
        factory = [row for row in got['code_calls'] if row['id'] != site]
        source = (len(uri) == 1 and len(factory) == 1 and factory[0]['level'] == 'exact'
                  and factory[0]['target'] == f'{prefix}make()')
        literal = consumer_is(got, f'http::GET::{route}', 'host:typed.example.com')
        absent = not got['consumers']
        expected = {'url': f'GET https://typed.example.com{route}', 'links': [], 'items': [],
                    'uri_call': site, 'factory': f'{prefix}make() (exact)'}
        if label == 'shadowed':
            expected['alternative'] = 'pre-#947 receiver ceiling, no HTTP consumer'
            expected['policy'] = 'pending maintainer decision #952'
        rig.check(f'{binary} {key}', expected, got, revision=binary, key=key,
                  matches=source and not got['items'] and not got['links']
                  and (literal or label == 'shadowed' and absent))
    return observations


def kotlin_shadow(rig, binary):
    fixture = ROOT / 'tests/fixtures/conformance-kotlin'
    (rig.run / 'kotlin-shadow-typed.kt').write_bytes((fixture / TYPED_FIXTURE).read_bytes())
    root = rig.work / f'kotlin-shadow-{binary}'
    shutil.copytree(fixture, root)
    git('init', '-q', cwd=root)
    git('remote', 'add', 'origin', 'https://example.com/proof/kotlin-shadow.git', cwd=root)
    git('add', '.', cwd=root)
    git('commit', '-qm', 'Kotlin shadowing fixture', cwd=root)
    data = rig.export(f'kotlin-shadow/{binary}', root, binary)
    save(rig.run / f'kotlin-shadow-{binary}.json', score_kotlin(rig, binary, data))


def order_rows(data):
    return [row for row in data['links'] if row['contract'] == ROUTE]


def upgrade(rig, outcome_bin):
    app, _, _ = rig.clone(f'upgrade-{outcome_bin}', 'original')
    rig.cli(f'{outcome_bin}/old-index', app, 'old', 'index')
    old = rig.export(f'{outcome_bin}/old-export', app, 'old')
    expected = ('neutral',)
    parts = lambda links: sorted(row['consumer'].split('::')[1].split('/')[1] for row in links)
    rig.check(f'{outcome_bin} #941 indexed old control', list(expected), parts(order_rows(old)))
    reopened = rig.export(f'{outcome_bin}/first-export', app, outcome_bin)
    for label in ('refresh', 'second-refresh'):
        rig.cli(f'{outcome_bin}/{label}-index', app, outcome_bin, 'index')
        result = rig.export(f'{outcome_bin}/{label}-export', app, outcome_bin)
        rig.check(f'{outcome_bin} {label} stable links after executable swap', order_rows(reopened), order_rows(result))
    cold, _, _ = rig.clone(f'cold-{outcome_bin}', 'original')
    rig.cli(f'{outcome_bin}/cold-index', cold, outcome_bin, 'index')
    separate = rig.export(f'{outcome_bin}/cold-export', cold, outcome_bin)
    rig.check(f'{outcome_bin} cold index has three source-proven Go links',
              ['httpkey', 'neutral', 'oskey'], parts(order_rows(separate)))
    rig.check(f'{outcome_bin} cold providers, level and reason from the declared peer', 'three lib links',
              order_rows(separate), matches=all(configured_links([row]) for row in order_rows(separate)))
    if outcome_bin == 'middle':
        rig.check('#943 historical stale warm index retains only neutral', list(expected), parts(order_rows(reopened)))
    else:
        rig.check('#946 warm and cold provider, level, reason and site equality', order_rows(separate), order_rows(reopened))
    save(rig.run / f'{outcome_bin}-upgrade-metadata.json', {phase: {field: export.get(field) for field in ('commit', 'dirty')}
         for phase, export in [('old', old), ('reopened', reopened), ('refreshed', result), ('cold', separate)]})


def sdk_transition(rig):
    app, _, _ = rig.clone('sdk-68f1c0b', 'expanded')
    rig.cli('sdk/old-index', app, 'old', 'index')
    data = rig.export('sdk/old-export', app, 'old')
    sqs(rig, data, 'old', historical=True)
    warm = rig.export('sdk/first-export', app, 'head')
    rig.cli('sdk/head-index', app, 'head', 'index')
    refreshed = rig.export('sdk/refresh', app, 'head')
    cold, _, _ = rig.clone('sdk-cold-head', 'expanded')
    rig.cli('sdk/cold-index', cold, 'head', 'index')
    current = rig.export('sdk/cold-export', cold, 'head')
    sqs(rig, current, 'head')
    rig.check('SDK extraction warm/reindex/cold provisions', [current['provisions']] * 2,
              [warm['provisions'], refreshed['provisions']])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--selftest', action='store_true')
    parser.add_argument('--oracle-archive', type=Path, help='replay the scorer on a #953 or #960 proof ZIP')
    parser.add_argument('--slice', choices=('resolution', 'lifecycle', 'upgrade', 'sqs'), default='resolution')
    parser.add_argument('--run')
    parser.add_argument('--base-bin')
    parser.add_argument('--head-bin')
    parser.add_argument('--old-bin')
    parser.add_argument('--middle-bin')
    parser.add_argument('--source-dir', help='optional verified fetch cache containing the pinned revisions')
    args = parser.parse_args()
    if args.selftest or args.oracle_archive:
        from smoke_oracle_test import selftest
        selftest(args.oracle_archive)
        return
    if not args.run or not args.head_bin or args.slice in ('upgrade', 'sqs') and not args.old_bin:
        print('run directory, head binary and any historical binaries for the slice are required', file=sys.stderr)
        return BLOCKED
    rig = Smoke(args)
    if args.slice == 'resolution':
        for language in ('go', 'python'):
            rig.phase(f'runtime-{language}', transport, language)
        for binary in ('base', 'head'):
            if binary in rig.bins:
                rig.phase(f'resolution-{binary}', resolution, binary, revision=binary)
                rig.phase(f'kotlin-shadow-{binary}', kotlin_shadow, binary, revision=binary)
        if 'base' in rig.bins:
            rig.phase('expanded-upgrade', expanded_upgrade)
        for binary in ('old', 'base', 'head'):
            if binary in rig.bins:
                rig.phase(f'class-{binary}', class_authority, binary, revision=binary)
        if {'old', 'base'} & rig.bins.keys():
            rig.phase('class-upgrade', class_upgrade)
        rig.phase('private-methods', jvm_controls)
        rig.phase('override-controls', override_controls)
        rig.phase('peer-controls', published_controls)
        rig.phase('configuration-matrix', configuration_matrix)
    elif args.slice == 'upgrade':
        if args.middle_bin:
            rig.phase('historical-upgrade', upgrade, 'middle')
        rig.phase('current-upgrade', upgrade, 'head')
    elif args.slice == 'sqs':
        rig.phase('sdk-transition', sdk_transition)
    else:
        from smoke_lifecycle import lifecycle
        lifecycle(rig)
    return rig.finish()


if __name__ == '__main__':
    run_driver(main)
