import hashlib
import io
import itertools
import json
import re
import sys
import tarfile
import tomllib
from collections import Counter
from pathlib import Path
from zipfile import ZipFile

HEAD = 'd147178f8ac41743ebd4017a020a3c32b9b1b1fd'
BASE = 'b9d4c2315354fc348a77a3b12647fd78d92753aa'
OLD = '674b7ee52939ed58dccc43935b12322a56496696'
HIST = 'def34e4f887e8d97ebe02d4cca2aee3cfc8474aa'
OLD_SUM = '071609ff150a8543ab949611cdb762841f5d266a4614530502054ab91b102c55'
APP, LIB, SHARED = [f'github.com/claudiomascaro/meada-smoke-{x}' for x in ('app','lib','shared')]
ROUTE = 'http::POST::/orders'
AUTH = 'env:SMOKE_ORDERS_URL'
PEERS = [LIB, SHARED]
QUEUE = 'channel::sqs::smoke-orders'
CLASS_PIN = 'ed13044ebad552f4dc8dd8ac4ab8fb35bbab8f3a'
HTTP = [('go/client.go','SendConfigured','http.Post'),('go/client.go','SendShared','http.Post'),('python/client.py','send_configured','requests.post'),('python/client.py','send_shared','requests.post'),('java/OrdersClient.java','OrdersClient.sendOrder()','http.postForObject'),('kotlin/Orders.kt','KotlinOrders.sendOrder()','http.postForObject'),('csharp/OrdersClient.cs','OrdersClient.SendOrder()','http.PostAsync'),('rust/src/lib.rs','send_order','(call_expression).post'),('swift/Orders.swift','sendOrder()','URLSession.shared.data'),('typescript/client.ts','sendOrder','fetch')]
SQS = ('publishOrder','publishLiteral','publishSelected')
F = 'smoke-resolution'

class Archive:
    def __init__(self, p, checksum):
        with open(p,'rb') as fd:
            actual = hashlib.file_digest(fd, 'sha256').hexdigest()
        assert actual == checksum.removeprefix('sha256:'), (p,actual)
        self.z = ZipFile(p)
        assert self.z.testzip() is None
        self.members = set(self.z.namelist())
        assert len(self.members) == len(self.z.namelist())
        self.r = json.loads(self.z.read('receipt.json'))
        print('ZIP',actual,Path(p).stat().st_size,'bytes;',len(self.members),'unique CRC-checked members')
    def raw(self, f, n): return self.z.read(f'flows/{f}/{n}')
    def get(self, f, n): return json.loads(self.raw(f,n))
    def cmd(self, f, n): return self.get(f,'commands/'+n+'.out')
    def source(self, corpus, repo, path):
        with tarfile.open(fileobj=io.BytesIO(self.raw(F,f'sources/{corpus}/meada-smoke-{repo}.tar'))) as archive:
            return archive.extractfile(path).read()

def site(path, symbol, fn): return f'{APP}::orders/clients/{path}::{symbol}::{fn}#0'
def provider(peer, path='/orders'): return f'{peer}::openapi.yaml::POST {path}'
def reason(peer): return f'base URL from SMOKE_ORDERS_URL, a base_url_env of peer {peer}'
def qsite(name): return f'{APP}::orders/events/publisher.ts::{name}::SendMessageCommand#0'
def obs(x, site_id):
    return {a:[r for r in x.get(a,[]) if r[k]==site_id] for a,k in [('consumers','site'),('links','consumer'),('open_items','call'),('calls','id'),('providers','site'),('provisions','side')]}
def edges(o): return sorted((r['contract'],r['provider'],r['level'],r['reason']) for r in o['links'])
def candidates(path='/orders', peers=PEERS): return [f'{p}::http::POST::{path}' for p in peers]
def via(o, contract, authority): return [(r['contract'],r.get('via')) for r in o['consumers']] == [(contract,authority)]
def config(o, peer=LIB, path='/orders'):
    c=f'http::POST::{path}'
    return via(o,c,AUTH) and not o['open_items'] and edges(o)==[(c,provider(peer,path),'inferred',reason(peer))]
