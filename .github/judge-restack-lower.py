"""Independently count lower-PR captures against the public mock source keys."""
import hashlib
import io
import json
from pathlib import Path
import re
import sys
import tarfile
from zipfile import ZipFile

APP, LIB, SHARED = (f'github.com/claudiomascaro/meada-smoke-{part}' for part in ('app', 'lib', 'shared'))
NAMES = ('meada-smoke-app', 'meada-smoke-lib', 'meada-smoke-shared')
ORIGINAL = ('280efe9d33217ac8750659b987877a004c963f32',
            'a36abd395d4a5fa07ddbd37ce23971a6aceab961',
            '51703923bd1f76c0b948db8afb1fa52fe86300c1')
LINKED = 'd4d76058f3dbe777854d05e8e0d781a37bee6cd1'
MALFORMED = 'ad48b4c17536a2c35102ae87c34fca85807eb700'
DRIVER = 'b9d4c2315354fc348a77a3b12647fd78d92753aa'
HEADS = ('e27664e23d5da1ea5dffcdb91ae49915d501b95a', '7dee48a7b7fec9695c5e1c4a4d80208bbe827671', 'd7fbc5b74363500b375718fdff0135b606e971d9')
OLD = ('e4058ebc17f97f0ae08adfd89374be159fe11040',
       'e0925fe4463ea44b6d1a928509fc68c0d8c14b81',
       '5d322753da33274f14facb53996f5f4f2caabac5')
PROVIDER = f'{LIB}::openapi.yaml::POST /orders'
CONTRACT = 'http::POST::/orders'
REASON = f'base URL from SMOKE_ORDERS_URL, a base_url_env of peer {LIB}'


def site(name):
    return f'{APP}::orders/{name}/client.go::Client.CreateOrder::http.NewRequest#0'


class Capture:
    def __init__(self, archive, root, head, **slots):
        self.archive, self.root = archive, root
        inputs = self.json('inputs.json')
        assert inputs['driver_revision'] == DRIVER, inputs
        for name, sha in {'head': head, **slots}.items():
            origin = inputs['binary_provenance'][name]
            assert origin['from'].startswith(f'commit {sha}, features: default'), origin
            assert re.fullmatch('[0-9a-f]{64}', origin['sha256']), origin
        print(root, inputs['binary_provenance'])
        for label, repo in self.json('checkouts.json').items():
            app = MALFORMED if label.endswith('malformed') else (
                LINKED if label.startswith('decisions-') else ORIGINAL[0])
            for name, sha in zip(NAMES, (app, *ORIGINAL[1:])):
                assert repo[name]['origin'] == f'https://github.com/ClaudioMascaro/{name}.git', repo
                assert repo[name]['head'] == sha, (label, repo[name])
                assert re.fullmatch('[0-9a-f]{40}', repo[name]['tree']), repo

    def read(self, name):
        return self.archive.read(f'{self.root}/{name}')

    def json(self, name):
        return json.loads(self.read(name))

    def command(self, name):
        path = f'commands/{name}'
        return self.json(path + '.json')['exit'], self.read(path + '.out'), self.read(path + '.err')

    def source(self, pin, repo, path, *, symlink=False):
        with tarfile.open(fileobj=io.BytesIO(self.read(f'sources/{pin}/{repo}.tar'))) as archive:
            entry = archive.getmember(path)
            if symlink:
                assert entry.issym(), entry
                return entry.linkname
            assert entry.isfile(), entry
            return archive.extractfile(entry).read()

    def count(self, passed, failed):
        counts = self.json('result.json')['counts']
        assert counts == {'pass': passed, 'fail': failed, 'blocked': 0, 'timeout': 0, 'error': 0}, counts
        return [row['name'] for row in self.json('checks.json') if row['result'] != 'pass']


def symlinks(zipfile):
    modes = ('interactive', 'item-no-such-item', 'decider', 'prune')
    reads = ('list', 'dry', 'show', 'index', 'status', 'export')
    old, candidate = (Capture(zipfile, slot, rev)
                      for slot, rev in zip(('historical', 'candidate'), (OLD[0], HEADS[0])))
    assert len(old.count(49, 4)) == 4
    assert candidate.count(53, 0) == []
    for variant in ('linked', 'dangling', 'malformed', 'waiting-peer'):
        label = f'symlink-{variant}'
        pin = 'malformed' if variant == 'malformed' else 'linked'
        target = 'openapi.yaml' if pin == 'malformed' else 'orders/decisions.toml'
        want_target = candidate.source(pin, NAMES[0], 'meada.decisions.toml', symlink=True)
        assert want_target == f'../{NAMES[1]}/{target}', want_target
        want_bytes = b'' if variant == 'dangling' else candidate.source(pin, NAMES[1], target)
        for cap in (old, candidate):
            assert cap.json(f'decider/{label}/requests.json') == []
            assert cap.read(f'decider/{label}/final.toml') == want_bytes
            for mode in modes:
                exit_code, stdout, stderr = cap.command(f'{label}/write-{mode}')
                print('#932', cap.root, variant, mode, exit_code, stderr.decode().splitlines()[0])
                assert exit_code != 0 and stdout == b'' and b'meada.decisions.toml' in stderr
                if cap is old and variant == 'malformed':
                    assert b'symbolic link' not in stderr and re.search(rb'parse|expected|line 1', stderr, re.I), stderr
                else:
                    assert b'a symbolic link; Meada writes this file in the repository root only' in stderr, stderr
            for mode in reads:
                exit_code, stdout, stderr = cap.command(f'{label}/read-{mode}')
                assert b'a symbolic link' not in stderr, stderr
                if variant == 'malformed':
                    assert exit_code and re.search(rb'parse|expected|line 1', stderr, re.I), stderr
                else:
                    assert exit_code == 0, (variant, mode, exit_code, stderr)
                if variant == 'waiting-peer' and mode == 'list':
                    assert LIB.encode() in stdout and b'waiting' in stdout, stdout
        for mode in ('list', 'dry', 'index', 'status') if variant == 'linked' else ():
            assert candidate.command(f'{label}/read-{mode}') == candidate.command(f'{label}/regular-{mode}')
    export = candidate.json('commands/symlink-linked/read-export.out')
    links = [r for r in export['links'] if r['consumer'] == site('shadow')]
    assert len(links) == 1 and (links[0]['provider'], links[0]['contract'], links[0]['level']) == (PROVIDER, CONTRACT, 'decided'), links
    print('#932: 53 candidate keys. Raw commands, requests and target bytes verified. Link metadata and absence of a temp file come from the driver and Rust tests.')


