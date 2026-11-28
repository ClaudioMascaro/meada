"""Read-only workspace and decision checks for smoke_stack.py."""
from contextlib import contextmanager
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import re
import shutil
import sqlite3
import subprocess
from threading import Event, Thread
import tomllib

from prove import git
from smoke_stack import APP, LIB, NAMES, ORDERS, SHARED, save


def write(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


def file_bytes(path):
    return path.read_bytes() if path.is_file() else b''


def hashes(directory):
    return {path.relative_to(directory).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in sorted(directory.rglob('*')) if path.is_file()}


def decision_ids(raw):
    return [row['call'] for row in tomllib.loads(raw.decode()).get('decision', [])]


@contextmanager
def decider(rig, app, label, *, stall=None, refuse=None, fail=None):
    path = app / 'meada.decisions.toml'
    observed, request_waiting, release = [], Event(), Event()
    directory = rig.run / 'decider' / label
    directory.mkdir(parents=True)

    class Handler(BaseHTTPRequestHandler):
        def do_POST(self):
            request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            number = len(observed) + 1
            before = file_bytes(path)
            (directory / f'before-{number}.toml').write_bytes(before)
            observed.append({'path': self.path, 'request': request, 'saved': decision_ids(before) if stall or fail else []})
            save(directory / 'requests.json', observed)
            if number == stall:
                request_waiting.set()
                release.wait(timeout=20)
                return
            if number == refuse:
                app.chmod(0o555)
            body = json.dumps({'model': 'fake/model', 'choices': [{'message': {'role': 'assistant',
                   'content': json.dumps({'choice': '1', 'probability': 0.9})}}]}).encode()
            code = 503 if fail is not None and number >= fail else 200
            self.send_response(code)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *args):
            pass

    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    server.daemon_threads = True
    thread = Thread(target=server.serve_forever, daemon=True)
    thread.start()
    env = {'MEADA_DECIDER_URL': f'http://127.0.0.1:{server.server_port}/v1',
           'MEADA_DECIDER_PROTOCOL': 'chat', 'MEADA_DECIDER_MODEL': 'fake/model'}
    try:
        yield env, observed, request_waiting
    finally:
        release.set()
        server.shutdown()
        server.server_close()
        thread.join(timeout=10)
        app.chmod(0o755)
        save(directory / 'requests.json', observed)
        (directory / 'final.toml').write_bytes(file_bytes(path))


def orphan_roles(rig, scope):
    app, lib, shared = rig.clone(f'orphans-{scope}', 'original')
    key = {
        'image::localhost:5000/smoke/preview': 'no runner',
        'alert::team::retired': 'no matcher',
        'channel::sqs::smoke-legacy': 'no provisioner',
        'image::localhost:5000/smoke/retired': 'no builder',
        'alert::team::inventory': 'no emitter',
        'channel::sqs::smoke-idle': 'no use',
    }
    if scope == 'isolated':
        (app / 'meada.toml').write_text('')
        for name in ('alertmanager.yaml', 'queues.tf', 'compose.yaml'):
            shutil.copyfile(lib / name, app / name)
        for name in ('build.yml', 'orphan.yml'):
            shutil.copyfile(shared / '.github/workflows' / name, app / '.github/workflows' / name)
        write(app / '.github/workflows/isolated-build.yml',
              'name: Local control\non: workflow_dispatch\njobs:\n  call:\n    uses: ./.github/workflows/build.yml\n')
        shutil.rmtree(lib)
        shutil.rmtree(shared)
    key[f'ci::{APP if scope == "isolated" else SHARED}/.github/workflows/orphan.yml'] = 'no consumer'
    rig.cli(f'orphans-{scope}/index', app, 'head', 'index')
    raw = rig.cli(f'orphans-{scope}/status', app, 'head', 'status').stdout
    expected_scope = APP if scope == 'isolated' else 'the workspace'
    expected_header = 'workspace: 1 repository' if scope == 'isolated' else 'workspace: 3 repositories'
    rig.check(f'#855 {scope} workspace header', 1, raw.splitlines().count(expected_header))
    for contract, absence in key.items():
        lines = [line.strip() for line in raw.splitlines() if line.strip().startswith(f'orphan {contract}:')]
        prefix = f'orphan {contract}: {absence} in {expected_scope}; '
        rig.check(f'#855 {scope} {contract}', prefix, lines,
                  matches=len(lines) == 1 and lines[0].startswith(prefix))