def dynamic(o, helper):
    return via(o,None,None) and not o['links'] and [(r['kind'],r['candidates'],r.get('limit')) for r in o['open_items']]==[('dynamic-url',candidates(),'base from the return value of '+helper)]
def sqs(o, named):
    assert [p['contract'] for p in o['providers']] == [QUEUE if named else 'channel::sqs']
    assert [(p['contract'],p['provisioner'],p['level']) for p in o['provisions']] == ([(QUEUE,f'{LIB}::queues.tf::aws_sqs_queue.orders','exact')] if named else [])
def whole(a, flow, cold, stages):
    before=a.raw(flow,f'commands/{cold}.out')
    for stage in stages:
        after=a.raw(flow,f'commands/{stage}.out')
        assert json.loads(after)==json.loads(before),stage
        print('Whole export',stage,'equals',cold,'bytes equal',after==before,hashlib.sha256(after).hexdigest())

def records(a,b):
    r=a.r
    assert [r[k] for k in ['base','head','driver_revision']] == [BASE,HEAD,HEAD]
    assert r['run_url'].endswith('/44935478498')
    assert r['answer_keys']==HEAD+':.claude/skills/prove-meada/references/scope.md'
    assert set(r['surfaces'])=={'cli','console','goldens'} and r['repos']=='all'
    assert r['binaries']['meada-base']['from']==f'commit {BASE}, features: default'
    assert r['binaries']['meada-head']['from']==f'working tree at {HEAD}, uncommitted: 0 path(s), features: default'
    assert r['binaries']['meada-before-class-authority']['from']==f'commit {OLD}, features: default'
    print('Receipt',r['capture'],r['verdict'],r['run_url'],BASE,HEAD,r['surfaces'],len(r['steps']),sum(s.get('seconds',0) for s in r['steps']),Counter(s['exit_code'] for s in r['steps']),[(s['name'],s['exit_code']) for s in r['steps'] if s['exit_code']])
    print('Binary identities',json.dumps(r['binaries'],sort_keys=True))
    marks=a.get(F,'checks.json')
    print('Rejected candidate or shared keys',[(c['revision'],c['key'] or c['name'],c['result']) for c in marks if c['revision'] in ('head','shared') and c['result']!='pass'])
    print('Auxiliary denominators',json.dumps(a.get(F,'auxiliary-denominators.json')))
    for member in sorted(a.members):
        if member.endswith('/rebless.md') or member=='rebless.md':
            print('Golden deltas',member,[line for line in a.z.read(member).decode().splitlines() if line.startswith(('## ', '+extraction revision','-extraction revision'))])
    n=0
    for member in sorted(a.members):
        m=re.fullmatch(r'flows/(smoke-[^/]+)/sources/([^/]+)/(meada-smoke-[^/]+).tar',member)
        if not m: continue
        flow,kind,repo=m.groups()
        inputs=a.get(flow,'inputs.json')
        assert inputs['driver_revision']==HEAD
        for slot,p in inputs['binaries'].items(): assert inputs['binary_provenance'][slot]==r['binaries'][Path(p).name]
        pin=inputs['corpora'][kind][repo]
        raw=a.z.read(member)
        assert member in b.members, member
        assert raw == b.z.read(member) and pin==b.get(flow,'inputs.json')['corpora'][kind][repo]
        with tarfile.open(fileobj=io.BytesIO(raw)) as z: assert z.pax_headers['comment']==pin
        seen=[entry[repo] for entry in a.get(flow,'checkouts.json').values() if entry[repo]['head']==pin]
        assert seen and all(x['origin']==f'https://github.com/ClaudioMascaro/{repo}.git' for x in seen)
        n+=1
    assert n==24,n
    print('Source pins',n,'archives, 24 byte-identical to historical artifacts')


