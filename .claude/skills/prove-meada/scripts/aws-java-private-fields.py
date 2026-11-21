#!/usr/bin/env python3
"""Check both PR binaries against the Java oracle and optionally replay the #775 transition."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

from prove import git
from proof_result import Comparisons, run_driver

HERE = Path(__file__).resolve().parent
PROBE = HERE / 'aws-java-private-fields'
NONE = 'resource::dynamodb'  # the side with no table: unresolved
PRE_PRIVATE_FIELDS = 'b73e1e5318508d69a8e98e1081e2e9fb3c61076e'  # parent of e78c38c in #885
KEY = {  # (case, side) -> (historical, Java oracle)
    ('a', 'Fields.keptField'): (NONE, NONE + '::kept-private'),
    ('a', 'Fields.ownField'): (NONE, NONE + '::own-private'),
    ('b', 'Fields.keptField'): (NONE, NONE),
    ('b', 'Fields.ownField'): (NONE + '::own-private', NONE + '::own-private'),
    ('c', 'Held.heldField'): (NONE + '::held-old', NONE),
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
    assert all(expected == NONE or expected.removeprefix(NONE + '::') in sent for _, expected in KEY.values())
    for revision in (PRE_PRIVATE_FIELDS, 'ac905be259fd4abd77eee8788a9be62711f49a60'):
        for case, side in KEY:
            old, java = KEY[case, side]
            rows = [{'site': f'input::{side}()::putItem', 'contract': old if revision == PRE_PRIVATE_FIELDS else java}]
            assert contracts(rows, side) == [expected_contract(case, side, revision)]
            assert contracts(rows, side) == [java] or old != java and revision == PRE_PRIVATE_FIELDS
    wrong = [{'site': 'input::Fields.keptField()::putItem', 'contract': NONE + '::own-private'}]
    assert contracts(wrong, 'Fields.keptField') != [expected_contract('a', 'Fields.keptField', None)]
    print(f'aws-java-private-fields self-test: {len(KEY)} sides in {len(CASES)} cases')


def expected_contract(case, side, revision):
    historical, java = KEY[case, side]
    return historical if revision == PRE_PRIVATE_FIELDS else java


def contracts(providers, side):
    return [row['contract'] for row in providers
            if f'::{side}()::' in row['site'] and row['contract'].split('::')[:2] == ['resource', 'dynamodb']]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--selftest', action='store_true')
    parser.add_argument('--head-bin')
    parser.add_argument('--base-bin')
    parser.add_argument('--base-revision')
    parser.add_argument('--head-revision')
    parser.add_argument('--historical-bin', help=f'Binary built from {PRE_PRIVATE_FIELDS}')
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
    checks, observations, binaries = [], {}, {}
    comparisons = Comparisons(out)
    inputs = [('base', args.base_bin, args.base_revision), ('head', args.head_bin, args.head_revision)]
    if args.historical_bin:
        inputs.append(('historical', args.historical_bin, PRE_PRIVATE_FIELDS))
    for binary, path, revision in inputs:
        with Path(path).open('rb') as stream:
            binaries[binary] = {'revision': revision, 'sha256': hashlib.file_digest(stream, 'sha256').hexdigest()}
        observations[binary] = {}
        for case in CASES:
            work = out / binary / case
            shutil.copytree(out / 'input' / case, work, symlinks=True)
            result = subprocess.run([str(Path(path).resolve()), 'export'], cwd=work, capture_output=True, text=True, timeout=300)
            (out / binary / f'{case}.export.json').write_text(result.stdout)
            (out / binary / f'{case}.export.err').write_text(result.stderr)
            result.check_returncode()
            providers = json.loads(result.stdout).get('providers', [])
            for key_case, side in KEY:
                if key_case != case:
                    continue
                want = [expected_contract(case, side, revision)]
                found = contracts(providers, side)
                observations[binary][f'{case} {side}'] = found
                name = f'{binary} {case} {side}'
                checks.append({'name': name, 'answer_key': revision if revision == PRE_PRIVATE_FIELDS else 'java-run.txt',
                               'expected': want, 'actual': found, 'passed': comparisons.compare(name, want, found, oracle=True)})
    comparisons.compare('PR base vs head decisions by case and site', observations['base'], observations['head'])
    (out / 'binaries.json').write_text(json.dumps(binaries, indent=2) + '\n')
    (out / 'checks.json').write_text(json.dumps(checks, indent=2) + '\n')
    failed = [check['name'] for check in checks if not check['passed']]
    (out / 'receipt.md').write_text('# #775 slice 1: private final field builders and the reflection guard\n\n'
                                    'Answer key: `README.md` and the Java run in `java-run.txt`.\n\n'
                                    + ('Checks passed; independent verdict pending.\n' if not failed else 'BROKEN checks:\n' + ''.join(f'- {name}\n' for name in failed)))
    for check in checks:
        print(('PASS ' if check['passed'] else 'FAIL ') + check['name'])
    return comparisons.exit_code()


if __name__ == '__main__':
    run_driver(main)
