#!/usr/bin/env python3
"""Drive #675 through a real built console. No test fixtures or implementation tables are read.

console-graph.py --head-bin BIN --ui-dir ui/dist --run .prove-it/runs/UTC-console-graph
Needs PROVE_NODE (playwright-core and axe-core) and PROVE_CHROMIUM. Retains raw CLI/API,
browser tables, screenshots and budgets; never writes a PROVED verdict on the checker's behalf.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import time

from console import http


PREFIX = 'example.com/console-graph/'


def write(root, name, text):
    file = root / name
    file.parent.mkdir(parents=True, exist_ok=True)
    file.write_text(text)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--head-bin', required=True)
    parser.add_argument('--ui-dir', required=True)
    parser.add_argument('--run', required=True)
    args = parser.parse_args()
    binary = Path(args.head_bin).resolve()
    frontend = Path(args.ui_dir).resolve()
    run = Path(args.run).resolve()
    run.mkdir(parents=True, exist_ok=False)
    if not (frontend / 'meada-ui.json').is_file():
        raise RuntimeError('BLOCKED: build ui/dist from the head before this flow')

    def command(label, *words):
        result = subprocess.run([str(binary), *words], cwd=run, capture_output=True, timeout=120)
        (run / f'{label}.out').write_bytes(result.stdout)
        (run / f'{label}.err').write_bytes(result.stderr)
        (run / f'{label}.exit').write_text(str(result.returncode))
        if result.returncode:
            raise RuntimeError(f'{label}: exit {result.returncode}, see {run}/{label}.err')
        return result.stdout

    repos = {name: run / 'work' / name for name in ['app', 'deploy', 'infra', 'monitoring']}
    write(repos['app'], 'app.py', 'def anchor():\n    return 7\n')
    write(repos['app'], 'settings.yaml', 'queue: console-jobs\nsupport: console-team\n')
    write(repos['app'], '.github/workflows/build.yml', '''name: publish
on: push
jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - uses: docker/build-push-action@v6
        with:
          tags: |
            ghcr.io/console/worker:latest
            ghcr.io/console/unrun:latest
''')
    write(repos['deploy'], 'compose.yaml', '''services:
  worker:
    image: ghcr.io/console/worker:v1
  missing:
    image: ghcr.io/console/missing:v1
''')
    for root in ['dev', 'prod']:
        write(repos['infra'], f'envs/{root}/main.tf', 'resource "aws_sqs_queue" "jobs" { name = "console-jobs" }\n')
    write(repos['monitoring'], 'rules.yaml', '''groups:
  - name: console
    rules:
      - alert: WorkerDown
        expr: up == 0
        labels: {support: console-team}
      - alert: OpsDown
        expr: up == 0
        labels: {support: ops-team}
''')
    write(repos['monitoring'], 'alertmanager.yaml', '''route:
  receiver: fallback
  routes:
    - receiver: console
      continue: true
      matchers: ['support="console-team"']
    - receiver: teams
      matchers: ['support=~".*-team"']
receivers:
  - name: fallback
  - name: console
  - name: teams
''')
    for name, repo in repos.items():
        for words in [['init', '-q'], ['config', 'user.name', 'Console proof'],
                      ['config', 'user.email', 'console-proof@example.invalid'],
                      ['remote', 'add', 'origin', f'https://{PREFIX}{name}.git'], ['add', '.'],
                      ['commit', '-qm', 'Neutral console graph input']]:
            subprocess.run(['git', *words], cwd=repo, check=True, capture_output=True)
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        port = listener.getsockname()[1]
    url = f'http://127.0.0.1:{port}'
    write(run, 'url', url)
    manifest = 'index_dir = "indexes"\n'
    for name, repo in repos.items():
        manifest += f'\n[[repository]]\nrepo = "{PREFIX}{name}"\npath = "{repo}"\n'
    manifest += f'\n[ui]\ndir = "{frontend}"\npublic_url = "{url}"\n'
    write(run, 'workspace.toml', manifest)
    version = command('version', '--version').decode().strip()
    write(run, 'identity.json', json.dumps({'url': url, 'binary': str(binary), 'version': version,
          'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
          'frontend': str(frontend), 'frontend_version': json.loads((frontend / 'meada-ui.json').read_text())}, indent=2))
    print(f'Target {url}; binary {binary} ({version}); frontend {frontend}', flush=True)
    command('index', 'index', '--workspace', 'workspace.toml')
    lines = []
    with (run / 'console.log').open('wb') as log:
        server = subprocess.Popen([str(binary), 'ui', '--listen', f'127.0.0.1:{port}', '--workspace', 'workspace.toml'],
                                  cwd=run, stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)
        write(run, 'console.pid', str(server.pid))
        try:
            for _ in range(150):
                if server.poll() is not None:
                    raise RuntimeError('BLOCKED: console exited, see console.log')
                try:
                    status, body = http(url + '/api/v1/config')
                    if status == 200:
                        (run / 'config.json').write_bytes(body)
                        break
                except OSError:
                    pass
                time.sleep(.2)
            else:
                raise RuntimeError('BLOCKED: console did not become usable in 30 seconds')
            for name in repos:
                identity = PREFIX + name
                status, raw = http(url + f'/api/v1/repositories/{identity}/export')
                write(run, f'exports/{name}.json', raw.decode())
                cli = command(f'export-{name}', 'export', '--workspace', 'workspace.toml', '--repo', identity)
                assert status == 200 and raw == cli, f'{name}: full same-mode CLI/API byte parity'
                lines.append(f'{name}: HTTP 200, complete CLI/API export bytes equal')
            queries = []
            for repository, kind, value in [('app', 'value', 'console-jobs'), ('infra', 'iac', 'aws_sqs_queue.jobs'),
                                            ('monitoring', 'alert', 'support')]:
                body = {'repository': PREFIX + repository, 'tool': 'search', 'arguments': {'query': value, 'kind': kind}}
                status, raw = http(url + '/api/v1/query', 'POST', body, {'Origin': url, 'Content-Type': 'application/json'})
                payload = json.loads(raw)
                assert status == 200 and payload.get('text'), (kind, status, payload)
                queries.append({'body': body, 'answer': payload['text']})
            write(run, 'queries.json', json.dumps(queries, indent=2))
            browser = subprocess.run(['node', str(Path(__file__).with_name('console-graph.mjs')), str(run)],
                                     capture_output=True, text=True, timeout=180)
            write(run, 'browser.out', browser.stdout)
            write(run, 'browser.err', browser.stderr)
            write(run, 'browser.exit', str(browser.returncode))
            if browser.returncode:
                raise RuntimeError(f'browser exited {browser.returncode}: {browser.stderr}')
            lines.append(browser.stdout.strip())
        finally:
            server.terminate()
            try:
                server.wait(timeout=10)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait(timeout=10)
            (run / 'console.pid').unlink(missing_ok=True)
            write(run, 'cleanup.json', json.dumps({'pid': server.pid, 'exit': server.returncode, 'stopped': True}))
    write(run, 'checks.md', '# Console graph checks\n\nchecks passed\n\n' + '\n'.join('- ' + line for line in lines) + '\n')
    print('checks passed\n' + '\n'.join(lines))


if __name__ == '__main__':
    main()