def http(a,b):
    go=[json.loads(line.removeprefix('PROBE ')) for line in a.raw(F,'commands/transport/go.out').decode().splitlines() if line.startswith('PROBE ')]
    for runtime, names in [(go,['SendShared','SendConfigured']),(a.cmd(F,'transport/python'),['send_shared','send_configured'])]:
        assert runtime==[{'caller':n,'method':'POST','url':f'https://{p}.smoke.example/orders'} for n,p in zip(names,['shared','lib'])]
    assert a.get(F,'denominator.json')['checks']==b.get(F,'denominator.json')['checks']
    for slot in ('base','head'):
        exp=a.cmd(F,slot+'/app'); prior=b.cmd(F,slot+'/app'); count=0
        for i,(path,name,fn) in enumerate(HTTP):
            sid=site(path,name,fn); x=obs(exp,sid)
            assert x==obs(prior,sid),sid
            good=dynamic(x,'sharedBase' if i==1 else 'shared_base') if i in (1,3) else config(x)
            assert good==(slot=='head' or i not in (1,3,4,5)),(slot,sid,x)
            if i in (4,5):
                h=obs(exp,site(path,name,'ordersBase'))['calls']
                assert [(k['level'],k['target']) for k in h]==[('exact',f'{APP}::orders/clients/{path}::{name.split(".")[0]}.ordersBase()')]
            print('HTTP',slot,sid,good,[(r['contract'],r.get('via')) for r in x['consumers']],edges(x),x['open_items'])
            count+=good
        for name in SQS:
            x=obs(exp,qsite(name)); assert x==obs(prior,qsite(name)); sqs(x,name!='publishSelected'); count+=1
            print('SQS',slot,qsite(name),x['providers'],x['provisions'])
        assert count=={'base':9,'head':13}[slot]
        assert a.get(F,'denominator.json')['totals'][slot]=={'passed':count,'checked':13,'expected':13}
    whole(a,F,'head/app',['expanded-upgrade/'+s for s in ('first-export','after-index','reopened')])
    print('Original 13 identities and complete observations unchanged, 9/13 -> 13/13')

def mcp(a):
    def where(path, marker, within=None):
        lines=a.source('expanded','app',path).decode().splitlines(); matches=[]; enabled=within is None
        for i,line in enumerate(lines,1):
            if within and line.startswith('func '): enabled=within in line
            if enabled and marker in line: matches.append(i)
        n,=matches
        return f'{APP} {path}:{n} {ROUTE}'
    root='orders/clients/'
    probes=[('go/client.go','http.Post(configuredBase()'),('python/client.py','post(configured_base()'),('csharp/OrdersClient.cs','http.PostAsync('),('rust/src/lib.rs','reqwest::Client::new()'),('swift/Orders.swift','URLSession.shared.data('),('typescript/client.ts','return fetch(ordersBase()')]
    stable=[where(root+p,m) for p,m in probes]
    stable += [where('orders/evidence/client.go',m) for m in ('http.Post(c.ordersBase()', 'func ViaParameter()', 'func Relative()', 'os.Getenv("SMOKE_ORDERS_URL")+"/orders"+query,')]
    stable += [where('orders/evidence/client.go','os.Getenv("SMOKE_ORDERS_URL")+path,','func QueryAppend(')]
    stable += [where(f'orders/{p}/client.go','return http.NewRequest(http.MethodPost') for p in ('httpkey','neutral','oskey')]
    assert len(set(stable))==14
    for slot, probes in [('base',[('go/client.go','http.Post(sharedBase()'),('python/client.py','post(shared_base()')]),('head',[('java/OrdersClient.java','return http.postForObject('),('kotlin/Orders.kt','return http.postForObject(')])]:
        expected=stable+[where(root+p,m) for p,m in probes]+[f'{peer} openapi.yaml (documented only) {ROUTE}' for peer in PEERS]
        pack=a.get(F,f'mcp/{slot}/answers.json')
        assert [q['tool'] for q in pack['queries']]==['search','context','trace']
        search,context,trace=pack['answers']
        trim=lambda s:[x.strip() for x in s.splitlines() if x.strip()]
        assert Counter(trim(search))==Counter(expected),(slot,Counter(trim(search))-Counter(expected),Counter(expected)-Counter(trim(search)))
        assert Counter(trim(context))==Counter([ROUTE,*[f'{peer} provider openapi.yaml (documented only)' for peer in PEERS],'16 linked consumers omitted. See trace with direction "up".'])
        edges_=[x for x in trim(trace) if x.startswith('→')]
        assert len(edges_)==2 and APP in trim(trace)[0] and 'jobs["through-lib"]' in trim(trace)[0]
        assert edges_[0].startswith(f'→[exact: @main (indexed cb746ac)] {LIB} .github/workflows/check.yml:')
        assert edges_[1].startswith(f'→[exact: @main (indexed 0aabcee)] {SHARED} .github/workflows/build.yml:')
        assert 'chain: exact · 2 hops · 3 repos' in trace
        print('MCP',slot,'3/3, two providers and 16 consumer locations', [where(root+p,m) for p,m in probes],edges_)
    assert a.get(F,'auxiliary-denominators.json')['http-mcp']=={s:{'passed':3,'checked':3,'expected':3} for s in ('base','head')}

