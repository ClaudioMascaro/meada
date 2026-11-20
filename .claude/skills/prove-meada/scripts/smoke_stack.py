#!/usr/bin/env python3
"""Drive the pinned public smoke workspaces through Meada's CLI and MCP."""
import argparse
import itertools
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import traceback

from proof_result import BLOCKED, ORACLE_FAILED, run_driver
from prove import ROOT, git, mcp_answers

NAMES = tuple(f'meada-smoke-{name}' for name in ('app', 'lib', 'shared'))
ORIGINS = {name: f'https://github.com/ClaudioMascaro/{name}.git' for name in NAMES}
APP, LIB, SHARED = (f'github.com/claudiomascaro/{name}' for name in NAMES)
ROUTE = 'http::POST::/orders'
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
    'published': ('fffac4a975ccb37b72abadb01a82159e3b6374f1', *EXPANDED[1:]),
    'linked': ('d4d76058f3dbe777854d05e8e0d781a37bee6cd1', *ORIGINAL[1:]),
    'malformed': ('ad48b4c17536a2c35102ae87c34fca85807eb700', *ORIGINAL[1:]),
}
KEY = Path(__file__).with_name('smoke-stack')


def save(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + '\n')


def checkout(source, destination, revision, name):
    shutil.copytree(source, destination, symlinks=True, ignore=shutil.ignore_patterns('.meada'))
    git('checkout', '-q', '--detach', revision, cwd=destination)
    origin = git('remote', 'get-url', 'origin', cwd=destination).strip()
    head = git('rev-parse', 'HEAD', cwd=destination).strip()
    if (origin, head) != (ORIGINS[name], revision) or git('status', '--porcelain', cwd=destination):
        raise ValueError(f'unexpected source at {destination}: {origin}, {head}')
    return {'origin': origin, 'head': head, 'tree': git('rev-parse', 'HEAD^{tree}', cwd=destination).strip()}


class Smoke:
    def __init__(self, args):
        self.run = Path(args.run).resolve()
        self.run.mkdir(parents=True)
        self.work = self.run / 'work'
        self.work.mkdir()
        self.checks, self.checkouts = [], {}
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
        save(self.run / 'inputs.json', {'driver_revision': git('rev-parse', 'HEAD', cwd=ROOT).strip(),
                                       'corpora': {kind: dict(zip(NAMES, pins)) for kind, pins in PINS.items()},
                                       'binaries': self.bins})

    def check(self, name, expected, actual, *, matches=None):
        passed = expected == actual if matches is None else matches
        self.record({'name': name, 'result': 'pass' if passed else 'fail',
                     'expected': expected, 'actual': actual})
        return passed

    def record(self, check):
        self.checks.append(check)
        save(self.run / 'checks.json', self.checks)
        print(f"{check['result'].upper()} {check['name']}", flush=True)

    def phase(self, name, action, *args):
        try:
            action(self, *args)
        except Exception:
            evidence = f'{name}.blocked.txt'
            (self.run / evidence).write_text(traceback.format_exc())
            self.record({'name': name, 'result': 'blocked', 'evidence': evidence})

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
                self.command(f'fetch/{name}-{sha}', source,
                             ['git', 'fetch', '-q', '--no-tags', '--depth=1', 'origin', sha], timeout=90)
                self.fetched.add((name, sha))
            target = self.work / label / name
            checkouts[name] = checkout(source, target, sha, name)
            evidence = self.run / 'sources' / kind / f'{name}.tar'
            if not evidence.exists():
                evidence.parent.mkdir(parents=True, exist_ok=True)
                with evidence.open('wb') as archive:
                    subprocess.run(['git', 'archive', sha], cwd=target, stdout=archive, check=True, timeout=30)
        self.checkouts[label] = checkouts
        save(self.run / 'checkouts.json', self.checkouts)
        return tuple(self.work / label / name for name in NAMES)

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
        self.scored.setdefault(binary, {})[name] = self.check(f'{binary} {name}', expected, actual, matches=matches)
        save(self.run / 'denominator.json', {'corpus': 'expanded', 'checks': self.scored})


def rows_at(data, section, field, path, name):
    return [row for row in data[section]
            if f'::{path}::' in row[field] and name in row[field].split('::')[2]]


