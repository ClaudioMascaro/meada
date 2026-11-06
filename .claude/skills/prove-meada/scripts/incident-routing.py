#!/usr/bin/env python3
"""Drive #673 on neutral repositories; preserve evidence, report checks, not a verdict."""
import argparse
import hashlib
import json
from pathlib import Path
import socket
import subprocess
import time
from alerts import command, write
from prove import git, free_port, Http, answer, mcp_answers


def link_rows(export):
    return sorted((x['contract'], x['level'], x['provider'], x['consumer']) for x in export['links'])


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--head-bin', required=True)
    parser.add_argument('--base-bin')
    parser.add_argument('--run', required=True)
    parser.add_argument('--http', action='store_true')
    args = parser.parse_args()
    binary = str(Path(args.head_bin).resolve())
    run = Path(args.run).resolve()
    run.mkdir(parents=True, exist_ok=False)
    roots = {name: run / 'work' / name for name in ['rules', 'monitors', 'routing', 'app']}
    identities = {name: 'example.com/incident-proof/' + name for name in roots}
    for name, root in roots.items():
        root.mkdir(parents=True)
        git('init', '-q', cwd=root)
        git('remote', 'add', 'origin', 'https://' + identities[name] + '.git', cwd=root)
        write(root, 'meada.toml', ''.join(f'[[peer]]\nrepo = "{identity}"\n' for other, identity in identities.items() if other != name))
    write(roots['rules'], 'signals.yaml', '''groups:
  - name: checkout
    rules:
      - alert: CheckoutDelay
        expr: latency_seconds > 10
        labels: {support: squad-cedar, owner: squad-birch}
      - alert: OtherTeam
        expr: errors_total > 2
        labels: {support: squad-maple, owner: squad-birch}
''')
    dispatch = '''route:
  receiver: fallback
  group_by: [support, owner]
  routes:
    - receiver: incidents
      match: {support: squad-cedar}
receivers:
  - name: fallback
  - name: incidents
    opsgenie_configs:
      - tags: 'dispatch:{{ .CommonLabels.owner }}'
    pagerduty_configs:
      - routing_key: neutral-not-a-real-credential
        details:
          support: '{{ .GroupLabels.support }}'
'''
    write(roots['routing'], 'dispatch.yaml', dispatch)
    write(roots['routing'], 'native.tf', '''resource "opsgenie_team_routing_rule" "datadog" {
  team_id = "platform-id"
  criteria {
    type = "match-all-conditions"
    conditions {
      field = "tags"
      operation = "matches"
      expected_value = "service:checkout"
    }
  }
  notify { type = "none" }
}
resource "opsgenie_team_routing_rule" "transformed" {
  team_id = "birch-id"
  criteria {
    type = "match-all-conditions"
    conditions {
      field = "tags"
      operation = "equals"
      expected_value = "dispatch:squad-birch"
    }
  }
  notify { type = "none" }
}
resource "pagerduty_event_orchestration_service" "checkout" {
  service = "checkout-id"
  set {
    id = "start"
    rule {
      condition { expression = "event.custom_details.support matches 'squad-cedar'" }
      actions {}
    }
  }
  catch_all {
    actions {}
  }
}
''')
    monitor = '''resource "datadog_monitor" "checkout" {
  name = "checkout health"
  type = "query alert"
  query = "avg(last_5m):avg:system.load.1{service:checkout} > 5"
  message = "Inspect checkout"
  tags = ["service:checkout", "support:squad-cedar"]
}
'''
    write(roots['monitors'], 'main.tf', monitor)
    write(roots['app'], 'values.yaml', 'groups:\n  - name: unrendered\n    rules:\n      - alert: HelmOnly\n        expr: up == 0\n        labels: {support: squad-cedar}\n')
    write(roots['app'], 'monitor.yaml', 'name: nonstandard monitor\ntags: ["service:checkout", "support:squad-cedar"]\n')
    for root in roots.values():
        git('add', '.', cwd=root)
        git('commit', '-qm', 'neutral incident forwarding proof', cwd=root)
    inventory = {str(p.relative_to(run)): hashlib.sha256(p.read_bytes()).hexdigest()
                 for root in roots.values() for p in root.rglob('*')
                 if p.is_file() and '.git' not in p.parts}
    write(run, 'inputs.sha256.json', json.dumps(inventory, indent=2, sort_keys=True))
    command(binary, roots['routing'], run, 'version', '--version')
    exports = {name: json.loads(command(binary, root, run, name, 'export')) for name, root in roots.items()}
    assert sorted(p['contract'] for p in exports['rules']['providers']) == [
        'alert::alertname::CheckoutDelay', 'alert::alertname::OtherTeam', 'alert::owner::squad-birch',
        'alert::owner::squad-birch', 'alert::support::squad-cedar', 'alert::support::squad-maple']
    assert sorted(p['contract'] for p in exports['monitors']['providers']) == ['alert::service::checkout', 'alert::support::squad-cedar']
    assert exports['app']['providers'] == [], exports['app']
    expected_consumers = sorted([
        ('alert::support::squad-cedar', identities['routing'] + '::dispatch.yaml::route.routes[0]'),
        ('alert::service::checkout', identities['routing'] + '::native.tf::opsgenie_team_routing_rule.datadog'),
        ('alert::dispatch::squad-birch', identities['routing'] + '::native.tf::opsgenie_team_routing_rule.transformed'),
        ('alert::support::squad-cedar', identities['routing'] + '::native.tf::pagerduty_event_orchestration_service.checkout.set[0].rule[0]')])
    assert sorted((x['contract'], x['site']) for x in exports['routing']['consumers']) == expected_consumers, exports['routing']
    expected = sorted([
        ('alert::support::squad-cedar', 'exact', identities['rules'] + '::signals.yaml::groups[0].rules[0]', identities['routing'] + '::dispatch.yaml::route.routes[0]'),
        ('alert::service::checkout', 'inferred', identities['monitors'] + '::main.tf::datadog_monitor.checkout', identities['routing'] + '::native.tf::opsgenie_team_routing_rule.datadog'),
        ('alert::dispatch::squad-birch', 'exact', identities['rules'] + '::signals.yaml::groups[0].rules[0]', identities['routing'] + '::native.tf::opsgenie_team_routing_rule.transformed'),
        ('alert::support::squad-cedar', 'exact', identities['rules'] + '::signals.yaml::groups[0].rules[0]', identities['routing'] + '::native.tf::pagerduty_event_orchestration_service.checkout.set[0].rule[0]')])
    write(run, 'expected-links.json', json.dumps(expected, indent=2))
    assert link_rows(exports['routing']) == expected, exports['routing']
    for name, export in exports.items():
        prefix = identities[name] + '::'
        local = [row for row in expected if row[2].startswith(prefix) or row[3].startswith(prefix)]
        assert link_rows(export) == local, export
    command(binary, roots['routing'], run, 'status', 'status')
    queries = [
        {'tool': 'search', 'arguments': {'query': 'squad-cedar', 'kind': 'alert', 'budget': 12000}},
        {'tool': 'context', 'arguments': {'symbol': 'native.tf::opsgenie_team_routing_rule.transformed', 'budget': 12000}},
        {'tool': 'trace', 'arguments': {'from': 'alert::support::squad-cedar', 'direction': 'down', 'budget': 16000}},
        {'tool': 'trace', 'arguments': {'from': 'native.tf::pagerduty_event_orchestration_service.checkout.set[0].rule[0]', 'direction': 'up', 'budget': 12000}},
        {'tool': 'context', 'arguments': {'symbol': 'alert::dispatch::squad-birch', 'budget': 12000}}]
    tools, texts, _ = mcp_answers(binary, roots['routing'], queries, run / 'stdio.err')
    write(run, 'queries.json', json.dumps(queries, indent=2))
    write(run, 'stdio.json', json.dumps(texts, indent=2))
    assert 'alert::support::squad-cedar' in texts[0], texts[0]
    assert 'receiver: birch-id' in texts[1], texts[1]
    assert all(name in texts[2] for name in ['datadog_monitor.checkout', 'opsgenie_team_routing_rule.datadog', 'opsgenie_team_routing_rule.transformed', 'pagerduty_event_orchestration_service.checkout']), texts[2]
    assert 'signals.yaml' in texts[3] and 'forwarding:' in texts[3], texts[3]
    assert 'signals.yaml' in texts[4] and 'emitter' in texts[4], texts[4]
    # Filtering out the receiver also removes both of its incident field forwardings.
    write(roots['routing'], 'dispatch.yaml', dispatch.replace('match: {support: squad-cedar}', 'match: {support: squad-absent}'))
    filtered = json.loads(command(binary, roots['routing'], run, 'filtered-receiver', 'export'))
    assert link_rows(filtered) == [row for row in expected if 'datadog_monitor' in row[2]], filtered
    write(roots['routing'], 'dispatch.yaml', dispatch)
    restored = json.loads(command(binary, roots['routing'], run, 'restored-receiver', 'export'))
    assert link_rows(restored) == expected, restored
    write(roots['monitors'], '.meadaignore', 'main.tf\n')
    ignored = json.loads(command(binary, roots['routing'], run, 'ignored-monitor', 'export'))
    assert link_rows(ignored) == [row for row in expected if 'datadog_monitor' not in row[2]], ignored
    (roots['monitors'] / '.meadaignore').unlink()
    restored = json.loads(command(binary, roots['routing'], run, 'restored-monitor', 'export'))
    assert link_rows(restored) == expected, restored
    if args.base_bin:
        existing = run / 'existing'
        existing.mkdir()
        git('init', '-q', cwd=existing)
        git('remote', 'add', 'origin', 'https://example.com/incident-proof/existing.git', cwd=existing)
        write(existing, 'rules.yaml', 'groups:\n  - name: known\n    rules:\n      - alert: Known\n        expr: up == 0\n        labels: {support: squad-cedar}\n')
        write(existing, 'dispatch.yaml', 'route:\n  receiver: known\n  match: {support: squad-cedar}\nreceivers: [{name: known}]\n')
        git('add', '.', cwd=existing)
        git('commit', '-qm', 'existing alert parity', cwd=existing)
        before = command(str(Path(args.base_bin).resolve()), existing, run, 'existing-base', 'export')
        after = command(binary, existing, run, 'existing-head', 'export')
        assert before == after, 'Existing alert export bytes changed'
    if args.http:
        manifest = 'index_dir = "indexes"\n' + ''.join(f'[[repository]]\nrepo = "{identities[name]}"\npath = "{root}"\n' for name, root in roots.items())
        write(run, 'workspace.toml', manifest)
        command(binary, run, run, 'index', 'index', '--workspace', 'workspace.toml')
        port = free_port()
        with open(run / 'http.out', 'w') as out, open(run / 'http.err', 'w') as err:
            server = subprocess.Popen([binary, 'mcp', '--listen', f'127.0.0.1:{port}', '--workspace', 'workspace.toml'], cwd=run, stdin=subprocess.DEVNULL, stdout=out, stderr=err)
            try:
                for _ in range(100):
                    try:
                        socket.create_connection(('127.0.0.1', port), timeout=1).close()
                        break
                    except OSError:
                        if server.poll() is not None: raise RuntimeError('HTTP server exited before readiness')
                        time.sleep(.1)
                client = Http(f'http://127.0.0.1:{port}/{identities["routing"]}')
                assert client.call('tools/list', {})['result']['tools'] == tools
                http = [answer(client.call('tools/call', {'name': q['tool'], 'arguments': q['arguments']})) for q in queries]
                write(run, 'http.json', json.dumps(http, indent=2))
                assert http == texts, http
            finally:
                server.terminate()
                server.wait(timeout=30)
                write(run, 'http.cleanup.json', json.dumps({'pid': server.pid, 'exit': server.returncode}))
    write(run, 'checks.txt', 'CLI provider/consumer/link sets, explicit receiver transformations, native levels, tag trace starts, source/receiver filtering and restoration passed.\n' + ('Base alert export parity passed.\n' if args.base_bin else 'Base parity not requested.\n') + ('HTTP parity and server cleanup passed.\n' if args.http else 'HTTP not requested.\n'))
    print(run / 'checks.txt')


if __name__ == '__main__':
    main()