def classes(a):
    source=a.source('class-authority','app','orders/clients/python/class_probe.py')
    assert hashlib.sha1(f'blob {len(source)}\0'.encode()+source).hexdigest()=='ae63c6111eafea2fa4fdecb7b588c8b0df744c0c'
    recorder=a.source('class-authority','app','scripts/record-class-authority.py')
    assert hashlib.sha1(f'blob {len(recorder)}\0'.encode()+recorder).hexdigest()=='8e2b8b806dd84bc9312da9c0f314325033d8ce07'
    for state in ('decorated','removed','restored'):
        assert a.raw(F,f'class-sources/{state}.py')==(source.replace(b'@install\n',b'') if state=='removed' else source)
    totals={}
    for slot in ('old','base','head'):
        accepted=0
        for state,host in [('decorated','shared'),('removed','lib'),('restored','shared')]:
            stem=f'class/{slot}/{state}'; exp=a.cmd(F,stem+'/export')
            assert exp['commit']==CLASS_PIN and exp['dirty']==(state=='removed')
            records=a.cmd(F,stem+'/runtime')
            assert records==[{'caller':n,'method':'POST','url':f'https://{p}.smoke.example/orders'} for n,p in [('Changed.send',host),('Ordinary.send','lib')]]
            for r in records:
                sid=site('python/class_probe.py',r['caller'],'requests.post'); point=obs(exp,sid)
                lib=r['url'].startswith('https://lib.')
                assert (config(point) if lib or slot!='head' else dynamic(point,'self.changed_origin')),(slot,state,sid,point)
                okay=lib or slot=='head'; accepted+=okay
                print('CLASS',slot,state,sid,r['url'],okay,[(v['contract'],v.get('via')) for v in point['consumers']],edges(point),point['open_items'])
        totals[slot]=accepted
        whole(a,F,f'class/{slot}/decorated/export',[f'class/{slot}/restored/export'])
    assert totals=={'old':4,'base':4,'head':6}
    assert {k:v['passed'] for k,v in a.get(F,'auxiliary-denominators.json')['python-class'].items()}==totals
    whole(a,F,'class/old/decorated/export',['class/upgrade/old-export'])
    whole(a,F,'class/head/decorated/export',['class/upgrade/'+x for x in ('first-export','after-index','reopened')])
    cmd=lambda x:a.get(F,f'commands/class/upgrade/{x}.json')
    labels=['old-index','old-export','first-export','index','after-index','reopened']
    assert len({cmd(x)['cwd'] for x in labels})==1 and all(cmd(x)['exit']==0 for x in labels)
    assert len({cmd(x)['argv'][0] for x in labels[:2]})==1
    assert len({cmd(x)['argv'][0] for x in labels[2:]})==1 and cmd('old-index')['argv'][0]!=cmd('first-export')['argv'][0]
    named={x['name']:x['actual'] for x in a.get(F,'checks.json')}
    x=named['#962 first export refreshes the extraction fingerprint']; s=hashlib.sha256(source).hexdigest()
    assert x['old']!=x['cold']==x['first-export']==x['after-index']==x['reopened']
    assert named['#962 binary swap preserves pinned source bytes']==[s,'','','']
    print('Class totals',totals,'PRAGMA user_version',x,'class source SHA256',s)

