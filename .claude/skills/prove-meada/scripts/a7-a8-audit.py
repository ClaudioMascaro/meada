#!/usr/bin/env python3
"""Bounded #676/#684 CI phases. Script results are never an independent PROVED verdict.

See references/a7-a8-audit.md. No command plants more than one mutation. The candidate
phase requires a separately judged clean baseline, restores before publishing its binary,
and keeps mutation details in an auditor packet separate from the checker packet.
"""
import argparse
from contextlib import contextmanager
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import shutil
import subprocess
import sys
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
SKILL = HERE.parent
PLAN = HERE / 'a7-a8-mutations.json'
ALL = ('values', 'iac', 'terraform-json', 'ssm-values', 'provisions', 'environment',
       'http-services', 'alerts', 'incidents', 'incident-guards', 'aws-workloads',
       'gcp', 'azure', 'a8', 'a8-queries')


def digest(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def save(path, value):
    Path(path).parent.mkdir(parents=True, exist_ok=True)
    Path(path).write_text(json.dumps(value, indent=2, sort_keys=True) + '\n')


def git(*args):
    return subprocess.check_output(['git', '-C', str(ROOT), *args], text=True).strip()


def keys():
    return {str(p.relative_to(ROOT)): digest(p) for p in sorted(SKILL.rglob('*'))
            if p.is_file() and '__pycache__' not in p.parts and p.suffix != '.pyc'}


def status():
    return subprocess.check_output(['git', '-C', str(ROOT), 'status', '--porcelain=v1'], text=True)


def tracked():
    files = subprocess.check_output(['git', '-C', str(ROOT), 'ls-files', '-z']).split(b'\0')
    return {os.fsdecode(p): digest(ROOT / os.fsdecode(p)) for p in files
            if p and (ROOT / os.fsdecode(p)).is_file()}


def record_command(command, folder, name, cwd=ROOT, env=None, timeout=1200):
    folder = Path(folder)
    folder.mkdir(parents=True, exist_ok=True)
    start = time.monotonic()
    save(folder / (name + '.command.json'), [str(x) for x in command])
    timed_out = False
    with (folder / (name + '.stdout')).open('wb') as out, (folder / (name + '.stderr')).open('wb') as err:
        process = subprocess.Popen(command, cwd=cwd, env=env, stdout=out, stderr=err, start_new_session=True)
        try:
            code = process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            timed_out, code = True, 124
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=10)
    save(folder / (name + '.result.json'), {'exit': code, 'seconds': round(time.monotonic() - start, 3),
                                           'process_group': process.pid, 'timed_out': timed_out})
    return code


@contextmanager
def compiler_lock(run):
    # The managed host has one common compiler lock; an isolated public runner needs only
    # its own. Waiting is bounded in CI by the job timeout, with no active compiler outside it.
    path = Path(os.environ.get('PROVE_COMPILER_LOCK', '/workspace/meada-cargo.lock'
                if Path('/workspace/meada-cargo.lock').exists() else str(run / 'compiler.lock')))
    with path.open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        if shutil.disk_usage(ROOT).free < 6 * 1024 ** 3:
            raise RuntimeError('BLOCKED: fewer than 6 GiB free before compilation')
        yield


def build(source, output, logs):
    source, output, logs = Path(source), Path(output), Path(logs)
    output.parent.mkdir(parents=True, exist_ok=True)
    logs.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, CARGO_INCREMENTAL='0', CARGO_PROFILE_DEV_DEBUG='0')
    env.setdefault('CARGO_BUILD_JOBS', '2')
    target = Path(env.get('CARGO_TARGET_DIR', str(ROOT / 'target'))).resolve()
    with compiler_lock(logs):
        # Same-version worktrees share package fingerprints. A frozen binary is copied
        # before releasing the lock, and each package root is touched inside that lock.
        for path in ('src/lib.rs', 'src/main.rs', 'meada-core/src/lib.rs'):
            (source / path).touch()
        code = record_command(['cargo', 'build', '--locked', '--bin', 'meada', '--target-dir', str(target)],
                              logs, 'build', cwd=source, env=env, timeout=2700)
        if code:
            raise RuntimeError(f'BLOCKED: cargo build exited {code}; see {logs}')
        shutil.copy2(target / 'debug/meada', output)
    build_commit = subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip()
    save(output.with_suffix('.build.json'), {'source': str(source), 'commit': build_commit,
                                            'sha256': digest(output), 'features': 'default'})


