#!/usr/bin/env python3
"""#641 public-surface checks, answer key: #641 and #642 stories 61/62.

Neutral inputs below are independent of feature tests and goldens. A script pass is evidence,
not a PROVED verdict. The coordinator separately runs shared-code base/corpus/console parity.
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
from prove import Http, Stdio, answer, free_port, git

PREFIX = 'example.com/via-proof/'
CONTRACT = 'resource::routing::team-z'
KEY = 'settings.yaml#groups[0].rules[0].labels.support'
SELECTOR = 'settings.yaml#groups[name=availability].rules[alert=ProbeDown].labels.support'
ADDRESSES = [
    ('main.tf::opsgenie_team_routing_rule.team_z', 'opsgenie_team_routing_rule.team_z'),
    ('object.yaml::ConfigMap/proof/routing', 'ConfigMap/proof/routing'),
    ('managed.yaml::Bucket.s3.aws.upbound.io/archive', 'Bucket.s3.aws.upbound.io/archive'),
    ('compose.yaml::services.router', 'services.router'),
    ('template.yaml::Routing', 'template.yaml::Routing'),
    ('sam.yaml::Routing', 'sam.yaml::Routing'),
    ('serverless.yml::functions.route', 'serverless.yml::functions.route'),
]


def write(root, name, data):
    path = root / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(data)


def dump(root, name, data):
    write(root, name, json.dumps(data, indent=2, ensure_ascii=False) + '\n')


def seed(root):
    repos = {name: root / name for name in ['app', 'infra']}
    for name, path in repos.items():
        path.mkdir(parents=True)
        git('init', '-q', cwd=path)
        git('remote', 'add', 'origin', f'https://{PREFIX}{name}.git', cwd=path)
    app, infra = repos['app'], repos['infra']
    write(app, 'settings.yaml', 'groups:\n  - name: availability\n    rules:\n      - alert: ProbeDown\n        labels:\n          support: team-z\n')
    write(app, 'meada.toml', f'[[peer]]\nrepo = "{PREFIX}infra"\n\n[[resource]]\nkind = "routing"\nid = "team-z"\nrole = "write"\nvia = [{json.dumps(SELECTOR)}]\n')
    vias = ', '.join(json.dumps(via) for _, via in ADDRESSES)
    write(infra, 'meada.toml', f'[[peer]]\nrepo = "{PREFIX}app"\n\n[[resource]]\nkind = "routing"\nid = "team-z"\nrole = "read"\nvia = [{vias}, "template.yaml::Deleted", "Routing"]\n')
    write(infra, 'main.tf', 'resource "opsgenie_team_routing_rule" "team_z" { name = "team-z" }\n')
    write(infra, 'object.yaml', 'apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: routing\n  namespace: proof\ndata:\n  team: team-z\n')
    write(infra, 'managed.yaml', 'apiVersion: s3.aws.upbound.io/v1beta1\nkind: Bucket\nmetadata:\n  name: archive\nspec:\n  forProvider:\n    region: eu-west-2\n')
    write(infra, 'compose.yaml', 'services:\n  router:\n    command: route\n')
    write(infra, 'template.yaml', 'AWSTemplateFormatVersion: "2010-09-09"\nResources:\n  Routing:\n    Type: Custom::Routing\n')
    write(infra, 'sam.yaml', 'Transform: AWS::Serverless-2016-10-31\nResources:\n  Routing:\n    Type: AWS::Serverless::Function\n    Properties:\n      Runtime: python3.12\n      Handler: app.handler\n')
    write(infra, 'serverless.yml', 'service: routing\nfunctions:\n  route:\n    handler: app.handler\n')
    return repos


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--head-bin', required=True)
    parser.add_argument('--run', required=True)
    parser.add_argument('--http', action='store_true')
    args = parser.parse_args()
    binary = str(Path(args.head_bin).resolve())
    run = Path(args.run).resolve()
    run.mkdir(parents=True, exist_ok=False)
    repos = seed(run / 'input')
    dump(run, 'identity.json', {'binary': binary, 'sha256': hashlib.sha256(Path(binary).read_bytes()).hexdigest()})
    checks = []

    def check(name, ok, evidence):
        checks.append({'name': name, 'passed': bool(ok), 'evidence': evidence})
        dump(run, 'checks.json', checks)

    def command(repo, label, *words):
        result = subprocess.run([binary, *words], cwd=repo, env={**os.environ, 'LANG': 'en_US.UTF-8'}, text=True, capture_output=True, timeout=120)
        write(run, label + '.stdout', result.stdout)
        write(run, label + '.stderr', result.stderr)
        dump(run, label + '.exit.json', result.returncode)
        result.check_returncode()
        return result.stdout

    expected_provider = PREFIX + 'app::' + KEY
    expected_readers = sorted(PREFIX + 'infra::' + address for address, _ in ADDRESSES)
    dump(run, 'oracle.json', {'provider': expected_provider, 'readers': expected_readers, 'contract': CONTRACT, 'level': 'declared', 'stale': ['template.yaml::Deleted', 'Routing']})
    exported = {}
    for name, repo in repos.items():
        raw = command(repo, name + '-export', 'export')
        exported[name] = json.loads(raw)
        check(name + ' deterministic export', raw == command(repo, name + '-export-repeat', 'export'), 'whole bytes')
        status = command(repo, name + '-status', 'status')
        stale = [line.strip() for line in status.splitlines() if '`via`' in line]
        check(name + ' only deleted and ambiguous locations are stale', len(stale) == 2 and any('template.yaml::Deleted matches no symbol, ignored' in line for line in stale) and any('`via` Routing matches no symbol, ignored' in line for line in stale), stale)
        links = exported[name]['links']
        got = sorted((row['provider'], row['consumer'], row['contract'], row['level'], row['reason']) for row in links)
        want = [(expected_provider, consumer, CONTRACT, 'declared', CONTRACT) for consumer in expected_readers]
        check(name + ' complete declared link set', got == want, got)
    check('one key writer', exported['app']['providers'] == [{'contract': CONTRACT, 'site': expected_provider}], exported['app']['providers'])
    check('every IaC reader', sorted(exported['infra']['consumers'], key=lambda row: row['site']) == [{'contract': CONTRACT, 'site': site, 'via': None} for site in expected_readers], exported['infra']['consumers'])
    check('every linked IaC node is exported', sorted(row['identity'] for row in exported['infra']['iac_nodes']) == expected_readers, exported['infra']['iac_nodes'])
    queries = [('context', {'symbol': CONTRACT}), ('context', {'symbol': SELECTOR}), ('trace', {'from': SELECTOR, 'direction': 'down'})]
    for _, via in ADDRESSES:
        queries += [('context', {'symbol': via}), ('trace', {'from': via, 'direction': 'up'})]
    queries = [(tool, {**params, 'budget': 200000}) for tool, params in queries]
    dump(run, 'queries.json', queries)
    client = Stdio(binary, repos['app'], run / 'stdio.err')
    try:
        replies = [answer(client.call('tools/call', {'name': tool, 'arguments': params})) for tool, params in queries]
        dump(run, 'stdio.answers.json', replies)
        for (tool, params), reply in zip(queries, replies):
            ok = CONTRACT in reply and KEY in reply
            if tool == 'trace' and params['direction'] == 'down':
                rows = re.findall(r'^\s+→\[declared: ' + re.escape(CONTRACT) + r'\] ' + re.escape(PREFIX + 'infra ') + r'([^: ]+):\d+ (.+)$', reply, re.MULTILINE)
                actual = sorted(PREFIX + 'infra::' + file + '::' + address for file, address in rows)
                ok = ok and actual == expected_readers
            elif tool == 'trace':
                rows = re.findall(r'^\s+→\[declared: ' + re.escape(CONTRACT) + r'\] ' + re.escape(PREFIX + 'app ' + KEY) + r' \(line \d+\)$', reply, re.MULTILINE)
                ok = ok and len(rows) == 1
            check(tool + ' ' + str(params), ok, reply)
        before = repos['app'].joinpath('settings.yaml').read_text()
        write(repos['app'], 'settings.yaml', before.replace('availability', 'unavailablex'))
        answer(client.call('tools/call', {'name': queries[2][0], 'arguments': queries[2][1]}))
        stale = command(repos['app'], 'stale-key-status', 'status')
        check('deleted selector becomes stale', SELECTOR in stale and 'no `via` matches a symbol' in stale, stale)
        write(repos['app'], 'settings.yaml', before)
        restored = answer(client.call('tools/call', {'name': queries[2][0], 'arguments': queries[2][1]}))
        check('restore whole trace', restored == replies[2], restored)
    finally:
        client.close()
    if args.http:
        folder = run / 'http'
        folder.mkdir()
        manifest = 'index_dir = "indexes"\n' + ''.join(f'\n[[repository]]\nrepo = "{PREFIX}{name}"\npath = {json.dumps(str(path))}\n' for name, path in repos.items())
        write(folder, 'workspace.toml', manifest)
        command(folder, 'http/index', 'index', '--workspace', 'workspace.toml')
        port = free_port()
        with open(folder / 'server.log', 'w') as log:
            server = subprocess.Popen([binary, 'mcp', '--listen', f'127.0.0.1:{port}', '--workspace', 'workspace.toml'], cwd=folder, stdin=subprocess.DEVNULL, stdout=log, stderr=log)
            write(folder, 'server.pid', str(server.pid))
            try:
                for _ in range(150):
                    if server.poll() is not None:
                        raise RuntimeError('HTTP server exited')
                    try:
                        socket.create_connection(('127.0.0.1', port), timeout=.2).close()
                        break
                    except OSError:
                        time.sleep(.1)
                http = Http(f'http://127.0.0.1:{port}/{PREFIX}app')
                responses = [answer(http.call('tools/call', {'name': tool, 'arguments': params})) for tool, params in queries]
                dump(folder, 'answers.json', responses)
                check('HTTP whole-answer parity', responses == replies, len(responses))
            finally:
                server.terminate()
                server.wait(timeout=20)
                write(folder, 'stopped', str(server.returncode))
    failed = [row['name'] for row in checks if not row['passed']]
    write(run, 'receipt.md', '# Resource via checks\n\nAnswer key: #641 and #642, neutral input declarations in oracle.json.\n\n' + ('Checks passed; independent verdict pending.\n' if not failed else 'BROKEN checks:\n' + '\n'.join('- ' + name for name in failed) + '\n'))
    return bool(failed)


if __name__ == '__main__':
    raise SystemExit(main())