def kotlin(a,b):
    prefix='example.com/proof/kotlin-shadow::src/main/kotlin/example/jvm/Typed.kt::example.jvm.'
    for slot in ('base','head'):
        q=a.cmd(F,'kotlin-shadow/'+slot)
        for symbol,p in [('Typed.typed()','typed'),('Shadowing.hidden()','hidden')]:
            sid=prefix+symbol+'::(call_expression).uri#0'; x=obs(q,sid)
            assert x==obs(b.cmd(F,'kotlin-shadow/'+slot),sid)
            assert via(x,None,None) and not x['links'] and [(v['kind'],v['candidates']) for v in x['open_items']]==[('dynamic-url',[])]
            assert [(r['level'],r['target']) for r in obs(q,prefix+symbol+'::make#0')['calls']]==[('exact',prefix+'make()')]
            print('Kotlin #952',slot,sid,'GET https://typed.example.com/persons/'+p,'observed null contract/via, no provider',x['open_items'])


def matrix(a,b):
    ctr=0
    combos=list(itertools.product(PEERS,(False,True),(False,True),('', '?variant=fixed'),(False,True)))
    led=a.get(F,'matrix.json')
    assert len(combos)==len(led['configurations'])==32 and led['routes']==96 and led['callers']==192
    for n,((owner,dual,order,query,warm),case) in enumerate(zip(combos,led['configurations'])):
        paths=[f'/probe-order-{3*n+i:03}' for i in range(3)]; file=f'matrix/{n:02}/app-export'
        assert case==dict(owner=owner,both_providers=dual,reverse_peer_order=order,query=query,warm=warm,paths=paths,file=file+'.out')
        got=a.cmd(F,file); prior=b.cmd(F,file)
        for i,p in enumerate(paths):
            contract=f'http::POST::{p}'; peers=PEERS if dual else [owner]
            x,y=[obs(got,f'{APP}::matrix.go::{k}{i}::http.Post#0') for k in ('Configured','Relative')]
            ox,oy=[obs(prior,f'{APP}::matrix.go::{k}{i}::http.Post#0') for k in ('Configured','Relative')]
            assert x==ox and y==oy and config(x,owner,p) and via(y,contract,None)
            assert [(c,v,l) for c,v,l,r in edges(y)]==[(contract,provider(k,p),'ambiguous' if dual else 'inferred') for k in peers]
            assert all(r for c,v,l,r in edges(y))
            assert [(r['kind'],r['candidates']) for r in y['open_items']]==([('ambiguous-link',candidates(p,peers))] if dual else [])
            ctr+=2
    assert ctr==192
    pub=a.cmd(F,'published/app'); relative=obs(pub,f'{APP}::orders/evidence/client.go::Relative::http.Post#0')
    assert via(relative,ROUTE,None) and sorted((r['provider'],r['level']) for r in relative['links'])==[(provider(p),'ambiguous') for p in PEERS]
    assert [(r['kind'],r['candidates']) for r in relative['open_items']]==[('ambiguous-link',candidates())]
    for label in ('reordered','restored'):
        altered=a.cmd(F,'peers/'+label)
        assert all(pub[k]==altered[k] for k in ('links','open_items'))
    w,c=[a.cmd(F,'peers/edited-'+x) for x in ('warm','cold')]
    assert all(w[k]==c[k] for k in ('links','open_items')) and not any(x['provider']==provider(LIB) for x in w['links'])
    changed=obs(a.cmd(F,'peers/reassigned'),f'{APP}::orders/evidence/client.go::Client.ViaMethod::http.Post#0')
    assert config(changed,SHARED)
    print('Matrix',ctr,'/192 exact raw site rows match history, across 96 routes and 32 configurations; legitimate Relative and semantic mutations verified')