def upgrade(zipfile):
    cap = Capture(zipfile, 'upgrade', HEADS[1], old=OLD[0], middle=OLD[1])
    assert cap.count(12, 0) == []
    assert b'base_url_env = ["SMOKE_ORDERS_URL"]' in cap.source('original', NAMES[0], 'meada.toml')
    assert b'  /orders:\n    post:' in cap.source('original', NAMES[1], 'openapi.yaml')
    assert b'platform(os string)' in cap.source('original', NAMES[0], 'orders/shadow/client.go')
    three = {'httpkey', 'neutral', 'oskey'}
    metadata = []
    for revision in ('middle', 'head'):
        cold = cap.json(f'commands/{revision}/cold-export.out')
        for phase in ('old-export', 'first-export', 'refresh-export', 'second-refresh-export', 'cold-export'):
            raw = cap.json(f'commands/{revision}/{phase}.out')
            wanted = {'neutral'} if phase == 'old-export' or revision == 'middle' and phase != 'cold-export' else three
            rows = [r for r in raw['links'] if r['consumer'] in {site(n) for n in three | {'shadow'}}]
            assert sorted((r['consumer'], r['provider'], r['contract'], r['level'], r['reason']) for r in rows) == sorted((site(n), PROVIDER, CONTRACT, 'inferred', REASON) for n in wanted), (revision, phase, rows)
            metadata.append((raw.get('commit'), raw.get('dirty')))
            if revision == 'head' and phase != 'old-export':
                assert raw == cold, (revision, phase)
            print('#946', revision, phase, [(r['consumer'], r['provider'], r['level'], r['reason']) for r in rows], 'metadata', metadata[-1])
    assert set(metadata) == {(ORIGINAL[0], False)}, metadata
    print('#946: 12 keys. Source-derived links and complete candidate JSON parity checked. The original upgrade slice does not store PRAGMA values.')


def status(zipfile):
    old, candidate = (Capture(zipfile, slot, rev)
                      for slot, rev in zip(('historical', 'candidate'), (OLD[2], HEADS[2])))
    assert len(old.count(4, 1)) == 1
    assert candidate.count(5, 0) == []
    assert old.command('status/healthy') == candidate.command('status/healthy')
    prefix = b'decisions and other workspace lines not evaluated:'
    for cap in (old, candidate):
        for name, expect in zip(('absent', 'healthy', 'old-schema', 'broken-lib', 'empty'), (1, 0, 1, 0, 1)):
            code, out, err = cap.command(f'status/{name}')
            print('#934', cap.root, name, code, out.decode())
            assert code == expect and not err, (code, out, err)
            lines = out.splitlines()
            warnings = [line for line in lines if line.startswith(prefix)]
            if name == 'absent':
                assert {line.split()[0].decode() for line in lines if line.endswith(b'not indexed')} == {APP, LIB, SHARED}
                assert len(warnings) == 1 and b'run meada index --workspace' in warnings[0]
            elif name == 'healthy':
                assert not warnings and {line.split()[0].decode() for line in lines if b' indexed,' in line} == {APP, LIB, SHARED}
            elif name == 'old-schema':
                assert [line.split()[0].decode() for line in lines if b'another version' in line] == [LIB]
                assert len(warnings) == 1 and b'run meada index --workspace' in warnings[0]
            elif name == 'broken-lib':
                expected = (LIB + ': ').encode() if cap is candidate else b''
                assert warnings == [prefix + b' ' + expected + b'no such table: symbols'], out
            else:
                assert out == b'workspace: 0 repositories\n', out
    print('#934: five manifest states; healthy bytes identical. SQLite immutability is recorded by the driver.')


for path, check in zip(sys.argv[1:], (symlinks, upgrade, status), strict=True):
    with open(path, 'rb') as data:
        print(Path(path).name, hashlib.file_digest(data, 'sha256').hexdigest())
    with ZipFile(path) as archive:
        assert archive.testzip() is None and len(set(archive.namelist())) == len(archive.namelist())
        check(archive)