def endpoint_observation(data, path, name):
    consumers = rows_at(data, 'consumers', 'site', path, name)
    links = rows_at(data, 'links', 'consumer', path, name)
    sites = {row['site'] for row in consumers}
    return {'consumers': consumers, 'links': links,
            'items': [row for row in data.get('open_items', []) if row['call'] in sites]}


def configured_order(links, repo=LIB):
    return (len(links) == 1 and links[0]['provider'] == ORDERS[repo]
            and links[0]['contract'] == ROUTE and links[0]['level'] == 'inferred'
            and 'SMOKE_ORDERS_URL' in links[0]['reason'] and repo in links[0]['reason'])


def overridden_order(links):
    if not links:
        return True  # The issues permit no established host.
    return (len(links) == 1 and links[0]['provider'] == ORDERS[SHARED]
            and links[0]['contract'] == ROUTE and links[0]['level'] in ('exact', 'inferred'))


HTTP = (
    ('go/client.go', 'SendConfigured', None),
    ('go/client.go', 'SendShared', '#948'),
    ('python/client.py', 'send_configured', None),
    ('python/client.py', 'send_shared', '#949'),
    ('java/OrdersClient.java', 'sendOrder', '#810'),
    ('kotlin/Orders.kt', 'sendOrder', '#810'),
    ('csharp/OrdersClient.cs', 'SendOrder', None),
    ('rust/src/lib.rs', 'send_order', None),
    ('swift/Orders.swift', 'sendOrder', None),
    ('typescript/client.ts', 'sendOrder', None),
)


def provision_ok(rows, known):
    if not known:
        return not any(row['level'] == 'exact' for row in rows)
    return (len(rows) == 1 and (rows[0]['contract'], rows[0]['provisioner'], rows[0]['level'])
            == (QUEUE, QUEUE_NODE, 'exact'))


def sqs(rig, data, binary, *, historical=False):
    for name, known in (('publishOrder', True), ('publishLiteral', True), ('publishSelected', False)):
        path = 'orders/events/publisher.ts'
        rows = {'providers': rows_at(data, 'providers', 'site', path, name),
                'provisions': rows_at(data, 'provisions', 'side', path, name)}
        expected = [QUEUE, QUEUE_NODE, 'exact'] if known else 'no exact queue at runtime choice'
        if historical:
            # The inherited SDK regression drops the single-return helper at this tier.
            known = name == 'publishLiteral'
        providers = rows['providers']
        matches = (len(providers) == 1 and providers[0]['contract'] == (QUEUE if known else 'channel::sqs')
                   and provision_ok(rows['provisions'], known)
                   and all(row['side'] == providers[0]['site'] for row in rows['provisions']))
        if historical:
            rig.check(f'68f1c0b {name} historical observation', [QUEUE, QUEUE_NODE, 'exact'] if known else
                      'channel::sqs with no exact queue', rows, matches=matches)
        else:
            rig.score(binary, f'SQS {name}', expected, rows, matches)


def jvm(rig, data, binary, variant, path, owner, helper):
    calls = [row for row in rows_at(data, 'calls', 'id', f'orders/clients/{path}', 'sendOrder')
             if row['id'].split('::')[-1] == f'{helper}#0']
    target = f'{APP}::orders/clients/{path}::{owner}.{helper}()'
    actual = [{'target': row['target'], 'level': row['level'], 'candidates': row.get('candidates', [])} for row in calls]
    rig.check(f'{binary} {variant} {owner}: private code hop', [{'target': target, 'level': 'exact', 'candidates': []}], actual)


def transport(rig, language):
    app, _, _ = rig.clone(f'transport-{language}', 'expanded')
    root = rig.work / f'record-{language}'
    root.mkdir()
    url = 'https://lib.smoke.example'
    if language == 'go':
        shutil.copy(app / 'orders/clients/go/client.go', root)
        shutil.copy(KEY / 'transport_test.go', root)
        rig.command('transport/go-version', root, ['go', 'version'])
        got = rig.command('transport/go', root, ['go', 'test', '-v', 'client.go', 'transport_test.go'],
                          env={'GO111MODULE': 'off', 'GOTOOLCHAIN': 'local', 'GOCACHE': str(rig.work / 'go-cache'),
                               'SMOKE_ORDERS_URL': url}, timeout=150)
        records = [json.loads(line[len('PROBE '):]) for line in got.stdout.splitlines() if line.startswith('PROBE ')]
        callers = ('SendShared', 'SendConfigured')
    else:
        source = app / 'orders/clients/python/client.py'
        got = rig.command('transport/python', root, [sys.executable, str(KEY / 'transport_record.py'), str(source)],
                          env={'SMOKE_ORDERS_URL': url})
        records = json.loads(got.stdout)
        callers = ('send_shared', 'send_configured')
    rig.check(f'{language} transport receives overridden and ordinary URL', [
        {'caller': callers[0], 'method': 'POST', 'url': 'https://shared.smoke.example/orders'},
        {'caller': callers[1], 'method': 'POST', 'url': url + '/orders'},
    ], records)


