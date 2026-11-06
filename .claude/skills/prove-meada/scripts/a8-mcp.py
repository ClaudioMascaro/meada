#!/usr/bin/env python3
"""#683 query/refresh lessons on generated peer copies; never substitute parity for the spec."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import time
from prove import Stdio, Http, answer, free_port, mcp_answers

HERE = Path(__file__).resolve().parent


def save(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + '\n')


def export(binary, root, folder, label):
    proc = subprocess.run([binary, 'export'], cwd=root, capture_output=True, timeout=90)
    (folder / (label + '.export.json')).write_bytes(proc.stdout)
    (folder / (label + '.export.err')).write_bytes(proc.stderr)
    proc.check_returncode()
    return json.loads(proc.stdout)


def scope(source, dest, workspace):
    for name in (workspace, 'api', 'other', 'inventory'):
        shutil.copytree(source / name, dest / name, ignore=shutil.ignore_patterns('.meada', 'export.json'))
    return dest / workspace


def ordinary(head, base, source, folder):
    """The narrowed #683 context explanation must leave ordinary HTTP answers intact."""
    observations = []
    for side, binary in (('base', base), ('head', head)):
        root = scope(source, folder / side, 'g8')
        (root / 'meada.toml').write_text('[[peer]]\nrepo = "example.com/acme/api"\npath = "../api"\nbase_url_env = ["API_URL"]\n')
        (root.parent / 'api/meada.toml').write_text('[serve]\nhosts = ["api.example.com"]\nprefix = "/gateway"\n')
        (root / 'src/controls.ts').write_text(
            "export function ordersRelative() { return fetch('/orders'); }\n"
            "export function ordersHost() { return fetch('https://api.example.com/gateway/orders'); }\n")
        queries = [q for symbol in ('ordersTpl', 'ordersRelative', 'ordersHost') for q in (
            {'tool': 'context', 'arguments': {'symbol': symbol, 'budget': 16000}},
            {'tool': 'trace', 'arguments': {'from': symbol, 'direction': 'down', 'budget': 16000}})]
        tools, texts, _ = mcp_answers(binary, root, queries, folder / (side + '.stdio.err'))
        save(folder / (side + '.json'), {'queries': queries, 'tools': tools, 'answers': texts})
        observations.append(texts)
    assert observations[0] == observations[1], 'ordinary relative/host/template context or trace differs from base'


