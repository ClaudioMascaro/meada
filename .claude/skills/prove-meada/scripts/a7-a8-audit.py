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
ALL = ('values', 'iac', 'terraform-json', 'ssm-values', 'aws-provisioners', 'provisions', 'environment',
       'http-services', 'alerts', 'incidents', 'incident-guards', 'aws-workloads',
       'gcp', 'azure', 'a8', 'a8-queries', 'iac-http-routes', 'gcp-sdk', 'azure-sdk', 'custom-iac',
       'aws-sdk-jvm-go-dotnet', 'resource-via', 'orphans', 'patterns', 'images-mixed', 'image-ko')
UI_FLOWS = ('aws-sdk-js-python', 'config-uses', 'images')
SEPARATE_SCOPE = ('corpus-api-viewer', 'public-infra', 'native-hcl', 'serverless', 'generic-iac', 'supplementary-sdk')
SOURCE_CAPTURES = ('pinned-monitoring', 'adjacent-monitoring', 'native-hcl-values', 'generic-boutique',
                   'generic-kube-prometheus', 'generic-crossplane-aws', 'generic-kube-controls',
                   'generic-compose-controls', 'serverless-nodes')
# Historical keys required by the existing drivers, not the final clean binary.
PRE_CONFIG = '6a5eca5853193d205bcf756983b41e9cb16ffa9d'  # before #661, after cloud prerequisites
PRE_IMAGE = 'eae2a567ed250d0ac4a8b8e6f57b62f7a3041c62'  # before #662
PRE_EXTENSIONS = 'e6d27d0d7c046eceb8771f17111f00511a8d6c3d'  # compatible A7 before #666/#668/#670/#671


def digest(path):
    if Path(path).is_symlink():
        return hashlib.sha256(os.fsencode(os.readlink(path))).hexdigest()
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def save(path, value):
    Path(path).parent.mkdir(parents=True, exist_ok=True)
    Path(path).write_text(json.dumps(value, indent=2, sort_keys=True) + '\n')


def git(*args):
    return subprocess.check_output(['git', '-C', str(ROOT), *args], text=True).strip()


def keys():
    return {str(p.relative_to(ROOT)): digest(p) for p in sorted(SKILL.rglob('*'))
            if (p.is_file() or p.is_symlink()) and '__pycache__' not in p.parts and p.suffix != '.pyc'}