def http_cases(rig, binary, app, data):
    for path, name, issue in HTTP:
        evidence = endpoint_observation(data, f'orders/clients/{path}', name)
        expected = {'contract': ROUTE, 'provider': ORDERS[LIB],
                    'level': 'inferred', 'reason': 'via SMOKE_ORDERS_URL; lib is the declaring peer'}
        if issue in ('#948', '#949'):
            expected = {'correct provider': ORDERS[SHARED], 'otherwise': 'unresolved; must not select lib'}
        links = evidence['links']
        matches = overridden_order(links) if issue in ('#948', '#949') else configured_order(links)
        rig.score(binary, f'HTTP {path} {name}' + (f' {issue}' if issue else ''), expected, evidence, matches)
    jvm(rig, data, binary, 'combined', 'java/OrdersClient.java', 'OrdersClient', 'ordersBase')
    jvm(rig, data, binary, 'combined', 'kotlin/Orders.kt', 'KotlinOrders', 'ordersBase')
    sqs(rig, data, binary)
    mcp = [
        {'tool': 'search', 'arguments': {'query': '/orders', 'kind': 'route', 'budget': 4000}},
        {'tool': 'context', 'arguments': {'symbol': ROUTE, 'budget': 8000}},
        {'tool': 'trace', 'arguments': {'from': f'{APP}::.github/workflows/ci.yml::jobs["through-lib"]',
                                        'direction': 'down', 'budget': 8000}},
    ]
    texts = rig.mcp(binary, app, binary, mcp)
    hops = [line for line in texts[2].splitlines() if '→[exact' in line and ('check.yml' in line or 'build.yml' in line)]
    rig.check(f'{binary} app to lib to shared two exact CI hops', [LIB, SHARED], hops,
              matches=len(hops) == 2 and LIB in hops[0] and SHARED in hops[1] and '2 hops · 3 repos' in texts[2])


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
        for path, owner in [('java/OrdersClient.java', 'OrdersClient'), ('kotlin/Orders.kt', 'KotlinOrders')]:
            if path == drop:
                continue
            jvm(rig, data, 'head', variant, path, owner, 'ordersBasePrivateProbe' if path == rename else 'ordersBase')
            got = endpoint_observation(data, f'orders/clients/{path}', 'sendOrder')
            rig.check(f'#810 {variant} {owner} HTTP provider', ORDERS[LIB], got, matches=configured_order(got['links']))


def peer_controls(rig, app, lib, baseline):
    wanted = [(ORDERS[LIB], 'ambiguous'), (ORDERS[SHARED], 'ambiguous')]
    relative = endpoint_observation(baseline, 'orders/evidence/client.go', 'Relative')
    links = relative['links']
    rig.check('authority-free /orders has two alternatives', wanted, relative,
              matches=sorted((row['provider'], row['level']) for row in links) == wanted
              and len(relative['items']) == 1 and relative['items'][0]['kind'] == 'ambiguous-link'
              and relative['items'][0]['candidates'] == [f'{LIB}::{ROUTE}', f'{SHARED}::{ROUTE}'])
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
    outcome = endpoint_observation(other, 'orders/evidence/client.go', 'ViaMethod')
    rig.check('provider changes with the declared authority', ORDERS[SHARED], outcome,
              matches=configured_order(outcome['links'], SHARED))
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
                got = endpoint_observation(data, 'matrix.go', name)
                expected_level = 'inferred' if name.startswith('Configured') else level
                expected = [(contract, f'{peer}::openapi.yaml::POST {path}', expected_level)
                            for peer in expected_peers]
                links = got['links']
                candidates = [f'{peer}::{contract}' for peer in expected_peers] if expected_level == 'ambiguous' else []
                rig.check(f'{label} {name} {path}', {'links': expected, 'candidates': candidates}, got,
                          matches=len(got['consumers']) == 1
                          and sorted((row['contract'], row['provider'], row['level']) for row in links) == expected
                          and [item['candidates'] for item in got['items']] == ([candidates] if candidates else [])
                          and all(row.get('reason') for row in links)
                          and (not name.startswith('Configured') or all('SMOKE_ORDERS_URL' in row['reason'] for row in links)))
    save(rig.run / 'matrix.json', {'configurations': matrix, 'routes': 96, 'callers': 192})


