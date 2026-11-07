#!/usr/bin/env python3
"""Independent #661 public-surface proof. No feature code or feature tests are an oracle.

Source/spec key: #661, #642 stories 42/43/59/63/64, base ADR 0026.
Imports only pre-existing transport mechanics. Exit 0 checks passed, 1 differences, 2 blocked.
"""
import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import socket
import subprocess
import time
import traceback
from prove import Http, Stdio, answer, free_port, git

PREFIX = 'example.com/config-use-proof/'
# Curated AWS kinds in base ADR 0026, with standard structured AWS syntax.
# type, attribute, identity, literal, structured identifier
CASES = [
 ('aws_sqs_queue', 'name', 'channel::sqs', 'parcel', 'arn:aws:sqs:eu-west-2:123456789012:parcel'),
 ('aws_sns_topic', 'name', 'channel::sns', 'notice', 'arn:aws:sns:eu-west-2:123456789012:notice'),
 ('aws_cloudwatch_event_bus', 'name', 'channel::eventbridge', 'audit', 'arn:aws:events:eu-west-2:123456789012:event-bus/audit'),
 ('aws_kinesis_stream', 'name', 'channel::kinesis', 'flow', 'arn:aws:kinesis:eu-west-2:123456789012:stream/flow'),
 ('aws_s3_bucket', 'bucket', 'resource::s3', 'vault', 'arn:aws:s3:::vault'),
 ('aws_dynamodb_table', 'name', 'resource::dynamodb', 'ledger', 'arn:aws:dynamodb:eu-west-2:123456789012:table/ledger'),
 ('aws_db_instance', 'identifier', 'resource::rds', 'store', 'arn:aws:rds:eu-west-2:123456789012:db:store'),
 ('aws_rds_cluster', 'cluster_identifier', 'resource::rds', 'group', 'arn:aws:rds:eu-west-2:123456789012:cluster:group'),
 ('aws_elasticache_cluster', 'cluster_id', 'resource::elasticache', 'cache', 'arn:aws:elasticache:eu-west-2:123456789012:cluster:cache'),
 ('aws_elasticache_replication_group', 'replication_group_id', 'resource::elasticache', 'replica', 'arn:aws:elasticache:eu-west-2:123456789012:replicationgroup:replica'),
 ('aws_ssm_parameter', 'name', 'resource::ssm', '/app/token', 'arn:aws:ssm:eu-west-2:123456789012:parameter/app/token'),
 ('aws_secretsmanager_secret', 'name', 'resource::secretsmanager', 'app/login', 'arn:aws:secretsmanager:eu-west-2:123456789012:secret:app/login-AbCdEf'),
]


def write(root, name, text):
    p = root / name
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(text)


def dumps(value):
    return json.dumps(value, indent=2, ensure_ascii=False) + '\n'


def file_sha256(path):
    digest=hashlib.sha256()
    with open(path,'rb') as source:
        for chunk in iter(lambda:source.read(1024*1024),b''):
            digest.update(chunk)
    return digest.hexdigest()


def expected_tools(base):
    """#672 adds only the alert search description to the pre-config catalog."""
    expected = copy.deepcopy(base)
    search = next(tool for tool in expected if tool['name'] == 'search')
    kind = search['inputSchema']['properties']['kind']
    before = '`image` (an image builder or runner), or `ambiguous`'
    after = '`image` (an image builder or runner), `alert` (an alert emitter or matcher), or `ambiguous`'
    if kind['description'].count(before) != 1:
        raise ValueError('pre-config search description differs from the pinned catalog')
    kind['description'] = kind['description'].replace(before, after, 1)
    return expected


