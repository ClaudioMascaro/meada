// @vitest-environment node
import { readFileSync } from 'node:fs';
import { beforeAll, expect, test } from 'vitest';
import { initSync, init, validate, workspaceVersionId } from '../wasm/meada_core';

beforeAll(() => {
  initSync({ module: readFileSync(new URL('../wasm/meada_core_bg.wasm', import.meta.url)) });
  init('en');
});

test.each([
  ['[[repository]]\nrepo = "a.com/x"\npath = "x"\n', []],
  ['[[repository]]\npath = "x"\n', ['ops/meada.workspace.toml:1: `repository.repo` is missing']],
  ['[[repository]]\nrepo = "a.com/x"\npath = "x"\nbranch = "main"\n',
    ['ops/meada.workspace.toml:4: `repository.branch` is not a meada.workspace.toml key']],
  ['[[repository]]\nrepo = "a.com/x"\npath = "x"\n[repository.serve]\nprefix = "edge"\n',
    ['ops/meada.workspace.toml:5: `repository.serve.prefix`: `edge` must start with `/`']],
])('WebAssembly validates workspace text with the CLI messages (%s)', (source, errors) => {
  expect(validate('ops/meada.workspace.toml', source)).toEqual(errors);
});

test('WebAssembly uses the workspace entries to name a version', () => {
  expect(workspaceVersionId('[[repository]]\nrepo = "a.com/x"\npath = "x"\n')).toMatch(/^[a-f0-9]{16}$/);
  expect(workspaceVersionId('not a manifest')).toBeUndefined();
});
