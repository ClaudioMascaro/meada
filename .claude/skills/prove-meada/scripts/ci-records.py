#!/usr/bin/env python3
"""The run records of the A9 yardstick (#904), captured once into `ci-records.json`: for each repository
of the pinned CI sets, GitHub's runs whose `head_sha` is its pin, each with its file (`path`) and the
reusable pipelines it called (`referenced_workflows`). They are the answer key of the pipeline calls,
never fetched while measuring: `ci.py --corpus` copies this file beside its output and reads nothing else.

Every repository of a set is asked, not only those whose files call a pipeline, so no reading of the
source chooses which records exist. Uses `gh api` and the caller's token; stops before `--max-calls`.
"""
import argparse
import datetime
import json
from pathlib import Path
import subprocess
import sys

import prove

HERE = Path(__file__).resolve().parent
OUT = HERE / 'ci-records.json'
KEPT = ('id', 'name', 'path', 'event', 'status', 'conclusion', 'run_attempt')


def sets():
    """infra.tsv name -> (owner/repo, pin) for every CI row: set P (`ci`) and the yardstick's (`ci-*`)."""
    return {name: (url.removeprefix('https://github.com/'), sha) for name, (flows, url, sha) in prove.infra().items()
            if any(flow == 'ci' or flow.startswith('ci-') for flow in flows)}


def kept(run):
    row = {key: run.get(key) for key in KEPT}
    row['referenced_workflows'] = sorted(({key: ref.get(key) for key in ('path', 'sha', 'ref')} for ref in run.get('referenced_workflows') or []),
                                         key=lambda ref: json.dumps(ref, sort_keys=True))
    return row


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--max-calls', type=int, default=1000)
    args = parser.parse_args()
    rows = sets()
    remaining = json.loads(subprocess.run(['gh', 'api', 'rate_limit'], check=True, capture_output=True, text=True).stdout)['resources']['core']['remaining']
    if remaining < args.max_calls:
        sys.exit(f'{remaining} REST calls left this hour, fewer than --max-calls {args.max_calls}: try after the reset')
    calls, records = 0, {}
    for name, (repository, sha) in sorted(rows.items()):
        runs, page = [], 1
        while True:
            if calls >= args.max_calls:
                sys.exit(f'stopped at {calls} calls, before {name} page {page}: nothing written')
            reply = json.loads(subprocess.run(['gh', 'api', f'repos/{repository}/actions/runs?head_sha={sha}&per_page=100&page={page}'],
                                              check=True, capture_output=True, text=True).stdout)
            calls += 1
            runs += reply['workflow_runs']
            if len(runs) >= reply['total_count'] or not reply['workflow_runs']:
                break
            page += 1
        records[name] = {'repository': repository, 'sha': sha, 'total_count': reply['total_count'],
                         'runs': sorted((kept(run) for run in runs), key=lambda run: run['id'])}
    OUT.write_text(json.dumps({'captured': datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%MZ'),
                               'calls': calls, 'records': records}, indent=1, sort_keys=True) + '\n')
    print(f'{OUT.name}: {len(records)} repositories, {sum(len(r["runs"]) for r in records.values())} runs, {calls} calls')


if __name__ == '__main__':
    main()