def linked_decisions(rig, variant):
    config = 'malformed' if variant == 'malformed' else 'linked'
    app, lib, _ = rig.clone(f'decisions-{variant}', config)
    link = app / 'meada.decisions.toml'
    original = link.readlink()
    if variant == 'dangling':
        link.resolve().unlink()
    if variant == 'waiting-peer':
        contents = link.read_bytes()
        relative_target = link.resolve().relative_to(lib)
        shutil.move(lib, rig.work / 'waiting-decisions-lib')
        target = lib / relative_target
        target.parent.mkdir(parents=True)
        target.write_bytes(contents)
    target_bytes = file_bytes(link)
    stat = link.lstat()
    label = f'symlink-{variant}'
    with decider(rig, app, label) as (env, requests, _):
        for arguments in ((), ('--item', 'no-such-item'), ('--decider',), ('--prune',)):
            prefix = label + '/write-' + ('-'.join(arg.lstrip('-') for arg in arguments) or 'interactive')
            result = rig.cli(prefix, app, 'head', 'decide', *arguments, stdin='1\n', env=env, check=False)
            rig.check(f'#932 {variant} {" ".join(arguments) or "interactive"} checks link first',
                      'nonzero, path and symbolic link in stderr, empty stdout',
                      {'exit': result.returncode, 'stdout': result.stdout, 'stderr': result.stderr},
                      matches=result.returncode != 0 and 'symbolic link' in result.stderr
                      and 'meada.decisions.toml' in result.stderr and not result.stdout)
        rig.check(f'#932 {variant} zero decider requests before refusal', [], requests)
        for name, argv in (
            ('list', ('decide', '--list')), ('dry', ('decide', '--dry-run')),
            ('show', ('decide', '--decider', '--show-request')), ('index', ('index',)),
            ('status', ('status',)), ('export', ('export',)),
        ):
            result = rig.cli(f'{label}/read-{name}', app, 'head', *argv, env=env, check=False)
            if variant == 'malformed':
                ok = result.returncode != 0 and 'symbolic link' not in result.stderr and 'meada.decisions.toml' in result.stderr
                expected = 'decision parse error from the linked bytes'
            elif variant == 'waiting-peer' and name == 'list':
                ok = result.returncode == 0 and 'waiting' in result.stdout and LIB in result.stdout
                expected = 'waiting for the declared lib peer'
            else:
                ok = 'symbolic link' not in result.stderr and (result.returncode == 0 or name == 'status')
                expected = 'normal read, following the linked path'
            rig.check(f'#932 {variant} read {name}', expected,
                      {'exit': result.returncode, 'stderr': result.stderr, 'raw': f'commands/{label}/read-{name}.out'},
                      matches=ok)
    rig.check(f'#932 {variant} link and bytes preserved', True,
              link.is_symlink() and link.readlink() == original and link.lstat().st_ino == stat.st_ino
              and target_bytes == file_bytes(link) and not list(app.glob('meada.decisions.toml.tmp*')))
    if variant != 'linked':
        return
    data = json.loads((rig.run / 'commands' / label / 'read-export.out').read_text())
    observed = [row for row in data['links'] if 'orders/shadow/client.go' in row['consumer']]
    rig.check('#932 linked decision applies to lib',
              [ORDERS[LIB]], [row['provider'] for row in observed])
    regular, _, _ = rig.clone('decisions-regular', 'linked')
    regular_file = regular / 'meada.decisions.toml'
    regular_file.unlink()
    regular_file.write_bytes(target_bytes)
    for name, argv in (
        ('list', ('decide', '--list')), ('dry', ('decide', '--dry-run')),
        ('index', ('index',)), ('status', ('status',)),
    ):
        result = rig.cli(f'{label}/regular-{name}', regular, 'head', *argv, check=False)
        linked_out = (rig.run / 'commands' / label / f'read-{name}.out').read_text()
        rig.check(f'#932 linked and regular {name} agree', [0, linked_out], [result.returncode, result.stdout])


