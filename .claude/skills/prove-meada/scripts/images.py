#!/usr/bin/env python3
"""Independent #662 runtime probes. Keys: #662/#642 and base ADRs 0025/0026.

No product readers or feature fixtures are imported. Every generated input, raw command,
MCP result and complete export is retained. Run after independently building exact base/head.
"""
import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import time
import difflib
import re
import yaml
from console import http, query_equal
from prove import Http, Stdio, answer, free_port, git, fetch, infra

PREFIX = 'example.com/image-proof/'


def boutique_non_image_key(source, base):
    """Source-authorized A7 additions after the pinned pre-image binary.

    Keep every old consumer and process field. #668 adds two unresolved Secret
    Manager reads; #667 adds one literal GCS provisioner; #661/ADR 0026 retain
    the AWS meaning of the generic final key `bucket`, even in a GCS backend.
    """
    repository = 'github.com/googlecloudplatform/microservices-demo'
    calls = [
        ('src/cartservice/src/cartstore/AlloyDBCartStore.cs',
         'cartservice.cartstore.AlloyDBCartStore.AlloyDBCartStore(IConfiguration)',
         ['using Google.Cloud.SecretManager.V1;',
          'SecretManagerServiceClient client = SecretManagerServiceClient.Create();',
          'new SecretVersionName(projectId, secretId, "latest")',
          'client.AccessSecretVersion(secretVersionName)']),
        ('src/productcatalogservice/catalog_loader.go', 'getSecretPayload',
         ['secretmanager "cloud.google.com/go/secretmanager/apiv1"',
          'client, err := secretmanager.NewClient(ctx)',
          'Name: fmt.Sprintf("projects/%s/secrets/%s/versions/%s", project, secret, version)',
          'client.AccessSecretVersion(ctx, req)']),
    ]
    consumers = []
    for file, symbol, witnesses in calls:
        text = (source/file).read_text()
        if not all(text.count(witness) == 1 for witness in witnesses):
            raise ValueError('pinned Boutique SDK source changed: '+file)
        consumers.append({'contract': 'resource::gcpsecretmanager',
                          'site': f'{repository}::{file}::{symbol}::client.AccessSecretVersion#0',
                          'via': None})
    processes = copy.deepcopy(base['processes'])
    by_parent = {row['site'].rsplit('::', 1)[0]: row for row in consumers}
    for process in processes:
        steps = []
        for step in process['steps']:
            steps.append(step)
            if step['node'] in by_parent:
                side = by_parent[step['node']]
                steps.append({'depth': step['depth']+1, 'repository': repository,
                              'level': None, 'via': 'call', 'node': side['site'],
                              'mark': 'consumes', 'contract': side['contract']})
        process['steps'] = steps
    terraform = (source/'.github/terraform/main.tf').read_text()
    if not re.search(r'backend\s+"gcs"\s*\{\s*bucket\s*=\s*"cicd-terraform-state"', terraform) \
            or not re.search(r'resource\s+"google_storage_bucket"\s+"terraform_state_storage_bucket"\s*\{\s*name\s*=\s*"cicd-terraform-state"', terraform):
        raise ValueError('pinned Boutique Terraform source changed')
    return {
        'consumers': sorted(base['consumers']+consumers, key=lambda row: row['site']),
        'processes': processes,
        'provisioners': [{'contract': 'resource::gcs::cicd-terraform-state',
                          'node': repository+'::.github/terraform/main.tf::google_storage_bucket.terraform_state_storage_bucket',
                          'root': '.github/terraform'}],
        'uses': [{'contract': 'resource::s3::cicd-terraform-state', 'site': repository,
                  'at': '.github/terraform/main.tf#terraform.backend.gcs.bucket'}],
    }


def write(root, path, text):
    p = root / path
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(text)