def commands(head, existing, base, run):
    py = sys.executable
    def driver(name, *extra):
        return [py, str(HERE / (name + '.py')), '--head-bin', str(head), '--run', str(run / name), *extra]
    return {
        'values': [py, str(HERE / 'prove.py'), 'values', '--head-bin', str(head), '--base-bin', str(existing),
                   '--repos', 'a7-probe,a7-noconfig', '--sample', '2', '--http', '--run', str(run / 'values')],
        'iac': [py, str(HERE / 'prove.py'), 'iac', '--head-bin', str(head), '--base-bin', str(existing),
                '--repos', 'a7-iac,a7-names,a7-cfn', '--sample', '2', '--http', '--run', str(run / 'iac')],
        'terraform-json': driver('terraform-json', '--http'),
        'ssm-values': driver('aws', '--json-values'),
        'provisions': driver('provisions'),
        'environment': driver('environment'),
        'http-services': driver('http-iac', '--base-bin', str(existing)),
        'alerts': driver('alerts', '--http'),
        'incidents': driver('incident-routing', '--base-bin', str(existing), '--http'),
        'incident-guards': driver('incident-guards'),
        'aws-workloads': driver('aws-workloads'),
        'gcp': driver('gcp'),
        'azure': driver('azure', '--http'),
        'a8': ['sh', str(HERE / 'a8-peers.sh'), 'check', str(base), str(head), str(run / 'a8')],
        'a8-queries': driver('a8-mcp', '--base-bin', str(base)),
    }


def check_flows(head, existing, base, run, flows):
    planned, results = commands(head, existing, base, run), []
    for flow in flows:
        print('Driving ' + flow, flush=True)
        code = record_command(planned[flow], run / 'logs', flow)
        results.append({'flow': flow, 'exit': code, 'raw': 'logs/' + flow})
        save(run / 'results.json', results)
    result = 'BLOCKED' if any(r['exit'] in (2, 124) for r in results) else \
        'BROKEN' if any(r['exit'] for r in results) else 'CHECKS_PASSED'
    receipt = {'script_result': result, 'checker': 'script only, not blind or independent',
               'scope': list(flows), 'head_binary_sha256': digest(head), 'checks': results}
    save(run / 'checks.json', receipt)
    return receipt


def read_manifest(packet):
    packet = Path(packet).resolve()
    manifest = json.loads((packet / 'baseline.json').read_text())
    if manifest['source_commit'] != git('rev-parse', 'HEAD'):
        raise RuntimeError('BLOCKED: source changed; rerun the clean baseline on this exact head')
    if manifest['verifier_keys'] != keys():
        raise RuntimeError('BLOCKED: frozen verifier/answer keys changed; rerun the baseline')
    for name, expected in manifest['binaries'].items():
        if digest(packet / 'bin' / name) != expected['sha256']:
            raise RuntimeError('BLOCKED: baseline binary pin mismatch: ' + name)
    if manifest['checks']['script_result'] != 'CHECKS_PASSED' or manifest['checks']['scope'] != list(ALL):
        raise RuntimeError('BLOCKED: baseline checks are incomplete or failing')
    return packet, manifest


