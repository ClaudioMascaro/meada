// @vitest-environment node
import { readFileSync } from 'node:fs';
import { beforeAll, expect, test } from 'vitest';
import fixtures from '../../tests/fixtures/workspace-validation.json';
import { initSync, init, validate, workspaceVersionId, workspaceDiff, entriesText } from '../wasm/meada_core';

beforeAll(() => {
  initSync({ module: readFileSync(new URL('../wasm/meada_core_bg.wasm', import.meta.url)) });
  init('en');
});

test.each(fixtures)('WebAssembly and CLI share validation: $name', row => {
  for (const locale of ['pt-BR', 'en'] as const) {
    init(locale);
    expect(validate(row.file, row.text)).toEqual(row.errors[locale]);
  }
});

test('WebAssembly uses the workspace entries to name a version', () => {
  expect(workspaceVersionId('[[repository]]\nrepo = "a.com/x"\npath = "x"\n')).toMatch(/^[a-f0-9]{16}$/);
  expect(workspaceVersionId('not a manifest')).toBeUndefined();
});

test('WebAssembly computes workspace diffs without a request', () => {
  expect(JSON.parse(workspaceDiff('', fixtures[0]!.text))).toEqual({
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
