#!/usr/bin/env python3
"""#775, slice 1: a Java builder held in a `private final` field, and the reflection guard, through both
binaries. The probe `aws-java-private-fields/` holds three repositories (README.md: the cases and their
key); `oracle/java-run.txt` is what each case sends when compiled and run on a JDK with a stub SDK.

The key, per DynamoDB side (`Class.method`): the contract the base binary gives and the one the head
gives. A case the change does not touch keeps the base's answer; where they differ, the head's is the
one the Java run allows (a name only where every run sends it, else none). A base at or after 1.5.0
holds #775 (#885), so its column is the head's. A script pass is evidence, not a PROVED verdict.
"""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import sys

from prove import git

HERE = Path(__file__).resolve().parent
PROBE = HERE / 'aws-java-private-fields'
NONE = 'resource::dynamodb'  # the side with no table: unresolved
KEY = {  # (case, side) -> {binary: contract}
    ('a', 'Fields.keptField'): {'base': NONE + '::kept-private', 'head': NONE + '::kept-private'},
    ('a', 'Fields.ownField'): {'base': NONE + '::own-private', 'head': NONE + '::own-private'},
    ('b', 'Fields.keptField'): {'base': NONE, 'head': NONE},
    ('b', 'Fields.ownField'): {'base': NONE + '::own-private', 'head': NONE + '::own-private'},
    ('c', 'Held.heldField'): {'base': NONE, 'head': NONE},
}
CASES = sorted({case for case, _ in KEY})


def selftest():
    sent = (PROBE / 'oracle/java-run.txt').read_text()
    for case in CASES:
        assert (PROBE / case).is_dir() and (PROBE / 'oracle' / case / 'Main.java').is_file(), case
        assert f'case {case}:' in sent, case
    # Each name the head gives is one the Java run sends, and the run of each case the head leaves
    # unnamed sends two tables for it, or one the source never names as the field's own.
    assert 'putItem kept-private' in sent and 'putItem own-private' in sent and 'putItem kept-new' in sent
    assert 'putItem held-old' in sent and 'putItem held-new' in sent
    assert all(want['head'] == NONE or want['head'].removeprefix(NONE + '::') in sent for want in KEY.values())
    print(f'aws-java-private-fields self-test: {len(KEY)} sides in {len(CASES)} cases')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--selftest', action='store_true')
    parser.add_argument('--head-bin')
    parser.add_argument('--base-bin')
    parser.add_argument('--run')
    args = parser.parse_args()
    if args.selftest:
        selftest()
        return 0
    if not args.head_bin or not args.base_bin or not args.run:
        parser.error('--head-bin, --base-bin and --run are required: the key holds both binaries')
    out = Path(args.run).resolve()
    out.mkdir(parents=True, exist_ok=False)
    shutil.copy(PROBE / 'README.md', out / 'README.md')
    shutil.copy(PROBE / 'oracle/java-run.txt', out / 'java-run.txt')
    for case in CASES:  # one repository per case, committed once; each binary reads its own copy
        source = out / 'input' / case
        shutil.copytree(PROBE / case, source)
        git('-c', 'init.defaultBranch=main', 'init', '-q', cwd=source)
        git('remote', 'add', 'origin', f'https://github.com/zz-prove/aws-java-private-fields-{case}.git', cwd=source)
        git('add', '-A', cwd=source)
        git('commit', '-q', '-m', 'probe', cwd=source)
    checks = []
    for binary, path in (('base', args.base_bin), ('head', args.head_bin)):
        for case in CASES:
            work = out / binary / case
            shutil.copytree(out / 'input' / case, work, symlinks=True)
            result = subprocess.run([str(Path(path).resolve()), 'export'], cwd=work, capture_output=True, text=True, timeout=300)
            (out / binary / f'{case}.export.json').write_text(result.stdout)
            (out / binary / f'{case}.export.err').write_text(result.stderr)
            result.check_returncode()
            providers = json.loads(result.stdout).get('providers', [])
            for (key_case, side), want in KEY.items():
                if key_case != case:
                    continue
                found = [row for row in providers if f'::{side}()::' in row['site'] and row['contract'].startswith(NONE)]
                checks.append({'name': f'{binary} {case} {side}: {want[binary]}', 'passed': [row['contract'] for row in found] == [want[binary]],
                               'evidence': found})
    (out / 'checks.json').write_text(json.dumps(checks, indent=2) + '\n')
    failed = [check['name'] for check in checks if not check['passed']]
    (out / 'receipt.md').write_text('# #775 slice 1: private final field builders and the reflection guard\n\n'
                                    'Answer key: `README.md` and the Java run in `java-run.txt`.\n\n'
                                    + ('Checks passed; independent verdict pending.\n' if not failed else 'BROKEN checks:\n' + ''.join(f'- {name}\n' for name in failed)))
    for check in checks:
        print(('PASS ' if check['passed'] else 'FAIL ') + check['name'])
    return int(bool(failed))


if __name__ == '__main__':
    sys.exit(main())