def lifecycle(a,b):
    f='smoke-lifecycle'; marks=a.get(f,'checks.json'); prior=b.get(f,'checks.json')
    assert len(marks)==len(prior)==87 and [(r['name'],r['result']) for r in marks]==[(r['name'],'pass') for r in prior]
    deltas=[(r['name'],key,r.get(key),old.get(key)) for r,old in zip(marks,prior,strict=True)
            for key in ('expected','actual') if r.get(key)!=old.get(key)]
    print('Lifecycle expected/actual deltas across stacks:',len(deltas))
    for name,key,new,old in deltas:
        print(name,key,'restack',repr(new)[:700],'historical',repr(old)[:700])

    for status,exit_code,needle in [('absent',1,'not evaluated'),('old-schema',1,'another version'),('healthy',0,'workspace: 3 repositories'),('broken-lib',0,f'{LIB}: no such table: symbols'),('empty',1,'workspace: 0 repositories')]:
        name='commands/status/'+status
        assert a.get(f,name+'.json')['exit']==exit_code
        assert needle.encode() in a.raw(f,name+'.out')+a.raw(f,name+'.err')
    for scope in ('isolated','three'):
        label=f'commands/orphans-{scope}/status.out'; raw=a.raw(f,label)
        assert raw==b.raw(f,label)
        orphan_lines=[l for l in raw.splitlines() if l.strip().startswith(b'orphan ')]
        assert f'workspace: {1 if scope=="isolated" else 3} repositor'.encode() in raw
        expected = {
            'image::localhost:5000/smoke/preview': 'no runner',
            'alert::team::retired': 'no matcher',
            'channel::sqs::smoke-legacy': 'no provisioner',
            'image::localhost:5000/smoke/retired': 'no builder',
            'alert::team::inventory': 'no emitter',
            'channel::sqs::smoke-idle': 'no use',
            f'ci::{APP if scope=="isolated" else SHARED}/.github/workflows/orphan.yml': 'no consumer',
        }
        for contract, absence in expected.items():
            scope_key = APP if scope == 'isolated' else 'the workspace'
            prefix = f'orphan {contract}: {absence} in {scope_key}; '.encode()
            assert sum(line.strip().startswith(prefix) for line in orphan_lines) == 1, prefix
    for variant in ('dangling','linked','malformed','waiting-peer'):
        for op in ('decider','interactive','item-no-such-item','prune'):
            root=f'commands/symlink-{variant}/write-{op}'
            assert a.get(f,root+'.json')['exit']!=0 and b'symbolic link' in a.raw(f,root+'.err').lower()
        assert a.get(f,f'decider/symlink-{variant}/requests.json')==[]
        for op in ('dry','export','index','list','show','status'):
            root=f'commands/symlink-{variant}/read-{op}'
            exit_code=a.get(f,root+'.json')['exit']
            assert exit_code in ({1} if variant=='malformed' else {0,1} if op=='status' else {0})
            if variant=='malformed': assert b'meada.decisions.toml:1: key with no value' in a.raw(f,root+'.err')
    req=lambda tag:a.get(f,f'decider/{tag}/requests.json')
    save=lambda tag:a.raw(f,f'decider/{tag}/final.toml')
    prompt=lambda rows:[r['request'] for r in rows]
    assert [len(r['saved']) for r in req('interrupted')]==[0,1,2]
    assert a.get(f,'decider/interrupted/process.json')['exit']==-15
    assert prompt(req('resumed'))==prompt(req('continuous'))[2:] and save('resumed')==save('continuous')
    for label,n in [('failed-write',1),('failed-response',2)]:
        assert len(tomllib.loads(save(label).decode())['decision'])==n
        assert prompt(req(label+'-resumed'))==prompt(req('continuous'))[n:] and save(label+'-resumed')==save('continuous')
    assert len(req('previews'))==3 and not save('previews')
    comp=[json.loads(line) for line in a.raw(f,'comparisons.jsonl').splitlines()]
    assert [r['name'] for r in comp]==['#934 healthy status stdout','#937 continuous decider stdout','#937 continuous decisions TOML']
    print('Lifecycle',len(marks),'strict controls, three parities',[(r['name'],r['matches']) for r in comp], 'orphan, status, symlink and resumed bytes read')

