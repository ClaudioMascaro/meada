#!/usr/bin/env python3
"""Run the accepted smoke driver slices against independently identified binaries."""
from argparse import Namespace
import json
from pathlib import Path
import sys

pr, evidence, binary_dir, scripts = sys.argv[1:]
evidence, binary_dir = Path(evidence), Path(binary_dir)
sys.path.insert(0, scripts)
from smoke_stack import Smoke, compare_export, upgrade
from smoke_lifecycle import linked_decisions, manifest_status


def read(path):
    return json.loads(path.read_text())


def fixture(rig, pr):
    if pr == '941':
        for variant in ('linked', 'dangling', 'malformed', 'waiting-peer'):
            rig.phase(f'symlink-{variant}', linked_decisions, variant)
    else:
        rig.phase('manifest-status', manifest_status)


def run(label, phase, *, head, old=None, middle=None):
    rig = Smoke(Namespace(run=evidence / label, slice=phase, base_bin=None,
                          head_bin=binary_dir / head, old_bin=old, middle_bin=middle, source_dir=None))
    if pr == '943':
        rig.phase('historical-upgrade', upgrade, 'middle')
        rig.phase('current-upgrade', upgrade, 'head')
    else:
        fixture(rig, pr)
    status = rig.finish()
    counts = read(rig.run / 'result.json')['counts']
    print(f'{label}: {status}: {counts}', flush=True)
    return rig, status, counts


def manifest_unevaluated(rig):
    p = rig.run / 'commands/status/broken-lib.out'
    import smoke_stack
    result = p.read_text()
    print(result)
    assert f'decisions and other workspace lines not evaluated: {smoke_stack.LIB}: no such table: symbols' in result


def full_upgrade(rig):
    commands = rig.run / 'commands/head'
    raw = {n: read(commands / (n + '.out')) for n in ('cold-export', 'first-export',
                                                    'refresh-export', 'second-refresh-export')}
    rows = {}
    for name, content in raw.items():
        rows[name] = {k: content.get(k) for k in ('commit', 'dirty')}
        assert content == raw['cold-export'], (name, sorted(k for k, v in content.items()
                                                           if raw['cold-export'].get(k) != v))
    print(f'Full JSON cold/first/reindex/reopen parity, metadata {rows}', flush=True)


if pr == '943':
    head, status, counts = run('upgrade', 'upgrade', head='candidate',
                               old=binary_dir / 'source', middle=binary_dir / 'historical')
    assert (status, counts['pass'], counts['fail']) == (0, 12, 0)
    full_upgrade(head)
else:
    old, status, counts = run('historical', 'lifecycle', head='historical')
    assert (status, counts['pass'], counts['fail']) == ((3, 49, 4) if pr == '941' else (3, 4, 1))
    head, status, counts = run('candidate', 'lifecycle', head='candidate')
    assert (status, counts['pass'], counts['fail']) == (0, 53 if pr == '941' else 5, 0)
    if pr == '944':
        manifest_unevaluated(head)
