#!/usr/bin/env python3
"""Exercise audit safety gates on a disposable dummy git repository, never product code."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
from unittest.mock import patch

SCRIPT = Path(__file__).with_name('a7-a8-audit.py')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--run', required=True)
    args = parser.parse_args()
    run = Path(args.run).resolve()
    run.mkdir(parents=True, exist_ok=False)
    spec = importlib.util.spec_from_file_location('audit_runner', SCRIPT)
    audit = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(audit)
    checks = []
    with tempfile.TemporaryDirectory(prefix='zz-prove-audit-') as temp:
        root = Path(temp)
        skill = root / '.claude/skills/prove-meada'
        skill.mkdir(parents=True)
        source = root / 'dummy.rs'
        original = b'fn dummy() { correct(); }\r\n'
        source.write_bytes(original)
        (root / '.gitignore').write_text('.prove-it/\n')
        (skill / 'frozen-key.txt').write_text('independent dummy answer\n')
        for name in ('frozen-inputs-a', 'frozen-inputs-b'):
            (skill / name).mkdir()
            (skill / name / 'value.txt').write_text(name + '\n')
        source_link = skill / 'source-link'
        source_link.symlink_to('frozen-inputs-a', target_is_directory=True)
        plan = skill / 'mutations.json'
        plan.write_text(json.dumps([{'id': 'dummy', 'file': 'dummy.rs', 'before': 'correct()',
                                    'after': 'incorrect()', 'check': 'values', 'evidence': 'dummy symptom'}]))
        for command in (['init', '-q'], ['add', '.'], ['-c', 'user.name=Proof', '-c', 'user.email=proof@example.com',
                                               'commit', '-qm', 'dummy audit safety input']):
            subprocess.run(['git', '-C', str(root), *command], check=True, capture_output=True)
        audit.ROOT, audit.SKILL, audit.PLAN = root, skill, plan
        commands = audit.commands(root / 'dummy-head', root / 'dummy-existing', root / 'dummy-base',
                                  root / '.prove-it/runs/commands', root / 'dummy-pre-extensions')
        outputs = [command[command.index('--run') + 1] if '--run' in command else command[-1] for command in commands.values()]
        assert set(commands) == set(audit.ALL) and len(set(outputs)) == len(audit.ALL)
        for flow in ['custom-iac', 'iac-http-routes']:
            assert commands[flow][commands[flow].index('--base-bin') + 1] == str(root / 'dummy-pre-extensions')
        checks.append({'check': 'every named flow has a distinct evidence directory, including both AWS modes', 'passed': True})
        baseline = root / '.prove-it/runs/baseline'
        (baseline / 'bin').mkdir(parents=True)
        for name in ('meada-clean', 'meada-base'):
            (baseline / 'bin' / name).write_bytes(b'dummy immutable binary\n')
        audit.save(baseline / 'baseline.json', {
            'source_commit': audit.git('rev-parse', 'HEAD'), 'source_tree': audit.git('rev-parse', 'HEAD^{tree}'),
            'verifier_keys': audit.keys(),
            'binaries': {name: {'sha256': audit.digest(baseline / 'bin' / name)}
                         for name in ('meada-clean', 'meada-base')},
            'checks': {'script_result': 'CHECKS_PASSED', 'scope': list(audit.ALL)}})
        receipt = json.dumps({'verdict': 'PROVED', 'checker': 'dummy fixture, not a real checker',
                             'receipt': 'fixture only', 'baseline_manifest_sha256': audit.digest(baseline / 'baseline.json')})

        def refused(label, operation):
            try:
                operation()
            except (ValueError, RuntimeError):
                checks.append({'check': label, 'passed': True})
            else:
                raise AssertionError('Safety gate did not refuse ' + label)

        def phase(name):
            return argparse.Namespace(baseline=str(baseline), mutation='dummy',
                                      run=str(root / '.prove-it/audits' / name))

        with patch.dict(os.environ, {'PROVE_BASELINE_ATTESTATION': ''}):
            refused('missing baseline judgment cannot plant', lambda: audit.candidate(phase('no-verdict')))
        assert source.read_bytes() == original
        bad = json.loads(receipt)
        bad['verdict'] = 'BROKEN'
        with patch.dict(os.environ, {'PROVE_BASELINE_ATTESTATION': json.dumps(bad)}):
            refused('BROKEN baseline cannot plant', lambda: audit.candidate(phase('broken-verdict')))
        key = skill / 'frozen-key.txt'
        key.write_text('rewritten key\n')
        refused('changed verifier key invalidates baseline', lambda: audit.read_manifest(baseline))
        key.write_text('independent dummy answer\n')
        source_link.unlink()
        source_link.symlink_to('frozen-inputs-b', target_is_directory=True)
        refused('changed directory symlink invalidates frozen source input keys', lambda: audit.read_manifest(baseline))
        source_link.unlink()
        source_link.symlink_to('frozen-inputs-a', target_is_directory=True)
        binary = baseline / 'bin/meada-clean'
        binary.write_bytes(b'replaced binary\n')
        refused('changed baseline binary invalidates pin', lambda: audit.read_manifest(baseline))
        binary.write_bytes(b'dummy immutable binary\n')

        def failed_build(*_):
            assert 'incorrect()' in source.read_text()
            raise RuntimeError('intentional dummy build failure')

        with patch.dict(os.environ, {'PROVE_BASELINE_ATTESTATION': receipt}), patch.object(audit, 'build', failed_build):
            refused('failed build still restores exact bytes', lambda: audit.candidate(phase('failed-build')))
        restoration = json.loads((root / '.prove-it/audits/failed-build/auditor/restoration.json').read_text())
        assert restoration['cmp_exit'] == 0 and restoration['tracked_hashes_equal']
        assert source.read_bytes() == original and audit.status() == ''
        assert not (root / '.prove-it/audits/failed-build/checker/candidate.json').exists()

        def fake_build(_, output, __):
            assert 'incorrect()' in source.read_text()
            output.parent.mkdir(parents=True)
            output.write_bytes(b'dummy changed binary\n')

        with patch.dict(os.environ, {'PROVE_BASELINE_ATTESTATION': receipt}), patch.object(audit, 'build', fake_build):
            assert audit.candidate(phase('success')) == 0
        assert source.read_bytes() == original and audit.status() == ''
        checker = root / '.prove-it/audits/success/checker'
        assert sorted(p.name for p in checker.iterdir()) == ['candidate.json', 'meada-candidate']
        assert 'dummy.rs' not in (checker / 'candidate.json').read_text()
        checks.append({'check': 'successful dummy build restores CRLF bytes and separates opaque packet', 'passed': True})
        audit.save(run / 'dummy-restoration.json', json.loads((checker.parent / 'auditor/restoration.json').read_text()))
        observed = root / '.prove-it/runs/observed'
        observed.mkdir(parents=True)
        baseline_hash = audit.digest(baseline / 'baseline.json')
        audit.save(observed / 'checks.json', {
            'head_binary_sha256': audit.digest(checker / 'meada-candidate'),
            'baseline_manifest_sha256': baseline_hash, 'script_result': 'BROKEN',
            'checks': [{'flow': 'values', 'exit': 1}]})
        evidence = observed / 'raw.txt'
        evidence.write_text('unrelated launch failure\n')
        judgment = observed / 'judgment.json'
        audit.save(judgment, {'verdict': 'BROKEN', 'checker': 'dummy fixture, not a fresh checker',
                             'symptom': 'unrelated launch failure', 'evidence': 'raw.txt',
                             'script_names_symptom': True, 'fresh_blind_checker': False})
        scored = observed / 'score.json'
        audit.score(argparse.Namespace(auditor=str(checker.parent / 'auditor'), checks=str(observed),
                                       judgment=str(judgment), output=str(scored)))
        item = json.loads(scored.read_text())
        assert item['result'] == 'incomplete' and item['script_detection'] == 'caught by luck'
        checks.append({'check': 'unrelated failure cannot count as a symptom catch', 'passed': True})
        evidence.write_text('dummy symptom visibly occurred\n')
        audit.score(argparse.Namespace(auditor=str(checker.parent / 'auditor'), checks=str(observed),
                                       judgment=str(judgment), output=str(scored)))
        item = json.loads(scored.read_text())
        assert item['result'] == 'incomplete' and item['script_detection'] == 'caught by luck'
        checks.append({'check': 'judged symptom must occur in the named raw evidence', 'passed': True})
        judged = json.loads(judgment.read_text())
        judged['symptom'] = 'dummy symptom visibly occurred'
        audit.save(judgment, judged)
        audit.score(argparse.Namespace(auditor=str(checker.parent / 'auditor'), checks=str(observed),
                                       judgment=str(judgment), output=str(scored)))
        assert json.loads(scored.read_text())['script_caught']
        audit.save(observed / 'final.json', {
            'phase': 'final clean rerun', 'script_result': 'CHECKS_PASSED',
            'baseline_manifest_sha256': baseline_hash,
            'head_binary_sha256': audit.digest(baseline / 'bin/meada-clean')})
        final_judgment = observed / 'final-judgment.json'
        final_judgment.write_text(receipt)
        result = audit.report(argparse.Namespace(baseline=str(baseline), rounds=[str(scored)],
            final=str(observed / 'final.json'), final_judgment=str(final_judgment), output=str(observed / 'report.json')))
        assert result == 1 and json.loads((observed / 'report.json').read_text())['audit_result'] == 'INCOMPLETE'
        checks.append({'check': 'script catches without fresh blind evidence cannot complete the audit', 'passed': True})
        audit.save(run / 'dummy-report.json', json.loads((observed / 'report.json').read_text()))
        # The independent review found that a script detection overrode PROVED. Keep the
        # actual contradictory verdict in both the score and aggregate regression.
        judged.update(verdict='PROVED', fresh_blind_checker=True, receipt='dummy contradictory verdict fixture only')
        audit.save(judgment, judged)
        audit.score(argparse.Namespace(auditor=str(checker.parent / 'auditor'), checks=str(observed),
                                       judgment=str(judgment), output=str(scored)))
        item = json.loads(scored.read_text())
        assert item['script_caught'] and item['result'] == 'missed'
        assert item['fresh_blind_checker'] and not item['checker_caught'] and not item['blind_caught']
        result = audit.report(argparse.Namespace(baseline=str(baseline), rounds=[str(scored)],
            final=str(observed / 'final.json'), final_judgment=str(final_judgment), output=str(observed / 'report.json')))
        report = json.loads((observed / 'report.json').read_text())
        assert result == 1 and report['audit_result'] == 'INCOMPLETE' and report['caught'] == 0
        assert report['missed'] == ['dummy'] and report['script_only_detections'] == ['dummy']
        assert report['missed_count'] == 1 and report['script_only_count'] == 1 and report['incomplete_count'] == 0
        assert report['script_caught'] == 1 and report['fresh_blind_rounds'] == 1
        checks.append({'check': 'fresh PROVED remains missed despite a script detection, including aggregate', 'passed': True})
        audit.save(run / 'dummy-contradictory-score.json', item)
        audit.save(run / 'dummy-contradictory-report.json', report)
        # Old or inconsistent derived fields must not bypass the nested actual verdict.
        inconsistent = dict(item, result='caught', checker_caught=True, blind_caught=True)
        audit.save(observed / 'inconsistent-score.json', inconsistent)
        refused('aggregate rejects catch flags contradicting actual PROVED verdict', lambda: audit.report(
            argparse.Namespace(baseline=str(baseline), rounds=[str(observed / 'inconsistent-score.json')],
                final=str(observed / 'final.json'), final_judgment=str(final_judgment), output=str(observed / 'inconsistent-report.json'))))
        judged['verdict'] = 'BROKEN'
        audit.save(judgment, judged)
        audit.score(argparse.Namespace(auditor=str(checker.parent / 'auditor'), checks=str(observed),
                                       judgment=str(judgment), output=str(scored)))
        item = json.loads(scored.read_text())
        assert item['result'] == 'caught' and item['blind_caught'] and item['checker_caught']
        result = audit.report(argparse.Namespace(baseline=str(baseline), rounds=[str(scored)],
            final=str(observed / 'final.json'), final_judgment=str(final_judgment), output=str(observed / 'positive-control.json')))
        assert result == 0
        checks.append({'check': 'actual fresh BROKEN with named symptom is the positive catch control', 'passed': True})
        for verdict in ('BLOCKED', 'UNPROVEN'):
            judged['verdict'] = verdict
            audit.save(judgment, judged)
            audit.score(argparse.Namespace(auditor=str(checker.parent / 'auditor'), checks=str(observed),
                                           judgment=str(judgment), output=str(scored)))
            item = json.loads(scored.read_text())
            assert item['result'] == 'incomplete' and item['script_caught'] and not item['blind_caught']
            result = audit.report(argparse.Namespace(baseline=str(baseline), rounds=[str(scored)],
                final=str(observed / 'final.json'), final_judgment=str(final_judgment), output=str(observed / 'unfinished-report.json')))
            unfinished = json.loads((observed / 'unfinished-report.json').read_text())
            assert result == 1 and unfinished['incomplete_rounds'] == ['dummy'] and unfinished['caught'] == 0
            assert unfinished['incomplete_count'] == 1 and unfinished['script_only_count'] == 1
        checks.append({'check': 'fresh BLOCKED and UNPROVEN judgments leave detected rounds incomplete', 'passed': True})
        # Native checks alone must never authorize missing clean UI/source proof. These
        # are disposable receipts, not actual product or independent-checker evidence.
        original_manifest = (baseline / 'baseline.json').read_bytes()
        manifest = json.loads(original_manifest)
        manifest['requires_ui_receipt'] = True
        manifest['required_separate_scope'] = list(audit.SEPARATE_SCOPE)
        audit.save(baseline / 'baseline.json', manifest)
        judged = json.loads(receipt)
        judged['baseline_manifest_sha256'] = audit.digest(baseline / 'baseline.json')
        ui_path, scope_path = baseline / 'dummy-ui.json', baseline / 'dummy-scope.json'
        (baseline / 'bin/meada-ui').write_bytes(b'dummy UI binary, never product\n')
        frontend = baseline / 'frontend'
        frontend.mkdir()
        (frontend / 'meada-ui.json').write_text('dummy frontend, never product\n')
        ui = {'source_commit': manifest['source_commit'], 'source_tree': manifest['source_tree'],
              'baseline_manifest_sha256': judged['baseline_manifest_sha256'],
              'verifier_keyset_sha256': audit.keyset_hash(manifest['verifier_keys']),
              'scope': list(audit.UI_FLOWS), 'script_result': 'CHECKS_PASSED', 'console_graph': 'success',
              'source_and_binary_unchanged': True,
              'checks': [{'flow': name, 'exit': 0} for name in audit.UI_FLOWS],
              'ui_binary_sha256': audit.digest(baseline / 'bin/meada-ui'),
              'frontend_sha256': {'meada-ui.json': audit.digest(frontend / 'meada-ui.json')}}
        audit.save(ui_path, ui)
        judged['ui_receipt_sha256'] = audit.digest(ui_path)
        refused('native pass cannot authorize missing UI evidence', lambda: audit.attestation(baseline, json.dumps(judged)))
        refused('native/UI pass cannot authorize missing scope captures',
                lambda: audit.attestation(baseline, json.dumps(judged), ui_path))
        from prove import corpus
        entries = list(corpus())
        flows = ['corpus', 'viewer-start', 'viewer-pages', 'viewer-stop', 'source-and-ui-binary-unchanged']
        flows += [name + '-' + phase for name in entries for phase in ('ui-stdio', 'start', 'api', 'stop')]
        captured = {key: ui[key] for key in ('source_commit', 'source_tree', 'baseline_manifest_sha256',
                    'verifier_keyset_sha256', 'ui_binary_sha256', 'frontend_sha256')}
        captured.update(script_result='CHECKS_PASSED', capture_result='CAPTURED', entries=entries,
                        checks=[{'flow': name, 'exit': 0} for name in flows],
                        source_captures=[{'flow': name, 'exit': 0} for name in audit.SOURCE_CAPTURES])
        audit.save(scope_path, captured)
        judged['scope_receipt_sha256'] = audit.digest(scope_path)
        refused('script captures cannot replace missing independent source judgments',
                lambda: audit.attestation(baseline, json.dumps(judged), ui_path, scope_path))
        judged['separate_scope_receipt'] = {'receipt': 'dummy source judgment only', 'sha256': '2' * 64,
            'verdict': 'PROVED', 'covers': list(audit.SEPARATE_SCOPE), 'source_commit': manifest['source_commit'],
            'verifier_keyset_sha256': audit.keyset_hash(manifest['verifier_keys'])}
        audit.attestation(baseline, json.dumps(judged), ui_path, scope_path)
        checks.append({'check': 'complete same-source/key dummy UI and source receipts are the positive gate control', 'passed': True})
        original_ui = ui_path.read_bytes()
        ui['verifier_keyset_sha256'] = '3' * 64
        audit.save(ui_path, ui)
        judged['ui_receipt_sha256'] = audit.digest(ui_path)
        refused('same-source UI from different keys cannot authorize a mutation',
                lambda: audit.attestation(baseline, json.dumps(judged), ui_path, scope_path))
        ui_path.write_bytes(original_ui)
        judged['ui_receipt_sha256'] = audit.digest(ui_path)
        captured['checks'][0]['exit'] = 1
        captured['script_result'] = 'BROKEN'
        audit.save(scope_path, captured)
        judged['scope_receipt_sha256'] = audit.digest(scope_path)
        audit.attestation(baseline, json.dumps(judged), ui_path, scope_path)
        checks.append({'check': 'retained corpus differences need the separate source judgment; parity is not a new key', 'passed': True})
        captured['checks'].append({'flow': 'extra-api-failure', 'exit': 1})
        audit.save(scope_path, captured)
        judged['scope_receipt_sha256'] = audit.digest(scope_path)
        refused('a fresh source judgment cannot waive failed API or incomplete transport scope',
                lambda: audit.attestation(baseline, json.dumps(judged), ui_path, scope_path))
        captured['checks'].pop()
        captured['checks'].pop()  # a missing real entry cleanup result cannot masquerade as complete
        audit.save(scope_path, captured)
        judged['scope_receipt_sha256'] = audit.digest(scope_path)
        refused('missing actual corpus/API/viewer phase cannot count as complete',
                lambda: audit.attestation(baseline, json.dumps(judged), ui_path, scope_path))
        (baseline / 'baseline.json').write_bytes(original_manifest)
    audit.save(run / 'checks.json', {'checks': checks, 'scope': 'disposable dummy repository only',
                                    'product_mutations': 0, 'compiler_runs': 0})
    print(f'{len(checks)} audit safety gates passed; no product audit score.')


if __name__ == '__main__':
    main()