def seed(root):
    names = ['app', 'deploy', 'direct', 'builder-only', 'runner-only', 'code-only', 'pattern-app', 'pattern-deploy', 'decision-app', 'decision-deploy']
    repos = {}
    for name in names:
        p = root / name
        p.mkdir(parents=True)
        git('init', '-q', cwd=p)
        git('remote', 'add', 'origin', f'https://{PREFIX}{name}.git', cwd=p)
        repos[name] = p
        symbol = 'parcel_entry' if name == 'app' else 'entry_'+name.replace('-', '_')
        write(p, 'app.py', f'def {symbol}():\n    return "parcel"\n')
    for name, peer in [('app', 'deploy'), ('deploy', 'app'), ('pattern-app','pattern-deploy'), ('pattern-deploy','pattern-app'), ('decision-app','decision-deploy'), ('decision-deploy','decision-app')]:
        write(repos[name], 'meada.toml', f'[[peer]]\nrepo = "{PREFIX}{peer}"\n')
    write(repos['app'], '.github/workflows/publish.yml', '''name: images
on: push
jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - uses: docker/build-push-action@v6
        with:
          push: true
          tags: |
            GHCR.IO/Parcel/Worker:release
            alpine:3.21
            Parcel/Api:latest
            registry.example:5000/parcel/tool:one@sha256:abcdef
            ghcr.io/parcel/idle:one
            ghcr.io/parcel/init:one
      - uses: docker/metadata-action@v5
        with:
          images: name=ghcr.io/parcel/meta,enable=true
      - run: ko build --bare ./cmd/worker
        env:
          KO_DOCKER_REPO: ghcr.io/parcel/ko
''')
    write(repos['app'], 'skaffold.yaml', '''apiVersion: skaffold/v4beta11
kind: Config
build:
  artifacts:
    - image: ghcr.io/parcel/skaffold
''')
    write(repos['app'], '.ko.yaml', 'defaultBaseImage: ghcr.io/parcel/not-a-builder\n')
    write(repos['app'], 'pom.xml', '''<project><modelVersion>4.0.0</modelVersion>
<groupId>parcel</groupId><artifactId>probe</artifactId><version>1</version>
<build><plugins><plugin><groupId>com.google.cloud.tools</groupId>
<artifactId>jib-maven-plugin</artifactId><configuration><to>
<image>ghcr.io/parcel/jib-maven:one</image></to></configuration></plugin></plugins></build></project>
''')
    write(repos['app'], 'build.gradle', '''plugins { id 'com.google.cloud.tools.jib' version '3.4.5' }
jib { to { image = 'ghcr.io/parcel/jib-gradle:one' } }
''')
    images = ['ghcr.io/parcel/worker@sha256:abcdef', 'docker.io/library/alpine:latest',
              'docker.io/parcel/api:next', 'registry.example:5000/parcel/tool:two',
              'ghcr.io/parcel/meta:next', 'ghcr.io/parcel/skaffold:next',
              'ghcr.io/parcel/ko:next', 'ghcr.io/parcel/jib-maven:next',
              'ghcr.io/parcel/jib-gradle:next', 'ghcr.io/parcel/lost:next']
    text = 'apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: parcel\n  namespace: proof\nspec:\n  template:\n    spec:\n      containers:\n'
    for i, image in enumerate(images):
        text += f'        - name: image-{i}\n          image: {image}\n'
    text += '      initContainers:\n        - name: init\n          image: ghcr.io/parcel/init:one\n'
    write(repos['deploy'], 'deployment.yaml', text)
    write(repos['deploy'], 'values.yaml', 'image:\n  repository: ghcr.io/parcel/worker\n  tag: next\n')
    write(repos['deploy'], 'compose.yaml', '''services:
  literal:
    image: ghcr.io/parcel/worker:one
  defaulted:
    image: ${REGISTRY:-ghcr.io}/parcel/worker:two
''')
    write(repos['deploy'], 'base/pod.yaml', '''apiVersion: v1
kind: Pod
metadata:
  name: overlay
spec:
  containers:
    - name: app
      image: old.example/placeholder:old
''')
    write(repos['deploy'], 'overlays/prod/kustomization.yaml', '''apiVersion: kustomize.config.k8s.io/v1beta1
kind: Kustomization
resources:
  - ../../base/pod.yaml
namePrefix: prod-
images:
  - name: old.example/placeholder
    newName: ghcr.io/parcel/worker
    newTag: three
''')
    write(repos['direct'], 'compose.yaml', '''services:
  worker:
    build: .
    image: ghcr.io/parcel/direct:one
  noimage:
    build:
      context: src
  outside:
    build: ../app
  absolute:
    build: /tmp
  linked:
    build: linked-src
''')
    write(repos['direct'], 'src/local.py', 'def inside_entry():\n    return 1\n')
    # ADR 0026 (#755, #782): `COPY . .` keeps worker's whole context at `exact`; `src` holds no
    # Dockerfile, so noimage runs its context at `inferred` and the reason says it was not read.
    write(repos['direct'], 'Dockerfile', 'FROM scratch\nCOPY . .\n')
    (repos['direct'] / 'linked-src').symlink_to('src', target_is_directory=True)
    write(repos['builder-only'], '.github/workflows/publish.yml', '''jobs:
  build:
    steps:
      - uses: docker/build-push-action@v6
        with:
          tags: ghcr.io/parcel/alone:one
''')
    write(repos['runner-only'], 'compose.yaml', 'services:\n  alone:\n    image: ghcr.io/parcel/alone:one\n')
    write(repos['pattern-app'], '.github/workflows/publish.yml', '''jobs:
  build:
    steps:
      - uses: docker/build-push-action@v6
        with:
          tags: |
            ghcr.io/probe-many/worker-east:one
            ghcr.io/probe-many/worker-west:one
            ghcr.io/probe-one/worker-east:one
            ghcr.io/probe-build/worker-${{ env.REGION }}:one
            ghcr.io/probe-both/worker-${{ env.REGION }}:one
            ghcr.io/probe-bad/worker:one
            busybox:one
''')
    write(repos['pattern-deploy'], 'compose.yaml', '''services:
  many:
    image: ghcr.io/probe-many/worker-${STAGE}:one
  single:
    image: ghcr.io/probe-one/worker-${STAGE}:one
  builtpattern:
    image: ghcr.io/probe-build/worker-east:one
  both:
    image: ghcr.io/probe-both/worker-${STAGE}:one
  interior:
    image: ghcr.io/probe-bad/wor${STAGE}ker:one
  whole:
    image: ${IMAGE}
''')
    write(repos['decision-app'],'.github/workflows/publish.yml','''jobs:
  publish:
    steps:
      - uses: docker/build-push-action@v6
        with:
          tags: |
            ghcr.io/decision-probe/app-east:one
            ghcr.io/decision-probe/app-west:one
            ghcr.io/decision-probe/init-east:one
            ghcr.io/decision-probe/init-west:one
''')
    write(repos['decision-deploy'],'dual.yaml','''apiVersion: v1
kind: Pod
metadata:
  name: dual
spec:
  containers:
    - name: app
      image: ghcr.io/decision-probe/app-${REGION}:one
  initContainers:
    - name: init
      image: ghcr.io/decision-probe/init-${REGION}:one
''')
    write(repos['decision-deploy'],'shared/pod.yaml','''apiVersion: v1
kind: Pod
metadata:
  name: shared
spec:
  containers:
    - name: app
      image: ghcr.io/decision-probe/base:one
''')
    for region,image in [('east','app'),('west','init')]:
        write(repos['decision-deploy'],f'overlays/{region}/kustomization.yaml',f'''resources:
  - ../../shared/pod.yaml
images:
  - name: ghcr.io/decision-probe/base
    newName: ghcr.io/decision-probe/{image}-${{REGION}}
''')
    for p in repos.values():
        git('add', '-A', cwd=p)
    return repos