def manifest_status(rig):
    app, _, _ = rig.clone('status-manifest', 'original')
    indexes = app.parent / '.smoke-indexes'
    argv = ('--workspace', 'meada.workspace.toml')
    status = lambda name, bin='head': rig.cli(f'status/{name}', app, bin, 'status', *argv, check=False)
    cold = status('absent')
    rig.check('#934 absent indexes report unevaluated without creating indexes',
              [1, True, False], [cold.returncode, cold.stdout.count('not indexed') == 3
              and 'not evaluated' in cold.stdout, indexes.exists()])
    rig.cli('status/index', app, 'head', 'index', *argv)
    healthy = status('healthy')
    rig.check('#934 three healthy indexes', [0, True], [healthy.returncode,
              all(re.search(rf'^{re.escape(repo)} .* indexed,', healthy.stdout, re.M) for repo in (APP, LIB, SHARED))
              and 'not evaluated' not in healthy.stdout])
    if 'base' in rig.bins:
        old, _, _ = rig.clone('status-manifest-base', 'original')
        rig.cli('status/base-index', old, 'base', 'index', *argv)
        before = rig.cli('status/base-healthy', old, 'base', 'status', *argv)
        rig.parity.compare('#934 healthy status stdout', before.stdout, healthy.stdout)
    lib_db, = [path for path in indexes.rglob('index.db') if NAMES[1] in path.parts]
    with sqlite3.connect(lib_db) as db:
        version, = db.execute('PRAGMA user_version').fetchone()
        db.execute('PRAGMA user_version=2')
    before = hashes(indexes)
    result = status('old-schema')
    old = [line for line in result.stdout.splitlines() if 'another version' in line]
    rig.check('#934 old lib schema is diagnosed without rebuilding',
              [1, LIB, 'another version', 'not evaluated', True],
              [result.returncode, old, result.stdout, before == hashes(indexes)],
              matches=result.returncode == 1 and len(old) == 1 and old[0].startswith(LIB + ' ')
              and 'not evaluated' in result.stdout and before == hashes(indexes))
    with sqlite3.connect(lib_db) as db:
        db.execute(f'PRAGMA user_version={version}')
        db.execute('DROP TABLE symbols')
    before = hashes(indexes)
    result = status('broken-lib')
    lines = [line for line in (result.stdout + result.stderr).splitlines() if 'no such table: symbols' in line]
    rig.check('#934 broken lib identity, SQLite cause and unchanged indexes',
              [0, LIB, 'no such table: symbols', True],
              [result.returncode, lines, before == hashes(indexes)],
              matches=result.returncode == 0 and len(lines) == 1 and LIB in lines[0] and before == hashes(indexes))
    write(app / 'empty.workspace.toml', 'index_dir = "../empty-indexes"\n')
    empty = rig.cli('status/empty', app, 'head', 'status', '--workspace', 'empty.workspace.toml', check=False)
    rig.check('#934 empty manifest remains empty', [1, 'workspace: 0 repositories\n', False],
              [empty.returncode, empty.stdout, (app.parent / 'empty-indexes').exists()])


def decision_fixture(rig, label):
    folder = rig.work / label
    app, api = folder / 'app', folder / 'api'
    write(app / 'meada.toml', '[[peer]]\nrepo = "example.com/proof/api"\npath = "../api"\n')
    write(app / 'client.ts', ''.join(f'export function request{n}(url: string) {{ return fetch(url); }}\n' for n in range(3)))
    write(api / 'app/orders/route.ts', 'export function GET() { return new Response("orders"); }\n')
    for name, repo in [('app', app), ('api', api)]:
        git('init', '-q', cwd=repo)
        git('remote', 'add', 'origin', f'https://example.com/proof/{name}.git', cwd=repo)
        git('add', '.', cwd=repo)
        git('commit', '-qm', 'neutral decider input', cwd=repo)
    return app


def requests_of(rows):
    return [row['request'] for row in rows]


