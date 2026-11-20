#!/usr/bin/env python3
"""Check #894's independently keyed Java and Go transaction entry sides."""
import argparse
import collections
import json
from pathlib import Path
import re
import shutil
import subprocess

from proof_result import Comparisons, run_driver
from prove import fetch, git, infra, mcp_answers, ROOT

HERE = Path(__file__).resolve().parent
DYNAMODB = 'resource::dynamodb'
MCP_SIDE = re.compile(r'(\S+) ([^:]+):[0-9]+ (?:(produces|consumes) )?(resource::dynamodb(?:::\S+)?)(?: |$)')
CONTEXT = {
    'immutableList': 'TransactionExamples.immutableList()',
    'getList': 'TransactionExamples.getList()',
    'deferredWrite': 'deferredWrite',
    'deferredControl': 'deferredControl',
}
CONTEXT_ROLES = {'writer': 'produces', 'reader': 'consumes'}


def key(source):
    rows = []
    for line in (source / 'README.md').read_text().splitlines():
        if line.startswith(('| Transactions.java |', '| entries.go |')):
            file, name, role, table = (cell.strip() for cell in line.split('|')[1:-1])
            rows.append((file, name, role, DYNAMODB if table == 'unresolved' else f'{DYNAMODB}::{table}'))
    if not rows:
        raise ValueError('transaction answer key is empty')
    return sorted(rows)


def sides(data):
    result = []
    for role, field in [('writer', 'providers'), ('reader', 'consumers')]:
        for row in data[field]:
            if row['contract'].split('::')[:2] == ['resource', 'dynamodb']:
                _, file, function, *_ = row['site'].split('::')
                result.append((file, function.split('(')[0].rsplit('.', 1)[-1], role, row['contract']))
    return sorted(result)


def mcp_sides(answer):
    return [match.groups() for line in answer.splitlines() if (match := MCP_SIDE.match(line))]


def selftest():
    source = HERE / 'dynamodb-transactions'
    oracle = key(source)
    assert len(oracle) == 21 and len(set(oracle)) == len(oracle)
    for file, name, _, _ in oracle:
        assert name + '(' in (source / file).read_text()
    for file, name, role, contract in oracle:
        field = 'providers' if role == 'writer' else 'consumers'
        doc = {'providers': [], 'consumers': []}
        doc[field] = [{'site': f'fixture::{file}::{name}::sdk#0', 'contract': contract}]
        assert sides(doc) == [(file, name, role, contract)]
        doc[field][0]['contract'] = DYNAMODB + '::wrong-table'
        assert sides(doc) != [(file, name, role, contract)]
    expected = [('example.com/transactions', 'Transactions.java', 'consumes', DYNAMODB + '::lookup')]
    reply = 'example.com/transactions Transactions.java:43 consumes resource::dynamodb::lookup (unresolved)'
    assert mcp_sides(reply) == expected
    assert mcp_sides('no symbol matches getList') != expected
    assert mcp_sides(reply.replace('::lookup', '::wrong-table')) != expected
    print('DynamoDB transaction selftest: 21 source-keyed sides, wrong table and missing context rejected')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--selftest', action='store_true')
    parser.add_argument('--head-bin')
    parser.add_argument('--repos', default='dynamodb-transactions')
    parser.add_argument('--run')
    args = parser.parse_args()
    if args.selftest:
        selftest()
        return
    if not args.head_bin or not args.run:
        parser.error('--head-bin and --run are required')
    run = Path(args.run).resolve()
    run.mkdir(parents=True)
    binary = str(Path(args.head_bin).resolve())
    comparisons = Comparisons(run)
    for name in args.repos.split(','):
        source = run / 'input' / name
        if name not in infra() or not infra()[name][1].startswith('probe:'):
            raise ValueError(f'unknown fixture: {name}')
        fetch(name, source)
        expected = key(source)
        shutil.copy(source / 'README.md', run / f'{name}-answer-key.md')
        (run / f'{name}-identity.json').write_text(json.dumps({
            'driver_revision': git('rev-parse', 'HEAD', cwd=ROOT).strip(),
            'input_tree': git('rev-parse', 'HEAD^{tree}', cwd=source).strip(),
            'input_commit': git('rev-parse', 'HEAD', cwd=source).strip(),
        }, indent=2))
        for command in ('index', 'export'):
            proc = subprocess.run([binary, command], cwd=source, capture_output=True, text=True, timeout=90)
            (run / f'{name}-{command}.out').write_text(proc.stdout)
            (run / f'{name}-{command}.err').write_text(proc.stderr)
            proc.check_returncode()
        actual = sides(json.loads(proc.stdout))
        expected_counts, actual_counts = collections.Counter(expected), collections.Counter(actual)
        for row in sorted(expected_counts.keys() | actual_counts.keys()):
            name_at = f'{row[0]} {row[1]} {row[2]} {row[3]}'
            comparisons.compare(name_at, expected_counts[row], actual_counts[row], oracle=True)
        (run / f'{name}-sides.json').write_text(json.dumps({'expected': expected, 'actual': actual}, indent=2))
        repository = f'github.com/zz-prove/{name}'
        queries, reply_key = [], []
        for file, function, role, contract in expected:
            if contract != DYNAMODB:
                queries.append({'tool': 'search', 'arguments': {'query': contract, 'kind': 'resource', 'budget': 6000}})
                reply_key.append((repository, file, None, contract))
            if function in CONTEXT:
                symbol = f'{repository}::{file}::{CONTEXT[function]}'
                queries.append({'tool': 'context', 'arguments': {'symbol': symbol, 'budget': 6000}})
                reply_key.append((repository, file, CONTEXT_ROLES[role], contract))
        tools, answers, _ = mcp_answers(binary, source, queries, run / f'{name}-stdio.err')
        (run / f'{name}-mcp.json').write_text(json.dumps({
            'queries': queries, 'catalog': tools, 'answers': answers, 'expected_sides': reply_key,
        }, indent=2))
        for query, wanted, answer in zip(queries, reply_key, answers, strict=True):
            target = query['arguments'].get('query') or query['arguments']['symbol']
            comparisons.compare(f'MCP {query["tool"]} {target}', [wanted], mcp_sides(answer), oracle=True)
    return comparisons.exit_code()


if __name__ == '__main__':
    run_driver(main)