def keyset_hash(keyset):
    return hashlib.sha256(json.dumps(keyset, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def status():
    return subprocess.check_output(['git', '-C', str(ROOT), 'status', '--porcelain=v1'], text=True)


def tracked():
    files = subprocess.check_output(['git', '-C', str(ROOT), 'ls-files', '-z']).split(b'\0')
    return {os.fsdecode(p): digest(ROOT / os.fsdecode(p)) for p in files
            if p and ((ROOT / os.fsdecode(p)).is_file() or (ROOT / os.fsdecode(p)).is_symlink())}


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


def commands(head, existing, base, run, extensions):
    py = sys.executable
    def driver(name, *extra, output=None):
        return [py, str(HERE / (name + '.py')), '--head-bin', str(head), '--run', str(run / (output or name)), *extra]
    return {
        'values': [py, str(HERE / 'prove.py'), 'values', '--head-bin', str(head), '--base-bin', str(existing),
                   '--repos', 'a7-probe,a7-noconfig', '--sample', '2', '--http', '--run', str(run / 'values')],
        'iac': [py, str(HERE / 'prove.py'), 'iac', '--head-bin', str(head), '--base-bin', str(existing),
                '--repos', 'a7-iac,a7-names,a7-cfn,terraform-sqs,sam-rust', '--sample', '2', '--http', '--run', str(run / 'iac')],
        'terraform-json': driver('terraform-json', '--http'),
        'ssm-values': driver('aws', '--json-values'),
        'aws-provisioners': driver('aws', '--base-bin', str(existing), output='aws-provisioners'),
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
        'iac-http-routes': driver('iac-http-routes', '--base-bin', str(extensions)),
        'gcp-sdk': driver('gcp-sdk', '--http'),
        'azure-sdk': driver('azure-sdk', '--http'),
        'custom-iac': driver('custom-iac', '--base-bin', str(extensions), '--http'),
        'aws-sdk-jvm-go-dotnet': driver('aws-sdk-jvm-go-dotnet', '--http'),
        'resource-via': driver('resource-via', '--http'),
        'orphans': driver('orphans', '--base-bin', str(existing), '--base-has-orphans', '--http'),
        'patterns': driver('patterns', '--http'),
        'images-mixed': driver('images-mixed', '--base-bin', str(existing)),
        'image-ko': driver('image-ko'),
    }


def check_flows(head, existing, base, run, flows, extensions):
    planned, results = commands(head, existing, base, run, extensions), []
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


def read_manifest(packet, require_checks=True):
    packet = Path(packet).resolve()
    manifest = json.loads((packet / 'baseline.json').read_text())
    if manifest['source_commit'] != git('rev-parse', 'HEAD'):
        raise RuntimeError('BLOCKED: source changed; rerun the clean baseline on this exact head')
    if manifest['verifier_keys'] != keys():
        raise RuntimeError('BLOCKED: frozen verifier/answer keys changed; rerun the baseline')
    for name, expected in manifest['binaries'].items():
        if digest(packet / 'bin' / name) != expected['sha256']:
            raise RuntimeError('BLOCKED: baseline binary pin mismatch: ' + name)
    if require_checks and (manifest['checks']['script_result'] != 'CHECKS_PASSED' or manifest['checks']['scope'] != list(ALL)):
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
    historical = {'meada-base': base, 'meada-pre-config': PRE_CONFIG,
                  'meada-pre-image': PRE_IMAGE, 'meada-pre-extensions': PRE_EXTENSIONS}
    for label, revision in historical.items():
        worktree = run / ('source-' + label)
        subprocess.run(['git', '-C', str(ROOT), 'worktree', 'add', '--detach', str(worktree), revision], check=True)
        try:
            build(worktree, run / 'bin' / label, run / (label + '-build'))
        finally:
            subprocess.run(['git', '-C', str(ROOT), 'worktree', 'remove', '--force', str(worktree)], check=True)
    checks = check_flows(baseline_bin, baseline_bin, base_bin, run / 'checks', ALL, run / 'bin/meada-pre-extensions')
    manifest = {'source_commit': head, 'source_tree': git('rev-parse', 'HEAD^{tree}'), 'base_commit': base,
                'verifier_keys': keys(), 'binaries': {
                    label: {'sha256': digest(run / 'bin' / label), 'built_commit': revision}
                    for label, revision in {'meada-clean': head, **historical}.items()},
                'checks': checks,
                'requires_ui_receipt': True,
                'required_separate_scope': list(SEPARATE_SCOPE),
                'existing_answer_key': 'immutable clean A7 binary; A8 unchanged fields additionally use merge-base',
                'not_covered': ['Separate UI/corpus/API/public-source and supplementary controls require bound receipts',
                                'GitHub CI full suites/UI/docs/deployment remain separate from this audit',
                                'a script pass is not a PROVED baseline or a blind mutation verdict']}
    save(run / 'baseline.json', manifest)
    if status():
        raise RuntimeError('BLOCKED: baseline changed the checkout')
    print('Baseline manifest SHA256: ' + digest(run / 'baseline.json'))
    return 0 if checks['script_result'] == 'CHECKS_PASSED' else 1


def attestation(packet, raw, ui_receipt=None, scope_receipt=None):
    judged = json.loads(raw)
    if judged.get('verdict') != 'PROVED' or not judged.get('checker') or not judged.get('receipt') \
            or judged.get('baseline_manifest_sha256') != digest(packet / 'baseline.json'):
        raise RuntimeError('BLOCKED: need a real fresh-checker PROVED receipt for this exact clean baseline')
    manifest = json.loads((packet / 'baseline.json').read_text())
    if manifest.get('requires_ui_receipt'):
        if not ui_receipt:
            raise RuntimeError('BLOCKED: same-source UI supplemental receipt is required, not a UI candidate binary')
        ui = json.loads(Path(ui_receipt).read_text())
        ui_packet = Path(ui_receipt).parent
        frontend = ui_packet / 'frontend'
        if ui.get('source_commit') != manifest['source_commit'] or ui.get('source_tree') != manifest['source_tree'] \
                or ui.get('baseline_manifest_sha256') != digest(packet / 'baseline.json') \
                or ui.get('verifier_keyset_sha256') != keyset_hash(manifest['verifier_keys']) \
                or ui.get('script_result') != 'CHECKS_PASSED' or ui.get('scope') != list(UI_FLOWS) or ui.get('console_graph') != 'success' \
                or ui.get('source_and_binary_unchanged') is not True \
                or digest(ui_packet / 'bin/meada-ui') != ui.get('ui_binary_sha256') \
                or {str(p.relative_to(frontend)): digest(p) for p in sorted(frontend.rglob('*')) if p.is_file()} != ui.get('frontend_sha256') \
                or sorted((item['flow'], item['exit']) for item in ui.get('checks', [])) != sorted((name, 0) for name in UI_FLOWS) \
                or judged.get('ui_receipt_sha256') != digest(ui_receipt):
            raise RuntimeError('BLOCKED: UI receipt is failing or not bound to this actual source/key judgment')
        if not scope_receipt:
            raise RuntimeError('BLOCKED: actual corpus/API/viewer receipt is required beside independent source judgments')
        captured = json.loads(Path(scope_receipt).read_text())
        from prove import corpus
        entries = list(corpus())
        expected = ['corpus', 'viewer-start', 'viewer-pages', 'viewer-stop', 'source-and-ui-binary-unchanged']
        expected += [name + '-' + phase for name in entries for phase in ('ui-stdio', 'start', 'api', 'stop')]
        # Source-confirmed changes from main are adjudicated, never silently reblessed.
        # Only that comparison may have exit 1; the fresh checker must judge every
        # delta and budget. API, feature parity, viewer and cleanup must pass directly.
        failures = [item for item in captured.get('checks', []) if item['exit']]
        if captured.get('source_commit') != manifest['source_commit'] or captured.get('source_tree') != manifest['source_tree'] \
                or captured.get('baseline_manifest_sha256') != digest(packet / 'baseline.json') \
                or captured.get('verifier_keyset_sha256') != keyset_hash(manifest['verifier_keys']) \
                or captured.get('ui_binary_sha256') != ui.get('ui_binary_sha256') \
                or captured.get('frontend_sha256') != ui.get('frontend_sha256') \
                or captured.get('script_result') not in ('CHECKS_PASSED', 'BROKEN') \
                or captured.get('capture_result') != 'CAPTURED' \
                or captured.get('entries') != entries \
                or sorted(item['flow'] for item in captured.get('checks', [])) != sorted(expected) \
                or sorted((item['flow'], item['exit']) for item in captured.get('source_captures', [])) != sorted((name, 0) for name in SOURCE_CAPTURES) \
                or any(item['flow'] != 'corpus' or item['exit'] != 1 for item in failures) \
                or judged.get('scope_receipt_sha256') != digest(scope_receipt):
            raise RuntimeError('BLOCKED: scope capture is incomplete/failing or not bound to this source/key/UI judgment')
        scope = judged.get('separate_scope_receipt', {})
        if not scope.get('receipt') or not re.fullmatch(r'[0-9a-f]{64}', str(scope.get('sha256', ''))) \
                or scope.get('verdict') != 'PROVED' or set(scope.get('covers', [])) != set(SEPARATE_SCOPE) \
                or scope.get('source_commit') != manifest['source_commit'] \
                or scope.get('verifier_keyset_sha256') != keyset_hash(manifest['verifier_keys']):
            raise RuntimeError('BLOCKED: fresh checker must bind actual separate corpus/API/source-scope evidence')
    return judged


def candidate(args):
    packet, manifest = read_manifest(args.baseline)
    judged = attestation(packet, os.environ.get('PROVE_BASELINE_ATTESTATION', ''),
                         getattr(args, 'ui_receipt', None), getattr(args, 'scope_receipt', None))
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
    receipt = check_flows(head, packet / 'bin/meada-clean', packet / 'bin/meada-base', run, ALL, packet / 'bin/meada-pre-extensions')
    receipt['baseline_manifest_sha256'] = digest(packet / 'baseline.json')
    receipt['phase'] = 'candidate check' if args.candidate else 'final clean rerun'
    save(run / 'checks.json', receipt)
    return 0 if receipt['script_result'] == 'CHECKS_PASSED' else 1


def ui_check(args):
    """Same-head clean UI supplement. Never claimed as coverage of a native-only mutant."""
    packet, manifest = read_manifest(args.baseline, require_checks=False)
    if status():
        raise RuntimeError('BLOCKED: UI supplement requires a clean checkout')
    run = Path(args.run).resolve()
    run.mkdir(parents=True, exist_ok=False)
    binary, ui = Path(args.head_bin).resolve(), Path(args.ui_dir).resolve()
    frozen = run / 'bin/meada-ui'
    frozen.parent.mkdir()
    shutil.copy2(binary, frozen)
    binary = frozen
    shutil.copytree(ui, run / 'frontend')
    ui = run / 'frontend'
    py = sys.executable
    planned = {
        'aws-sdk-js-python': [py, str(HERE / 'aws-sdk.py'), '--head-bin', str(binary),
            '--base-bin', str(packet / 'bin/meada-pre-extensions'), '--ui-bin', str(binary), '--ui-dir', str(ui),
            '--run', str(run / 'aws-sdk-js-python')],
        'config-uses': [py, str(HERE / 'config_uses.py'), '--head-bin', str(binary),
            '--base-bin', str(packet / 'bin/meada-pre-config'), '--http', '--ui-dir', str(ui), '--run', str(run / 'config-uses')],
        'images': [py, str(HERE / 'images.py'), '--head-bin', str(binary), '--base-bin', str(packet / 'bin/meada-pre-image'),
            '--http', '--public', '--ui-bin', str(binary), '--ui-dir', str(ui), '--run', str(run / 'images')],
    }
    receipt = {'source_commit': manifest['source_commit'], 'source_tree': manifest['source_tree'],
        'baseline_manifest_sha256': digest(packet / 'baseline.json'),
        'verifier_keyset_sha256': keyset_hash(manifest['verifier_keys']), 'scope': list(UI_FLOWS),
        'ui_binary_sha256': digest(binary), 'frontend_sha256': {str(p.relative_to(ui)): digest(p) for p in sorted(ui.rglob('*')) if p.is_file()},
        'console_graph': args.console_graph, 'script_result': 'BLOCKED',
        'checker': 'script only; clean UI supplement, zero mutant UI coverage', 'checks': []}
    save(run / 'receipt.json', receipt)
    for flow, command in planned.items():
        code = record_command(command, run / 'logs', flow)
        receipt['checks'].append({'flow': flow, 'exit': code})
        save(run / 'receipt.json', receipt)
    receipt['source_and_binary_unchanged'] = not status() and keys() == manifest['verifier_keys'] \
        and git('rev-parse', 'HEAD') == manifest['source_commit'] and digest(binary) == receipt['ui_binary_sha256'] \
        and all(digest(packet / 'bin' / name) == pin['sha256'] for name, pin in manifest['binaries'].items()) \
        and {str(p.relative_to(ui)): digest(p) for p in sorted(ui.rglob('*')) if p.is_file()} == receipt['frontend_sha256']
    failed = any(item['exit'] for item in receipt['checks']) or args.console_graph != 'success' or not receipt['source_and_binary_unchanged']
    blocked = any(item['exit'] in (2, 124) for item in receipt['checks']) or args.console_graph in ('', 'skipped', 'cancelled') \
        or not receipt['source_and_binary_unchanged']
    receipt['script_result'] = 'BLOCKED' if blocked else 'BROKEN' if failed else 'CHECKS_PASSED'
    save(run / 'receipt.json', receipt)
    return 1 if failed else 0


def scope_check(args):
    """Existing corpus/API/viewer mechanics; independent source supplements stay pending."""
    from prove import corpus, mcp_answers
    packet, manifest = read_manifest(args.baseline, require_checks=False)
    if status():
        raise RuntimeError('BLOCKED: scope capture requires a clean checkout')
    run = Path(args.run).resolve()
    run.mkdir(parents=True, exist_ok=False)
    binary, ui = Path(args.head_bin).resolve(), Path(args.ui_dir).resolve()
    os.environ['LANG'] = 'C.UTF-8'
    receipt = {'source_commit': manifest['source_commit'], 'source_tree': manifest['source_tree'],
        'baseline_manifest_sha256': digest(packet / 'baseline.json'),
        'verifier_keyset_sha256': keyset_hash(manifest['verifier_keys']), 'ui_binary_sha256': digest(binary),
        'frontend_sha256': {str(p.relative_to(ui)): digest(p) for p in sorted(ui.rglob('*')) if p.is_file()},
        'corpus_table_sha256': digest(HERE / 'corpus.tsv'), 'entries': list(corpus()),
        'script_result': 'BLOCKED', 'checker': 'script only; no independent source judgment', 'checks': [], 'source_captures': [],
        'pending_independent_scope': list(SEPARATE_SCOPE[1:]) + ['source/spec judgment of every corpus delta']}
    save(run / 'receipt.json', receipt)
    save(run / 'corpus-pins.json', {name: {'packs': entry[0], 'url': entry[1],
        'pin': manifest['source_commit'] if entry[1] == 'self' else entry[2]} for name, entry in corpus().items()})
    target = run / 'corpus'
    code = record_command([sys.executable, str(HERE / 'prove.py'), 'run', '--base-bin', str(packet / 'bin/meada-base'),
        '--head-bin', str(packet / 'bin/meada-clean'), '--repos', 'all', '--run', str(target), '--http'],
        run / 'logs', 'corpus', timeout=2700)
    receipt['checks'].append({'flow': 'corpus', 'exit': code})
    save(run / 'receipt.json', receipt)
    for name in corpus():  # exact loader membership, seven pinned repositories/eight packs
        record_path = target / 'head' / name / 'record.json'
        if not record_path.is_file():
            receipt['checks'].append({'flow': name + '-api', 'exit': 2, 'reason': 'native corpus record unavailable'})
            save(run / 'receipt.json', receipt)
            continue
        record = json.loads(record_path.read_text())
        if record.get('failed') or not record.get('answers'):
            receipt['checks'].append({'flow': name + '-api', 'exit': 2, 'reason': 'native corpus record incomplete'})
            save(run / 'receipt.json', receipt)
            continue
        out = run / 'ui-stdio' / name
        out.mkdir(parents=True)
        try:
            tools, texts, seconds = mcp_answers(binary, target / 'work/head' / name, record['queries'], out / 'mcp.err')
            save(out / 'record.json', {'tools': tools, 'answers': texts, 'queries': record['queries'],
                'seconds': seconds, 'native_record_sha256': digest(record_path), 'ui_binary_sha256': digest(binary)})
            equal = tools == record.get('tools') and texts == record.get('answers')
            code = 0 if equal else 1
        except Exception as error:
            save(out / 'blocked.json', {'error': repr(error)})
            code = 2
        receipt['checks'].append({'flow': name + '-ui-stdio', 'exit': code})
        try:
            for phase, extra in [('start', ['--bin', str(binary), '--ui-dir', str(ui), '--repos', name]), ('api', [])]:
                result = record_command([sys.executable, str(HERE / 'console.py'), phase, '--run', str(target), *extra],
                                        run / 'logs', name + '-' + phase)
                receipt['checks'].append({'flow': name + '-' + phase, 'exit': result})
                if result:
                    break
        finally:
            code = record_command([sys.executable, str(HERE / 'console.py'), 'stop', '--run', str(target)],
                                  run / 'logs', name + '-stop', timeout=60)
            receipt['checks'].append({'flow': name + '-stop', 'exit': code})
            if (target / 'console').exists():
                shutil.move(str(target / 'console'), str(target / ('console-' + name)))
        save(run / 'receipt.json', receipt)
    try:
        result = record_command([sys.executable, str(HERE / 'console.py'), 'start', '--bin', str(binary),
            '--ui-dir', str(ui), '--run', str(target)], run / 'logs', 'viewer-start')
        receipt['checks'].append({'flow': 'viewer-start', 'exit': result})
        if not result:
            result = record_command(['node', str(HERE / 'console-pages.mjs'), str(target)], run / 'logs', 'viewer-pages')
            receipt['checks'].append({'flow': 'viewer-pages', 'exit': result})
    finally:
        code = record_command([sys.executable, str(HERE / 'console.py'), 'stop', '--run', str(target)],
                              run / 'logs', 'viewer-stop', timeout=60)
        receipt['checks'].append({'flow': 'viewer-stop', 'exit': code})
        if (target / 'console').exists():
            shutil.move(str(target / 'console'), str(target / 'console-viewer'))
    source_keys = HERE / 'a7-source-keys'
    monitoring, adjacent, hcl, generic = [source_keys / name for name in
        ('pinned-monitoring-completeness', 'adjacent-monitoring', 'native-hcl-values', 'generic-iac')]
    captures = [
        ('pinned-monitoring', ['--entry', 'kube-prometheus'], monitoring / 'queries.jsonl', monitoring / 'input-sha256.json', None),
        ('adjacent-monitoring', ['--source', str(adjacent / 'repo'), '--name', 'adjacent-monitoring'],
            adjacent / 'queries.jsonl', adjacent / 'input-sha256.json', adjacent / 'phase-plan.json'),
        ('native-hcl-values', ['--source', str(hcl / 'repo'), '--name', 'native-hcl-values'],
            hcl / 'queries.jsonl', hcl / 'input-sha256.json', hcl / 'expected-values.json'),
    ]
    for scope in json.loads((generic / 'capture-plan.json').read_text())['scopes']:
        inputs = ['--entry', scope['entry']] if 'entry' in scope else ['--source', str(generic / scope['source']), '--name', scope['name']]
        source_key = generic / 'source-sha256.json' if 'entry' in scope else generic / scope['source_identity']
        phase_key = generic / scope['lifecycle'].split('#')[0] if scope['lifecycle'] else None
        captures.append((scope['name'], inputs, generic / scope['queries'], source_key, phase_key))
    if tuple(name for name, *_ in captures) != SOURCE_CAPTURES:
        raise RuntimeError('BLOCKED: frozen source capture scope changed')
    for name, inputs, queries, source_key, phase_key in captures:
        command = [sys.executable, str(HERE / 'a7-source-capture.py'), '--head-bin', str(packet / 'bin/meada-clean'),
            *inputs, '--queries', str(queries), '--source-key', str(source_key), '--run', str(run / 'source' / name)]
        if phase_key:
            command += ['--phase-key', str(phase_key)]
        code = record_command(command, run / 'logs', 'capture-' + name, timeout=2700)
        receipt['source_captures'].append({'flow': name, 'exit': code, 'semantic_judgment': 'PENDING'})
        save(run / 'receipt.json', receipt)
    unchanged = not status() and digest(binary) == receipt['ui_binary_sha256'] \
        and git('rev-parse', 'HEAD') == manifest['source_commit'] and keys() == manifest['verifier_keys'] \
        and all(digest(packet / 'bin' / name) == pin['sha256'] for name, pin in manifest['binaries'].items()) \
        and {str(p.relative_to(ui)): digest(p) for p in sorted(ui.rglob('*')) if p.is_file()} == receipt['frontend_sha256']
    receipt['checks'].append({'flow': 'source-and-ui-binary-unchanged', 'exit': 0 if unchanged else 2})
    all_checks = receipt['checks'] + receipt['source_captures']
    failed = any(item['exit'] for item in all_checks)
    blocked = any(item['exit'] in (2, 124) for item in all_checks)
    receipt['script_result'] = 'BLOCKED' if blocked else 'BROKEN' if failed else 'CHECKS_PASSED'
    capture_failed = any(item['exit'] and (item['flow'] != 'corpus' or item['exit'] != 1) for item in all_checks)
    receipt['capture_result'] = 'INCOMPLETE' if capture_failed else 'CAPTURED'
    save(run / 'receipt.json', receipt)
    # A completed capture is not a semantic pass over corpus differences. Their raw
    # exit 1 and BROKEN script result stay visible until the fresh source judgment.
    return 2 if blocked else 1 if capture_failed else 0


def blind_result(judgment, names_symptom):
    """A script detection cannot replace the fresh checker's actual verdict."""
    fresh = bool(judgment.get('fresh_blind_checker') is True and judgment.get('checker') and judgment.get('receipt'))
    if not fresh or judgment.get('verdict') not in ('PROVED', 'BROKEN'):
        return 'incomplete', fresh
    if judgment['verdict'] == 'PROVED':
        return 'missed', fresh
    return ('caught' if names_symptom else 'caught by luck'), fresh


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
    checker_caught = bool(judged.get('verdict') == 'BROKEN' and names_symptom and judged.get('checker') and judged.get('receipt'))
    script_detection = 'caught' if script_caught else \
        'caught by luck' if checks['script_result'] != 'CHECKS_PASSED' else 'missed'
    result, fresh_blind = blind_result(judged, names_symptom)
    save(args.output, {'schema': 2, 'mutation': case['id'], 'result': result, 'script_caught': script_caught,
         'script_detection': script_detection, 'names_symptom': names_symptom, 'blind_caught': result == 'caught',
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
        expected_result, fresh = blind_result(item['judgment'], item.get('names_symptom') is True)
        checker_caught = bool(item['judgment'].get('verdict') == 'BROKEN' and item.get('names_symptom') is True
                              and item['judgment'].get('checker') and item['judgment'].get('receipt'))
        if item.get('schema') != 2 or item['result'] != expected_result \
                or item['fresh_blind_checker'] != fresh or item.get('checker_caught') != checker_caught \
                or item.get('blind_caught') != (expected_result == 'caught'):
            raise RuntimeError('BLOCKED: round contradicts its actual checker verdict; re-score with this verifier')
    final = json.loads(Path(args.final).read_text())
    judged = json.loads(Path(args.final_judgment).read_text())
    final_clean = final['phase'] == 'final clean rerun' and final['script_result'] == 'CHECKS_PASSED' \
        and final['baseline_manifest_sha256'] == baseline_hash \
        and final['head_binary_sha256'] == json.loads((packet / 'baseline.json').read_text())['binaries']['meada-clean']['sha256']
    final_judgment_error = None
    try:
        attestation(packet, Path(args.final_judgment).read_text(),
                    getattr(args, 'ui_receipt', None), getattr(args, 'scope_receipt', None))
        independent_final = True
    except (OSError, ValueError, RuntimeError) as error:
        independent_final, final_judgment_error = False, str(error)
    missing = [name for name in planned if name not in latest]
    caught = sum(item['result'] == 'caught' for item in current)
    scripts = sum(item['script_caught'] for item in current)
    blind = sum(item['fresh_blind_checker'] for item in current)
    missed = [item['mutation'] for item in current if item['result'] == 'missed']
    luck = [item['mutation'] for item in current if item['result'] == 'caught by luck']
    incomplete = [item['mutation'] for item in current if item['result'] == 'incomplete']
    script_only = [item['mutation'] for item in current if item['script_caught'] and not item['blind_caught']]
    complete = not (missing or missed or luck or incomplete) and caught == len(planned) \
        and blind == len(planned) and final_clean and independent_final
    result = {'audit_result': 'COMPLETE' if complete else 'INCOMPLETE', 'baseline_manifest_sha256': baseline_hash,
              'planned': len(planned), 'caught': caught, 'script_caught': scripts, 'fresh_blind_rounds': blind,
              'script_only_detections': script_only, 'missed': missed, 'caught_by_luck': luck, 'incomplete_rounds': incomplete,
              'script_only_count': len(script_only), 'missed_count': len(missed),
              'caught_by_luck_count': len(luck), 'incomplete_count': len(incomplete), 'missing_count': len(missing),
              'missing': missing, 'final_clean_checks': bool(final_clean), 'final_fresh_verdict': bool(independent_final),
              'history': history, 'latest': current, 'final_judgment': judged, 'final_judgment_error': final_judgment_error,
              'limit': 'Bounded observed bugs, not a whole-app proof or a general completeness claim.'}
    save(args.output, result)
    lines = ['# A7/A8 bounded audit', '', f"Result: {result['audit_result']}; {caught}/{len(planned)} blind catches, {scripts} script detections, {blind} fresh blind judgments.",
             f'Final clean checks: {bool(final_clean)}; final independent PROVED: {bool(independent_final)}.', '',
             '| Mutation | Result | Script caught | Fresh blind | Symptom |', '|---|---|---|---|---|']
    lines += [f"| {item['mutation']} | {item['result']} | {item['script_caught']} | {item['fresh_blind_checker']} | {item['symptom'].replace('|', '/')} |"
              for item in history]
    lines += ['', 'Missing: ' + (', '.join(missing) or 'none'),
              'Missed: ' + (', '.join(missed) or 'none'), 'Incomplete: ' + (', '.join(incomplete) or 'none'),
              'Script detections without a blind catch: ' + (', '.join(script_only) or 'none'), '', result['limit']]
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
    p.add_argument('--ui-receipt')
    p.add_argument('--scope-receipt')
    p = sub.add_parser('check')
    p.add_argument('--baseline', required=True)
    p.add_argument('--candidate')
    p.add_argument('--run', required=True)
    p = sub.add_parser('ui-check')
    for option in ('baseline', 'head-bin', 'ui-dir', 'run', 'console-graph'):
        p.add_argument('--' + option, required=True)
    p = sub.add_parser('scope-check')
    for option in ('baseline', 'head-bin', 'ui-dir', 'run'):
        p.add_argument('--' + option, required=True)
    p = sub.add_parser('score')
    for option in ('auditor', 'checks', 'judgment', 'output'):
        p.add_argument('--' + option, required=True)
    p = sub.add_parser('report')
    for option in ('baseline', 'final', 'final-judgment', 'output'):
        p.add_argument('--' + option, required=True)
    p.add_argument('--rounds', nargs='*', required=True, help='chronological score files, retaining misses before repairs')
    p.add_argument('--ui-receipt')
    p.add_argument('--scope-receipt')
    sub.add_parser('validate-plan')
    args = parser.parse_args()
    try:
        return validate_plan() if args.phase == 'validate-plan' else globals()[args.phase.replace('-', '_')](args)
    except (OSError, ValueError, RuntimeError, StopIteration, AssertionError, KeyError) as error:
        print('BLOCKED: ' + str(error), file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
