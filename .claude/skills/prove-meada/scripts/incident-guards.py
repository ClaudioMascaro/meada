#!/usr/bin/env python3
"""Preserved independent #673 findings: opaque AND and first-match routing barriers.

The seven keys come from the provider's condition semantics and the review's neutral
counterexamples, not Meada's extraction or feature-test fixtures. No verdict is inferred
from transport parity. Keep every original source and complete export on failure.
"""
import argparse
import hashlib
import json
from pathlib import Path
from alerts import command, write
from prove import git

DYNAMIC = '''resource "datadog_monitor" "cedar" {
  name = "Neutral dynamic-criteria control"
  type = "query alert"
  query = "sum(last_5m):sum:neutral.metric{*} > 1"
  message = "Neutral"
  tags = ["support:squad-elm"]
}
resource "opsgenie_team_routing_rule" "guarded" {
  team_id = "team-elm-id"
  criteria {
    type = "match-all-conditions"
    conditions {
      field = "tags"
      operation = "equals"
      expected_value = "support:squad-elm"
    }
    dynamic "conditions" {
      for_each = [1]
      content {
        field = "tags"
        operation = "equals"
        expected_value = "gate:closed"
      }
    }
  }
  notify { type = "none" }
}
'''
ORDER = '''resource "datadog_monitor" "source" {
  name = "Neutral first-match control"
  tags = ["support:squad-ash"]
}
resource "opsgenie_team_routing_rule" "first" {
  count = 1
  team_id = "team-ash-id"
  order = 0
  criteria {
    type = "match-all-conditions"
    conditions {
      field = "tags"
      operation = "equals"
      expected_value = "support:squad-ash"
    }
  }
  notify { type = "none" }
}
resource "opsgenie_team_routing_rule" "second" {
  team_id = "team-ash-id"
  order = 1
  criteria {
    type = "match-all-conditions"
    conditions {
      field = "tags"
      operation = "equals"
      expected_value = "support:squad-ash"
    }
  }
  notify { type = "none" }
}
'''


def cases():
    elm = ('alert::support::squad-elm', 'exact', 'datadog_monitor.cedar', 'opsgenie_team_routing_rule.guarded')
    ash = lambda target: ('alert::support::squad-ash', 'exact', 'datadog_monitor.source', 'opsgenie_team_routing_rule.' + target)
    return [
        ('dynamic-and', DYNAMIC, [], 1),
        ('dynamic-or', DYNAMIC.replace('match-all-conditions', 'match-any-condition'), [elm], 1),
        ('counted-first', ORDER, [], 1),
        ('for-each-first', ORDER.replace('count = 1', 'for_each = { once = 1 }'), [], 1),
        ('unknown-team-first', ORDER.replace('  count = 1\n', '').replace('team_id = "team-ash-id"', 'team_id = var.incident.id', 1), [], 2),
        ('different-team-first', ORDER.replace('team_id = "team-ash-id"', 'team_id = "other-team-id"', 1), [ash('second')], 1),
        ('literal-first', ORDER.replace('  count = 1\n', ''), [ash('first')], 2),
    ]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--head-bin', required=True)
    parser.add_argument('--run', required=True)
    args = parser.parse_args()
    run, binary = Path(args.run).resolve(), str(Path(args.head_bin).resolve())
    run.mkdir(parents=True, exist_ok=False)
    results = []
    for name, source, expected_short, consumers in cases():
        folder, root = run / name, run / name / 'repo'
        root.mkdir(parents=True)
        identity = 'example.com/incident-guards/' + name
        write(root, 'main.tf', source)
        write(root, 'meada.toml', '')
        git('init', '-q', cwd=root)
        git('remote', 'add', 'origin', 'https://' + identity + '.git', cwd=root)
        git('add', '.', cwd=root)
        git('commit', '-qm', 'neutral incident guard proof', cwd=root)
        prefix = identity + '::main.tf::'
        expected = sorted((contract, level, prefix + provider, prefix + consumer)
                          for contract, level, provider, consumer in expected_short)
        write(folder, 'expected-links.json', json.dumps(expected, indent=2))
        write(folder, 'inputs.sha256.json', json.dumps({'main.tf': hashlib.sha256(source.encode()).hexdigest()}))
        data = json.loads(command(binary, root, folder, 'export', 'export'))
        command(binary, root, folder, 'status', 'status')
        actual = sorted((x['contract'], x['level'], x['provider'], x['consumer']) for x in data['links'])
        assert actual == expected, (name, 'incident routing links', expected, actual)
        assert len(data['consumers']) == consumers, (name, 'consumer count', data['consumers'])
        if name in {'counted-first', 'for-each-first', 'different-team-first'}:
            assert [c['site'] for c in data['consumers']] == [prefix + 'opsgenie_team_routing_rule.second'], name
            assert all(n['identity'] != prefix + 'opsgenie_team_routing_rule.first' for n in data['iac_nodes']), name
        results.append({'case': name, 'passed': True})
        write(run, 'checks.json', json.dumps({'checks': results, 'answer_key': '#673/#642, native provider conditions',
              'checker': 'script only, not independent or blind'}, indent=2))
    print('7 incident guard checks passed; no independent verdict.')


if __name__ == '__main__':
    main()
