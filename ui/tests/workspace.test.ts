// @vitest-environment node
import { readFileSync } from 'node:fs';
import { beforeAll, expect, test, vi } from 'vitest';
import { initSync } from '../wasm/meada_core';
import { entriesText, workspaceVersionId } from '../wasm/meada_core';
import { applyAction, editorModel, entryRows, plainEntries, saveAction, teamApplyAction, teamEditorModel, tomlEscaped, versionText, ownerAt } from '../src/workspace';
import type { Authority, Entry } from '../src/workspace';

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

const text = (entries: object[]) => entriesText(JSON.stringify(entries));
const x: Entry = { repo: 'a.com/x', path: 'x', owner: 'team-a' };
const y: Entry = { repo: 'a.com/y', path: 'y', owner: 'team-b' };
const z: Entry = { repo: 'a.com/z', path: 'z' };
const who: Authority = { groups: ['team-a'], grant: ['a.com/w', 'a.com/x', 'a.com/y', 'a.com/z'] };

test('view annotations never enter a text, an id or the diff keys', () => {
  const held = [x, y, z];
  const annotated = entryRows(held, who);
  expect(annotated.map(row => row.may_change)).toEqual([true, false, false]);
  expect(versionText(annotated)).toBe(text(held));
  expect(versionText(annotated)).not.toMatch(/may_change|refusal/);
  expect(workspaceVersionId(versionText(annotated))).toBe(workspaceVersionId(text(held)));
  expect(plainEntries(annotated)).toEqual(held);
  const owned = editorModel(text([x, y, { ...z, owner: 'team-a' }]), versionText(annotated), 'en');
  expect(owned.diff?.changed).toEqual([{ repo: 'a.com/z', keys: ['repository.owner'] }]);
});

test('each entry is shown with the refusal of changing it alone', () => {
  expect(entryRows([x, y, z], who).map(({ repo, may_change, refusal }) => ({ repo, may_change, refusal }))).toEqual([
    { repo: 'a.com/x', may_change: true }, { repo: 'a.com/y', may_change: false, refusal: 'not_your_team' },
    { repo: 'a.com/z', may_change: false, refusal: 'ownerless' },
  ]);
});

test('every change is composed, diffed and authorized locally, and undoing a refusal clears it', () => {
  const fetch = vi.fn(); vi.stubGlobal('fetch', fetch);
  const held = [x, y, z];
  const model = (entries: Entry[]) => teamEditorModel(text(entries), held, who, 'en');
  expect(model(held).refusals).toEqual([]);
  expect(model([x, { ...y, path: 'changed' }, z]).refusals).toEqual([{ identity: 'a.com/y', change: 'edit', reason: 'not_your_team' }]);
  expect(model([{ ...x, owner: 'team-b' }, y, z]).refusals).toEqual([{ identity: 'a.com/x', change: 'edit', reason: 'owner_change' }]);
  expect(model([x, y, { ...z, path: 'changed' }]).refusals).toEqual([{ identity: 'a.com/z', change: 'edit', reason: 'ownerless' }]);
  expect(model([x, y, z, { repo: 'a.com/v', path: 'v', owner: 'team-a' }]).refusals).toEqual([{ identity: 'a.com/v', change: 'add', reason: 'outside_grant' }]);
  expect(model([x, y]).refusals).toEqual([{ identity: 'a.com/z', change: 'remove', reason: 'ownerless' }]);
  expect(teamEditorModel('[[repository]]\npath="x"\n', held, who, 'en').errors).toEqual(['entries:1: `repository.repo` is missing']);
  expect(fetch).not.toHaveBeenCalled(); vi.unstubAllGlobals();
});

test('the diff is of the composed version: the entries outside the grant stay in place and an addition goes last', () => {
  const outside: Entry = { repo: 'b.com/hidden', path: 'h', owner: 'team-z' };
  const held = [x, outside, y];
  const submitted = text([{ ...x, path: 'edited' }, { repo: 'a.com/w', path: 'w', owner: 'team-a' }]);
  const reading = { ...who, grant: ['a.com/w', 'a.com/x', 'a.com/y'] };
  const model = teamEditorModel(submitted, held, reading, 'en');
  expect(model.diff).toEqual({ added: ['a.com/w'], removed: ['a.com/y'], changed: [{ repo: 'a.com/x', keys: ['repository.path'] }] });
  expect(model.refusals).toEqual([{ identity: 'a.com/y', change: 'remove', reason: 'not_your_team' }]);
  expect(model.owners).toEqual(['team-a', 'team-b']);
  expect(teamEditorModel(text([x, y, { repo: 'a.com/w', path: 'w', owner: 'team-a' }]), held, reading, 'en').diff?.added).toEqual(['a.com/w']);
});

test('an apply is offered when nothing is refused over the entries the session reads, and names the teams it touches', () => {
  const versions = [{ id: '1111111111111111', author: 'a', time: '', message: '' }, { id: '2222222222222222', author: 'a', time: '', message: '' }];
  const held = [x, y, z];
  const older = teamApplyAction(versions[1]!, versions[0]!.id, versions, held, [{ ...x, path: 'older' }, y, z], who, 'en');
  expect(older).toEqual({ kind: 'apply', route: 'workspace/apply', body: { version: versions[1]!.id }, rollback: true, teams: ['team-a'] });
  expect(older).not.toHaveProperty('command');
  expect(teamApplyAction(versions[0]!, versions[1]!.id, versions, held, held, who, 'en')).toMatchObject({ rollback: false, teams: [] });
  expect(teamApplyAction(versions[1]!, versions[0]!.id, versions, held, [x, { ...y, path: 'changed' }, z], who, 'en')).toBeUndefined();
  expect(teamApplyAction(versions[1]!, versions[0]!.id, versions, held, [x, { ...y, owner: 'team-c' }, z], who, 'en')).toBeUndefined();
});

test('the owner picker reads the string being typed and escapes what it inserts', () => {
  const text = 'owner = "a\\"b';
  expect(ownerAt(text, text.length)).toEqual({ prefix: 'a"b', start: 9 });
  expect(ownerAt('owner = "a\\\\', 12)).toEqual({ prefix: 'a\\', start: 9 });
  expect(ownerAt('owner = "x"', 11)).toBeUndefined();
  expect(ownerAt('path = "x', 9)).toBeUndefined();
  expect(ownerAt('repo = "r"\nowner = "te', 22)).toEqual({ prefix: 'te', start: 20 });
  expect(tomlEscaped('a"b\\c')).toBe('a\\"b\\\\c');
});