def baseline(args):
    run = Path(args.run).resolve()
    run.mkdir(parents=True, exist_ok=False)
    if status():
        raise RuntimeError('BLOCKED: baseline requires an untouched tracked/untracked checkout')
    head = git('rev-parse', 'HEAD')
    base = git('rev-parse', '--verify', args.base_ref + '^{commit}')
    baseline_bin, base_bin = run / 'bin/meada-clean', run / 'bin/meada-base'
    build(ROOT, baseline_bin, run / 'head-build')
    worktree = run / 'source-base'
    subprocess.run(['git', '-C', str(ROOT), 'worktree', 'add', '--detach', str(worktree), base], check=True)
    try:
        build(worktree, base_bin, run / 'base-build')
    finally:
        subprocess.run(['git', '-C', str(ROOT), 'worktree', 'remove', '--force', str(worktree)], check=True)
    checks = check_flows(baseline_bin, baseline_bin, base_bin, run / 'checks', ALL)
    manifest = {'source_commit': head, 'source_tree': git('rev-parse', 'HEAD^{tree}'), 'base_commit': base,
                'verifier_keys': keys(), 'binaries': {
                    'meada-clean': {'sha256': digest(baseline_bin), 'built_commit': head},
                    'meada-base': {'sha256': digest(base_bin), 'built_commit': base}},
                'checks': checks,
                'existing_answer_key': 'immutable clean A7 binary; A8 unchanged fields additionally use merge-base',
                'not_covered': ['remaining scoped child flows, UI and whole-app proof require their own receipts',
                                'GitHub CI full suites/UI/docs/deployment remain separate from this audit',
                                'a script pass is not a PROVED baseline or a blind mutation verdict']}
    save(run / 'baseline.json', manifest)
    if status():
        raise RuntimeError('BLOCKED: baseline changed the checkout')
    print('Baseline manifest SHA256: ' + digest(run / 'baseline.json'))
    return 0 if checks['script_result'] == 'CHECKS_PASSED' else 1


def attestation(packet, raw):
    judged = json.loads(raw)
    if judged.get('verdict') != 'PROVED' or not judged.get('checker') or not judged.get('receipt') \
            or judged.get('baseline_manifest_sha256') != digest(packet / 'baseline.json'):
        raise RuntimeError('BLOCKED: need a real fresh-checker PROVED receipt for this exact clean baseline')
    return judged


def candidate(args):
    packet, manifest = read_manifest(args.baseline)
    judged = attestation(packet, os.environ.get('PROVE_BASELINE_ATTESTATION', ''))
    case = next(c for c in json.loads(PLAN.read_text()) if c['id'] == args.mutation)
    run = Path(args.run).resolve()
    run.mkdir(parents=True, exist_ok=False)
    before_status, before_hashes = status(), tracked()
    if before_status:
        raise RuntimeError('BLOCKED: candidate requires a clean checkout')
    product = ROOT / case['file']
    backup = run / 'auditor/originals' / case['id'] / case['file']
    backup.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(product, backup)
    source = product.read_text()
    if source.count(case['before']) != 1:
        raise RuntimeError('BLOCKED: mutation anchor drifted; do not guess or plant another bug')
    restore, failure = {}, None
    try:
        product.write_text(source.replace(case['before'], case['after'], 1))
        save(run / 'auditor/planted.json', {'case': case, 'baseline': judged,
             'source_sha256_before': digest(backup), 'source_sha256_mutated': digest(product)})
        patch = subprocess.check_output(['git', '-C', str(ROOT), 'diff', '--', case['file']])
        (run / 'auditor/mutation.patch').write_bytes(patch)
        build(ROOT, run / 'checker/meada-candidate', run / 'auditor/build')
    except BaseException as error:
        failure = error
    finally:
        shutil.copy2(backup, product)
        compared = subprocess.run(['cmp', str(backup), str(product)], capture_output=True)
        restore = {'cmp_exit': compared.returncode, 'status_before': before_status, 'status_after': status(),
                   'tracked_hashes_equal': tracked() == before_hashes, 'restored_sha256': digest(product)}
        save(run / 'auditor/restoration.json', restore)
        if compared.returncode or restore['status_after'] != before_status or not restore['tracked_hashes_equal']:
            raise RuntimeError('STOP: exact restore failed; retained byte backup is authoritative')
    if failure:
        raise failure
    # This packet contains no mutation name, source file, patch or expected symptom. A
    # fresh checker must still avoid auditor artifacts, workflow inputs and this plan.
    save(run / 'checker/candidate.json', {'source_commit': manifest['source_commit'],
         'baseline_manifest_sha256': digest(packet / 'baseline.json'),
         'binary_sha256': digest(run / 'checker/meada-candidate'), 'restored_before_publication': True})
    save(run / 'auditor/round.json', {'mutation': case, 'restoration': restore,
                                     'candidate_sha256': digest(run / 'checker/meada-candidate')})
    print('Candidate built and source restored byte for byte. No checking verdict yet.')
    return 0


