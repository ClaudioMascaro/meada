#!/usr/bin/env python3
"""#664 acceptance through public CLI and MCP, with independent generated inputs.

Answer keys: #664/#642 and ADR 0025/0026. A script pass supplies evidence, not a verdict.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import time
from prove import git, free_port, Http, answer, mcp_answers
from provisions import write, export

PREFIX = 'example.com/aws-workload-proof/'
IMAGE = 'registry.example.com/proof/worker'
MISSING = 'registry.example.com/proof/unbuilt'
SECRET = 'aws-workload-noecho-canary'


def sdk(variable='TASK_QUEUE', function='dispatch'):
    return ("import { SQSClient, SendMessageCommand } from '@aws-sdk/client-sqs';\n"
            "const client = new SQSClient({});\n"
            f"export function {function}() {{ return client.send(new SendMessageCommand({{QueueUrl: process.env.{variable}, MessageBody: 'x'}})); }}\n")


def seed(app, infra):
    for root, name, peer in [(app, 'app', 'infra'), (infra, 'infra', 'app')]:
        root.mkdir(parents=True)
        git('init', '-q', cwd=root)
        git('remote', 'add', 'origin', f'https://{PREFIX}{name}.git', cwd=root)
        write(root, 'meada.toml', f'[[peer]]\nrepo = "{PREFIX}{peer}"\n')
    write(app, '.github/workflows/build.yaml', f'jobs:\n  publish:\n    steps:\n      - uses: docker/build-push-action@v6\n        with:\n          tags: {IMAGE}:1\n')
    write(app, 'send.ts', sdk() +
          "export function other() { return client.send(new SendMessageCommand({QueueUrl: process.env.OTHER_QUEUE, MessageBody: 'x'})); }\n")
    write(infra, 'main.tf', '''variable "code_dir" { default = "lambda" }
resource "aws_sqs_queue" "orders" { name = "orders" }
resource "aws_sqs_queue" "jobs" { name = "jobs" }
resource "aws_lambda_function" "image" {
  function_name = "image"
  image_uri = "registry.example.com/proof/worker:1"
  environment { variables = { TASK_QUEUE = "orders" } }
}
resource "aws_lambda_function" "code" {
  function_name = "code"
  filename = var.code_dir
  handler = "send.dispatch"
  environment { variables = { TASK_QUEUE = aws_sqs_queue.jobs.name } }
}
resource "aws_lambda_function" "zip" {
  function_name = "zip"
  filename = "worker.zip"
}
resource "aws_lambda_function" "remote" {
  function_name = "remote"
  s3_bucket = "remote"
  s3_key = "code.zip"
}
resource "aws_lambda_function" "unbuilt" {
  function_name = "unbuilt"
  image_uri = "registry.example.com/proof/unbuilt:1"
}
resource "aws_ecs_task_definition" "task" {
  family = "task"
  container_definitions = jsonencode([
    { name = "worker", image = "registry.example.com/proof/worker:1", environment = [{ name = "TASK_QUEUE", value = "orders" }] },
    { name = "sidecar", image = "registry.example.com/proof/unbuilt:1", environment = [{ name = "OTHER_QUEUE", value = "must-not-cross-image" }] }
  ])
}
''')
    write(infra, 'worker.zip', 'opaque archive location; never extract\n')
    for path in ['lambda/send.ts', 'other/send.ts', 'outside.ts', 'sam/send.ts', 'functions/send.ts']:
        write(infra, path, sdk())
    write(infra, 'sam/secret.ts', sdk('SECRET_QUEUE', 'secret'))
    write(infra, 'sam.yaml', f'''Transform: AWS::Serverless-2016-10-31
Parameters:
  Hidden:
    Type: String
    NoEcho: true
    Default: {SECRET}
Globals:
  Function:
    Environment:
      Variables:
        TASK_QUEUE: wrong-global
Resources:
  SamJobs:
    Type: AWS::SQS::Queue
    Properties:
      QueueName: sam-jobs
  SamWorker:
    Type: AWS::Serverless::Function
    Properties:
      CodeUri: sam
      Handler: send.dispatch
      Environment:
        Variables:
          TASK_QUEUE: !Ref SamJobs
          SECRET_QUEUE: !Ref Hidden
''')
    write(infra, 'serverless.yml', '''service: worker
provider:
  name: aws
  environment:
    TASK_QUEUE: wrong-provider
functions:
  local:
    handler: functions/send.dispatch
    environment:
      TASK_QUEUE: sls-jobs
''')


def provider_set(data):
    return {(p['site'], p['contract']) for p in data['providers'] if p['contract'].startswith('channel::')}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--head-bin', required=True)
    parser.add_argument('--run', required=True)
    args = parser.parse_args()
    binary = str(Path(args.head_bin).resolve())
    run = Path(args.run).resolve()
    run.mkdir(parents=True, exist_ok=False)
    app, infra = run / 'work/app', run / 'work/infra'
    os.environ['TASK_QUEUE'] = 'process-value-must-not-win'
    seed(app, infra)
    inventory = {}
    for root in [app, infra]:
        for path in root.rglob('*'):
            if path.is_file() and '.git' not in path.parts:
                inventory[str(path.relative_to(run))] = hashlib.sha256(path.read_bytes()).hexdigest()
        git('add', '.', cwd=root)
        git('commit', '-qm', 'independent AWS workload inputs', cwd=root)
    (run / 'input-sha256.json').write_text(json.dumps(inventory, indent=2))
    (run / 'binary.json').write_text(json.dumps({'path': binary, 'sha256': hashlib.sha256(Path(binary).read_bytes()).hexdigest(), 'version': subprocess.check_output([binary, '--version'], text=True)}, indent=2))
    initial = {name: export(binary, root) for name, root in [('app', app), ('infra', infra)]}
    for name, data in initial.items():
        (run / f'{name}.json').write_text(json.dumps(data, indent=2))
    expected_app = {(f'{PREFIX}app::send.ts::{fn}::SendMessageCommand#0', contract)
                    for fn, contract in [('dispatch', 'channel::sqs::orders'), ('other', 'channel::sqs')]}
    expected_infra = {(f'{PREFIX}infra::{path}::{fn}::SendMessageCommand#0', contract) for path, fn, contract in [
        ('lambda/send.ts', 'dispatch', 'channel::sqs::jobs'), ('other/send.ts', 'dispatch', 'channel::sqs'),
        ('outside.ts', 'dispatch', 'channel::sqs'), ('sam/send.ts', 'dispatch', 'channel::sqs::sam-jobs'),
        ('sam/secret.ts', 'secret', 'channel::sqs'), ('functions/send.ts', 'dispatch', 'channel::sqs::sls-jobs')]}
    assert provider_set(initial['app']) == expected_app, initial['app']
    assert provider_set(initial['infra']) == expected_infra, initial['infra']
    consumers = {(p['site'], p['contract']) for p in initial['infra']['consumers']}
    assert consumers == {(f'{PREFIX}infra::main.tf::{node}', 'image::' + image) for node, image in [
        ('aws_lambda_function.image', IMAGE), ('aws_lambda_function.unbuilt', MISSING),
        ('aws_ecs_task_definition.task', IMAGE), ('aws_ecs_task_definition.task', MISSING)]}, consumers
    executions = {(x['workload'].split('::')[-1], x['path'], x['root'], x['level']) for x in initial['infra']['executions']}
    expected_exec = {('aws_lambda_function.code', 'lambda', '.', 'exact'), ('aws_lambda_function.zip', 'worker.zip', '.', 'exact'),
                     ('SamWorker', 'sam', 'sam.yaml', 'exact'), ('functions.local', 'functions/send.ts', 'serverless.yml', 'exact')}
    assert executions == expected_exec, executions
    expected_links = {(f'{PREFIX}infra::main.tf::{node}', PREFIX + 'app', 'image::' + IMAGE, '.', 'exact')
                      for node in ['aws_lambda_function.image', 'aws_ecs_task_definition.task']}
    actual_links = {(l['consumer'], l['provider'], l['contract'], l['root'], l['level']) for l in initial['infra']['links']}
    assert actual_links == expected_links, initial['infra']['links']
    app_provisions = [p for p in initial['app'].get('provisions', []) if p['side'].startswith(PREFIX + 'app::')]
    assert {(p['side'], p['provisioner'], p['root'], p['contract'], p['level']) for p in app_provisions} == {
        (f'{PREFIX}app::send.ts::dispatch::SendMessageCommand#0', f'{PREFIX}infra::main.tf::aws_sqs_queue.orders', '.', 'channel::sqs::orders', 'inferred')}, app_provisions
    assert all('TASK_QUEUE' in p['reason'] for p in app_provisions), app_provisions
    status = subprocess.check_output([binary, 'status'], cwd=infra, text=True)
    (run / 'status.txt').write_text(status)
    assert f'orphan image::{MISSING}: no builder' in status, status
    queries = [
        {'tool': 'context', 'arguments': {'symbol': 'aws_ecs_task_definition.task', 'budget': 12000}},
        {'tool': 'context', 'arguments': {'symbol': 'SamWorker', 'budget': 12000}},
        {'tool': 'context', 'arguments': {'symbol': 'functions.local', 'budget': 12000}},
        {'tool': 'trace', 'arguments': {'from': 'aws_lambda_function.image', 'direction': 'down', 'budget': 12000}},
        {'tool': 'trace', 'arguments': {'from': 'SamWorker', 'direction': 'down', 'budget': 12000}},
        {'tool': 'trace', 'arguments': {'from': PREFIX + 'infra::sam/send.ts::dispatch', 'direction': 'up', 'budget': 12000}},
    ]
    tools, texts, _ = mcp_answers(binary, infra, queries, run / 'stdio.err')
    (run / 'stdio.json').write_text(json.dumps(texts, indent=2))
    assert 'worker' in texts[0] and 'sidecar' in texts[0] and 'TASK_QUEUE' in texts[0], texts[0]
    assert 'code:' in texts[1] and 'channel::sqs::sam-jobs' in texts[1] and 'unresolved' in texts[1], texts[1]
    assert 'functions/send.ts' in texts[2] and 'sls-jobs' in texts[2], texts[2]
    assert 'send.ts' in texts[3] and 'sam/send.ts' in texts[4] and 'SamWorker' in texts[5], texts
    assert SECRET not in json.dumps(initial) + json.dumps(texts) and 'process-value-must-not-win' not in json.dumps(initial), initial
    manifest = 'index_dir = "indexes"\n'
    for root, name in [(app, 'app'), (infra, 'infra')]:
        manifest += f'\n[[repository]]\nrepo = "{PREFIX}{name}"\npath = "{root}"\n'
    write(run, 'workspace.toml', manifest)
    with (run / 'index.out').open('w') as out, (run / 'index.err').open('w') as err:
        subprocess.run([binary, 'index', '--workspace', 'workspace.toml'], cwd=run, check=True, stdout=out, stderr=err)
    port = free_port()
    with (run / 'http.out').open('w') as out, (run / 'http.err').open('w') as err:
        server = subprocess.Popen([binary, 'mcp', '--listen', f'127.0.0.1:{port}', '--workspace', 'workspace.toml'], cwd=run, stdout=out, stderr=err)
        try:
            for _ in range(100):
                try:
                    socket.create_connection(('127.0.0.1', port), timeout=1).close()
                    break
                except OSError:
                    time.sleep(.1)
            client = Http(f'http://127.0.0.1:{port}/{PREFIX}infra')
            assert client.call('tools/list', {})['result']['tools'] == tools
            replies = [answer(client.call('tools/call', {'name': q['tool'], 'arguments': q['arguments']})) for q in queries]
            (run / 'http.json').write_text(json.dumps(replies, indent=2))
            assert replies == texts
        finally:
            server.terminate()
            server.wait(timeout=30)
    main_hash = hashlib.sha256((infra / 'main.tf').read_bytes()).hexdigest()
    write(infra, 'terraform.tfvars', 'code_dir = "other"\n')
    changed = export(binary, infra)
    (run / 'varfile-only.json').write_text(json.dumps(changed, indent=2))
    assert any(x['path'] == 'other' and x['workload'].endswith('aws_lambda_function.code') for x in changed['executions']), changed
    assert (f'{PREFIX}infra::other/send.ts::dispatch::SendMessageCommand#0', 'channel::sqs::jobs') in provider_set(changed), changed
    assert hashlib.sha256((infra / 'main.tf').read_bytes()).hexdigest() == main_hash
    write(infra, 'terraform.tfvars', 'code_dir = "lambda/../lambda"\n')
    refused = export(binary, infra)
    (run / 'parent-path.json').write_text(json.dumps(refused, indent=2))
    assert not any(x['workload'].endswith('aws_lambda_function.code') for x in refused.get('executions', [])), refused
    (infra / 'terraform.tfvars').unlink()
    (infra / 'lambda').rename(infra / 'lambda-saved')
    (infra / 'lambda').symlink_to('lambda-saved', target_is_directory=True)
    try:
        symlink = export(binary, infra)
        (run / 'symlink-only.json').write_text(json.dumps(symlink, indent=2))
        assert not any(x['workload'].endswith('aws_lambda_function.code') for x in symlink.get('executions', [])), symlink
    finally:
        (infra / 'lambda').unlink()
        (infra / 'lambda-saved').rename(infra / 'lambda')
    write(infra, 'functions/send.js', sdk())
    ambiguous = export(binary, infra)
    (run / 'ambiguous-handler.json').write_text(json.dumps(ambiguous, indent=2))
    assert not any(x['workload'].endswith('functions.local') for x in ambiguous['executions']), ambiguous
    (infra / 'functions/send.js').unlink()
    restored = export(binary, infra)
    (run / 'restored.json').write_text(json.dumps(restored, indent=2))
    assert provider_set(restored) == expected_infra
    assert {(x['workload'].split('::')[-1], x['path'], x['root'], x['level']) for x in restored['executions']} == expected_exec
    (run / 'checks.json').write_text(json.dumps({'checks': 'passed', 'answer_key': '#664/#642; ADR 0025/0026', 'surfaces': ['CLI', 'MCP stdio', 'MCP HTTP'], 'independent_verdict': 'pending'}, indent=2))


if __name__ == '__main__':
    main()
