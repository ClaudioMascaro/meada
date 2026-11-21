#!/usr/bin/env python3
"""Read immutable Proof receipts and replay #951's full export comparisons, with no product run."""
import argparse
import hashlib
import json
from pathlib import Path
import re
from zipfile import ZipFile

from prove import ROOT, export_diff, git

ARCHIVES = {
    936: (44191641900, 14379249483, '0ea8fe36e65daf082eb49004dd6b85cc8e7ef1280906befbe5f14d4df02d0cb8'),
    941: (44399516222, 14472907188, '2ceaa3f2f792a870a18c45cb8eee7ea2c2c74f95aa6d2d0985ef38b0e7cf1c8f'),
    943: (44429356459, 14486677773, '57930383206c5e21b9f4a0039b8c6f961fee803bca5f3f3f5ecad11b0799390e'),
    944: (44760633398, 14645838189, 'eebda28789d61be409e394b7c061ff3320c013568ee46edfd3cd39e7f20cb3f3'),
    947: (44760675646, 14643844620, 'b86ff2b447103335d5161721c23873a28853e13bf831a19de694f467effd67cf'),
}
FLOWS = {
    'iac-http-routes': ('iac-http-routes.py', '#666/#642 and ADR 0005/0025/0026'),
    'aws-java-private-fields': ('aws-java-private-fields.py', '#775 and aws-java-private-fields/oracle/java-run.txt'),
    'orphans': ('orphans.py', '#657/#855 and orphans.py source-keyed cases'),
    'ci': ('ci.py', '#820 F1 and ADR 0028'),
    'ci-corpus': ('ci.py', '#820/#904, infra.tsv and ci-records.json'),
    'ci-run': ('ci.py', '#820 F2 and ci.py generated cases'),
    'ci-permissions': ('ci.py', '#820 F3 and ci.py generated cases'),
    'a8-peers': ('a8-peers.sh', '#683 and a8-cases.tsv/a8-expected.jq'),
    'aws-sdk': ('aws-sdk.py', '#659/#642, ADR 0025/0026 and flows/aws-sdk/oracle.json'),
    'config-uses': ('config_uses.py', '#661/#642 and flows/config-uses/oracle.json'),
    'images': ('images.py', '#662/#642, ADR 0025/0026 and the generated fixtures'),
}
SCRIPTS = '.claude/skills/prove-meada/scripts'
GITHUB = 'https://github.com/ClaudioMascaro/meada/actions/runs'
SITES_952 = 'Typed.kt::example.jvm.Shadowing.hidden()'


def jload(archive, path):
    return json.loads(archive.read(path))


def rebless(archive, receipt, report):
    original = archive.read('rebless.md').decode()
    rows = []
    for heading, section in zip(re.findall(r'^## M (.+\.export\.json)$', original, re.M),
                                re.split(r'^## M .+\.export\.json$', original, flags=re.M)[1:]):
        left, right = (json.loads(git('show', f'{receipt[field]}:{heading}', cwd=ROOT)) for field in ('base', 'head'))
        delta, _ = export_diff(left, right)
        rows.append({'golden': heading, 'previously_formatting_only': 'only formatting changed' in section, 'delta': delta})
        report.extend([f'## {heading}', *delta, ''])
    return rows


def export_pair(archive, repository):
    return [jload(archive, f'{side}/{repository}/export1.json') for side in ('base', 'head')]


def oracle_failures(archive, name):
    path = f'flows/{name}/checks.json'
    if path not in archive.namelist():
        return []
    checks = jload(archive, path)
    return [row for row in checks if isinstance(row, dict) and
            (row.get('passed') is False or row.get('pass') is False)] if isinstance(checks, list) else []


def review(path, out):
    with path.open('rb') as stream:
        digest = hashlib.file_digest(stream, 'sha256').hexdigest()
    pr, = (number for number, (_, _, sha) in ARCHIVES.items() if digest == sha)
    run, artifact, _ = ARCHIVES[pr]
    with ZipFile(path) as archive:
        receipt = jload(archive, 'receipt.json')
        from_files = {name.removeprefix('bin/'): archive.read(name).decode().strip()
                      for name in archive.namelist() if name.startswith('bin/') and name.endswith('.from')}
        data = {'url': f'{GITHUB}/{run}/artifacts/{artifact}', 'sha256': digest,
                'driver_revision': receipt['head'], 'bin_from': from_files, 'raw_receipt': receipt}
        # These original jobs never exercised the pinned smoke app/lib/shared workspaces.
        data['smoke_mocks'] = 'not selected'
        by_step = {step['name']: step for step in receipt['steps']}
        data['drivers'] = {}
        for name, (driver, key) in FLOWS.items():
            item = {'step': by_step.get(name), 'key': key, 'driver': f"{receipt['head']}:{SCRIPTS}/{driver}",
                    'rejected_checks': oracle_failures(archive, name)}
            if step := by_step.get(name):
                log = f'logs/{name}.log'
                if log in archive.namelist():
                    item['log'] = log
                    item['last_lines'] = archive.read(log).decode(errors='replace').splitlines()[-5:]
            data['drivers'][name] = item
        if pr == 947:
            report = ['# #947 tracked goldens with the current export reader', '']
            data['goldens'] = rebless(archive, receipt, report)
            (out / '947-rebless-current.md').write_text('\n'.join(report))
            petclinic = export_pair(archive, 'petclinic')
            data['petclinic'] = {'delta': export_diff(*petclinic)[0],
                                 'open_items': [doc.get('open_items') for doc in petclinic]}
            data['kotlin_shadowing'] = [
                {field: [row for row in doc.get(field, []) if SITES_952 in row[id_field]]
                 for field, id_field in (('consumers', 'site'), ('open_items', 'call'))}
                for doc in export_pair(archive, 'meada')
            ]
            data['iac_http_routes_later_captures'] = {
                name: f'flows/iac-http-routes/{name}' in archive.namelist() for name in
                ('stdio-answers.json', 'http-answers.json', 'source-deleted-http.json',
                 'handler-source-deleted-http.json', 'source-deleted-consumer.json')
            }
            peers = archive.read('flows/a8-peers/peers.md').decode()
            data['a8_table_rows'] = len(re.findall(r'^\| PASS \|', peers, flags=re.M))
            data['a8_complete_export_diff'] = archive.read('flows/a8-peers/g8-export.diff').decode()
        (out / f'{pr}.json').write_text(json.dumps(data, indent=2) + '\n')
        print(f"#{pr} {receipt['base'][:8]} -> {receipt['head'][:8]} {receipt['capture']}; {data['url']}")
        if pr == 947:
            semantic = [row for row in data['goldens'] if row['previously_formatting_only'] and row['delta']]
            print(f'{len(semantic)} formerly formatting-only exports had substantive JSON deltas')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run', required=True)
    parser.add_argument('archives', nargs='+', type=Path)
    args = parser.parse_args()
    out = Path(args.run)
    out.mkdir(parents=True)
    for archive in args.archives:
        review(archive, out)


if __name__ == '__main__':
    main()