def check(args):
    packet, manifest = read_manifest(args.baseline)
    run = Path(args.run).resolve()
    run.mkdir(parents=True, exist_ok=False)
    head = packet / 'bin/meada-clean'
    if args.candidate:
        candidate_dir = Path(args.candidate).resolve()
        info = json.loads((candidate_dir / 'candidate.json').read_text())
        head = candidate_dir / 'meada-candidate'
        if info['baseline_manifest_sha256'] != digest(packet / 'baseline.json') \
                or info['source_commit'] != manifest['source_commit'] or info['binary_sha256'] != digest(head):
            raise RuntimeError('BLOCKED: candidate does not belong to this frozen baseline')
    receipt = check_flows(head, packet / 'bin/meada-clean', packet / 'bin/meada-base', run, ALL)
    receipt['baseline_manifest_sha256'] = digest(packet / 'baseline.json')
    receipt['phase'] = 'candidate check' if args.candidate else 'final clean rerun'
    save(run / 'checks.json', receipt)
    return 0 if receipt['script_result'] == 'CHECKS_PASSED' else 1


def score(args):
    """Adjudicate one round. Exit alone is never a catch; require named symptom evidence."""
    auditor = Path(args.auditor).resolve()
    round_ = json.loads((auditor / 'round.json').read_text())
    checks_dir = Path(args.checks).resolve()
    checks = json.loads((checks_dir / 'checks.json').read_text())
    case, restoration = round_['mutation'], round_['restoration']
    if round_['candidate_sha256'] != checks['head_binary_sha256'] or restoration['cmp_exit'] \
            or not restoration['tracked_hashes_equal'] or restoration['status_before'] != restoration['status_after']:
        raise RuntimeError('BLOCKED: mismatched checked binary or failed restore')
    failed = next(r for r in checks['checks'] if r['flow'] == case['check'])
    judged = json.loads(Path(args.judgment).read_text())
    evidence = (checks_dir / judged.get('evidence', '')).resolve()
    if not evidence.is_relative_to(checks_dir) or not evidence.is_file():
        raise RuntimeError('BLOCKED: judgment must name retained checker evidence')
    evidence_text = evidence.read_text(errors='replace')
    symptom = judged.get('symptom', '')
    names_symptom = bool(symptom and symptom in evidence_text and re.search(case['evidence'], evidence_text, re.I))
    script_caught = failed['exit'] == 1 and names_symptom and judged.get('script_names_symptom') is True
    checker_caught = bool(judged.get('verdict') == 'BROKEN' and names_symptom and judged.get('checker'))
    fresh_blind = bool(judged.get('fresh_blind_checker') is True and judged.get('checker') and judged.get('receipt'))
    result = 'caught' if script_caught or checker_caught else \
        'caught by luck' if checks['script_result'] != 'CHECKS_PASSED' else 'missed'
    save(args.output, {'mutation': case['id'], 'result': result, 'script_caught': script_caught,
         'baseline_manifest_sha256': checks['baseline_manifest_sha256'],
         'fresh_blind_checker': fresh_blind,
         'checker_caught': checker_caught, 'symptom': symptom, 'evidence': str(evidence),
         'restoration': restoration, 'judgment': judged,
         'limit': 'One observed plausible bug, not general completeness; no score for missing rounds.'})
    return 0