def seed(root):
    repos = {}
    for name in ['infra', 'app', 'plain']:
        p = root / name
        p.mkdir(parents=True)
        git('init', '-q', cwd=p)
        git('remote', 'add', 'origin', f'https://{PREFIX}{name}.git', cwd=p)
        repos[name] = p
    for name, peer in [('infra', 'app'), ('app', 'infra')]:
        write(repos[name], 'meada.toml', f'[[peer]]\nrepo = "{PREFIX}{peer}"\n')
    oracle = {'uses': [], 'provisioners': [], 'provisions': [], 'orphans': {}}
    def use(contract, at, level, value, line):
        oracle['uses'].append({'contract': contract, 'site': PREFIX + 'app', 'at': at,
                               'level': level, 'value': value, 'line': line})
    tf = ''
    for i, (typ, attr, prefix, name, arn) in enumerate(CASES):
        tf += f'resource "{typ}" "case_{i}" {{ {attr} = {json.dumps(name)} }}\n'
        for env in ['east', 'west']:
            oracle['provisioners'].append({'contract': prefix+'::'+name,
                'node': PREFIX+f'infra::{env}/main.tf::{typ}.case_{i}', 'root': env,
                'overridable': 'var.parcel_name' if env=='west' and i==0 else None})
    # Separate, unused identity per group; every group has both sides in scope.
    for typ, attr, prefix, _, _ in {c[2]: c for c in CASES}.values():
        name = 'idle-' + prefix.split('::')[1]
        tf += f'resource "{typ}" "idle" {{ {attr} = "{name}" }}\n'
        oracle['orphans'][prefix+'::'+name] = 'no use'
        for env in ['east', 'west']:
            oracle['provisioners'].append({'contract': prefix+'::'+name,
                'node': PREFIX+f'infra::{env}/main.tf::{typ}.idle', 'root': env})
    for env in ['east', 'west']:
        body=tf
        if env=='west':
            body='variable "parcel_name" { default = "parcel" }\n'+body.replace(
                'resource "aws_sqs_queue" "case_0" { name = "parcel" }',
                'resource "aws_sqs_queue" "case_0" { name = var.parcel_name }')
        write(repos['infra'], f'{env}/main.tf', body)
    # Spring configuration has every ARN kind, plus two equivalent SQS forms.
    lines = ['spring:', '  cloud:']
    for i, (_, _, prefix, name, arn) in enumerate(CASES):
        lines.append(f'    reference_{i}: {json.dumps(arn)}')
        use(prefix+'::'+name, f'application.yml#spring.cloud.reference_{i}', 'exact', arn, len(lines))
    url = 'https://sqs.eu-west-2.amazonaws.com/123456789012/parcel'
    lines += [f'    endpoint: "{url}"', '    archive: "s3://vault/folder/object.txt"']
    use('channel::sqs::parcel', 'application.yml#spring.cloud.endpoint', 'exact', url, len(lines)-1)
    use('resource::s3::vault', 'application.yml#spring.cloud.archive', 'exact', 's3://vault/folder/object.txt', len(lines))
    for key,value in [('archive_fragment','s3://vault/reports/2026#v1.json'),
                      ('archive_space','s3://vault/a b.json')]:
        lines.append(f'    {key}: {json.dumps(value)}')
        use('resource::s3::vault','application.yml#spring.cloud.'+key,'exact',value,len(lines))
    write(repos['app'], 'application.yml', '\n'.join(lines)+'\n')
    # Last key step, not any ancestor or a substring, must name the type.
    yaml = ('worker:\n  queue: parcel\n  topic: notice\n  bucket: vault\n  table: ledger\n'
            '  service:\n    queue:\n      name: parcel\n  queue_url: parcel\n  label: parcel\n'
            '  missing:\n    queue: absent\n  numeric:\n    queue: 17\n  flag:\n    bucket: false\n'
            '  unsupported: "arn:aws:ec2:eu-west-2:123456789012:instance/i-0123456789abcdef0"\n')
    write(repos['app'], 'helm/values.yaml', yaml)
    for key, prefix, name, line in [('queue','channel::sqs','parcel',2), ('topic','channel::sns','notice',3),
                                   ('bucket','resource::s3','vault',4), ('table','resource::dynamodb','ledger',5)]:
        use(prefix+'::'+name, f'helm/values.yaml#worker.{key}', 'inferred', name, line)
    use('channel::sqs::absent', 'helm/values.yaml#worker.missing.queue', 'inferred', 'absent', 12)
    oracle['orphans']['channel::sqs::absent'] = 'no provisioner'
    # JSON, list paths and quoted keys are independent config reader branches.
    obj = {'services': [{'queue':'parcel'}, {'bucket':'vault'}], 'special.key': {'topic':'notice'},
           'bare':'ledger', 'queueName':'parcel'}
    text = dumps(obj)
    write(repos['app'], 'config.json', text)
    for at, contract, value in [('services[0].queue','channel::sqs::parcel','parcel'),
                                ('services[1].bucket','resource::s3::vault','vault'),
                                ('["special.key"].topic','channel::sns::notice','notice')]:
        needle = '"'+at.rsplit('.',1)[-1]+'": "'+value+'"'
        line = next(i for i,l in enumerate(text.splitlines(),1) if needle in l)
        use(contract, 'config.json#'+at, 'inferred', value, line)
    # Unresolved templates and incomplete/unsupported structured-looking values are not
    # plain names. In particular a typed key must not rescue a malformed ARN by fallback.
    negatives = {'template': {'queue': '${QUEUE_NAME}'},
                 'templated_arn': {'queue': 'arn:aws:sqs:eu-west-2:123456789012:${QUEUE_NAME}'},
                 'unknown_service': {'queue': 'arn:aws:ec2:eu-west-2:123456789012:instance/i-0123456789abcdef0'},
                 'incomplete': {'queue': 'arn:aws:sqs:eu-west-2:123456789012:'},
                 'empty_bucket': {'bucket': 's3://'},
                 'foreign_url': {'queue': 'https://example.com/parcel'},
                 'cut': {'queue': 'x' * 1500},
                 'queue': ['parcel', 'notice']}
    write(repos['app'], 'negative.json', dumps(negatives))
    write(repos['app'], 'unreadable.yaml', 'queue: [parcel\nbucket: vault\n')
    # Redacted and ignored inputs must never become uses even when identifiers are valid.
    write(repos['app'], 'secret.yaml', 'apiVersion: v1\nkind: Secret\nmetadata:\n  name: proof\nstringData:\n  queue: "'+url+'"\n')
    write(repos['app'], 'excluded.yaml', 'queue: ignored\nbucket: "s3://ignored/key"\n')
    write(repos['app'], '.meadaignore', 'excluded.yaml\n')
    # Real directional siblings make an accidental producer/consumer use observable.
    write(repos['app'], 'worker.py', 'import boto3\nq = boto3.client("sqs")\n'
          'def emit():\n    q.send_message(QueueUrl="'+url+'", MessageBody="probe")\n'
          'def receive():\n    return q.receive_message(QueueUrl="'+url+'")\n')
    manifest = (repos['app'] / 'meada.toml').read_text()
    for role in ['read', 'write']:
        manifest += f'\n[[resource]]\nkind = "s3"\nid = "vault"\nrole = "{role}"\n'
    write(repos['app'], 'meada.toml', manifest)
    # Existing no-config export and all three query tools must stay identical.
    write(repos['plain'], 'plain.py', 'def anchor():\n    return 11\ndef caller():\n    return anchor()\n')
    for repo in repos.values():
        git('add', '.', cwd=repo)  # disposable repository only; no commit or project Git writes
    for use_row in oracle['uses']:
        for p in oracle['provisioners']:
            if p['contract'] == use_row['contract']:
                oracle['provisions'].append({'contract':p['contract'], 'side':use_row['site'],
                    'at':use_row['at'], 'provisioner':p['node'], 'root':p['root'],
                    'level':'inferred' if p.get('overridable') else use_row['level'],
                    'reason_contains':p.get('overridable')})
    return repos, oracle


