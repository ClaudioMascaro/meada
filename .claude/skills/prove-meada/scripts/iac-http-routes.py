#!/usr/bin/env python3
"""#666: complete source-derived HTTP provider/link sets and stored-query parity."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import time

from prove import Http, answer, free_port, git, mcp_answers
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
resource "aws_apigatewayv2_api" "public" { name = "arcs-http"
 protocol_type = "HTTP" }
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


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--head-bin', required=True)
    parser.add_argument('--base-bin')
    parser.add_argument('--run', required=True)
    args = parser.parse_args()
    binary = str(Path(args.head_bin).resolve())
    run = Path(args.run).resolve()
    run.mkdir(parents=True, exist_ok=False)
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
    # These expectations come from the declarations above and #666, never an observed export.
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
        assert data['providers'] == expected_providers[name], (name, data['providers'])
    assert links(results['consumer']) == expected_links, results['consumer']['links']

    # An unread conditional with two possible Lambda targets cannot select either one.
    ambiguous = TF.replace('integration_uri = aws_lambda_function.arc.invoke_arn',
                           'integration_uri = var.unread ? aws_lambda_function.arc.invoke_arn : aws_lambda_function.other.invoke_arn')
    ambiguous += '\nresource "aws_lambda_function" "other" { function_name = "other" }\n'
    write(repos['terraform'], 'main.tf', ambiguous)
    changed = export(binary, repos['consumer'])
    assert links(changed) == {row for row in expected_links if row[0] != 'readArc'}, changed['links']
    assert export(binary, repos['terraform'])['providers'] == []
    dump(run, 'ambiguous-target.json', changed)
    write(repos['terraform'], 'main.tf', TF)

    # Safe route facts survive an opaque code input, with repository fallback on both sides.
    write(repos['sam'], 'opaque.zip', 'opaque archive, never inspected')
    write(repos['sam'], 'template.yaml', SAM.replace('CodeUri: runtime/', 'CodeUri: opaque.zip'))
    fallback = export(binary, repos['consumer'])
    wanted = {(caller, contract, PREFIX + 'sam' if caller in {'readGarden', 'createGarden'} else site, level)
              for caller, contract, site, level in expected_links}
    assert links(fallback) == wanted, fallback['links']
    incoming = export(binary, repos['sam'])
    assert len(incoming['links']) == 2 and all(row['provider'] == PREFIX + 'sam' for row in incoming['links'])
    dump(run, 'repository-fallback.json', fallback)
    write(repos['sam'], 'template.yaml', SAM)
    assert links(export(binary, repos['consumer'])) == expected_links

    if args.base_bin:
        # A standalone code HTTP flow must retain every existing export field and query byte.
        control = run / 'control'
        control.mkdir()
        git('init', '-q', cwd=control)
        git('remote', 'add', 'origin', f'https://{PREFIX}control.git', cwd=control)
        write(control, 'meada.toml', '[serve]\nhosts = ["control.example.test"]\n')
        write(control, 'app/ready/route.ts', "export function GET() { return new Response('ready'); }\n")
        write(control, 'client.ts', "export function checkReady() { return fetch('https://control.example.test/ready'); }\n")
        git('add', '.', cwd=control)
        git('commit', '-qm', 'existing code HTTP control', cwd=control)
        head, base = export(binary, control), export(str(Path(args.base_bin).resolve()), control)
        dump(run, 'control-head.json', head)
        dump(run, 'control-base.json', base)
        assert head == base
        query = [{'tool': 'context', 'arguments': {'symbol': 'checkReady', 'budget': 10000}}]
        _, head_text, _ = mcp_answers(binary, control, query, run / 'control-head.err')
        _, base_text, _ = mcp_answers(str(Path(args.base_bin).resolve()), control, query, run / 'control-base.err')
        assert head_text == base_text

    queries = [
        {'tool': 'context', 'arguments': {'symbol': 'readGarden', 'budget': 10000}},
        {'tool': 'trace', 'arguments': {'from': 'readGarden', 'direction': 'down', 'budget': 10000}},
        {'tool': 'search', 'arguments': {'query': 'gardens', 'kind': 'route', 'budget': 10000}},
        {'tool': 'context', 'arguments': {'symbol': 'http::GET::/gardens/{}', 'budget': 10000}},
    ]
    tools, texts, _ = mcp_answers(binary, repos['consumer'], queries, run / 'stdio.err')
    dump(run, 'stdio-answers.json', texts)
    assert 'serveGarden' in texts[1], texts
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
        assert client.call('tools/list', {})['result']['tools'] == tools
        replies = [answer(client.call('tools/call', {'name': q['tool'], 'arguments': q['arguments']})) for q in queries]
        dump(run, 'http-answers.json', replies)
        assert replies == texts
        for name, path in [('sam', 'template.yaml'), ('sam', 'runtime/handler.ts'), ('sam', 'decoy.ts'),
                           ('serverless', 'serverless.yml'), ('serverless', 'worker.ts'),
                           ('terraform', 'main.tf'), ('terraform', 'runtime/index.ts')]:
            (repos[name] / path).unlink()
        deleted = [answer(client.call('tools/call', {'name': q['tool'], 'arguments': q['arguments']})) for q in queries]
        dump(run, 'source-deleted-http.json', deleted)
        assert deleted == texts
        for name in names:
            readonly = json.loads(subprocess.check_output([binary, 'export', '--workspace', 'workspace.toml', '--repo', PREFIX + name], cwd=run, text=True))
            dump(run, f'source-deleted-{name}.json', readonly)
            assert readonly['providers'] == expected_providers[name]
            assert readonly['links'] == results[name]['links']
    finally:
        server.terminate()
        server.wait(timeout=30)
    dump(run, 'checks.json', {'verdict': 'PROVED', 'answer_key': '#666/#642; ADR 0005/0025/0026',
                             'providers': 5, 'links': 5, 'base_parity': bool(args.base_bin),
                             'surfaces': ['CLI export', 'MCP stdio', 'MCP HTTP', 'source-deleted read-only export/HTTP']})
    print('checks passed; evidence:', run)


if __name__ == '__main__':
    main()