def upgrades(a):
    f='smoke-upgrade'
    def rows(data):
        out=[(r['consumer'],r['provider'],r['level'],r['reason']) for r in data['links'] if r['contract']==ROUTE]
        assert all((p,l,r)==(provider(LIB),'inferred',reason(LIB)) for c,p,l,r in out)
        return out
    names=lambda seq:sorted(r[0].split('::')[1].split('/')[1] for r in seq)
    for v in ('middle','head'):
        old,first,refresh,second,cold=[rows(a.cmd(f,v+'/'+x)) for x in ('old-export','first-export','refresh-export','second-refresh-export','cold-export')]
        assert names(old)==['neutral'] and names(cold)==['httpkey','neutral','oskey'] and first==refresh==second
        assert (first==cold)==(v=='head') and names(first)==(names(cold) if v=='head' else ['neutral'])
        print('Upgrade history',v,[names(r) for r in (old,first,refresh,second,cold)])
    assert a.get(f,'result.json')['counts']=={'pass':12,'fail':0,'blocked':0,'timeout':0,'error':0}
    f='smoke-sqs'; old,first,refresh,cold=[a.cmd(f,'sdk/'+k) for k in ('old-export','first-export','refresh','cold-export')]
    for name in SQS:
        sid=qsite(name); sqs(obs(old,sid),name=='publishLiteral'); sqs(obs(cold,sid),name!='publishSelected')
        for t in (first,refresh): assert all(obs(t,sid)[k]==obs(cold,sid)[k] for k in ('providers','provisions'))
    assert a.get(f,'result.json')['counts']=={'pass':7,'fail':0,'blocked':0,'timeout':0,'error':0}
    print('Historical upgrade 12/12, SQS 7/7 by source call and provision identity')
    for flow,cold,stages in [('smoke-upgrade','head/cold-export',['head/first-export','head/refresh-export','head/second-refresh-export']),
                             ('smoke-sqs','sdk/cold-export',['sdk/first-export','sdk/refresh'])]:
        wr=a.cmd(flow,cold)
        for stage in stages:
            actual=a.cmd(flow,stage)
            print('Whole historic',flow,stage,'delta fields',
                  sorted(key for key in set(wr)|set(actual) if key not in wr or key not in actual or wr[key]!=actual[key]))