def use_set(data):
    return [(u.get('contract'),u.get('site'),u.get('at')) for u in data.get('uses', [])]


def orphan_set(text):
    rows = [l for l in text.splitlines() if l.strip().startswith('orphan ')]
    got = [(m.group(1),m.group(2)) for l in rows if (m:=re.search(r'^\s*orphan (\S+): (no use|no provisioner);', l))]
    return sorted(got), len(rows)


def expected_orphan_rows(oracle):
    rows=[(p['contract'],'no use') for p in oracle['provisioners']
          if oracle['orphans'].get(p['contract'])=='no use']
    rows += [(contract,reason) for contract,reason in oracle['orphans'].items() if reason=='no provisioner']
    return sorted(rows)


def strip_orphans(text):
    return ''.join(l+'\n' for l in text.splitlines() if not l.strip().startswith('orphan '))


class Probe:
    def __init__(self, args):
        self.a, self.run, self.steps = args, Path(args.run).resolve(), []
        self.run.mkdir(parents=True, exist_ok=False)
        self.base, self.head = str(Path(args.base_bin).resolve()), str(Path(args.head_bin).resolve())

    def check(self, name, ok, detail=''):
        self.steps.append({'check':name,'passed':bool(ok),'detail':detail})
        write(self.run,'checks.json',dumps(self.steps))
        print(('PASS ' if ok else 'FAIL ')+name,flush=True)

    def command(self, binary, root, label, *args):
        p = subprocess.run([binary,*map(str,args)],cwd=root,capture_output=True,text=True,timeout=120)
        write(self.run,label+'.out',p.stdout)
        write(self.run,label+'.err',p.stderr)
        write(self.run,label+'.exit',str(p.returncode))
        if p.returncode:
            raise RuntimeError(f'{label}: exit {p.returncode}; see raw stderr')
        return p.stdout

    def queries(self, oracle):
        out=[]
        def add(tool, **arguments):
            arguments.setdefault('budget',200000)
            out.append({'tool':tool,'arguments':arguments})
        for contract in sorted(set(u['contract'] for u in oracle['uses'])):
            add('context',symbol=contract)
            add('search',query=contract,kind='contract')
            add('search',query=contract)
            for direction in ['up','down']:
                add('trace',**{'from':contract,'direction':direction})
        for root in ['east','west']:
            add('context',symbol=f'aws_sqs_queue.case_0 (root {root})')
            add('trace',**{'from':f'aws_sqs_queue.case_0 (root {root})','direction':'down'})
        for row in oracle['uses']:
            add('search',query=row['at'].split('#',1)[1],kind='value')
        for row in oracle['uses']:
            add('context',symbol=row['at'])
            for direction in ['up','down']:
                add('trace',**{'from':row['site']+'::'+row['at'],'direction':direction})
        for direction in ['up','down']:
            add('trace',**{'from':PREFIX+'app','direction':direction})
        add('search',query='worker.numeric.queue')
        add('search',query='zz_config_use_no_match')
        add('search',query='zz_config_use_no_match',kind='contract')
        add('context',symbol='zz_config_use_no_match')
        add('trace',**{'from':'zz_config_use_no_match','direction':'up'})
        add('search',query='parcel',kind='invalid-config-kind')
        add('context',symbol='channel::sqs::parcel',budget=120)
        return out

    def stdio(self,binary,repo,qs,label):
        client=Stdio(binary,repo,self.run/(label+'.err'))
        try:
            tools=client.call('tools/list',{})['result']['tools']
            texts=[answer(client.call('tools/call',{'name':q['tool'],'arguments':q['arguments']})) for q in qs]
            write(self.run,label+'.json',dumps({'queries':qs,'answers':texts,'tools':tools}))
            return tools,texts
        finally:
            client.close()

    def workspace(self,repos,label,ui=None):
        folder=self.run/label
        folder.mkdir(parents=True)
        manifest='index_dir = "indexes"\n'
        for name,root in repos.items():
            manifest+=f'\n[[repository]]\nrepo = "{PREFIX}{name}"\npath = "{root}"\n'
        if ui:
            manifest+=f'\n[ui]\ndir = "{Path(ui).resolve()}"\npublic_url = "http://127.0.0.1:{self.port}"\n'
        write(folder,'workspace.toml',manifest)
        self.command(self.head,folder,label+'/index','index','--workspace','workspace.toml')
        return folder

    def server(self,folder,label,command='mcp'):
        log=open(folder/'server.log','w')
        p=subprocess.Popen([self.head,command,'--listen',f'127.0.0.1:{self.port}','--workspace','workspace.toml'],
             cwd=folder,stdin=subprocess.DEVNULL,stdout=log,stderr=log,start_new_session=True)
        write(folder,'server.pid',str(p.pid))
        for _ in range(150):
            if p.poll() is not None:
                raise RuntimeError(f'{label}: exited {p.returncode}; see server.log')
            try:
                socket.create_connection(('127.0.0.1',self.port),timeout=.2).close()
                return p,log
            except OSError:
                time.sleep(.1)
        p.terminate(); p.wait(timeout=10); log.close()
        raise RuntimeError(f'{label}: did not listen')

    def http(self,repos,qs,tools,expected,label='http'):
        folder=self.workspace(repos,label)
        self.port=free_port()
        p,log=self.server(folder,label)
        try:
            client=Http(f'http://127.0.0.1:{self.port}/{PREFIX}app')
            ht=client.call('tools/list',{})['result']['tools']
            texts=[answer(client.call('tools/call',{'name':q['tool'],'arguments':q['arguments']})) for q in qs]
            write(folder,'answers.json',dumps(texts))
            self.check(label+' all answers and tools byte parity',texts==expected and ht==tools,f'{len(qs)} queries')
            if label=='visibility-http':
                exported=json.loads(self.command(self.head,folder,label+'/export','export','--workspace','workspace.toml','--repo',PREFIX+'app'))
                oracle=json.loads((self.run/'oracle.json').read_text())
                expected_uses=sorted((u['contract'],u['site'],u['at']) for u in oracle['uses'])
                self.check('filtered export retains known uses without hidden peer provisioners',
                           sorted(use_set(exported))==expected_uses and not exported.get('provisioners')
                           and not exported.get('provisions'),exported.get('provisions',[]))
        finally:
            p.terminate();p.wait(timeout=30);log.close();write(folder,'stopped',str(p.returncode))

    def check_exports(self,b,h,oracle,name):
        expected=sorted((u['contract'],u['site'],u['at']) for u in oracle['uses']) if name=='app' else []
        self.check(name+' complete use set/count/sites',sorted(use_set(h))==expected,
                   {'expected':expected,'got':sorted(use_set(h))})
        self.check(name+' use objects contain exactly graph identity fields',
                   all(set(row)=={'contract','site','at'} for row in h.get('uses',[])),h.get('uses',[]))
        self.check(name+' format remains 1',h.get('format')==1)
        # Existing public export order is side, provisioner, root, contract. The
        # optional at locator has no predating promised ordering among equal tuples.
        order=[(r.get('side',''),r.get('provisioner',''),r.get('root',''),r.get('contract',''))
               for r in h.get('provisions',[])]
        self.check(name+' provisioning rows keep canonical public order',order==sorted(order),order)
        # Existing arrays retain every field and ordering; only the new arrays may differ.
        bd={k:v for k,v in b.items() if k not in ['uses','provisions']}
        hd={k:v for k,v in h.items() if k not in ['uses','provisions']}
        self.check(name+' all existing export fields base parity',bd==hd)
        old=b.get('provisions',[])
        new=h.get('provisions',[])
        remaining=copy.deepcopy(new)
        for row in old:
            if row in remaining:
                remaining.remove(row)
            else:
                self.check(name+' existing provision retained '+str(row),False)
        if name=='plain':
            self.check(name+' whole export parity',b==h)
            return
        expected_links=oracle['provisions']
        # ADR0026 locates use at repository+at. A provisioning link may carry at as an
        # added field or in its reason; either must identify the precise originating key.
        got=[]
        for row in remaining:
            at=row.get('at')
            if at is None:
                matches=[u['at'] for u in oracle['uses'] if u['at'] in row.get('reason','')]
                at=matches[0] if len(matches)==1 else None
            got.append((row.get('contract'),row.get('side'),at,row.get('provisioner'),row.get('root'),row.get('level')))
            self.check(name+' provisioning reason identifies config key '+str(at),
                       bool(at) and bool(row.get('reason')),row)
            expected=next((p for p in expected_links if p['at']==at and p['provisioner']==row.get('provisioner')),None)
            if expected and expected.get('reason_contains'):
                self.check(name+' overridable provisioner reason '+str(at),
                           expected['reason_contains'] in row.get('reason',''),row)
        exp=[tuple(p[k] for k in ['contract','side','at','provisioner','root','level']) for p in expected_links]
        self.check(name+' complete new provisioning set/levels/count',sorted(got,key=str)==sorted(exp,key=str),
                   {'expected':exp,'got':got})
        self.check(name+' no directional graph changes',h['providers']==b['providers'] and
                   h['consumers']==b['consumers'] and h['links']==b['links'])

    def run_checks(self):
        _,oracle=seed(self.run/'input')
        write(self.run,'oracle.json',dumps(oracle))
        inventory={str(p.relative_to(self.run/'input')):hashlib.sha256(p.read_bytes()).hexdigest()
                   for p in sorted((self.run/'input').rglob('*')) if p.is_file() and '.git' not in p.parts}
        write(self.run,'input.sha256.json',dumps(inventory))
        captures={}
        identity={}
        for label,binary in [('base',self.base),('head',self.head)]:
            provenance=Path(binary+'.from')
            identity[label]={'binary':binary,'sha256':file_sha256(binary),
                             'from':provenance.read_text() if provenance.exists() else 'coordinator must identify source build'}
        write(self.run,'identity.json',dumps(identity))
        queries=self.queries(oracle)
        write(self.run,'queries.json',dumps(queries))
        for side,binary in [('base',self.base),('head',self.head)]:
            self.command(binary,self.run,side+'/identity','--version')
            roots=self.run/side/'work'
            shutil.copytree(self.run/'input',roots)
            repos={name:roots/name for name in ['infra','app','plain']}
            captures[side]={'repos':repos}
            for name,root in repos.items():
                exported=self.command(binary,root,f'{side}/{name}/export','export')
                status=self.command(binary,root,f'{side}/{name}/status','status')
                captures[side][name]={'export':json.loads(exported),'status':status}
                if side=='head':
                    repeated=self.command(binary,root,f'{side}/{name}/export-repeat','export')
                    self.check(name+' repeated export bytes deterministic',exported==repeated)
            tools,texts=self.stdio(binary,repos['app'],queries,side+'/stdio')
            captures[side]['tools'],captures[side]['texts']=tools,texts
            pq=[{'tool':'search','arguments':{'query':'anchor','budget':8000}},
                {'tool':'context','arguments':{'symbol':'anchor','budget':8000}},
                {'tool':'trace','arguments':{'from':'anchor','direction':'up','budget':8000}}]
            _,pt=self.stdio(binary,repos['plain'],pq,side+'/plain/stdio')
            captures[side]['plain_texts']=pt
        b,h=captures['base'],captures['head']
        for name in ['infra','app','plain']:
            self.check_exports(b[name]['export'],h[name]['export'],oracle,name)
            if name=='plain':
                self.check('plain whole status base parity',b[name]['status']==h[name]['status'])
            else:
                expected=expected_orphan_rows(oracle)
                self.check(name+' complete orphan set/count',orphan_set(h[name]['status'])==(expected,len(expected)),h[name]['status'])
                self.check(name+' existing status lines base parity',strip_orphans(h[name]['status'])==strip_orphans(b[name]['status']))
        self.check('whole tool catalog preserves base plus #672 alert description',expected_tools(b['tools'])==h['tools'])
        self.check('plain all query tools base parity',b['plain_texts']==h['plain_texts'])
        for i,(q,text) in enumerate(zip(queries,h['texts'])):
            args=q['arguments']
            contract=args.get('symbol',args.get('from',args.get('query','')))
            rows=[u for u in oracle['uses'] if u['contract']==contract]
            if rows and args['budget']>120 and q['tool']=='context':
                self.check(f'context {contract} all config locations',all(u['at'] in text for u in rows),text)
            if rows and q['tool']=='search':
                self.check(f'contract search discovers {contract}',contract in text,text)
            if q['tool']=='search' and rows and 'kind' not in args:
                explicit=next(j for j,other in enumerate(queries) if other['tool']=='search' and
                              other['arguments'].get('kind')=='contract' and other['arguments'].get('query')==contract)
                self.check('default full contract search equals explicit contract '+contract,text==h['texts'][explicit],text)
            if q['tool']=='search' and args.get('query') in ['worker.numeric.queue','zz_config_use_no_match']:
                self.check(f'existing default/failure search {i} base parity',text==b['texts'][i],text)
            if args.get('kind')=='value':
                # Search remains an existing flow: exact whole base answer on same input.
                self.check(f'existing value query {i} base parity',text==b['texts'][i],text)
            if q['tool']=='trace' and rows and contract!='channel::sqs::absent':
                self.check(f'existing contract trace advisory {args["direction"]} {contract} base parity',
                           text==b['texts'][i],text)
            key_rows=[u for u in oracle['uses'] if contract in [u['at'],u['site']+'::'+u['at']]]
            if q['tool']=='context' and key_rows:
                row=key_rows[0]
                self.check('direct key context '+row['at'],
                           (row['at']+' (line '+str(row['line'])+')') in text and row['contract'] in text and row['level'] in text,text)
            if q['tool']=='trace' and key_rows:
                row=key_rows[0]
                if args['direction']=='up' and row['contract']!='channel::sqs::absent':
                    self.check('direct key up trace provisions '+row['at'],
                               'provisions ' in text and 'east/main.tf' in text and 'west/main.tf' in text,text)
                if args['direction']=='down':
                    self.check('direct key down trace has no directional sibling '+row['at'],
                               '::emit' not in text and '::receive' not in text,text)
            if q['tool']=='trace' and contract==PREFIX+'app' and args['direction']=='up':
                self.check('repository aggregate trace includes provisioned config keys',
                           all(u['at'] in text for u in oracle['uses'] if u['contract']!='channel::sqs::absent'),text)
            if q['tool']=='trace' and contract.startswith('aws_sqs_queue.case_0'):
                queue_rows=[u for u in oracle['uses'] if u['contract']=='channel::sqs::parcel']
                self.check('IaC trace all config keys '+contract,all(u['at'] in text for u in queue_rows),text)
        self.check('negative wrong kind refused','TOOL ERROR ' in h['texts'][-2] or 'ERROR ' in h['texts'][-2],h['texts'][-2])
        self.check('budget affects context',h['texts'][-1]!=h['texts'][next(i for i,q in enumerate(queries)
                   if q['tool']=='context' and q['arguments'].get('symbol')=='channel::sqs::parcel')],h['texts'][-1])
        if self.a.http:
            self.http({n:h['repos'][n] for n in ['infra','app']},queries,h['tools'],h['texts'])
            # Scope filter: on-disk peer excluded from manifest is never read.
            isolated=self.run/'visibility-local'/'app'
            shutil.copytree(self.run/'input/app',isolated)
            it,ix=self.stdio(self.head,isolated,queries,'visibility-local/stdio')
            self.check('isolated known use has no provisioner marker',
                       'no provisioner in the workspace' in ix[0],ix[0])
            self.http({'app':h['repos']['app']},queries,it,ix,'visibility-http')
        self.refresh(h['repos'],oracle,h['app']['export'],h['app']['status'])
        self.negative_controls(h,oracle)
        if self.a.ui_dir:
            self.console(h['repos'],queries,h['texts'])
        else:
            write(self.run,'console-pending.txt','Console graph and query require --ui-dir built/ui/dist with a ui-enabled --head-bin.\n')
        return int(any(not s['passed'] for s in self.steps))

    def refresh(self,repos,oracle,exported,status):
        path=repos['app']/'helm/values.yaml'
        original=path.read_text()
        client=Stdio(self.head,repos['app'],self.run/'refresh.err')
        q={'name':'context','arguments':{'symbol':'channel::sqs::absent','budget':200000}}
        try:
            before=answer(client.call('tools/call',q))
            path.write_text(original.replace('queue: absent','label: absent'))
            after=answer(client.call('tools/call',q))
            changed=json.loads(self.command(self.head,repos['app'],'refresh/export','export'))
            self.check('warm same-size typed key removal changes live context',before!=after and 'worker.missing.queue' not in after,
                       {'before':before,'after':after})
            expected=[u for u in use_set(exported) if u[2]!='helm/values.yaml#worker.missing.queue']
            self.check('warm refresh removes exactly one use',sorted(use_set(changed))==sorted(expected))
            changed_status=self.command(self.head,repos['app'],'refresh/status','status')
            expected_orphans=[row for row in expected_orphan_rows(oracle) if row[0]!='channel::sqs::absent']
            self.check('warm refresh removes missing-use orphan',
                       orphan_set(changed_status)==(expected_orphans,len(expected_orphans)),changed_status)
            path.write_text(original)
            restored=answer(client.call('tools/call',q))
            re=json.loads(self.command(self.head,repos['app'],'restore/export','export'))
            rs=self.command(self.head,repos['app'],'restore/status','status')
            self.check('restore exact context/export/status round trip',restored==before and re==exported and rs==status)
            write(self.run,'refresh/live.json',dumps({'before':before,'after':after,'restored':restored}))
            # The missing use above exercises contract disappearance. A second edit
            # removes a provisioned key, so stale provisioning rows cannot hide.
            parcel={'name':'context','arguments':{'symbol':'channel::sqs::parcel','budget':200000}}
            linked_before=answer(client.call('tools/call',parcel))
            path.write_text(original.replace('  queue: parcel','  noque: parcel',1))
            linked_after=answer(client.call('tools/call',parcel))
            changed=json.loads(self.command(self.head,repos['app'],'refresh-linked/export','export'))
            removed='helm/values.yaml#worker.queue'
            self.check('warm linked typed key changes live context',linked_before!=linked_after and removed not in linked_after,
                       {'before':linked_before,'after':linked_after})
            expected=[u for u in use_set(exported) if u[2]!=removed]
            self.check('warm linked refresh removes exactly one use',sorted(use_set(changed))==sorted(expected))
            expected_links=[r for r in exported.get('provisions',[]) if r.get('at')!=removed]
            self.check('warm linked refresh removes exactly its two root provisions',
                       changed.get('provisions',[])==expected_links and
                       len(exported.get('provisions',[]))-len(expected_links)==2)
            path.write_text(original)
            linked_restored=answer(client.call('tools/call',parcel))
            re=json.loads(self.command(self.head,repos['app'],'restore-linked/export','export'))
            self.check('linked restore exact context/export round trip',linked_restored==linked_before and re==exported)
            write(self.run,'refresh-linked/live.json',dumps({'before':linked_before,'after':linked_after,'restored':linked_restored}))
        finally:
            path.write_text(original);client.close()

    def negative_controls(self,h,oracle):
        # Real captured responses, mutated only in saved verifier controls.
        if not h['app']['export'].get('uses'):
            self.check('negative controls have a real use capture',False,'Product returned no uses; earlier whole-set failure is decisive.')
            return
        exp=sorted((u['contract'],u['site'],u['at']) for u in oracle['uses'])
        for label,mutate in [('drop-use',lambda d:d['uses'].pop()),
                              ('wrong-site',lambda d:d['uses'][0].update(site='wrong')),
                              ('wrong-key',lambda d:d['uses'][0].update(at='wrong.yaml#wrong')),
                              ('extra-use',lambda d:d['uses'].append({'contract':'resource::s3::ignored','site':PREFIX+'app','at':'excluded.yaml#bucket'}))]:
            d=copy.deepcopy(h['app']['export']);mutate(d)
            write(self.run,'negative-controls/'+label+'.json',dumps(d))
            self.check('negative control rejected '+label,sorted(use_set(d))!=exp)
        text=h['app']['status']+'  orphan resource::s3::injected: no use; control\n'
        write(self.run,'negative-controls/extra-orphan.out',text)
        expected=expected_orphan_rows(oracle)
        self.check('negative control rejected extra orphan',orphan_set(text)!=(expected,len(expected)))
        link=copy.deepcopy(h['app']['export'])
        config_links=[r for r in link.get('provisions',[]) if r.get('at')]
        if config_links:
            config_links[0]['level']='declared'
            write(self.run,'negative-controls/wrong-level.json',dumps(link))
            fields=['contract','side','at','provisioner','root','level']
            got=sorted([tuple(r.get(k) for k in fields) for r in config_links],key=str)
            exp=sorted([tuple(r[k] for k in fields) for r in oracle['provisions']],key=str)
            self.check('negative control rejected wrong provisioning level',got!=exp)
        if self.a.http:
            http=json.loads((self.run/'http/answers.json').read_text())
            i=next(i for i,t in enumerate(http) if 'application.yml#' in t)
            http[i]=http[i].replace('application.yml#','removed-by-control#',1)
            write(self.run,'negative-controls/http-dropped-key.json',dumps(http))
            self.check('negative control rejected HTTP key loss',http!=h['texts'])

    def console(self,repos,qs,texts):
        # Local stdio scope is the app + explicit peer. Console manifest includes both.
        from console import http, query_equal
        self.port=free_port()
        folder=self.workspace({n:repos[n] for n in ['infra','app']},'console',self.a.ui_dir)
        p,log=self.server(folder,'console','ui')
        url=f'http://127.0.0.1:{self.port}'
        write(folder,'url',url)
        try:
            status,body=http(url+'/api/v1/config')
            self.check('console healthy',status==200,body.decode())
            for i,q in enumerate(qs):
                status,body=http(url+'/api/v1/query','POST',{'repository':PREFIX+'app','generation':None,
                                 'tool':q['tool'],'arguments':q['arguments']},{'Origin':url,'Content-Type':'application/json'})
                write(folder,f'query/{i}.json',dumps({'status':status,'body':body.decode()}))
                self.check(f'console query {i} full parity',query_equal(status,json.loads(body),texts[i]))
            status,body=http(url+f'/api/v1/repositories/{PREFIX}app/export')
            cli=self.command(self.head,folder,'console/export','export','--workspace','workspace.toml','--repo',PREFIX+'app')
            write(folder,'api-export.json',body.decode())
            self.check('console complete export byte parity',status==200 and body.decode()==cli)
            status,body=http(url+f'/api/v1/repositories/{PREFIX}infra/export')
            cli=self.command(self.head,folder,'console/infra-export','export','--workspace','workspace.toml','--repo',PREFIX+'infra')
            write(folder,'api-infra-export.json',body.decode())
            self.check('console complete infra export byte parity',status==200 and body.decode()==cli)
            # UI is exercised by the checked-in companion, keeping browser mechanics reusable.
            env=dict(os.environ,CONFIG_USES_URL=url,CONFIG_USES_RUN=str(folder))
            browser=subprocess.run(['node',str(Path(__file__).with_name('config-uses-console.mjs'))],
                                    env=env,capture_output=True,text=True,timeout=120)
            write(folder,'browser.out',browser.stdout);write(folder,'browser.err',browser.stderr)
            self.check('console graph shows every config use and provision',browser.returncode==0,browser.stdout+browser.stderr)
        finally:
            p.terminate();p.wait(timeout=30);log.close();write(folder,'stopped',str(p.returncode))

    def receipt(self,blocked=None):
        failed=[s for s in self.steps if not s['passed']]
        verdict='BLOCKED' if blocked else ('BROKEN' if failed else 'CHECKS PASSED; fresh checker must judge PROVED')
        write(self.run,'receipt.md',f'# Config uses #661 proof\n\n{verdict}\n\n'
              'Checker: scripted capture and comparisons; semantic judgment is recorded separately.\n'
              'Scope: #661 new flow; exact base/head binary hashes and provenance are in identity.json.\n\n'
              '## What the change touches\n'
              '- Config use export, provisioning, status, context/search/trace: new, #661/#642 and base ADR0026 (rung 4).\n'
              '- Existing exports and value/code answers: base binary on identical inputs (rung 1).\n'
              '- HTTP and console query/export: full stdio/CLI parity plus spec assertions (rungs 1+4).\n\n'
              f'{len(self.steps)} checks; {len(failed)} failures. Whole source-derived oracle: oracle.json; SHA256 input inventory: input.sha256.json.\n'
              'CLI exit/stderr/stdout and MCP wire captures retained as produced. Only this run starts/stops servers, stopped files record cleanup.\n\n'
              '## Not covered\n'
              '- Existing full corpus/shared-query parity is run separately by the coordinating checker.\n'
              + ('- Console pending: rerun with --ui-dir and ui-enabled --head-bin.\n' if not self.a.ui_dir else '')
              + '\n## Steps\n'+'\n'.join(f'- {"PASS" if s["passed"] else "FAIL"} {s["check"]}' for s in self.steps)
              +'\n\n## Retries and fixes\n'+(blocked or 'No verifier failures in this run; earlier runs, if any, remain as separate receipts.')+'\n')


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--head-bin',required=True);p.add_argument('--base-bin',required=True)
    p.add_argument('--run',required=True);p.add_argument('--http',action='store_true')
    p.add_argument('--ui-dir')
    args=p.parse_args();os.environ['LANG']='C.UTF-8'
    proof=Probe(args)
    try:
        result=proof.run_checks();proof.receipt();return result
    except Exception:
        error=traceback.format_exc();write(proof.run,'blocked.txt',error)
        proof.receipt(error);print(error,flush=True);return 2


if __name__=='__main__':
    raise SystemExit(main())
