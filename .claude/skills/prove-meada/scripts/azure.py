#!/usr/bin/env python3
"""#669 public CLI/MCP probe. Answer key: #669/#642 and ADR 0026, not feature fixtures."""
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


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--head-bin', required=True)
    parser.add_argument('--run', required=True)
    parser.add_argument('--http', action='store_true')
    args = parser.parse_args()
    binary = str(Path(args.head_bin).resolve())
    run = Path(args.run).resolve()
    run.mkdir(parents=True, exist_ok=False)
    os.environ['TASK_QUEUE'] = 'process-environment-must-not-resolve'
    infra, app = run / 'work/infra', run / 'work/app'
    prefix = '/subscriptions/deployment/resourceGroups/team(prod)/providers/'
    cases = [
        ('azurerm_servicebus_queue', 'parcel', 'channel::servicebus::parcel', 'servicebus_queue', prefix+'Microsoft.ServiceBus/namespaces/bus/queues/parcel'),
        ('azurerm_servicebus_topic', 'changes', 'channel::servicebus::changes', 'servicebus_topic', prefix+'Microsoft.ServiceBus/namespaces/bus/topics/changes'),
        ('azurerm_eventhub', 'telemetry', 'channel::eventhubs::telemetry', 'eventhubs_hub', prefix+'Microsoft.EventHub/namespaces/hubs/eventhubs/telemetry'),
        ('azurerm_storage_account', 'assetsstore', 'resource::azurestorage::assetsstore', 'azurestorage_account', prefix+'Microsoft.Storage/storageAccounts/assetsstore'),
        ('azurerm_storage_container', 'assetsstore', 'resource::azureblob::assetsstore', 'azureblob_container', prefix+'Microsoft.Storage/storageAccounts/assetsstore/blobServices/default/containers/assetsstore'),
        ('azurerm_key_vault_secret', 'api-token', 'resource::keyvault::api-token', 'keyvault_secret', 'https://keys.vault.azure.net/secrets/api-token/version'),
    ]
    for root, name, peer in [(infra, 'infra', 'app'), (app, 'app', 'infra')]:
        root.mkdir(parents=True)
        git('init', '-q', cwd=root)
        git('remote', 'add', 'origin', f'https://example.com/azure-proof/{name}.git', cwd=root)
        write(root, 'meada.toml', f'[[peer]]\nrepo = "example.com/azure-proof/{peer}"\n[serve]\n')
    tf, settings, expected = '', '', set()
    for index, (ty, name, contract, alias, identifier) in enumerate(cases):
        tf += f'resource "{ty}" "case_{index}" {{ name = "{name}" }}\n'
        settings += f'exact_{index}: {identifier}\n{alias}: {name}\n'
        expected.update([(contract, f'settings.yaml#exact_{index}'), (contract, f'settings.yaml#{alias}')])
    tf += '''resource "azurerm_servicebus_subscription" "audit" {
 name = "audit"
 topic_id = azurerm_servicebus_topic.case_1.id
}
resource "azurerm_servicebus_subscription_rule" "filter" {
 name = "region"
 subscription_id = azurerm_servicebus_subscription.audit.id
 filter_type = "SqlFilter"
 sql_filter = "region = 'north'"
}
resource "aws_sqs_queue" "task" { name = "function-tasks" }
resource "azurerm_linux_function_app" "worker" {
 name = "worker"
 app_settings = {
  TASK_QUEUE = aws_sqs_queue.task.id
  SECRET_QUEUE = "@Microsoft.KeyVault(SecretUri=https://keys.vault.azure.net/secrets/api-token/version)"
 }
 site_config {
 application_stack {
 docker {
  registry_url = "https://registry.example.com"
  image_name = "azure/worker"
  image_tag = "stable"
 }
 }
 }
}
resource "azurerm_windows_function_app" "archive" {
 name = "archive"
 zip_deploy_file = "worker.zip"
 app_settings = { UNUSED = "function-tasks" }
}
'''
    sub = prefix+'Microsoft.ServiceBus/namespaces/bus/topics/changes/subscriptions/audit'
    settings += f'subscription: {sub}\nservicebus_subscription: changes/audit\nbare: parcel\nwrong: {prefix}Microsoft.Storage/namespaces/bus/queues/parcel\n'
    expected.update([('channel::servicebus::subscription:changes/audit', 'settings.yaml#subscription'), ('channel::servicebus::subscription:changes/audit', 'settings.yaml#servicebus_subscription')])
    write(infra, 'main.tf', tf)
    write(infra, 'worker.zip', 'archive never opened\n')
    write(app, 'settings.yaml', settings)
    write(app, '.github/workflows/image.yaml', 'jobs:\n  publish:\n    steps:\n      - uses: docker/build-push-action@v6\n        with:\n          tags: registry.example.com/azure/worker:stable\n')
    write(app, 'app.ts', "import { SQSClient, SendMessageCommand } from '@aws-sdk/client-sqs';\nconst client = new SQSClient({});\nexport function dispatch() { return client.send(new SendMessageCommand({QueueUrl: process.env.TASK_QUEUE, MessageBody: 'x'})); }\nexport function secret() { return client.send(new SendMessageCommand({QueueUrl: process.env.SECRET_QUEUE, MessageBody: 'x'})); }\n")
    inventory = {}
    for root in [infra, app]:
        for path in root.rglob('*'):
            if path.is_file() and '.git' not in path.parts:
                inventory[str(path.relative_to(run))] = hashlib.sha256(path.read_bytes()).hexdigest()
        git('add', '.', cwd=root)
        git('commit', '-qm', 'independent Azure input', cwd=root)
    (run / 'input-sha256.json').write_text(json.dumps(inventory, indent=2))
    data = export(binary, app)
    (run / 'app.json').write_text(json.dumps(data, indent=2))
    uses = data.get('uses', [])
    assert len(uses) == 14 and {(u['contract'], u['at']) for u in uses} == expected, uses
    provisions = data.get('provisions', [])
    assert len(provisions) == 15, provisions
    for link in provisions:
        if link.get('at'):
            key = link['at'].split('#')[1]
            assert link['level'] == ('exact' if key.startswith('exact_') or key == 'subscription' else 'inferred'), link
        else:
            assert link['contract'] == 'channel::sqs::function-tasks' and link['level'] == 'inferred', link
    head = export(binary, infra)
    (run / 'infra.json').write_text(json.dumps(head, indent=2))
    assert len(head.get('executions', [])) == 1, head
    assert any(x['path'] == 'worker.zip' for x in head['executions']), head
    assert len(head.get('links', [])) == 1 and head['links'][0]['contract'] == 'image::registry.example.com/azure/worker', head
    assert len(head.get('provisioners', [])) == 8, head
    queries = [
        {'tool': 'context', 'arguments': {'symbol': 'azurerm_linux_function_app.worker', 'budget': 12000}},
        {'tool': 'context', 'arguments': {'symbol': 'channel::servicebus::subscription:changes/audit', 'budget': 12000}},
        {'tool': 'trace', 'arguments': {'from': 'azurerm_linux_function_app.worker', 'direction': 'down', 'budget': 12000}},
        {'tool': 'trace', 'arguments': {'from': 'settings.yaml#subscription', 'direction': 'up', 'budget': 12000}},
    ]
    tools, texts, _ = mcp_answers(binary, infra, queries, run / 'stdio.err')
    (run / 'stdio.json').write_text(json.dumps(texts, indent=2))
    assert 'SECRET_QUEUE' in texts[0] and 'redacted' in texts[0] and 'registry.example.com/azure/worker' in texts[0], texts[0]
    assert 'azurerm_servicebus_subscription.audit' in texts[1], texts[1]
    assert 'app.ts' in texts[2], texts[2]
    assert 'azurerm_servicebus_subscription.audit' in texts[3], texts[3]
    (run / 'status.txt').write_text(subprocess.check_output([binary, 'status'], cwd=infra, text=True))
    if args.http:
        manifest = 'index_dir = "indexes"\n'
        for root, name in [(infra, 'infra'), (app, 'app')]:
            manifest += f'\n[[repository]]\nrepo = "example.com/azure-proof/{name}"\npath = "{root}"\n'
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
                client = Http(f'http://127.0.0.1:{port}/example.com/azure-proof/infra')
                assert client.call('tools/list', {})['result']['tools'] == tools
                replies = [answer(client.call('tools/call', {'name': q['tool'], 'arguments': q['arguments']})) for q in queries]
                (run / 'http.json').write_text(json.dumps(replies, indent=2))
                assert replies == texts
            finally:
                server.terminate()
                server.wait(timeout=30)
    (infra / 'worker.zip').unlink()
    changed = export(binary, infra)
    assert not changed.get('executions'), changed
    write(infra, 'worker.zip', 'archive restored\n')
    assert len(export(binary, infra).get('executions', [])) == 1
    (run / 'checks.json').write_text(json.dumps({'checks': 'passed', 'answer_key': '#669/#642; ADR 0026', 'surfaces': ['CLI', 'MCP stdio'] + (['MCP HTTP'] if args.http else [])}, indent=2))


if __name__ == '__main__':
    main()
