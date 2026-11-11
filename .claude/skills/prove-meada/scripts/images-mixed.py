#!/usr/bin/env python3
"""Independent combined #658/#662 persistence, trace and same-size refresh probe.

Uses only transport/git mechanics, no product readers or test fixtures. Oracle is
the two issue specs and literal neutral inputs, written before any app answer.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import time

from prove import Stdio, Http, answer, free_port, git

OUT = HEAD = BASE = None
PREFIX = 'example.com/zz-prove-mixed/'
IMAGE = 'image::ghcr.io/mixed-proof/worker'
TOPIC = 'channel::sns::parcel'
QUEUE = 'channel::sqs::parcel'
EXPECTED_IMAGE_LINKS = {
    (PREFIX+'infra::compose.yaml::services.parcel', IMAGE, '.', 'exact', PREFIX+'app'),
    (PREFIX+'infra::compose.yaml::services.registry', 'image::*/mixed-proof/worker', '.', 'inferred', PREFIX+'app'),
}
EXPECTED_SUBSCRIPTION = (TOPIC, QUEUE, PREFIX+'infra::main.tf::aws_sns_topic_subscription.route', '.')
# ADR 0026 (#755): the app writes no Dockerfile, so the build runs its whole context at `inferred`
# and the reason says the Dockerfile was not read (#782).
EXPECTED_EXECUTION = (PREFIX+'app::compose.yaml::services.parcel', '.', PREFIX+'app', '.', 'inferred')
rows = []


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(value)


def check(label, good, detail=''):
    rows.append({'check': label, 'pass': bool(good), 'detail': detail})
    write(OUT / 'checks.json', json.dumps(rows, indent=2))
    print(('PASS ' if good else 'FAIL ') + label, flush=True)


def command(binary, cwd, label, *args):
    result = subprocess.run([str(binary), *args], cwd=cwd, capture_output=True,
                            text=True, timeout=180)
    for suffix, text in [('out', result.stdout), ('err', result.stderr),
                         ('exit', str(result.returncode))]:
        write(OUT / (label + '.' + suffix), text)
    if result.returncode:
        raise RuntimeError(label + ': command failed; raw captures retained')
    return result.stdout


def query(client, label, tool, **arguments):
    arguments['budget'] = 100000
    raw = client.call('tools/call', {'name': tool, 'arguments': arguments})
    text = answer(raw)
    write(OUT / (label + '.json'), json.dumps({'tool': tool, 'arguments': arguments,
                                             'raw': raw, 'text': text}, indent=2))
    return text


def main(args):
    global OUT, HEAD, BASE
    out = Path(args.run).resolve()
    out.mkdir(parents=True, exist_ok=False)
    OUT = out
    HEAD, BASE = Path(args.head_bin).resolve(), Path(args.base_bin).resolve()
    os.environ['LANG'] = 'C.UTF-8'
    # Everything expected below follows the literal source, before Meada is run.
    write(OUT / 'oracle.json', json.dumps({
        'image': IMAGE, 'source': TOPIC, 'target': QUEUE,
        'subscription': 'aws_sns_topic_subscription.route',
        'workload': 'services.parcel', 'direct_code': '.',
        'unknown_registry': 'image::*/mixed-proof/worker',
        'image_link_tuples': sorted(EXPECTED_IMAGE_LINKS),
        'subscription_tuple': EXPECTED_SUBSCRIPTION,
        'execution_tuple': EXPECTED_EXECUTION,
        # Same-repository Compose follows its direct execution rather than adding
        # a self image link (base ADR 0026 and original image binary's direct flow).
        'head_expected_image_links': 2, 'head_expected_channel_links': 1,
    }, indent=2))
    seed = OUT / 'input'
    app, infra = seed / 'app', seed / 'infra'
    for name, repo in [('app', app), ('infra', infra)]:
        repo.mkdir(parents=True)
        git('init', '-q', cwd=repo)
        git('remote', 'add', 'origin', f'https://{PREFIX}{name}.git', cwd=repo)
        other = 'infra' if name == 'app' else 'app'
        write(repo / 'meada.toml',
              ('[serve]\ndocuments = ["sns.asyncapi.json"]\n' if name == 'app' else '')
              + f'[[peer]]\nrepo = "{PREFIX}{other}"\n')
    write(app / 'app.py', 'import boto3\nqueue = boto3.client("sqs")\n'
          '\ndef receive_parcel():\n    return queue.receive_message('
          'QueueUrl="https://sqs.eu-west-1.amazonaws.com/123456789012/parcel")\n')
    write(app / 'sns.asyncapi.json', json.dumps({
        'asyncapi': '2.6.0', 'info': {'title': 'Mixed proof', 'version': '1'},
        'servers': {'sns': {'url': 'localhost', 'protocol': 'sns'}},
        'channels': {'parcel': {'subscribe': {'message': {'payload': {'type': 'object'}}}}},
    }, indent=2))
    write(app / '.github/workflows/publish.yml', 'jobs:\n  publish:\n    steps:\n'
          '      - uses: docker/build-push-action@v6\n        with:\n'
          '          tags: ghcr.io/mixed-proof/worker:one\n')
    write(app / 'compose.yaml', 'services:\n  parcel:\n    build: .\n'
          '    image: ghcr.io/mixed-proof/worker:one\n')
    write(infra / 'compose.yaml', 'services:\n  parcel:\n'
          '    image: ghcr.io/mixed-proof/worker:one\n  registry:\n'
          '    image: ${REGISTRY}/mixed-proof/worker:one\n')
    tf = ('resource "aws_sns_topic" "parcel" { name = "parcel" }\n'
          'resource "aws_sqs_queue" "parcel" { name = "parcel" }\n'
          'resource "aws_sns_topic_subscription" "route" {\n'
          '  topic_arn = "arn:aws:sns:eu-west-1:123456789012:parcel"\n'
          '  protocol = "sqs"\n'
          '  endpoint = "arn:aws:sqs:eu-west-1:123456789012:parcel"\n}\n')
    write(infra / 'main.tf', tf)
    for repo in [app, infra]:
        git('add', '.', cwd=repo)
        git('commit', '-qm', 'neutral mixed image and AWS inputs', cwd=repo)
    write(OUT / 'input.sha256.json', json.dumps({str(p.relative_to(seed)):
          hashlib.sha256(p.read_bytes()).hexdigest() for p in seed.rglob('*')
          if p.is_file() and '.git' not in p.parts}, indent=2))
    os.environ['REGISTRY'] = 'runtime.invalid'
    exports = {}
    for side, binary in [('tier', BASE), ('head', HEAD)]:
        work = OUT / 'work' / side
        shutil.copytree(seed, work)
        for name in ['app', 'infra']:
            exports[side, name] = json.loads(command(binary, work/name,
                f'{side}/{name}/export', 'export'))
            command(binary, work/name, f'{side}/{name}/status', 'status')
            check(f'{side} {name} cold/reopen full bytes',
                  command(binary, work/name, f'{side}/{name}/reopen', 'export') ==
                  (OUT/f'{side}/{name}/export.out').read_text())
    for name in ['app', 'infra']:
        old, new = exports['tier', name], exports['head', name]
        for field in ['providers', 'consumers', 'links']:
            check(name + ' existing ' + field + ' whole-row tier parity',
                  [r for r in new[field] if not str(r.get('contract') or '').startswith('image::')]
                  == [r for r in old[field] if not str(r.get('contract') or '').startswith('image::')])
        for field in ['files', 'symbols', 'calls', 'provisioners', 'provisions', 'subscriptions']:
            check(name + ' ' + field + ' complete tier parity', new.get(field) == old.get(field))
    app_data, infra_data = exports['head','app'], exports['head','infra']
    app_images=[r for r in app_data['links'] if r['contract'].startswith('image::')]
    infra_images=[r for r in infra_data['links'] if r['contract'].startswith('image::')]
    # Each endpoint's repository export carries its cross-repository links. This
    # is the existing export scope, not two additional links or a self link.
    check('each endpoint retains exactly both mixed image links and no direct self link',
          len(app_images)==2 and app_images==infra_images and {
              (r['consumer'],r['contract'],r.get('root'),r['level'],r['provider'])
              for r in app_images}==EXPECTED_IMAGE_LINKS,app_images)
    check('exact and registry-pattern image levels coexist',
          {r['level'] for r in infra_data['links'] if r['contract'].startswith('image::')}
          == {'exact', 'inferred'})
    registry = [r for r in infra_data['consumers'] if 'services.registry' in r['site']]
    check('registry wildcard retains unknown registry, ignores runtime env', len(registry)==1
          and registry[0]['contract']=='image::*/mixed-proof/worker', registry)
    routed = [r for r in app_data['links'] if r['contract'].startswith('channel::')]
    check('same-spelled SNS/SQS route is retained exactly once', len(routed)==1
          and routed[0]['contract']==QUEUE and routed[0]['level']=='exact'
          and 'aws_sns_topic_subscription.route' in routed[0]['reason'], routed)
    check('SNS subscription and direct execution both persist',
          len(infra_data.get('subscriptions',[]))==1 and len(app_data.get('executions',[]))==1)
    subscriptions=infra_data.get('subscriptions',[])
    check('subscription exactly matches the literal source target node and root',
          len(subscriptions)==1 and {
              (r['source'],r['target'],r['node'],r['root']) for r in subscriptions
          }=={EXPECTED_SUBSCRIPTION},subscriptions)
    executions=app_data.get('executions',[])
    check('direct execution exactly matches workload root repository path and level',
          len(executions)==1 and {
              (r['workload'],r['root'],r['repository'],r['path'],r['level']) for r in executions
          }=={EXPECTED_EXECUTION},executions)
    check('direct execution reason says the Dockerfile was not read',
          len(executions)==1 and 'Dockerfile' in executions[0].get('reason', '')
          and 'not read' in executions[0].get('reason', ''), executions)
    head = HEAD
    root = OUT/'work/head'
    client = Stdio(str(head), root/'app', OUT/'persistent.err')
    try:
        baseline = query(client, 'trace/receive-up-before', 'trace',
                         **{'from':'receive_parcel', 'direction':'up'})
        check('up trace has AWS subscription and image workload together',
              'sns.asyncapi.json' in baseline and 'services.parcel' in baseline
              and 'main.tf' in baseline, baseline)
        down = query(client, 'trace/workload-down', 'trace',
                     **{'from':f'{PREFIX}infra::compose.yaml::services.parcel', 'direction':'down'})
        check('workload trace reaches receiver code', 'receive_parcel' in down, down)
        context = query(client, 'trace/topic-context', 'context', symbol='aws_sns_topic.parcel')
        check('IaC context retains provisioned channel and producer',
              TOPIC in context and 'producer' in context, context)
        # Editing only image YAML must not disturb the AWS graph or live trace.
        image_file = root/'infra/compose.yaml'
        original = image_file.read_text()
        image_file.write_text(original.replace('mixed-proof/worker:one','mixed-proof/absent:one'))
        changed = json.loads(command(head, root/'infra', 'edit-image/export', 'export'))
        check('same-size image edit removes runner links',
              not [r for r in changed['links'] if r['contract'].startswith('image::')])
        check('image edit preserves AWS complete rows', all(changed.get(k)==infra_data.get(k)
              for k in ['provisioners','provisions','subscriptions']))
        image_file.write_text(original)
        restored = json.loads(command(head, root/'infra', 'restore-image/export', 'export'))
        check('image restore returns all complete contract arrays', all(restored.get(k)==infra_data.get(k)
              for k in ['providers','consumers','links','provisioners','provisions','subscriptions','executions']))
        # Editing only the subscription must remove its route, preserving images.
        tf_file = root/'infra/main.tf'
        tf_file.write_text(tf.replace('sns:eu-west-1:123456789012:parcel',
                                      'sns:eu-west-1:123456789012:absent'))
        changed_app = json.loads(command(head, root/'app', 'edit-aws/app-export', 'export'))
        changed_infra = json.loads(command(head, root/'infra', 'edit-aws/infra-export', 'export'))
        check('same-size AWS edit removes routed source link',
              not [r for r in changed_app['links'] if r['contract'].startswith('channel::')])
        check('AWS edit preserves every image row', all(
              [r for r in changed_infra[k] if str(r.get('contract') or '').startswith('image::')]
              == [r for r in infra_data[k] if str(r.get('contract') or '').startswith('image::')]
              for k in ['providers','consumers','links']))
        tf_file.write_text(tf)
        restored_app = json.loads(command(head, root/'app', 'restore-aws/app-export', 'export'))
        check('AWS restore returns whole app contract arrays', all(restored_app.get(k)==app_data.get(k)
              for k in ['providers','consumers','links','provisioners','provisions','subscriptions','executions']))
        final = query(client, 'trace/receive-up-restored', 'trace',
                      **{'from':'receive_parcel', 'direction':'up'})
        check('persistent mixed trace returns exactly after both restorations', final==baseline)
    finally:
        client.close()
    manifest = 'index_dir = "indexes"\n'
    for name in ['app','infra']:
        manifest += f'[[repository]]\nrepo = "{PREFIX}{name}"\npath = "{root/name}"\n'
    write(OUT/'http/workspace.toml', manifest)
    command(head, OUT/'http', 'http/index', 'index', '--workspace', 'workspace.toml')
    port = free_port()
    with open(OUT/'http/server.log','w') as log:
        proc = subprocess.Popen([str(head),'mcp','--listen',f'127.0.0.1:{port}',
                                 '--workspace','workspace.toml'],cwd=OUT/'http',stdout=log,stderr=log)
        write(OUT/'http/pid',str(proc.pid))
        try:
            for _ in range(100):
                if proc.poll() is not None:
                    raise RuntimeError('mixed HTTP exited early')
                try:
                    socket.create_connection(('127.0.0.1',port),timeout=.2).close()
                    break
                except OSError:
                    time.sleep(.1)
            http = Http(f'http://127.0.0.1:{port}/{PREFIX}app')
            got = []
            for label in ['receive-up-before','workload-down','topic-context']:
                capture = json.loads((OUT/f'trace/{label}.json').read_text())
                raw = http.call('tools/call',{'name':capture['tool'],'arguments':capture['arguments']})
                got.append({'query':capture,'http':raw,'equal':answer(raw)==capture['text']})
            write(OUT/'http/parity.json',json.dumps(got,indent=2))
            check('all combined image/AWS HTTP answers byte-equal to stdio',all(r['equal'] for r in got))
        finally:
            proc.terminate()
            proc.wait(timeout=30)
            write(OUT/'http/stopped',str(proc.returncode))
    failures = sum(not r['pass'] for r in rows)
    report = '# Combined image/AWS proof\n\n' + '\n'.join(
        f"- [{'PASS' if r['pass'] else 'FAIL'}] {r['check']}" for r in rows) + '\n'
    write(OUT/'mixed.md', report)
    print(f"{'differences found' if failures else 'checks passed'}: {len(rows)-failures}/{len(rows)}; {OUT}/mixed.md")
    return int(bool(failures))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base-bin', required=True, help='AWS-capable baseline, already supporting #658')
    parser.add_argument('--head-bin', required=True)
    parser.add_argument('--run', required=True, help='new evidence directory; never overwrite an earlier run')
    args = parser.parse_args()
    try:
        raise SystemExit(main(args))
    except Exception as exc:
        if OUT is not None:
            write(OUT/'blocked.txt', repr(exc))
        print(f'BLOCKED: {exc}; raw evidence in {OUT}')
        raise SystemExit(2)