class Probe:
    def __init__(self, args):
        self.run = Path(args.run).resolve()
        self.run.mkdir(parents=True, exist_ok=False)
        self.base, self.head = [str(Path(p).resolve()) for p in [args.base_bin, args.head_bin]]
        self.http = args.http
        self.ui_bin = str(Path(args.ui_bin).resolve()) if args.ui_bin else None
        self.ui_dir = str(Path(args.ui_dir).resolve()) if args.ui_dir else None
        self.public = args.public
        self.steps = []

    def check(self, label, ok, detail=''):
        self.steps.append({'check': label, 'pass': bool(ok), 'detail': detail})
        write(self.run, 'checks.json', json.dumps(self.steps, indent=2))

    def command(self, binary, cwd, args, label):
        p = subprocess.run([binary, *args], cwd=cwd, capture_output=True, text=True, timeout=180)
        for suffix, value in [('out', p.stdout), ('err', p.stderr), ('exit', str(p.returncode))]:
            write(self.run, f'{label}.{suffix}', value)
        if p.returncode:
            raise RuntimeError(f'{label}: exit {p.returncode}; see stderr capture')
        return p.stdout

    def queries(self, name):
        q = []
        def add(tool, **args):
            q.append({'tool': tool, 'arguments': dict(args, budget=200000)})
        add('search', query='image::', kind='contract')
        add('search', query='image::', kind='image')
        add('context', symbol='image::ghcr.io/parcel/worker')
        add('search', query='zz_image_missing', kind='contract')
        add('search', query='parcel_entry', kind='symbol')
        add('context', symbol='parcel_entry')
        if name in ['app', 'deploy']:
            add('context', symbol='Deployment.apps/proof/parcel')
            add('trace', **{'from': 'Deployment.apps/proof/parcel', 'direction': 'down'})
            add('trace', **{'from': 'parcel_entry', 'direction': 'up'})
            add('context', symbol=f'{PREFIX}deploy::values.yaml::image.repository')
            add('context', symbol='services.defaulted')
        if name == 'direct':
            for value in ['worker', 'noimage', 'outside', 'absolute', 'linked']:
                add('context', symbol=f'services.{value}')
            add('trace', **{'from': 'services.noimage', 'direction': 'down'})
            add('trace', **{'from': 'inside_entry', 'direction': 'up'})
        if name in ['builder-only', 'runner-only']:
            add('context', symbol='image::ghcr.io/parcel/alone')
        if name.startswith('pattern-'):
            for value in ['many','single','builtpattern','both','interior','whole']:
                add('context', symbol=f'services.{value}')
            add('search',query='many',kind='ambiguous')
        if name.startswith('decision-'):
            add('search',query='decision-probe',kind='ambiguous')
            for root in ['east','west']:
                add('context',symbol=f'Pod/shared (root overlays/{root})')
                add('trace',**{'from':f'Pod/shared (root overlays/{root})','direction':'down'})
        return q

    def stdio(self, repo, qs, label):
        client = Stdio(self.head, repo, self.run / f'{label}.err')
        try:
            raw = [client.call('tools/call', {'name': q['tool'], 'arguments': q['arguments']}) for q in qs]
            tools = client.call('tools/list', {})['result']['tools']
            texts = [answer(r) for r in raw]
            write(self.run, label+'.json', json.dumps({'queries': qs, 'answers': texts, 'raw': raw, 'tools': tools}, indent=2))
            return tools, texts
        finally:
            client.close()

    def transport(self, members, name, qs, tools, texts):
        folder = self.run / 'http' / name
        folder.mkdir(parents=True)
        manifest = 'index_dir = "indexes"\n'
        for n, p in members.items():
            manifest += f'\n[[repository]]\nrepo = "{PREFIX}{n}"\npath = "{p}"\n'
        write(folder, 'workspace.toml', manifest)
        self.command(self.head, folder, ['index', '--workspace', 'workspace.toml'], f'http/{name}/index')
        same = self.command(self.head, folder, ['export', '--workspace', 'workspace.toml', '--repo', PREFIX+name], f'http/{name}/export')
        self.check(name+' server export contains new optional fields', name not in ['deploy', 'direct'] or bool(json.loads(same).get('iac_nodes')))
        port = free_port()
        with open(folder / 'server.log', 'w') as log:
            process = subprocess.Popen([self.head, 'mcp', '--listen', f'127.0.0.1:{port}', '--workspace', 'workspace.toml'], cwd=folder, stdout=log, stderr=log)
            try:
                for _ in range(100):
                    if process.poll() is not None:
                        raise RuntimeError(f'HTTP {name} exited early')
                    try:
                        socket.create_connection(('127.0.0.1', port), timeout=.2).close()
                        break
                    except OSError:
                        time.sleep(.1)
                client = Http(f'http://127.0.0.1:{port}/{PREFIX}{name}')
                got_tools = client.call('tools/list', {})['result']['tools']
                raw = [client.call('tools/call', {'name': q['tool'], 'arguments': q['arguments']}) for q in qs]
                got = [answer(r) for r in raw]
                write(folder, 'answers.json', json.dumps({'raw': raw, 'answers': got}, indent=2))
                self.check(name+' HTTP complete text and tool parity', got == texts and got_tools == tools)
            finally:
                process.terminate()
                process.wait(timeout=30)

    def execute(self):
        write(self.run,'synthetic-process-env.json',json.dumps({k:os.environ[k] for k in ['REGISTRY','KO_DOCKER_REPO','STAGE','IMAGE']},indent=2))
        src = seed(self.run / 'input')
        inventory = {str(p.relative_to(self.run/'input')): hashlib.sha256(p.read_bytes()).hexdigest()
                     for p in sorted((self.run/'input').rglob('*')) if p.is_file() and '.git' not in p.parts}
        write(self.run, 'input.sha256.json', json.dumps(inventory, indent=2))
        heads = {}
        exports = {}
        for side, binary in [('base', self.base), ('head', self.head)]:
            root = self.run/'work'/side
            shutil.copytree(self.run/'input', root, symlinks=True)
            for name in src:
                p = root/name
                exports[side, name] = json.loads(self.command(binary, p, ['export'], f'{side}/{name}/export'))
                self.command(binary, p, ['status'], f'{side}/{name}/status')
                if side == 'head':
                    heads[name] = p
        app, deploy, direct = [exports['head', n] for n in ['app', 'deploy', 'direct']]
        for name in src:
            base,head=exports['base',name],exports['head',name]
            for key in ['providers','consumers','links']:
                existing=[row for row in head[key] if not str(row.get('contract') or '').startswith('image::')]
                self.check(f'{name} existing {key} whole-row base parity',existing==base[key])
            for key in ['files','symbols','calls']:
                self.check(f'{name} existing {key} full-field base parity',head[key]==base[key])
        expected = {'ghcr.io/parcel/'+n for n in ['worker', 'meta', 'skaffold', 'ko', 'jib-maven', 'jib-gradle', 'idle', 'init']}
        expected |= {'docker.io/library/alpine', 'docker.io/parcel/api', 'registry.example:5000/parcel/tool'}
        got = {p['contract'].removeprefix('image::') for p in app['providers'] if p['contract'].startswith('image::')}
        self.check('all named builders, normalization and no base-image false builder', got == expected, repr(sorted(got)))
        links = [l for l in deploy['links'] if l['contract'].startswith('image::')]
        self.check('literal image links identify repository builder', bool(links) and all(l['provider'] == PREFIX+'app' for l in links))
        for image in expected - {'ghcr.io/parcel/idle'}:
            self.check('runner links '+image, any(l['contract'] == 'image::'+image for l in links))
        helm = [l for l in links if 'values.yaml' in l['consumer']]
        default = [l for l in links if 'services.defaulted' in l['consumer']]
        self.check('Helm evaluation inferred with key reason', bool(helm) and all(l['level']=='inferred' and 'image.repository' in l['reason'] for l in helm))
        self.check('Compose default inferred with key reason', bool(default) and all(l['level']=='inferred' and 'compose.yaml' in l['reason'] for l in default))
        self.check('Kubernetes containers literal exact', any('Deployment.apps/proof/parcel' in l['consumer'] and l['level']=='exact' for l in links))
        self.check('Kustomize overridden workload linked', any('prod-overlay' in l['consumer'] and l['contract']=='image::ghcr.io/parcel/worker' for l in links))
        executions = direct.get('executions', [])
        self.check('direct build root exact through its Dockerfile, directory inferred without one', {(x['workload'].split('::')[-1],x['path'],x['level']) for x in executions} == {('services.worker','.','exact'),('services.noimage','src','inferred')}, repr(executions))
        self.check('direct directory reason says its Dockerfile was not read', any(x['path']=='src' for x in executions) and all('Dockerfile' in x.get('reason','') and 'not read' in x.get('reason','') for x in executions if x['path']=='src'), repr(executions))
        self.check('direct code path uses no image link', not direct['links'])
        self.check('code-only entire export unchanged', exports['base','code-only'] == exports['head','code-only'])
        patterns = exports['head','pattern-deploy']['links']
        def for_service(name):
            return [l for l in patterns if l['consumer'].endswith('::services.'+name)]
        many = for_service('many')
        self.check('runner pattern lists both ambiguous literal candidates',len(many)==2 and all(l['level']=='ambiguous' for l in many),repr(many))
        for name in ['single','builtpattern']:
            rows = for_service(name)
            self.check(name+' pattern match inferred with reason',len(rows)==1 and rows[0]['level']=='inferred' and bool(rows[0]['reason']),repr(rows))
        for name in ['both','interior','whole']:
            self.check(name+' unsafe pattern shape links nothing',not for_service(name),repr(for_service(name)))
        whole=[c for c in exports['head','pattern-deploy']['consumers'] if c['site'].endswith('::services.whole')]
        self.check('wholly unresolved image gains no synthetic Docker Hub literal evidence',
                   not any(str(c.get('contract') or '').startswith('image::docker.io/library/') for c in whole),repr(whole))
        for name in ['app', 'deploy']:
            status = (self.run/f'head/{name}/status.out').read_text()
            rows = [r.strip() for r in status.splitlines() if r.strip().startswith('orphan image::')]
            got = {r.split(';')[0] for r in rows}
            # CLI status reports every repository of the discovered workspace.
            expected_orphans = {'orphan image::ghcr.io/parcel/idle: no runner',
                                'orphan image::ghcr.io/parcel/lost: no builder'}
            self.check(name+' exact mixed-scope image orphan set', got == expected_orphans and len(rows)==len(got), repr(rows))
        records = {}
        for name in src:
            qs = self.queries(name)
            tools, texts = self.stdio(heads[name], qs, f'head/{name}/stdio')
            records[name] = (qs, texts)
            if name in ['app', 'deploy']:
                context = texts[6]
                self.check(name+' workload context names image and builder', 'image::ghcr.io/parcel/worker' in context and PREFIX+'app' in context, context)
                self.check(name+' trace down reaches code', PREFIX+'app' in texts[7] and 'app.py' in texts[7], texts[7])
                self.check(name+' trace up reaches workload', 'Deployment.apps/proof/parcel' in texts[8], texts[8])
            if name=='direct':
                self.check('direct root workload context identifies exact local code',
                           f'code: {PREFIX}direct . [exact]' in texts[6],texts[6])
                self.check('direct directory workload context identifies inferred local code',
                           f'code: {PREFIX}direct src [inferred]' in texts[7],texts[7])
                self.check('direct trace down reaches directory code','src/local.py' in texts[11] and 'inside_entry' in texts[11],texts[11])
                self.check('direct trace up reaches workload','services.noimage' in texts[12],texts[12])
            if name in ['builder-only', 'runner-only']:
                marker = 'no runner in the workspace' if name == 'builder-only' else 'no builder in the workspace'
                self.check(name+' scope absence context marker', marker in texts[-1], texts[-1])
                self.check(name+' scope absence search marker', marker in texts[0], texts[0])
                status = (self.run/f'head/{name}/status.out').read_text()
                self.check(name+' scope suppresses orphan status', 'orphan image::' not in status, status)
            if name.startswith('pattern-'):
                many_context = texts[6]
                self.check(name+' ambiguous image candidates visible in context',
                           all(x in many_context for x in ['ambiguous','worker-east','worker-west']),many_context)
                self.check(name+' ambiguous image open item candidates visible in search',
                           'candidates: 2' in texts[-1] and '--item ' in texts[-1],texts[-1])
            if name.startswith('decision-'):
                for index,image,other in [(7,'app-','init-'),(9,'init-','app-')]:
                    self.check(name+' root-specific context '+image,image in texts[index] and other not in texts[index],texts[index])
                    self.check(name+' root-specific trace '+image,image in texts[index+1] and other not in texts[index+1],texts[index+1])
            if self.http:
                group = ['app','deploy'] if name in ['app','deploy'] else ['pattern-app','pattern-deploy'] if name.startswith('pattern-') else ['decision-app','decision-deploy'] if name.startswith('decision-') else [name]
                members = {n:heads[n] for n in group}
                self.transport(members, name, qs, tools, texts)
        self.filtered_scope(heads)
        self.decisions(heads,records)
        if self.ui_bin:
            self.console(heads, records, ['app','deploy'])
            self.console(heads, records, ['direct'])
        # Same-byte-length image edit must refresh image facts in a warm index.
        p = heads['deploy']/'compose.yaml'
        original = p.read_text()
        (self.run/'refresh').mkdir(exist_ok=True)
        client=Stdio(self.head,heads['deploy'],self.run/'refresh/persistent.err')
        def current():
            return client.call('tools/call',{'name':'context','arguments':{'symbol':'services.literal','budget':200000}})
        try:
            before=current()
            p.write_text(original.replace('worker:one','absent:one'))
            after=current()
            warm = json.loads(self.command(self.head, heads['deploy'], ['export'], 'refresh/warm'))
            self.check('same-size image content refresh', any(c.get('contract')=='image::ghcr.io/parcel/absent' for c in warm['consumers']))
            p.write_text(original)
            back=current()
            write(self.run,'refresh/persistent.json',json.dumps({'before':before,'after':after,'restored':back},indent=2))
            self.check('persistent MCP sees edited image and restoration',
                       'image::ghcr.io/parcel/worker' in answer(before) and 'image::ghcr.io/parcel/absent' in answer(after) and answer(back)==answer(before))
            restored = json.loads(self.command(self.head, heads['deploy'], ['export'], 'refresh/restored'))
            self.check('image content restore complete export', restored == deploy)
        finally:
            p.write_text(original)
            client.close()
        if self.public:
            self.public_boutique()

    def decisions(self,heads,records):
        """Select observable open-item ids, never an implementation identity helper."""
        repo=heads['decision-deploy']
        before=json.loads((self.run/'head/decision-deploy/export.out').read_text())
        self.check('decision scenario has exactly eight ambiguous image links',
                   len(before['links'])==8 and all(l['level']=='ambiguous' and l['contract'].startswith('image::') for l in before['links']),repr(before['links']))
        qs,texts=records['decision-deploy']
        ids=re.findall(r'run `meada decide --item (.*?)`',texts[6])
        self.check('four independent two-candidate image open items',len(ids)==4 and len(set(ids))==4 and texts[6].count('candidates: 2')==4,texts[6])
        selected=[i for i in ids if 'dual.yaml' in i and '/app-' in i]
        if len(selected)!=1:
            self.check('observable app container decision identity is unique',False,repr(selected))
            return
        def decision(item,image):
            return ('[[decision]]\nkind = "ambiguous-link"\ncall = '+json.dumps(item)+
                    '\ntarget = '+json.dumps(PREFIX+'decision-app::image::ghcr.io/decision-probe/'+image)+'\nby = "human"\n')
        path=repo/'meada.decisions.toml'
        try:
            path.write_text(decision(selected[0],'app-east'))
            write(self.run,'decisions/container-choice.toml',path.read_text())
            after=json.loads(self.command(self.head,repo,['export'],'decisions/container-selected'))
            self.check('container choice leaves exactly seven complete links',len(after['links'])==7,repr(after['links']))
            dual=lambda data:[l for l in data['links'] if 'dual.yaml' in l['consumer']]
            initial_init=[l for l in dual(before) if '/init-' in l['contract']]
            current_init=[l for l in dual(after) if '/init-' in l['contract']]
            chosen=[l for l in dual(after) if '/app-' in l['contract']]
            self.check('deciding app leaves independent init-container candidates unchanged',len(initial_init)==2 and current_init==initial_init,repr(current_init))
            # Base decision semantics retain the contract the consumer uses, even when
            # the manual target names a literal candidate of its pattern.
            self.check('selected app candidate alone becomes decided',len(chosen)==1 and chosen[0]['level']=='decided' and chosen[0]['contract']=='image::ghcr.io/decision-probe/app-*' and chosen[0]['provider']==PREFIX+'decision-app' and chosen[0].get('by')=='human' and not after.get('stale_decisions'),repr(chosen))
            original_roots=[l for l in before['links'] if 'shared/pod.yaml' in l['consumer']]
            retained_roots=[l for l in after['links'] if 'shared/pod.yaml' in l['consumer']]
            self.check('container decision preserves both overlay candidate sets',len(original_roots)==4 and retained_roots==original_roots)
            _,current=self.stdio(repo,qs,'decisions/after-container-stdio')
            remaining=re.findall(r'run `meada decide --item (.*?)`',current[6])
            self.check('only selected container open item closes',len(remaining)==3 and selected[0] not in remaining,current[6])
            east=[i for i in remaining if 'shared/pod.yaml' in i and 'root overlays/east' in i]
            if len(east)!=1:
                self.check('observable east overlay decision identity is unique',False,repr(east))
                return
            path.write_text(decision(selected[0],'app-east')+'\n'+decision(east[0],'app-west'))
            write(self.run,'decisions/overlay-choice.toml',path.read_text())
            roots_after=json.loads(self.command(self.head,repo,['export'],'decisions/overlay-selected'))
            self.check('two choices leave exactly six complete links',len(roots_after['links'])==6,repr(roots_after['links']))
            west=lambda data:[l for l in data['links'] if 'shared/pod.yaml' in l['consumer'] and l.get('root')=='overlays/west']
            east_links=[l for l in roots_after['links'] if 'shared/pod.yaml' in l['consumer'] and l.get('root')=='overlays/east']
            self.check('deciding one overlay preserves other root complete candidates',len(west(before))==2 and west(roots_after)==west(before),repr(west(roots_after)))
            self.check('selected overlay root retained on decided link',len(east_links)==1 and east_links[0]['level']=='decided' and east_links[0]['contract']=='image::ghcr.io/decision-probe/app-*' and east_links[0]['provider']==PREFIX+'decision-app' and east_links[0].get('by')=='human' and not roots_after.get('stale_decisions'),repr(east_links))
            self.check('overlay decision also preserves independent init candidates',[l for l in dual(roots_after) if '/init-' in l['contract']]==initial_init)
            _,final=self.stdio(repo,qs,'decisions/after-overlay-stdio')
            final_ids=re.findall(r'run `meada decide --item (.*?)`',final[6])
            self.check('only the two selected image open items close',len(final_ids)==2 and all(i not in final_ids for i in [selected[0],east[0]]),final[6])
        finally:
            path.unlink(missing_ok=True)
            restored=json.loads(self.command(self.head,repo,['export'],'decisions/restored'))
            self.check('removing manual decisions restores complete original export',restored==before)

    def public_boutique(self):
        """Pinned real-world answer key read from source, never a Meada output."""
        source = self.run/'work'/'src'/'boutique'
        fetch('boutique',source)
        pin = infra()['boutique'][2]
        self.check('boutique pinned source SHA',git('rev-parse','HEAD',cwd=source).strip()==pin)
        docs = list(yaml.safe_load_all((source/'skaffold.yaml').read_text()))
        artifacts = [a['image'] for doc in docs for a in doc['build']['artifacts']]
        # All twelve source spellings at this pin are single-component short names.
        assert all('/' not in a and ':' not in a for a in artifacts)
        expected = {'image::docker.io/library/'+a for a in artifacts}
        write(self.run,'boutique/source-key.json',json.dumps({'pin':pin,'artifacts':artifacts,'expected_builders':sorted(expected)},indent=2))
        exports = {}
        repos = {}
        for side,binary in [('base',self.base),('head',self.head)]:
            repo = self.run/'work'/side/'boutique'
            shutil.copytree(source,repo,symlinks=True)
            repos[side]=repo
            exports[side]=json.loads(self.command(binary,repo,['export'],f'boutique/{side}/export'))
            self.command(binary,repo,['status'],f'boutique/{side}/status')
        head,base=exports['head'],exports['base']
        providers=[p for p in head['providers'] if p['contract'].startswith('image::')]
        self.check('boutique both Skaffold documents publish all twelve images',{p['contract'] for p in providers}==expected,repr(providers))
        links=[l for l in head['links'] if l['contract'].startswith('image::')]
        self.check('boutique literal short-image workloads link exact to own repository',bool(links) and all(l['level']=='exact' and l['provider']==head['repository'] and l['contract'] in expected for l in links))
        self.check('boutique no short-name match to unrelated registry',not any(l['contract'].startswith('image::us-central1-docker.pkg.dev/') for l in links))
        self.check('boutique plural Helm images key is not singular repository runner',not any(str(c.get('contract') or '').startswith('image::') and 'helm-chart/values.yaml' in c['site'] for c in head['consumers']))
        self.check('boutique busybox init image retained as runner',any(c.get('contract')=='image::docker.io/library/busybox' for c in head['consumers']))
        # Full original exports remain attached; inspect every top-level delta, including
        # unknown fields, rather than letting the generic comparator silently omit fields.
        allowed={'providers','consumers','links','iac_nodes','executions','processes','provisioners','uses'}
        changes={k:{'base':base.get(k),'head':head.get(k)} for k in sorted(set(base)|set(head)) if base.get(k)!=head.get(k)}
        write(self.run,'boutique/full-field-deltas.json',json.dumps(changes,indent=2))
        raw_base=(self.run/'boutique/base/export.out').read_text()
        raw_head=(self.run/'boutique/head/export.out').read_text()
        write(self.run,'boutique/export.diff',''.join(difflib.unified_diff(raw_base.splitlines(True),raw_head.splitlines(True),fromfile='base',tofile='head')))
        self.check('boutique unchanged whole fields outside new image model',not(set(changes)-allowed),repr(sorted(changes)))
        for key in ['providers','links']:
            old=[r for r in head[key] if not str(r.get('contract','')).startswith('image::')]
            self.check('boutique all existing '+key+' rows unchanged',old==base[key])
        non_image = boutique_non_image_key(source, base)
        write(self.run,'boutique/non-image-source-key.json',json.dumps(non_image,indent=2))
        for key, expected_rows in non_image.items():
            actual = head.get(key)
            if key == 'consumers':
                actual = [r for r in actual if not str(r.get('contract','')).startswith('image::')]
            self.check('boutique complete '+key+' equals base plus source-authorized A7 additions',actual==expected_rows)
        frontend=head['repository']+'::kubernetes-manifests/frontend.yaml::Deployment.apps/frontend'
        qs=[{'tool':'context','arguments':{'symbol':frontend,'budget':200000}},
            {'tool':'trace','arguments':{'from':frontend,'direction':'down','budget':200000}},
            {'tool':'search','arguments':{'query':'image::docker.io/library/loadgenerator','kind':'contract','budget':200000}}]
        tools,texts=self.stdio(repos['head'],qs,'boutique/head/stdio')
        self.check('boutique frontend context carries literal image and builder','image::docker.io/library/frontend' in texts[0] and head['repository'] in texts[0],texts[0])
        self.check('boutique frontend trace reaches actual code','src/frontend/' in texts[1],texts[1][:2000])
        if self.http:
            # HTTP helper has a neutral identity prefix; use a dedicated same-scope
            # manifest with the public identity, preserving every byte of each answer.
            self.public_http(repos['head'],head['repository'],qs,tools,texts)

    def public_http(self,repo,identity,qs,tools,texts):
        folder=self.run/'boutique/http'
        folder.mkdir()
        write(folder,'workspace.toml',f'index_dir = "indexes"\n\n[[repository]]\nrepo = "{identity}"\npath = "{repo}"\n')
        self.command(self.head,folder,['index','--workspace','workspace.toml'],'boutique/http/index')
        port=free_port()
        with open(folder/'server.log','w') as log:
            proc=subprocess.Popen([self.head,'mcp','--listen',f'127.0.0.1:{port}','--workspace','workspace.toml'],cwd=folder,stdout=log,stderr=log)
            try:
                for _ in range(100):
                    if proc.poll() is not None: raise RuntimeError('boutique HTTP exited early')
                    try:
                        socket.create_connection(('127.0.0.1',port),timeout=.2).close()
                        break
                    except OSError: time.sleep(.1)
                client=Http(f'http://127.0.0.1:{port}/{identity}')
                raw=[client.call('tools/call',{'name':q['tool'],'arguments':q['arguments']}) for q in qs]
                write(folder,'answers.json',json.dumps(raw,indent=2))
                self.check('boutique all HTTP answers byte-equal to stdio',[answer(r) for r in raw]==texts and client.call('tools/list',{})['result']['tools']==tools)
            finally:
                proc.terminate()
                proc.wait(timeout=30)

    def filtered_scope(self, heads):
        folder = self.run/'filtered-scope'
        folder.mkdir()
        manifest = f'index_dir = "indexes"\n\n[[repository]]\nrepo = "{PREFIX}deploy"\npath = "{heads["deploy"]}"\n'
        write(folder,'workspace.toml',manifest)
        self.command(self.head,folder,['index','--workspace','workspace.toml'],'filtered-scope/index')
        exp = json.loads(self.command(self.head,folder,['export','--workspace','workspace.toml','--repo',PREFIX+'deploy'],'filtered-scope/export'))
        self.check('server filtered workspace contains no hidden builder links',not exp['links'] and PREFIX+'app' not in json.dumps(exp))
        # A same-repository peer configuration must never expand server-mode grants.
        if self.http:
            port = free_port()
            with open(folder/'server.log','w') as log:
                proc = subprocess.Popen([self.head,'mcp','--listen',f'127.0.0.1:{port}','--workspace','workspace.toml'],cwd=folder,stdout=log,stderr=log)
                try:
                    for _ in range(100):
                        if proc.poll() is not None:
                            raise RuntimeError('filtered-scope MCP exited early')
                        try:
                            socket.create_connection(('127.0.0.1',port),timeout=.2).close()
                            break
                        except OSError:
                            time.sleep(.1)
                    client = Http(f'http://127.0.0.1:{port}/{PREFIX}deploy')
                    raw = client.call('tools/call',{'name':'context','arguments':{'symbol':'image::ghcr.io/parcel/worker','budget':200000}})
                    write(folder,'context.json',json.dumps(raw,indent=2))
                    text = answer(raw)
                    self.check('server context marks no visible builder without leaking peer', 'no builder in the workspace' in text and PREFIX+'app' not in text,text)
                finally:
                    proc.terminate()
                    proc.wait(timeout=30)

    def console(self, heads, records, names):
        label='console-images-'+names[-1]
        folder = self.run/label
        folder.mkdir()
        port = free_port()
        url = f'http://127.0.0.1:{port}'
        manifest = f'index_dir = "indexes"\n\n[ui]\ndir = "{self.ui_dir}"\npublic_url = "{url}"\n'
        for name in names:
            manifest += f'\n[[repository]]\nrepo = "{PREFIX}{name}"\npath = "{heads[name]}"\n'
        write(folder,'workspace.toml',manifest)
        write(folder,'url',url)
        self.command(self.ui_bin,folder,['index','--workspace','workspace.toml'],label+'/index')
        with open(folder/'server.log','w') as log:
            proc = subprocess.Popen([self.ui_bin,'ui','--listen',f'127.0.0.1:{port}','--workspace','workspace.toml'],cwd=folder,stdout=log,stderr=log)
            try:
                for _ in range(100):
                    if proc.poll() is not None:
                        raise RuntimeError('image console exited early')
                    try:
                        if http(url+'/api/v1/config')[0]==200:
                            break
                    except OSError:
                        pass
                    time.sleep(.1)
                for name in names:
                    qs,texts = records[name]
                    for i,(q,expected) in enumerate(zip(qs,texts)):
                        status,body = http(url+'/api/v1/query','POST',{'repository':PREFIX+name,'generation':None,'tool':q['tool'],'arguments':q['arguments']},{'Origin':url})
                        write(folder,f'{name}/query-{i}.json',json.dumps({'status':status,'body':body.decode(),'expected':expected},indent=2))
                        self.check(f'{name} console query {i} full text/error parity',query_equal(status,json.loads(body),expected))
                    status,body = http(url+f'/api/v1/repositories/{PREFIX}{name}/export')
                    cli = self.command(self.ui_bin,folder,['export','--workspace','workspace.toml','--repo',PREFIX+name],f'{label}/{name}/cli-export')
                    (folder/f'{name}/api-export.json').write_bytes(body)
                    self.check(name+' console full export byte parity with same-mode CLI',status==200 and body==cli.encode())
                browser = subprocess.run(['node',str(Path(__file__).with_name('images-graph.mjs')),str(folder),PREFIX+names[-1]],capture_output=True,text=True,timeout=120)
                write(folder,'graph.out',browser.stdout)
                write(folder,'graph.err',browser.stderr)
                self.check(label+' graph renders image contract or direct-code rows',browser.returncode==0,browser.stdout+browser.stderr)
                if len(names)>1:
                    locations=subprocess.run(['node',str(Path(__file__).with_name('images-graph.mjs')),str(folder),PREFIX+'app'],capture_output=True,text=True,timeout=120)
                    write(folder,'builder-graph.out',locations.stdout)
                    write(folder,'builder-graph.err',locations.stderr)
                    self.check('console builder locations visible',locations.returncode==0,locations.stdout+locations.stderr)
            finally:
                proc.terminate()
                proc.wait(timeout=30)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--base-bin', required=True)
    p.add_argument('--head-bin', required=True)
    p.add_argument('--run', required=True)
    p.add_argument('--http', action='store_true')
    p.add_argument('--ui-bin', help='optional independently built ui-feature head binary')
    p.add_argument('--ui-dir', help='front end built from the frozen head tree')
    p.add_argument('--public', action='store_true', help='also drive pinned boutique and audit image deltas against its source')
    args = p.parse_args()
    if bool(args.ui_bin) != bool(args.ui_dir):
        p.error('--ui-bin and --ui-dir must be supplied together')
    # These deliberately disagree with checked-in values. Never resolve process environment.
    os.environ.update(REGISTRY='runtime.invalid',KO_DOCKER_REPO='runtime.invalid/phantom',STAGE='runtime',IMAGE='busybox')
    probe = Probe(args)
    try:
        probe.execute()
    except Exception as exc:
        write(probe.run, 'blocked.txt', repr(exc))
        print(f'BLOCKED: {exc}; raw evidence in {probe.run}')
        return 2
    failures = [s['check'] for s in probe.steps if not s['pass']]
    report = '# Image contract proof\n\n' + '\n'.join(f"- [{'PASS' if s['pass'] else 'FAIL'}] {s['check']}: {s['detail']}" for s in probe.steps)
    write(probe.run, 'images.md', report+'\n')
    print(f"{'differences found' if failures else 'checks passed'}: {len(probe.steps)-len(failures)}/{len(probe.steps)}; {probe.run}/images.md")
    return bool(failures)


if __name__ == '__main__':
    raise SystemExit(main())