def kotlin_shadow(rig):
    path = 'src/main/kotlin/example/jvm/Typed.kt'
    fixture = ROOT / 'tests/fixtures/conformance-kotlin'
    (rig.run / 'kotlin-shadow-typed.kt').write_bytes((fixture / path).read_bytes())
    for binary in ('base', 'head'):
        if binary in rig.bins:
            root = rig.work / f'kotlin-shadow-{binary}'
            shutil.copytree(fixture, root)
            git('init', '-q', cwd=root)
            git('remote', 'add', 'origin', 'https://example.com/proof/kotlin-shadow.git', cwd=root)
            git('add', '.', cwd=root)
            git('commit', '-qm', 'Kotlin shadowing fixture', cwd=root)
            data = rig.export(f'kotlin-shadow/{binary}', root, binary)
            got = endpoint_observation(data, path, 'Shadowing.hidden')
            witness = endpoint_observation(data, path, 'Typed.typed')
            save(rig.run / f'kotlin-shadow-{binary}.json', {'shadowed': got, 'other_class': witness})
            resolved = [row for row in got['consumers'] if row['contract'] == 'http::GET::/persons/hidden']
            rig.check(f'#952 {binary} Kotlin shadowing, behavior pending maintainer decision',
                      'retain the documented receiver ceiling or recover the literal GET /persons/hidden',
                      got, matches=not got['consumers'] or len(resolved) == len(got['consumers']) == 1)


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
              order_rows(separate), matches=all(configured_order([row]) for row in order_rows(separate)))
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


def selftest():
    right = {'provider': ORDERS[LIB], 'contract': ROUTE, 'level': 'inferred',
             'reason': f'base URL from SMOKE_ORDERS_URL is provided by {LIB}'}
    assert configured_order([right])
    assert not configured_order([dict(right, provider=ORDERS[SHARED])])
    assert not overridden_order([right]) and overridden_order([])
    assert provision_ok([{'level': 'exact', 'contract': QUEUE, 'provisioner': QUEUE_NODE}], True)
    assert not provision_ok([{'level': 'exact', 'contract': QUEUE, 'provisioner': QUEUE_NODE}], False)
    assert len(HTTP) == 10 and len({(path, name) for path, name, _ in HTTP}) == 10
    print('smoke-stack selftest: swapped peer and runtime false exact rejected')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--selftest', action='store_true')
    parser.add_argument('--slice', choices=('resolution', 'lifecycle', 'upgrade', 'sqs'), default='resolution')
    parser.add_argument('--run')
    parser.add_argument('--base-bin')
    parser.add_argument('--head-bin')
    parser.add_argument('--old-bin')
    parser.add_argument('--middle-bin')
    parser.add_argument('--source-dir', help='optional verified fetch cache containing the pinned revisions')
    args = parser.parse_args()
    if args.selftest:
        selftest()
        return
    if not args.run or not args.head_bin or args.slice in ('upgrade', 'sqs') and not args.old_bin:
        parser.error('run directory, head binary and any historical binaries for the slice are required')
    rig = Smoke(args)
    if args.slice == 'resolution':
        for language in ('go', 'python'):
            rig.phase(f'runtime-{language}', transport, language)
        for binary in ('base', 'head'):
            if binary in rig.bins:
                rig.phase(f'resolution-{binary}', resolution, binary)
        rig.phase('private-methods', jvm_controls)
        rig.phase('kotlin-shadow', kotlin_shadow)
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
    counts = {status: sum(row['result'] == status for row in rig.checks) for status in ('pass', 'fail', 'blocked')}
    save(rig.run / 'result.json', {'counts': counts, 'verdict': 'UNPROVEN'})
    print(f'capture finished: {counts}. Judge against answer-key.md.')
    return BLOCKED if counts['blocked'] else ORACLE_FAILED if counts['fail'] else 0


if __name__ == '__main__':
    run_driver(main)