def refresh(binary, source, folder):
    root = scope(source, folder / 'work', 'g10b')
    # Map the second neutral peer explicitly, then edit ONLY the include. Same-size API/ALT
    # tokens force content freshness, rather than a convenient size change or source edit.
    manifest = root / 'meada.toml'
    manifest.write_text(manifest.read_text().replace('repo = "example.com/acme/other"\npath = "../other"',
        'repo = "example.com/acme/other"\npath = "../other"\nbase_url_env = ["ALT_URL"]'))
    partial = root / 'config/partials/acme.yaml'
    original = partial.read_bytes()
    backup = folder / 'partial.original'
    backup.write_bytes(original)
    client = Stdio(binary, root, folder / 'stdio.err')
    query = {'name': 'trace', 'arguments': {'from': 'ordersConfig', 'direction': 'down', 'budget': 12000}}
    try:
        before = answer(client.call('tools/call', query))
        before_export = export(binary, root, folder, 'before')
        partial.write_bytes(original.replace(b'API_URL', b'ALT_URL'))
        after = answer(client.call('tools/call', query))
        warm = export(binary, root, folder, 'warm')
        cold_root = scope(folder / 'work', folder / 'cold', 'g10b')
        cold = export(binary, cold_root, folder, 'cold')
        normalize = lambda doc: {k: v for k, v in doc.items() if k not in ('commit', 'dirty')}
        save(folder / 'answers.json', {'before': before, 'after': after})
        assert 'base URL from API_URL' in before and 'example.com/acme/api' in before, before
        assert 'base URL from ALT_URL' in after and 'example.com/acme/other' in after, after
        assert before != after and normalize(warm) == normalize(cold), 'include warm/cold full export mismatch'
        partial.write_bytes(original)
        subprocess.run(['cmp', str(backup), str(partial)], check=True)
        restored = answer(client.call('tools/call', query))
        restored_export = export(binary, root, folder, 'restored')
        save(folder / 'answers.json', {'before': before, 'after': after, 'restored': restored})
        assert restored == before and normalize(restored_export) == normalize(before_export), 'include restore differs'
    finally:
        partial.write_bytes(original)
        subprocess.run(['cmp', str(backup), str(partial)], check=True)
        client.close()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--head-bin', required=True)
    parser.add_argument('--base-bin', required=True)
    parser.add_argument('--run', required=True)
    parser.add_argument('--workspaces', help='focused maintenance only; omitted in the CI audit baseline')
    args = parser.parse_args()
    binary, run = str(Path(args.head_bin).resolve()), Path(args.run).resolve()
    run.mkdir(parents=True, exist_ok=False)
    os.environ['LANG'] = 'C.UTF-8'
    subprocess.run(['sh', str(HERE / 'a8-peers.sh'), 'gen', str(run / 'source')], check=True)
    rows = [line.split('\t') for line in (HERE / 'a8-cases.tsv').read_text().splitlines()
            if line and not line.startswith('#')]
    workspaces = args.workspaces.split(',') if args.workspaces else list(dict.fromkeys(row[0] for row in rows))
    checks = []
    for workspace in workspaces:
        folder = run / workspace
        folder.mkdir()
        root = scope(run / 'source', folder / 'work', workspace)
        selected = [row for row in rows if row[0] == workspace and row[3] != 'no consumer']
        symbols = list(dict.fromkeys(row[1] for row in selected))
        queries = [q for symbol in symbols for q in (
            {'tool': 'context', 'arguments': {'symbol': symbol, 'budget': 16000}},
            {'tool': 'trace', 'arguments': {'from': symbol, 'direction': 'down', 'budget': 16000}})]
        queries.append({'tool': 'search', 'arguments': {'query': 'http::GET::/orders', 'kind': 'contract', 'budget': 20000}})
        tools, texts, _ = mcp_answers(binary, root, queries, folder / 'stdio.err')
        save(folder / 'stdio.json', {'tools': tools, 'queries': queries, 'answers': texts})
        data = export(binary, root, folder, 'whole')
        for symbol in symbols:
            context, trace = texts[2 * symbols.index(symbol):2 * symbols.index(symbol) + 2]
            expected = [row for row in selected if row[1] == symbol]
            for row in expected:
                if row[3] not in ('null', 'no consumer') and row[2] == 'contract':
                    assert row[3] in context, (workspace, symbol, 'context contract', context)
                if row[5] != '-':
                    repository, _, reason = row[5].split(' ', 2)
                    assert reason in context, (workspace, symbol, 'context reason', reason, context)
                    assert repository in trace, (workspace, symbol, 'trace selected peer', trace)
            # Table and complete-export comparison remain independent drivers. Here the
            # closed-item assertion is over precisely the expected linked source sites.
        listed = subprocess.run([binary, 'decide', '--list'], cwd=root, capture_output=True, timeout=90)
        (folder / 'open-items.stdout').write_bytes(listed.stdout)
        (folder / 'open-items.stderr').write_bytes(listed.stderr)
        listed.check_returncode()
        for row in selected:
            if row[5] != '-':
                sites = [c['site'] for c in data['consumers'] if c['site'].split('::')[2] == row[1]]
                assert sites and all(site not in listed.stdout.decode() for site in sites), (workspace, row[1], listed.stdout)
        manifest = 'index_dir = "indexes"\n' + ''.join(
            f'[[repository]]\nrepo = "example.com/acme/{name}"\npath = "{folder / "work" / name}"\n'
            for name in (workspace, 'api', 'other', 'inventory'))
        (folder / 'workspace.toml').write_text(manifest)
        indexed = subprocess.run([binary, 'index', '--workspace', str(folder / 'workspace.toml')],
                                 cwd=folder, capture_output=True, timeout=90)
        (folder / 'index.stdout').write_bytes(indexed.stdout)
        (folder / 'index.stderr').write_bytes(indexed.stderr)
        indexed.check_returncode()
        port = free_port()
        with (folder / 'http.stdout').open('w') as out, (folder / 'http.stderr').open('w') as err:
            server = subprocess.Popen([binary, 'mcp', '--listen', f'127.0.0.1:{port}', '--workspace', str(folder / 'workspace.toml')],
                                      cwd=folder, stdin=subprocess.DEVNULL, stdout=out, stderr=err)
            save(folder / 'http.pid.json', {'pid': server.pid})
            try:
                for _ in range(100):
                    try:
                        socket.create_connection(('127.0.0.1', port), timeout=.2).close()
                        break
                    except OSError:
                        if server.poll() is not None:
                            raise RuntimeError('HTTP server exited before readiness')
                        time.sleep(.1)
                client = Http(f'http://127.0.0.1:{port}/example.com/acme/{workspace}')
                http_tools = client.call('tools/list', {})['result']['tools']
                answers = [answer(client.call('tools/call', {'name': q['tool'], 'arguments': q['arguments']})) for q in queries]
                save(folder / 'http.json', {'tools': http_tools, 'answers': answers})
                assert (http_tools, answers) == (tools, texts), (workspace, 'whole HTTP/stdio replies differ')
            finally:
                server.terminate()
                server.wait(timeout=30)
                save(folder / 'http.cleanup.json', {'pid': server.pid, 'exit': server.returncode})
        checks.append({'workspace': workspace, 'queries': len(queries), 'passed': True})
        save(run / 'checks.json', {'checks': checks, 'checker': 'script only, not independent or blind'})
    refresh(binary, run / 'source', run / 'include-refresh')
    ordinary(binary, str(Path(args.base_bin).resolve()), run / 'source', run / 'ordinary')
    with Path(binary).open('rb') as stream:
        binary_hash = hashlib.file_digest(stream, 'sha256').hexdigest()
    save(run / 'checks.json', {'checks': checks, 'include_only_refresh': True, 'ordinary_base_answer_parity': True,
                              'binary_sha256': binary_hash, 'checker': 'script only, not independent or blind'})
    print('A8 query, HTTP and partial-only refresh checks passed for ' + ','.join(workspaces))


if __name__ == '__main__':
    main()
