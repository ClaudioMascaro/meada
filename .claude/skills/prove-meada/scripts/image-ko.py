#!/usr/bin/env python3
# From the two maintainer specs and shell execution semantics, not feature fixtures.
"""#662 literal ko publications; refuse heredoc text and compound shell scripts."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import yaml

from prove import git, Stdio, answer


def main(args):
    out = Path(args.run).resolve()
    os.environ['LANG'] = 'C.UTF-8'
    out.mkdir(parents=True, exist_ok=False)
    args.created=True
    source = out/'input'
    source.mkdir()
    git('init','-q',cwd=source)
    git('remote','add','origin','https://example.com/zz-prove-ko-shell/app.git',cwd=source)
    cases = {
        'plain_build': 'ko build --bare ./cmd/app',
        'plain_publish': 'ko publish --bare ./cmd/app',
        'plain_bash': 'ko build --bare ./cmd/app',
        'plain_sh': 'ko publish --bare ./cmd/app',
        'targetless': 'ko build --bare',
        'heredoc': 'cat <<\'EOF\'\nko build --bare ./cmd/app\nEOF\n',
        'compound': 'false && ko build --bare ./cmd/app',
        'multiline': 'false\nko publish --bare ./cmd/app\n',
        'semicolon': 'ko build --bare ./cmd/app; echo skipped',
        'substitution': 'ko build --bare $(echo ./cmd/app)',
        'quoted_text': 'echo "ko build --bare ./cmd/app"',
        'nonbare': 'ko build ./cmd/app',
        'help_long': 'ko build --bare --help ./cmd/app',
        'help_short': 'ko publish -h --bare',
        'unknown_flag': 'ko build --bare --imagerefs=output ./cmd/app',
        'duplicate_bare': 'ko build --bare --bare ./cmd/app',
        'two_packages': 'ko build --bare ./cmd/app ./cmd/other',
        'python_shell': 'ko build --bare ./cmd/app',
        'custom_shell': 'ko build --bare ./cmd/app',
    }
    positive = {'plain_build','plain_publish','plain_bash','plain_sh','targetless',
                'default_job_sh','default_step_bash','default_step_sh'}
    expected = {'image::ghcr.io/ko-shell/'+label for label in positive}
    (out/'oracle.json').write_text(json.dumps({'expected_builders': sorted(expected),
        'commands': cases, 'refusals': sorted((set(cases)-positive)|{'default_python'}),
        'workflow_python_default': 'default_python is refused; job sh and step bash/sh override'},indent=2))
    jobs = {}
    for label, command in cases.items():
        jobs[label] = {'env': {'KO_DOCKER_REPO':'ghcr.io/ko-shell/'+label},
                       'steps':[{'run':command}]}
    for label, shell in [('plain_bash','bash'),('plain_sh','sh'),('python_shell','python'),
                         ('custom_shell','bash -c {0}')]:
        jobs[label]['steps'][0]['shell']=shell
    workflow = source/'.github/workflows/publish.yml'
    workflow.parent.mkdir(parents=True)
    workflow.write_text(yaml.safe_dump({'jobs':jobs},sort_keys=False))
    default_jobs={}
    for label in ['default_python','default_job_sh','default_step_bash','default_step_sh']:
        default_jobs[label]={'env':{'KO_DOCKER_REPO':'ghcr.io/ko-shell/'+label},
                             'steps':[{'run':'ko publish --bare ./cmd/app'}]}
    default_jobs['default_job_sh']['defaults']={'run':{'shell':'sh'}}
    default_jobs['default_step_bash']['steps'][0]['shell']='bash'
    default_jobs['default_step_sh']['defaults']={'run':{'shell':'python'}}
    default_jobs['default_step_sh']['steps'][0]['shell']='sh'
    (workflow.parent/'defaults.yml').write_text(yaml.safe_dump({
        'defaults':{'run':{'shell':'python'}},'jobs':default_jobs},sort_keys=False))
    (source/'app.py').write_text('def ko_entry():\n    return 1\n')
    git('add','.',cwd=source)
    git('commit','-qm','independent ko shell cases',cwd=source)
    os.environ['KO_DOCKER_REPO'] = 'runtime.invalid/false-builder'
    (out/'input.sha256.json').write_text(json.dumps({str(p.relative_to(source)):
        hashlib.sha256(p.read_bytes()).hexdigest() for p in source.rglob('*')
        if p.is_file() and '.git' not in p.parts},indent=2))
    checks=[]
    binaries=[]
    if args.before_bin:
        binaries.append(('old-image',Path(args.before_bin).resolve()))
    binaries.append(('head',Path(args.head_bin).resolve()))
    before_providers=[]
    for side, binary in binaries:
        repo=out/'work'/side
        shutil.copytree(source,repo)
        result=subprocess.run([str(binary),'export'],cwd=repo,capture_output=True,text=True,timeout=180)
        (out/(side+'.export.json')).write_text(result.stdout)
        (out/(side+'.export.err')).write_text(result.stderr)
        (out/(side+'.export.exit')).write_text(str(result.returncode))
        if result.returncode:
            raise RuntimeError(side+' export failed')
        data=json.loads(result.stdout)
        providers=data['providers']
        if side=='old-image':
            before_providers=providers
        if side=='head':
            checks.append({'check':'exact complete builder set, no shell false publications',
                           'pass':len(providers)==len(expected) and {r['contract'] for r in providers}==expected,
                           'providers':providers})
            client=Stdio(str(binary),repo,out/'stdio.err')
            try:
                for contract in sorted(expected):
                    raw=client.call('tools/call',{'name':'context','arguments':{'symbol':contract,'budget':20000}})
                    label=contract.rsplit('/',1)[-1]
                    (out/(label+'.context.json')).write_text(json.dumps(raw,indent=2))
                    file='defaults.yml' if label.startswith('default_') else 'publish.yml'
                    expected_at=f'.github/workflows/{file}#jobs.{label}.steps[0].run'
                    selected=[row for row in providers if row['contract']==contract]
                    checks.append({'check':label+' export retains exact checked-in command location',
                                   'pass':len(selected)==1 and selected[0].get('at')==expected_at})
                    expected_context=(contract+'\n  example.com/zz-prove-ko-shell/app builder '
                                      '(repository-level) (no runner in the workspace)\n')
                    checks.append({'check':label+' full repository-level context is correct',
                                   'pass':answer(raw)==expected_context})
            finally:
                client.close()
    if args.before_bin:
        required={'heredoc','help_long','help_short','python_shell'}
        before={row['contract'] for row in before_providers}
        checks.append({'check':'before-fix binary reproduces heredoc/help/non-shell false publications',
                       'pass':{'image::ghcr.io/ko-shell/'+label for label in required} <= before})
    (out/'checks.json').write_text(json.dumps(checks,indent=2))
    for c in checks:
        print(('PASS ' if c['pass'] else 'FAIL ')+c['check'])
    failures=sum(not c['pass'] for c in checks)
    (out/'ko.md').write_text('# Literal ko publication proof\n\n'+'\n'.join(
        f"- [{'PASS' if c['pass'] else 'FAIL'}] {c['check']}" for c in checks)+'\n')
    print(f"{'differences found' if failures else 'checks passed'}: {len(checks)-failures}/{len(checks)}; {out}/ko.md")
    return int(bool(failures))

if __name__ == '__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--head-bin',required=True)
    parser.add_argument('--before-bin',help='optional immutable image binary predating the shell/help fixes')
    parser.add_argument('--run',required=True,help='new evidence directory; never overwrite an earlier run')
    args=parser.parse_args()
    args.created=False
    try:
        raise SystemExit(main(args))
    except Exception as exc:
        out=Path(args.run).resolve()
        if args.created:
            (out/'blocked.txt').write_text(repr(exc))
        print(f'BLOCKED: {exc}; raw evidence in {out}')
        raise SystemExit(2)