def interruption(rig):
    app = decision_fixture(rig, 'decider-interrupted')
    path = app / 'meada.decisions.toml'
    rig.cli('decider/interrupted-index', app, 'head', 'index')
    with decider(rig, app, 'interrupted', stall=3) as (env, first, waiting):
        process = subprocess.Popen([rig.bins['head'], 'decide', '--decider'], cwd=app,
                                   env={**rig.env, **env}, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, text=True)
        try:
            if not waiting.wait(timeout=20):
                raise RuntimeError('decider did not reach request 3')
            process.terminate()
            stdout, stderr = process.communicate(timeout=10)
            save(rig.run / 'decider/interrupted/process.json',
                 {'exit': process.returncode, 'stdout': stdout, 'stderr': stderr})
        finally:
            if process.poll() is None:
                process.kill()
                process.communicate(timeout=10)
    saved = file_bytes(path)
    rig.check('#937 0, 1 and 2 accepted decisions persisted at each request', [0, 1, 2],
              [len(row['saved']) for row in first])
    rig.check('#937 termination keeps two accepted calls and no completed temp file', [2, False],
              [len(decision_ids(saved)), bool(list(app.glob('meada.decisions.toml.tmp*')))])
    with decider(rig, app, 'resumed') as (env, resumed, _):
        rig.cli('decider/resumed', app, 'head', 'decide', '--decider', env=env)
    rig.check('#937 resumed run sends only the third request', requests_of(first)[2:], requests_of(resumed))
    all_app = decision_fixture(rig, 'decider-continuous')
    with decider(rig, all_app, 'continuous') as (env, everyone, _):
        current = rig.cli('decider/continuous', all_app, 'head', 'decide', '--decider', env=env)
    canonical = file_bytes(all_app / path.name)
    rig.check('#937 interrupted/resumed and continuous file bytes', canonical.decode(), file_bytes(path).decode())
    rig.check('#937 continuous prompt order', requests_of(first), requests_of(everyone))
    if 'base' in rig.bins:
        other = decision_fixture(rig, 'decider-base')
        with decider(rig, other, 'base') as (env, _, _):
            prior = rig.cli('decider/base', other, 'base', 'decide', '--decider', env=env)
        rig.parity.compare('#937 continuous decider stdout', prior.stdout, current.stdout)
        rig.parity.compare('#937 continuous decisions TOML', file_bytes(other / path.name).decode(),
                           canonical.decode())

    for label, failure in [('failed-response', {'fail': 3}), ('failed-write', {'refuse': 2})]:
        if label == 'failed-write' and os.geteuid() == 0:
            rig.record({'name': '#937 failed write', 'result': 'blocked', 'reason': 'root bypasses mode bits'})
            continue
        app = decision_fixture(rig, f'decider-{label}')
        rig.cli(f'decider/{label}-index', app, 'head', 'index')
        with decider(rig, app, label, **failure) as (env, _, _):
            result = rig.cli(f'decider/{label}', app, 'head', 'decide', '--decider', env=env, check=False)
        passes = 1 if label == 'failed-write' else 2
        rig.check(f'#937 {label} preserves previous decisions', passes, len(decision_ids(file_bytes(app / path.name))))
        if label == 'failed-write':
            rig.check('#937 I/O error names the decision path', True,
                      result.returncode != 0 and 'meada.decisions.toml' in result.stderr)
        with decider(rig, app, f'{label}-resumed') as (env, reused, _):
            rig.cli(f'decider/{label}-resumed', app, 'head', 'decide', '--decider', env=env)
        rig.check(f'#937 {label} resumes from retained bytes', [canonical.decode(), requests_of(everyone)[passes:]],
                  [file_bytes(app / path.name).decode(), requests_of(reused)])

    app = decision_fixture(rig, 'decider-preflight')
    rig.cli('decider/preflight-index', app, 'head', 'index')
    with decider(rig, app, 'previews') as (env, requests, _):
        app.chmod(0o555)
        if os.geteuid() != 0:
            result = rig.cli('decider/preflight', app, 'head', 'decide', '--decider', env=env, check=False)
            rig.check('#937 read-only preflight sends zero requests and names path', True,
                      result.returncode != 0 and 'meada.decisions.toml' in result.stderr and requests == [])
        shown = rig.cli('decider/show', app, 'head', 'decide', '--decider', '--show-request', env=env)
        rig.check('#937 show-request contains prompts without sending', True, bool(shown.stdout) and requests == [])
        rig.cli('decider/dry', app, 'head', 'decide', '--decider', '--dry-run', env=env)
        rig.check('#937 dry-run makes three requests and neither preview writes',
                  [3, False], [len(requests), (app / path.name).exists()])


def lifecycle(rig):
    for scope in ('isolated', 'three'):
        rig.phase(f'orphans-{scope}', orphan_roles, scope)
    for variant in ('linked', 'malformed', 'dangling', 'waiting-peer'):
        rig.phase(f'symlink-{variant}', linked_decisions, variant)
    rig.phase('manifest-status', manifest_status)
    rig.phase('decider-restart', interruption)
