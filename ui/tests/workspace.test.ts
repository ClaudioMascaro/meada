// @vitest-environment node
import { readFileSync } from 'node:fs';
import { beforeAll, expect, test, vi } from 'vitest';
import { initSync } from '../wasm/meada_core';
import { editorModel, saveAction, applyAction } from '../src/workspace';

beforeAll(() => { initSync({ module: readFileSync(new URL('../wasm/meada_core_bg.wasm', import.meta.url)) }); });

test('every edit validates and computes its diff locally without a request', () => {
  const fetch = vi.fn(); vi.stubGlobal('fetch', fetch);
  const current = '[[repository]]\nrepo="a.com/x"\npath="x"\n';
  const invalid = editorModel('[[repository]]\npath="x"\n', current, 'en');
  expect(invalid.errors).toEqual(['entries:1: `repository.repo` is missing']);
  expect(invalid.diff).toBeUndefined();
  const edited = editorModel(current.replace('path="x"', 'path="changed"'), current, 'en');
  expect(edited.errors).toEqual([]);
  expect(edited.diff?.changed).toEqual([{ repo: 'a.com/x', keys: ['repository.path'] }]);
  expect(editorModel('', current, 'pt-BR').errors).toEqual([]);
  expect(editorModel('[[repository]]\npath="x"\n', current, 'pt-BR').errors).toEqual(['entries:1: falta `repository.repo`']);
  expect(fetch).not.toHaveBeenCalled(); vi.unstubAllGlobals();
});

test('save and rollback capture their exact payload and shell-quoted CLI command without a request', () => {
  const source = '[[repository]]\nrepo="a.com/x"\npath="x"\n';
  const save = saveAction(source, "team's rollout", "ops/team's workspace.toml");
  expect(save.body).toEqual({ entries: source, message: "team's rollout" });
  expect(save.command).toContain('meada workspace import <file> --message');
  expect(save.command).toContain(`'team'"'"'s rollout'`);
  expect(save.command).toContain(`--workspace 'ops/team'"'"'s workspace.toml'`);
  const versions = [{ id: '1111111111111111', author: 'alice', time: '', message: '' }, { id: '2222222222222222', author: 'bob', time: '', message: '' }];
  const apply = applyAction(versions[1]!, versions[0]!.id, versions, 'ops.toml');
  expect(apply.body).toEqual({ version: '2222222222222222' });
  expect(apply.rollback).toBe(true);
  expect(apply.command).toBe('meada workspace apply 2222222222222222 --workspace ops.toml');
  expect(applyAction(versions[0]!, versions[1]!.id, versions, 'ops.toml').rollback).toBe(false);
  expect(() => saveAction(source, '  ', 'ops.toml')).toThrow();
});
