#!/usr/bin/env python3
"""#666: complete source-derived HTTP provider/link sets and stored-query parity."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import time

from prove import Http, answer, free_port, git, mcp_answers
from proof_result import Comparisons, run_driver
from provisions import export, write


PREFIX = 'example.com/routes-proof/'
SAM = '''AWSTemplateFormatVersion: '2010-09-09'
Transform: AWS::Serverless-2016-10-31
Resources:
  Garden:
    Type: AWS::Serverless::Function
    Properties:
      Runtime: nodejs20.x
      CodeUri: runtime/
      Handler: handler.harvest
      Events:
        Read:
          Type: Api
          Properties: {Method: GET, Path: '/gardens/{plotId}'}
        Create:
          Type: HttpApi
          Properties: {Method: POST, Path: /gardens}
'''
SLS = '''service: stock
provider: {name: aws}
functions:
  restock:
    handler: worker.restock
    events:
      - httpApi: {method: PATCH, path: '/stock/{sku}'}
      - httpApi: '*'
      - http: {path: 'GET /invented'}
      - httpApi: {method: {Fn::If: [Choose, GET, POST]}, path: /unknown-method}
      - httpApi: {method: GET, path: {Ref: UnknownPath}}
'''
TF = '''data "archive_file" "source" {
 type = "zip"
 source_dir = "runtime"
 output_path = "arc.zip"
}
resource "aws_lambda_function" "arc" {
 function_name = "arc"
 role = "arn:aws:iam::123456789012:role/proof"
 runtime = "nodejs20.x"
 handler = "index.orbit"
 filename = data.archive_file.source.output_path
}
resource "aws_apigatewayv2_api" "public" {
 name = "arcs-http"
 protocol_type = "HTTP"
}
resource "aws_apigatewayv2_integration" "arc" {
 api_id = aws_apigatewayv2_api.public.id
 integration_type = "AWS_PROXY"
 integration_uri = aws_lambda_function.arc.invoke_arn
}
resource "aws_apigatewayv2_route" "read" {
 api_id = aws_apigatewayv2_api.public.id
 route_key = "GET /arcs/{arcId}"
 target = "integrations/${aws_apigatewayv2_integration.arc.id}"
}
'''


def dump(run, name, data):
    (run / name).write_text(json.dumps(data, indent=2) + '\n')


def provider(contract, site):
    return {'contract': contract, 'site': PREFIX + site}


def code_source_display_control(binaries, controls, run, comparisons):
    """The old binary supplies missing-code display policy, independently of IaC providers."""
    queries = [
        {'tool': 'trace', 'arguments': {'from': 'checkReady', 'direction': 'down', 'budget': 10000}},
        {'tool': 'context', 'arguments': {'symbol': 'http::GET::/ready', 'budget': 10000}},
    ]
    records = {}
    for label, binary in binaries.items():
        control = controls[label]
        path = control / 'app/ready/route.ts'
        source = path.read_bytes()
        manifest = f'control-{label}.toml'
        write(run, manifest, f'index_dir = "control-{label}-indexes"\n[[repository]]\nrepo = "{PREFIX}control"\npath = "{control}"\n')
        subprocess.run([binary, 'index', '--workspace', manifest], cwd=run, check=True,
                       stdout=(run / f'{label}-control-index.out').open('w'),
                       stderr=(run / f'{label}-control-index.err').open('w'))
        port = free_port()
        server = subprocess.Popen([binary, 'mcp', '--listen', f'127.0.0.1:{port}', '--workspace', manifest], cwd=run,
                                  stdout=(run / f'{label}-control-http.out').open('w'),
                                  stderr=(run / f'{label}-control-http.err').open('w'))
        try:
            for _ in range(100):
                try:
                    socket.create_connection(('127.0.0.1', port), timeout=1).close()
                    break
                except OSError:
                    time.sleep(.1)
            client = Http(f'http://127.0.0.1:{port}/{PREFIX}control')
            ask = lambda: [answer(client.call('tools/call', {'name': q['tool'], 'arguments': q['arguments']})) for q in queries]
            tools = client.call('tools/list', {})['result']['tools']
            before = ask()
            path.unlink()
            after = ask()
            data = json.loads(subprocess.check_output([binary, 'export', '--workspace', manifest, '--repo', PREFIX + 'control'], cwd=run, text=True))
            records[label] = {'queries': queries, 'tools': tools, 'before': before, 'after': after,
                              'providers': data['providers'], 'links': data['links']}
            dump(run, f'{label}-code-source-deleted.json', records[label])
            comparisons.compare(f'{label} control: remove only the line hint after source deletion',
                                [text.replace('app/ready/route.ts:1', 'app/ready/route.ts') for text in before],
                                after, oracle=True)
        finally:
            server.terminate()
            server.wait(timeout=30)
            path.write_bytes(source)
    comparisons.compare('existing code HTTP display with and without source', records['base'], records['head'])


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--head-bin', required=True)
    parser.add_argument('--base-bin')
    parser.add_argument('--run', required=True)
    args = parser.parse_args()
    binary = str(Path(args.head_bin).resolve())
    run = Path(args.run).resolve()
    run.mkdir(parents=True, exist_ok=False)
    comparisons = Comparisons(run)
    os.environ['LANG'] = 'C.UTF-8'
    dump(run, 'binary.json', {'path': binary, 'sha256': hashlib.sha256(Path(binary).read_bytes()).hexdigest(),
                              'version': subprocess.check_output([binary, '--version'], text=True).strip()})
    names = ('consumer', 'sam', 'serverless', 'terraform', 'code')
    repos = {name: run / 'work' / name for name in names}
    hosts = {'sam': 'gardens.example.test', 'serverless': 'stock.example.test',
             'terraform': 'arcs.example.test', 'code': 'ready.example.test'}
    for name, root in repos.items():
        root.mkdir(parents=True)
        git('init', '-q', cwd=root)
        git('remote', 'add', 'origin', f'https://{PREFIX}{name}.git', cwd=root)
        config = ''.join(f'[[peer]]\nrepo = "{PREFIX}{peer}"\n' for peer in names if peer != name)
        if name in hosts:
            config += f'[serve]\nhosts = ["{hosts[name]}"]\n'
        write(root, 'meada.toml', config)
    write(repos['sam'], 'template.yaml', SAM)
    write(repos['sam'], 'runtime/handler.ts', 'function serveGarden() { return {}; }\nexport { serveGarden as harvest };\n')
    write(repos['sam'], 'decoy.ts', 'export function harvest() { return {}; }\n')
    write(repos['serverless'], 'serverless.yml', SLS)
    write(repos['serverless'], 'worker.ts', 'export function restock() { return {}; }\n')
    write(repos['terraform'], 'main.tf', TF)
    write(repos['terraform'], 'runtime/index.ts', 'export function orbit() { return {}; }\n')
    write(repos['code'], 'app/ready/route.ts', "export function GET() { return new Response('ready'); }\n")
    write(repos['consumer'], 'client.ts', '''export function readGarden(plot: string) { return fetch(`https://gardens.example.test/gardens/${plot}`); }
export function createGarden() { return fetch('https://gardens.example.test/gardens', {method: 'POST'}); }
export function patchStock(sku: string) { return fetch(`https://stock.example.test/stock/${sku}`, {method: 'PATCH'}); }
export function readArc(id: string) { return fetch(`https://arcs.example.test/arcs/${id}`); }
export function ready() { return fetch('https://ready.example.test/ready'); }
export function wrongMethod(plot: string) { return fetch(`https://gardens.example.test/gardens/${plot}`, {method: 'POST'}); }
export function wrongPath() { return fetch('https://arcs.example.test/arcs'); }
export function wrongHost(id: string) { return fetch(`https://unknown.example.test/arcs/${id}`); }
''')
    expected_providers = {
        'consumer': [],
        'sam': [provider('http::GET::/gardens/{}', 'sam::runtime/handler.ts::serveGarden'),
                provider('http::POST::/gardens', 'sam::runtime/handler.ts::serveGarden')],
        'serverless': [provider('http::PATCH::/stock/{}', 'serverless::worker.ts::restock')],
        'terraform': [provider('http::GET::/arcs/{}', 'terraform::runtime/index.ts::orbit')],
        'code': [provider('http::GET::/ready', 'code::app/ready/route.ts::GET')],
    }
    expected_links = {
        ('createGarden', 'http::POST::/gardens', PREFIX + 'sam::runtime/handler.ts::serveGarden', 'exact'),
        ('readGarden', 'http::GET::/gardens/{}', PREFIX + 'sam::runtime/handler.ts::serveGarden', 'exact'),
        ('patchStock', 'http::PATCH::/stock/{}', PREFIX + 'serverless::worker.ts::restock', 'exact'),
        ('readArc', 'http::GET::/arcs/{}', PREFIX + 'terraform::runtime/index.ts::orbit', 'exact'),
        ('ready', 'http::GET::/ready', PREFIX + 'code::app/ready/route.ts::GET', 'exact'),
    }
    dump(run, 'oracle.json', {'answer_key': '#666/#642; ADR 0005/0025/0026',
                             'providers': expected_providers, 'links': sorted(expected_links)})
    inputs = {}
    for name, root in repos.items():
        for path in root.rglob('*'):
            if path.is_file() and '.git' not in path.parts:
                inputs[str(path.relative_to(run))] = hashlib.sha256(path.read_bytes()).hexdigest()
        git('add', '.', cwd=root)
        git('commit', '-qm', 'neutral declared-route sources', cwd=root)
    dump(run, 'input-sha256.json', inputs)

    def links(data):
        rows = [(row['consumer'].split('::')[-2], row['contract'], row['provider'], row['level'])
                for row in data['links']]
        assert len(rows) == len(set(rows)), rows
        return set(rows)

    results = {name: export(binary, root) for name, root in repos.items()}
    for name, data in results.items():
        dump(run, f'{name}.json', data)
        comparisons.compare(f'{name} providers', expected_providers[name], data['providers'], oracle=True)
    comparisons.compare('consumer links', sorted(expected_links), sorted(links(results['consumer'])), oracle=True)

    ambiguous = TF.replace('integration_uri = aws_lambda_function.arc.invoke_arn',
                           'integration_uri = var.unread ? aws_lambda_function.arc.invoke_arn : aws_lambda_function.other.invoke_arn')
    ambiguous += '\nresource "aws_lambda_function" "other" { function_name = "other" }\n'
    write(repos['terraform'], 'main.tf', ambiguous)
    changed = export(binary, repos['consumer'])
    dump(run, 'ambiguous-target.json', changed)
    ambiguous_provider = export(binary, repos['terraform'])
    dump(run, 'ambiguous-provider.json', ambiguous_provider)
    comparisons.compare('ambiguous target consumer links', sorted(row for row in expected_links if row[0] != 'readArc'),
                        sorted(links(changed)), oracle=True)
    comparisons.compare('ambiguous target providers', [], ambiguous_provider['providers'], oracle=True)
    write(repos['terraform'], 'main.tf', TF)

    write(repos['sam'], 'opaque.zip', 'opaque archive, never inspected')
    write(repos['sam'], 'template.yaml', SAM.replace('CodeUri: runtime/', 'CodeUri: opaque.zip'))
    fallback = export(binary, repos['consumer'])
    wanted = {(caller, contract, PREFIX + 'sam' if caller in {'readGarden', 'createGarden'} else site, level)
              for caller, contract, site, level in expected_links}
    incoming = export(binary, repos['sam'])
    dump(run, 'repository-fallback.json', fallback)
    dump(run, 'repository-fallback-provider.json', incoming)
    comparisons.compare('repository fallback outgoing links', sorted(wanted), sorted(links(fallback)), oracle=True)
    comparisons.compare('repository fallback incoming links', [PREFIX + 'sam'] * 2,
                        [row['provider'] for row in incoming['links']], oracle=True)
    write(repos['sam'], 'template.yaml', SAM)
    comparisons.compare('restored consumer links', sorted(expected_links),
                        sorted(links(export(binary, repos['consumer']))), oracle=True)

    if args.base_bin:
        control = run / 'work' / 'existing-head' / 'control'
        control.mkdir(parents=True)
        git('init', '-q', cwd=control)
        git('remote', 'add', 'origin', f'https://{PREFIX}control.git', cwd=control)
        write(control, 'meada.toml', '[serve]\nhosts = ["control.example.test"]\n')
        write(control, 'app/ready/route.ts', "export function GET() { return new Response('ready'); }\n")
        write(control, 'client.ts', "export function checkReady() { return fetch('https://control.example.test/ready'); }\n")
        git('add', '.', cwd=control)
        git('commit', '-qm', 'existing code HTTP control', cwd=control)
        controls = {'base': run / 'work' / 'existing-base' / 'control', 'head': control}
        shutil.copytree(control, controls['base'])
        binaries = {'base': str(Path(args.base_bin).resolve()), 'head': binary}
        exports = {label: export(exe, controls[label]) for label, exe in binaries.items()}
        for label, data in exports.items():
            dump(run, f'control-{label}.json', data)
        comparisons.compare('whole existing code HTTP export', exports['base'], exports['head'])
        query = [{'tool': 'context', 'arguments': {'symbol': 'checkReady', 'budget': 10000}}]
        observations = {}
        for name, exe in binaries.items():
            tools, texts, _ = mcp_answers(exe, controls[name], query, run / f'control-{name}.err')
            observations[name] = {'queries': query, 'tools': tools, 'answers': texts}
            dump(run, f'control-{name}-stdio.json', observations[name])
        comparisons.compare('existing code HTTP context and catalog', observations['base'], observations['head'])
        code_source_display_control(binaries, controls, run, comparisons)

    queries = [
        {'tool': 'context', 'arguments': {'symbol': 'readGarden', 'budget': 10000}},
        {'tool': 'trace', 'arguments': {'from': 'readGarden', 'direction': 'down', 'budget': 10000}},
        {'tool': 'search', 'arguments': {'query': 'gardens', 'kind': 'route', 'budget': 10000}},
        {'tool': 'context', 'arguments': {'symbol': 'http::GET::/gardens/{}', 'budget': 10000}},
    ]
    tools, texts, _ = mcp_answers(binary, repos['consumer'], queries, run / 'stdio.err')
    dump(run, 'queries.json', queries)
    dump(run, 'stdio-tools.json', tools)
    dump(run, 'stdio-answers.json', texts)
    comparisons.compare('stdio trace reaches serveGarden', True, 'serveGarden' in texts[1], oracle=True)
    manifest = 'index_dir = "indexes"\n' + ''.join(
        f'\n[[repository]]\nrepo = "{PREFIX}{name}"\npath = "{root}"\n' for name, root in repos.items())
    write(run, 'workspace.toml', manifest)
    subprocess.run([binary, 'index', '--workspace', 'workspace.toml'], cwd=run, check=True,
                   stdout=(run / 'index.out').open('w'), stderr=(run / 'index.err').open('w'))
    port = free_port()
    server = subprocess.Popen([binary, 'mcp', '--listen', f'127.0.0.1:{port}', '--workspace', 'workspace.toml'], cwd=run,
                              stdout=(run / 'http.out').open('w'), stderr=(run / 'http.err').open('w'))
    try:
        for _ in range(100):
            try:
                socket.create_connection(('127.0.0.1', port), timeout=1).close()
                break
            except OSError:
                time.sleep(.1)
        client = Http(f'http://127.0.0.1:{port}/{PREFIX}consumer')
        http_tools = client.call('tools/list', {})['result']['tools']
        dump(run, 'http-tools.json', http_tools)
        comparisons.compare('same-binary HTTP/stdin catalog parity', tools, http_tools, oracle=True)
        replies = [answer(client.call('tools/call', {'name': q['tool'], 'arguments': q['arguments']})) for q in queries]
        dump(run, 'http-answers.json', replies)
        comparisons.compare('same-binary HTTP/stdin answers', texts, replies, oracle=True)
        for name, path in [('sam', 'template.yaml'), ('serverless', 'serverless.yml'), ('terraform', 'main.tf')]:
            (repos[name] / path).unlink()
        deleted = [answer(client.call('tools/call', {'name': q['tool'], 'arguments': q['arguments']})) for q in queries]
        dump(run, 'source-deleted-http.json', deleted)
        comparisons.compare('IaC source deleted HTTP answers', texts, deleted, oracle=True)
        for name, path in [('sam', 'runtime/handler.ts'), ('sam', 'decoy.ts'),
                           ('serverless', 'worker.ts'), ('terraform', 'runtime/index.ts')]:
            (repos[name] / path).unlink()
        handlers_deleted = [answer(client.call('tools/call', {'name': q['tool'], 'arguments': q['arguments']})) for q in queries]
        dump(run, 'handler-source-deleted-http.json', handlers_deleted)
        expected_display = [text.replace('runtime/handler.ts:1', 'runtime/handler.ts')
                            .replace('worker.ts:1', 'worker.ts')
                            .replace('runtime/index.ts:1', 'runtime/index.ts') for text in texts]
        comparisons.compare('handler source deleted HTTP answers', expected_display, handlers_deleted, oracle=True)
        for name in names:
            readonly = json.loads(subprocess.check_output([binary, 'export', '--workspace', 'workspace.toml', '--repo', PREFIX + name], cwd=run, text=True))
            dump(run, f'source-deleted-{name}.json', readonly)
            comparisons.compare(f'{name} stored providers', expected_providers[name], readonly['providers'], oracle=True)
            comparisons.compare(f'{name} stored links', results[name]['links'], readonly['links'], oracle=True)
    finally:
        server.terminate()
        server.wait(timeout=30)
    dump(run, 'checks.json', {'script_exit': comparisons.exit_code(), 'verdict': 'UNPROVEN',
                             'answer_key': '#666/#642; ADR 0005/0025/0026',
                             'providers': 5, 'links': 5, 'base_parity': bool(args.base_bin),
                             'surfaces': ['CLI export', 'MCP stdio', 'MCP HTTP', 'IaC-source-deleted byte parity', 'handler-source-deleted graph and baseline display parity']})
    print('capture complete; evidence:', run)
    return comparisons.exit_code()


if __name__ == '__main__':
    run_driver(main)