def report(args):
    packet, _ = read_manifest(args.baseline)
    baseline_hash = digest(packet / 'baseline.json')
    history = [json.loads(Path(path).read_text()) for path in args.rounds]
    latest = {item['mutation']: item for item in history}
    planned = [case['id'] for case in json.loads(PLAN.read_text())]
    current = [latest[name] for name in planned if name in latest]
    for item in current:
        if item['baseline_manifest_sha256'] != baseline_hash:
            raise RuntimeError('BLOCKED: latest round belongs to another source/key baseline')
    final = json.loads(Path(args.final).read_text())
    judged = json.loads(Path(args.final_judgment).read_text())
    final_clean = final['phase'] == 'final clean rerun' and final['script_result'] == 'CHECKS_PASSED' \
        and final['baseline_manifest_sha256'] == baseline_hash \
        and final['head_binary_sha256'] == json.loads((packet / 'baseline.json').read_text())['binaries']['meada-clean']['sha256']
    independent_final = judged.get('verdict') == 'PROVED' and judged.get('receipt') and judged.get('checker') \
        and judged.get('baseline_manifest_sha256') == baseline_hash
    missing = [name for name in planned if name not in latest]
    caught = sum(item['result'] == 'caught' for item in current)
    scripts = sum(item['script_caught'] for item in current)
    blind = sum(item['fresh_blind_checker'] for item in current)
    complete = not missing and caught == len(planned) and blind == len(planned) and final_clean and independent_final
    result = {'audit_result': 'COMPLETE' if complete else 'INCOMPLETE', 'baseline_manifest_sha256': baseline_hash,
              'planned': len(planned), 'caught': caught, 'script_caught': scripts, 'fresh_blind_rounds': blind,
              'missing': missing, 'final_clean_checks': bool(final_clean), 'final_fresh_verdict': bool(independent_final),
              'history': history, 'latest': current, 'final_judgment': judged,
              'limit': 'Bounded observed bugs, not a whole-app proof or a general completeness claim.'}
    save(args.output, result)
    lines = ['# A7/A8 bounded audit', '', f"Result: {result['audit_result']}; {caught}/{len(planned)} caught, {scripts} by scripts, {blind} fresh blind rounds.",
             f'Final clean checks: {bool(final_clean)}; final independent PROVED: {bool(independent_final)}.', '',
             '| Mutation | Result | Script caught | Fresh blind | Symptom |', '|---|---|---|---|---|']
    lines += [f"| {item['mutation']} | {item['result']} | {item['script_caught']} | {item['fresh_blind_checker']} | {item['symptom'].replace('|', '/')} |"
              for item in history]
    lines += ['', 'Missing: ' + (', '.join(missing) or 'none'), '', result['limit']]
    Path(args.output).with_suffix('.md').write_text('\n'.join(lines) + '\n')
    return 0 if complete else 1


def validate_plan():
    cases = json.loads(PLAN.read_text())
    assert len(cases) == 13 and len({c['id'] for c in cases}) == 13
    for case in cases:
        assert (ROOT / case['file']).read_text().count(case['before']) == 1, case['id']
        assert case['before'] != case['after'] and case['check'] in ALL
    print('13 unique, single-anchor source mutations validated; no mutation planted.')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='phase', required=True)
    p = sub.add_parser('baseline')
    p.add_argument('--run', required=True)
    p.add_argument('--base-ref', required=True)
    p = sub.add_parser('candidate')
    p.add_argument('--baseline', required=True)
    p.add_argument('--run', required=True)
    p.add_argument('--mutation', required=True, choices=[c['id'] for c in json.loads(PLAN.read_text())])
    p = sub.add_parser('check')
    p.add_argument('--baseline', required=True)
    p.add_argument('--candidate')
    p.add_argument('--run', required=True)
    p = sub.add_parser('score')
    for option in ('auditor', 'checks', 'judgment', 'output'):
        p.add_argument('--' + option, required=True)
    p = sub.add_parser('report')
    for option in ('baseline', 'final', 'final-judgment', 'output'):
        p.add_argument('--' + option, required=True)
    p.add_argument('--rounds', nargs='*', required=True, help='chronological score files, retaining misses before repairs')
    sub.add_parser('validate-plan')
    args = parser.parse_args()
    try:
        return validate_plan() if args.phase == 'validate-plan' else globals()[args.phase](args)
    except (OSError, ValueError, RuntimeError, StopIteration, AssertionError, KeyError) as error:
        print('BLOCKED: ' + str(error), file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
