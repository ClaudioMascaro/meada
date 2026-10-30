// @vitest-environment node
import { readFileSync } from 'node:fs';
import { beforeAll, expect, test } from 'vitest';
import table from '../../tests/fixtures/named-workspaces.json';
import { init, initSync, namedAuthorize, namedChanged, namedDiff, namedEntriesJson, namedEntriesText } from '../wasm/meada_core';
import { diffLines, entriesJson, lines, normalize, removeAt, replaceAt } from '../src/guided';
import type { Entry, NamedDiff } from '../src/guided';
import { guidedCopy } from '../src/guided-i18n';

// The guided form's model only reshapes entries; the checks are `meada-core`'s, through the actual
// WebAssembly bundle. Y/A/B are the identities of the table the Rust tests read.
const identities = table.identities as Record<string, string>;
const Y = identities.Y!, A = identities.A!, B = identities.B!;
const json = JSON.stringify;
const read = (text: string) => JSON.parse(namedEntriesJson(text, 10)) as Entry[];

beforeAll(() => {
  initSync({ module: readFileSync(new URL('../wasm/meada_core_bg.wasm', import.meta.url)) });
  init('en');
});

const full: Entry[] = [
  {
    repo: Y,
    serve: { hosts: ['y.example'], prefix: '/api', strip: '/v1' },
    resource: [{ kind: 'postgres', id: 'orders', role: 'read-write', via: ['src/db.rs::Orders'] }],
    peer: [{ repo: A, base_url_env: ['A_URL', 'A_FALLBACK_URL'] }],
  },
  { repo: A, serve: {} },
];

test('the form and the text are one document: every form state reads back from its canonical text unchanged', () => {
  const text = namedEntriesText(entriesJson(full));
  expect(read(text)).toEqual(full);
  // The text written from what was read is the same text: no edit in either view changes the other's meaning.
  expect(namedEntriesText(json(read(text)))).toBe(text);
  // An emptied field is an absent field, and an undeclared serve block is absent from the text.
  const emptied = normalize({ repo: Y, serve: { hosts: [], prefix: '' }, resource: [], peer: [{ repo: A, base_url_env: [] }] });
  expect(emptied).toEqual({ repo: Y, serve: {}, peer: [{ repo: A }] });
  expect(namedEntriesText(entriesJson([{ repo: Y }]))).not.toContain('serve');
  expect(namedEntriesText(entriesJson([{ repo: Y, serve: {} }]))).toContain('[repository.serve]');
});

test('what an entry may hold is the core\'s decision, not the form\'s: it refuses a value, a bad kind and a path-less prefix', () => {
  const bad: Entry[][] = [
    [{ repo: Y, peer: [{ repo: A, base_url_env: ['A_URL=https://secret.example'] }] }],
    [{ repo: Y, resource: [{ kind: 'Postgres', id: 'x', role: 'read' }] }],
    [{ repo: Y, resource: [{ kind: '', id: 'x', role: 'read' }] }],
    [{ repo: Y, serve: { prefix: 'no-slash' } }],
    [{ repo: Y }, { repo: Y }],
  ];
  for (const entries of bad) expect(() => namedEntriesJson(namedEntriesText(entriesJson(entries)), 10), json(entries)).toThrow();
  // A text editor reaches the same refusals for what the form never offers.
  expect(() => namedEntriesJson(`[[repository]]\nrepo = "${Y}"\nowner = "g"\n`, 10)).toThrow(/reserved/);
  expect(() => namedEntriesJson(`[[repository]]\nrepo = "${Y}"\n[repository.serve]\ndocuments = ["d"]\n`, 10)).toThrow(/reserved/);
});

test('list helpers keep what a textarea means: one value per line, trimmed', () => {
  expect(lines(' a \n\n b\n')).toEqual(['a', 'b']);
  expect(lines('')).toEqual([]);
  expect(replaceAt([1, 2, 3], 1, 9)).toEqual([1, 9, 3]);
  expect(removeAt([1, 2, 3], 0)).toEqual([2, 3]);
});

test('the diff is the core\'s, in words, in both languages and never by color', () => {
  const before = read(namedEntriesText(entriesJson([{ repo: Y }, { repo: A }])));
  const after = read(namedEntriesText(entriesJson([{ repo: Y, peer: [{ repo: A }] }, { repo: B }])));
  const diff = JSON.parse(namedDiff(json(before), json(after))) as NamedDiff;
  expect(diff).toEqual({ added: [B], removed: [A], changed: [{ repo: Y, keys: ['peer'] }] });
  expect(diffLines(diff, guidedCopy.en)).toEqual([`Added: ${B}`, `Removed: ${A}`, `Changed: ${Y} (peers)`]);
  expect(diffLines(diff, guidedCopy['pt-BR'])).toEqual([`Adicionado: ${B}`, `Removido: ${A}`, `Alterado: ${Y} (peers)`]);
  expect(diffLines({ added: [], removed: [], changed: [] }, guidedCopy.en)).toEqual([]);
  // The identities to recheck are the ones the diff adds or changes: a removal is not rechecked.
  expect(JSON.parse(namedChanged(json(before), json(after)))).toEqual([Y, B]);
});

// The Y/A/B matrix, through the edit path: an edit of a workspace holding Y touches (and so must find
// eligible) only the identities it adds or changes, and the table decides each.
test.each(table.creation.filter(row => ['alice', 'bob'].includes(row.caller)))('an edit by $caller that selects $select is allowed: $allowed', row => {
  const callers = table.callers as Record<string, { rows: { identity: string; role: string }[] }>;
  const eligible = callers[row.caller]!.rows.filter(r => table.accepted_roles.includes(r.role)).map(r => r.identity);
  const current = [{ repo: 'Y' }];
  const next = row.select.map((repo: string) => ({ repo }));
  const touched = JSON.parse(namedChanged(json(current), json(next))) as string[];
  const got = JSON.parse(namedAuthorize(json(touched), json(eligible), json(eligible))) as { allowed: boolean };
  expect(touched).toEqual(row.select.filter((identity: string) => identity !== 'Y'));
  expect(got.allowed).toBe(row.allowed);
});

test('guided copy has the same keys in both console languages', () => {
  expect(Object.keys(guidedCopy['pt-BR']).sort()).toEqual(Object.keys(guidedCopy.en).sort());
});
