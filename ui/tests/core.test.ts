// @vitest-environment node
import { readFileSync } from 'node:fs';
import { beforeAll, expect, test } from 'vitest';
import fixtures from '../../tests/fixtures/workspace-validation.json';
import {
  initSync, init, validate, workspaceVersionId, workspaceDiff, entriesText, authorizeEntries, composeEntries,
} from '../wasm/meada_core';

// One table, three kinds of rows: text validation, authorization and a team operator's save (#323).
const validations = fixtures.flatMap(row => row.text === undefined ? [] : [row]);
const authorizations = fixtures.flatMap(row => row.refused === undefined ? [] : [row]);
const compositions = fixtures.flatMap(row => row.composed === undefined ? [] : [row]);
const json = JSON.stringify;

beforeAll(() => {
  initSync({ module: readFileSync(new URL('../wasm/meada_core_bg.wasm', import.meta.url)) });
  init('en');
});

test.each(validations)('WebAssembly and CLI share validation: $name', row => {
  for (const locale of ['pt-BR', 'en'] as const) {
    init(locale);
    expect(validate(row.file, row.text)).toEqual(row.errors[locale]);
  }
});

test.each(authorizations)('WebAssembly and CLI share authorization: $name', row => {
  const groups = json(row.groups), grant = json(row.grant), current = json(row.current), entries = json(row.new);
  expect(JSON.parse(authorizeEntries(row.role, groups, grant, current, entries))).toEqual(row.refused);
  expect(JSON.parse(authorizeEntries('operator', groups, grant, current, entries))).toEqual([]);
});

test.each(compositions)('WebAssembly and CLI share a team operator\'s save: $name', row => {
  expect(JSON.parse(composeEntries(json(row.grant), json(row.current), json(row.new)))).toEqual(row.composed);
});

test('WebAssembly refuses arguments that are not the shared formats', () => {
  expect(() => authorizeEntries('owner', '[]', '"*"', '[]', '[]')).toThrow();
  expect(() => composeEntries('"a.com/x"', '[]', '[]')).toThrow();
});

test('WebAssembly uses the workspace entries to name a version', () => {
  expect(workspaceVersionId('[[repository]]\nrepo = "a.com/x"\npath = "x"\n')).toMatch(/^[a-f0-9]{16}$/);
  expect(workspaceVersionId('not a manifest')).toBeUndefined();
});

test('WebAssembly computes workspace diffs without a request', () => {
  expect(JSON.parse(workspaceDiff('', validations[0]!.text))).toEqual({
    added: ['a.com/x'], removed: [], changed: [],
  });
});

test('stored entries use the CLI TOML conversion and retain their version id', () => {
  const text = entriesText(JSON.stringify([{ repo: 'a.com/x', path: 'x', serve: { prefix: '/edge' },
    peer: [{ repo: 'a.com/api', base_url_env: ['API'] }] }]));
  expect(text).toContain('[[repository]]');
  expect(validate('entries', text)).toEqual([]);
  expect(JSON.parse(workspaceDiff(text, text))).toEqual({ added: [], removed: [], changed: [] });
  expect(workspaceVersionId(text)).toMatch(/^[0-9a-f]{16}$/);
});