def dynamo(a,b):
    f='dynamodb-transactions'; prefix='input/dynamodb-transactions/'; stem='dynamodb-transactions'; base='resource::dynamodb'
    for file in ('README.md','Transactions.java','entries.go'): assert a.raw(f,prefix+file)==b.raw(f,prefix+file)
    expected=[]
    for line in a.raw(f,prefix+'README.md').decode().splitlines():
        if line.startswith(('| Transactions.java |','| entries.go |')):
            file,fn,role,table=[x.strip() for x in line.split('|')[1:-1]]
            expected.append((file,fn,role,base if table=='unresolved' else base+'::'+table))
    exp=a.get(f,stem+'-export.out'); actual=[]
    for field,kind in [('providers','writer'),('consumers','reader')]:
        for row in exp[field]:
            if (row['contract'] or '').startswith(base):
                repo,file,fn,*_=row['site'].split('::'); assert repo=='github.com/zz-prove/dynamodb-transactions'
                actual.append((file,fn.split('(')[0].split('.')[-1],kind,row['contract']))
    assert len(actual)==len(expected)==21 and Counter(actual)==Counter(expected)
    pack=a.get(f,stem+'-mcp.json'); assert len(pack['queries'])==len(pack['answers'])==len(pack['expected_sides'])==15
    regex=re.compile(r'(\S+) ([^:]+):[0-9]+ (?:(produces|consumes) )?(resource::dynamodb(?:::\S+)?)(?: |$)')
    for query,reply,recorded in zip(pack['queries'],pack['answers'],pack['expected_sides']):
        if query['tool']=='search':
            target=query['arguments']['query']; file,fn,role,contract=next(x for x in expected if x[3]==target); sense=None
        else:
            fn=query['arguments']['symbol'].split('::')[-1].split('(')[0].split('.')[-1]
            file,_,role,contract=next(x for x in expected if x[1]==fn); sense='produces' if role=='writer' else 'consumes'
        correct=('github.com/zz-prove/dynamodb-transactions',file,sense,contract)
        assert tuple(recorded)==correct and [m.groups() for l in reply.splitlines() if (m:=regex.match(l))]==[correct]
    print('DynamoDB 36/36 from fixture and raw answers, 21 sides + 15 MCP')

def restacked_history(a,b):
    assert b.r['head']==HIST and b.r['base']=='393603dce0cc7ff1a9613538ce17b7028dc09728'
    assert (a.r['capture'],a.r['verdict'])==('FAILED','UNPROVEN'), (a.r['capture'],a.r['verdict'])
    different = {step['name']:step['exit_code'] for step in a.r['steps'] if step['exit_code']}
    assert different=={'smoke-resolution':3,'config-uses':4,'images':4}, different
    bad=[m for m in a.get(F,'checks.json') if m['revision'] in ('head','shared') and m['result']!='pass']
    assert len(bad)==2 and all(m['result']=='fail' and '#952' in (m.get('key') or m['name']) for m in bad),bad
    for slot in ('base','head'):
        assert a.get(F,f'mcp/{slot}/answers.json')['answers']==b.get(F,f'mcp/{slot}/answers.json')['answers']
    for slot,state in itertools.product(('old','base','head'),('decorated','removed','restored')):
        label=f'class/{slot}/{state}/export'
        assert a.cmd(F,label)==b.cmd(F,label),(slot,state)
    for flow,cold,stages in [('smoke-upgrade','head/cold-export',['head/'+x+'-export' for x in ('first','refresh','second-refresh')]),('smoke-sqs','sdk/cold-export',['sdk/first-export','sdk/refresh'])]:
        whole(a,flow,cold,stages)
    print('Restack compared to 44905282046: same nine class exports and six MCP responses. Exactly two Kotlin head rejections and two historical parity deltas.')

if __name__=='__main__':
    new, prior, checksum=sys.argv[1:]
    a,b=Archive(new,checksum),Archive(prior,OLD_SUM)
    assert b.r['head']==HIST
    import traceback
    failed=[]
    for check,args in [(records,(a,b)),(http,(a,b)),(mcp,(a,)),(classes,(a,)),(kotlin,(a,b)),
                       (matrix,(a,b)),(lifecycle,(a,b)),(upgrades,(a,)),(dynamo,(a,b)),(restacked_history,(a,b))]:
        try:
            check(*args)
        except Exception:
            traceback.print_exc()
            failed.append(check.__name__)
        else:
            print('VERIFIED', check.__name__)
    assert not failed, failed
    print('Independent audit complete. Kotlin #952 observations remain separate; golden approval is pending.')
