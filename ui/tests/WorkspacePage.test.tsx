import { readFileSync } from 'node:fs';
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { fireEvent, getByRole, queryAllByRole, queryByRole, waitFor } from '@testing-library/dom';
import { afterEach, beforeAll, expect, test, vi } from 'vitest';
import { entriesText, initSync, workspaceVersionId } from '../wasm/meada_core';
import { WorkspacePage } from '../src/WorkspacePage';
import { Api } from '../src/api';
import { Auth } from '../src/auth';
import config from './fixtures/config.json';
import { workspaceCopy } from '../src/workspace-i18n';

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const current = { id: '1111111111111111', author: 'alice', time: '2026-10-23T10:00:00Z', message: 'Current workspace', repositories: [{ repo: 'a.com/x', path: 'current' }] };
const older = { ...current, id: '2222222222222222', author: 'bob', time: '2026-10-22T10:00:00Z', message: 'Earlier workspace', repositories: [{ repo: 'a.com/x', path: 'older' }] };
const host = document.createElement('div'); document.body.append(host);
const root = createRoot(host);
beforeAll(() => { initSync({ module: readFileSync('wasm/meada_core_bg.wasm') });
  for (const version of [current, older]) {
    const id = workspaceVersionId(entriesText(JSON.stringify(version.repositories)));
    if (!id) throw new Error('invalid workspace fixture');
    version.id = id;
  }
  HTMLDialogElement.prototype.showModal = function () { this.open = true; };
  HTMLDialogElement.prototype.close = function () { this.open = false; };
});
afterEach(async () => { await act(() => root.render(null)); vi.unstubAllGlobals(); });

function reads() {
  return vi.fn(async (url: string) => {
    if (url === '/api/v1/workspace') return Response.json({ current: { id: current.id, fence: '18446744073709551615' }, version: current });
    if (url === '/api/v1/workspace/versions') return Response.json({ current: { id: current.id, fence: '18446744073709551615' }, versions: [current, older], malformed: ['bad-version.json'] });
    if (url === `/api/v1/workspace/versions/${older.id}`) return Response.json(older);
    throw new Error(`unexpected GET ${url}`);
  });
}

test.each([{ role: 'viewer' as const, operations: true }, { role: 'operator' as const, operations: false }])('read-only $role can compare versions and sees no mutations', async ({ role, operations }) => {
  const fetch = reads(); vi.stubGlobal('fetch', fetch);
  await act(() => root.render(<WorkspacePage api={new Api(new Auth(config))} config={{ ...config, operations }} session={{ role, subject: 'reader' }} locale="en" />));
  await waitFor(() => expect(getByRole(host, 'heading', { name: 'Workspace versions' })).toBeTruthy());
  expect(host.textContent).toContain('18446744073709551615');
  expect(host.textContent).toContain('alice'); expect(host.textContent).toContain('bad-version.json');
  const from = getByRole(host, 'combobox', { name: 'From version' });
  await act(() => fireEvent.change(from, { target: { value: older.id } }));
  await waitFor(() => expect(getByRole(host, 'region', { name: 'Version diff' }).textContent).toContain('repository.path'));
  expect(queryByRole(host, 'button', { name: 'Save version' })).toBeNull();
  expect(queryByRole(host, 'button', { name: /Apply|Roll back/ })).toBeNull();
  expect(getByRole(host, 'textbox', { name: 'Version entries' }).getAttribute('readonly')).not.toBeNull();
});

test('operator save freezes entries/message, waits for confirmation and shows the CLI holder refusal', async () => {
  const read = reads();
  const fetch = vi.fn(async (url: string, init?: RequestInit) => {
    if (init?.method === 'POST') return Response.json({ error: { code: 'conflict', message: 'lock held by indexer / PID 42' } }, { status: 409 });
    if (url === '/api/v1/manifest') return Response.json({ file: "ops/team's workspace.toml", manifest: {}, modules: [], errors: [] });
    return read(url);
  });
  vi.stubGlobal('fetch', fetch);
  await act(() => root.render(<WorkspacePage api={new Api(new Auth(config))} config={{ ...config, operations: true }} session={{ role: 'operator', subject: 'alice' }} locale="en" />));
  const form = await waitFor(() => getByRole(host, 'form', { name: 'Workspace editor' }));
  const entries = getByRole(form, 'textbox', { name: 'Workspace entries' });
  const message = getByRole(form, 'textbox', { name: 'Save message' });
  const count = fetch.mock.calls.length;
  await act(() => fireEvent.change(entries, { target: { value: '[[repository]]\npath="x"\n' } }));
  expect(getByRole(form, 'alert').textContent).toContain('entries:1: `repository.repo` is missing');
  const source = '[[repository]]\nrepo="a.com/x"\npath="new"\n';
  await act(() => { fireEvent.change(entries, { target: { value: source } }); fireEvent.change(message, { target: { value: 'first message' } }); });
  expect(fetch.mock.calls.length).toBe(count);
  const save = getByRole(form, 'button', { name: 'Save version' }); save.focus();
  await act(() => fireEvent.click(save));
  let dialog = getByRole(host, 'dialog', { name: 'Save version' });
  expect(dialog.textContent).toContain('meada workspace import <file> --message');
  expect(dialog.textContent).toContain('first message');
  expect(fetch.mock.calls.filter(([, init]) => init?.method === 'POST')).toHaveLength(0);
  await act(() => fireEvent(dialog, new Event('cancel', { cancelable: true })));
  expect(queryByRole(host, 'dialog')).toBeNull(); expect(document.activeElement).toBe(save);
  await act(() => fireEvent.click(save));
  dialog = getByRole(host, 'dialog', { name: 'Save version' });
  await act(() => { fireEvent.change(message, { target: { value: 'later message' } }); fireEvent.change(entries, { target: { value: '' } }); });
  await act(() => fireEvent.click(getByRole(dialog, 'button', { name: 'Confirm save' })));
  const posts = fetch.mock.calls.filter(([, init]) => init?.method === 'POST');
  expect(posts).toHaveLength(1);
  expect(posts[0]?.[0]).toBe('/api/v1/workspace/versions');
  expect(JSON.parse(String(posts[0]?.[1]?.body))).toEqual({ entries: source, message: 'first message' });
  expect(getByRole(dialog, 'alert').textContent).toBe('lock held by indexer / PID 42');
});

test('an older apply is a rollback for the next refresh, confirms once and keeps success if the read refresh fails', async () => {
  const read = reads(); let written = false;
  const fetch = vi.fn(async (url: string, init?: RequestInit) => {
    if (init?.method === 'POST') { written = true; return Response.json({ result: 'workspace pointer applied' }); }
    if (url === '/api/v1/manifest') return Response.json({ file: 'ops.toml', manifest: {}, modules: [], errors: [] });
    if (written) return Response.json({ error: { code: 'unavailable', message: 'store unavailable after apply' } }, { status: 503 });
    return read(url);
  });
  vi.stubGlobal('fetch', fetch);
  await act(() => root.render(<WorkspacePage api={new Api(new Auth(config))} config={{ ...config, operations: true }} session={{ role: 'operator', subject: 'alice' }} locale="en" />));
  const rollback = await waitFor(() => getByRole(host, 'button', { name: `Roll back workspace ${older.id}` }));
  await act(() => fireEvent.click(rollback));
  let dialog = getByRole(host, 'dialog', { name: `Roll back workspace for the next refresh: ${older.id}` });
  expect(dialog.textContent).toContain(`meada workspace apply ${older.id} --workspace ops.toml`);
  expect(fetch.mock.calls.filter(([, init]) => init?.method === 'POST')).toHaveLength(0);
  await act(() => fireEvent.click(getByRole(dialog, 'button', { name: 'Cancel' })));
  expect(fetch.mock.calls.filter(([, init]) => init?.method === 'POST')).toHaveLength(0);
  await act(() => fireEvent.click(rollback));
  dialog = getByRole(host, 'dialog');
  await act(() => fireEvent.click(getByRole(dialog, 'button', { name: 'Confirm rollback' })));
  await waitFor(() => expect(queryByRole(host, 'dialog')).toBeNull());
  const posts = fetch.mock.calls.filter(([, init]) => init?.method === 'POST');
  expect(posts).toHaveLength(1); expect(posts[0]?.[0]).toBe('/api/v1/workspace/apply');
  expect(JSON.parse(String(posts[0]?.[1]?.body))).toEqual({ version: older.id });
  expect(host.textContent).toContain('workspace pointer applied');
  await waitFor(() => expect(getByRole(host, 'alert').textContent).toBe('store unavailable after apply'));
});

test('saving existing entries shows the first author and message, while a 422 keeps the server lines and keys', async () => {
  const read = reads(); let reject = false;
  const serverError = 'entries:9: `repository.peer.repo` is missing\nentries:12: `repository.serve.prefix`: `edge` must start with `/`';
  const fetch = vi.fn(async (url: string, init?: RequestInit) => {
    if (init?.method === 'POST') return reject ? Response.json({ error: { code: 'invalid_entries', message: serverError } }, { status: 422 }) : Response.json({ result: current.id });
    if (url === '/api/v1/manifest') return Response.json({ file: 'ops.toml', manifest: {}, modules: [], errors: [] });
    if (url.startsWith('/api/v1/workspace/versions/')) return Response.json({ ...current, id: url.split('/').at(-1) });
    return read(url);
  });
  vi.stubGlobal('fetch', fetch);
  await act(() => root.render(<WorkspacePage api={new Api(new Auth(config))} config={{ ...config, operations: true }} session={{ role: 'operator', subject: 'second-author' }} locale="en" />));
  const form = await waitFor(() => getByRole(host, 'form', { name: 'Workspace editor' }));
  await act(() => fireEvent.change(getByRole(form, 'textbox', { name: 'Save message' }), { target: { value: 'attempted replacement' } }));
  await act(() => fireEvent.click(getByRole(form, 'button', { name: 'Save version' })));
  await act(() => fireEvent.click(getByRole(getByRole(host, 'dialog'), 'button', { name: 'Confirm save' })));
  const saved = await waitFor(() => getByRole(host, 'region', { name: 'Saved version' }));
  expect(saved.textContent).toContain('alice'); expect(saved.textContent).toContain('Current workspace');
  expect(saved.textContent).not.toContain('second-author'); expect(saved.textContent).not.toContain('attempted replacement');
  reject = true;
  await act(() => fireEvent.click(getByRole(form, 'button', { name: 'Save version' })));
  const dialog = getByRole(host, 'dialog');
  await act(() => fireEvent.click(getByRole(dialog, 'button', { name: 'Confirm save' })));
  expect(getByRole(dialog, 'alert').textContent).toBe(serverError);
});

test('a successful save retains a failed immutable metadata read and retries it without another write', async () => {
  const read = reads(); let metadataReads = 0;
  const source = '[[repository]]\nrepo="a.com/x"\npath="new-metadata"\n';
  const savedId = workspaceVersionId(source);
  const fetch = vi.fn(async (url: string, init?: RequestInit) => {
    if (init?.method === 'POST') return Response.json({ result: savedId });
    if (url === '/api/v1/manifest') return Response.json({ file: 'ops.toml', manifest: {}, modules: [], errors: [] });
    if (url.startsWith('/api/v1/workspace/versions/')) {
      metadataReads++;
      return metadataReads === 1 ? Response.json({ error: { code: 'unavailable', message: 'immutable version metadata unavailable' } }, { status: 503 }) : Response.json({ ...current, id: url.split('/').at(-1) });
    }
    return read(url);
  });
  vi.stubGlobal('fetch', fetch);
  await act(() => root.render(<WorkspacePage api={new Api(new Auth(config))} config={{ ...config, operations: true }} session={{ role: 'operator', subject: 'alice' }} locale="en" />));
  const form = await waitFor(() => getByRole(host, 'form', { name: 'Workspace editor' }));
  await act(() => fireEvent.change(getByRole(form, 'textbox', { name: 'Workspace entries' }), { target: { value: source } }));
  await act(() => fireEvent.change(getByRole(form, 'textbox', { name: 'Save message' }), { target: { value: 'save once' } }));
  await act(() => fireEvent.click(getByRole(form, 'button', { name: 'Save version' })));
  await act(() => fireEvent.click(getByRole(getByRole(host, 'dialog'), 'button', { name: 'Confirm save' })));
  await waitFor(() => expect(queryByRole(host, 'dialog')).toBeNull());
  expect(host.textContent).toContain(savedId);
  await waitFor(() => expect(getByRole(host, 'alert').textContent).toBe('immutable version metadata unavailable'));
  await act(() => fireEvent.click(getByRole(host, 'button', { name: 'Refresh versions' })));
  await waitFor(() => expect(getByRole(host, 'region', { name: 'Saved version' }).textContent).toContain('alice'));
  expect(fetch.mock.calls.filter(([, init]) => init?.method === 'POST')).toHaveLength(1);
});

test.each(['pt-BR', 'en'] as const)('editor, list, diffs and empty-workspace confirmation are labelled for keyboard use in %s', async locale => {
  const read = reads(); const t = workspaceCopy[locale];
  const fetch = vi.fn(async (url: string) => url === '/api/v1/manifest' ? Response.json({ file: 'ops.toml', manifest: {}, modules: [], errors: [] }) : read(url));
  vi.stubGlobal('fetch', fetch);
  await act(() => root.render(<WorkspacePage api={new Api(new Auth(config))} config={{ ...config, operations: true }} session={{ role: 'operator', subject: 'alice' }} locale={locale} />));
  const form = await waitFor(() => getByRole(host, 'form', { name: t.editor }));
  await act(() => { fireEvent.change(getByRole(form, 'textbox', { name: t.editEntries }), { target: { value: '' } }); fireEvent.change(getByRole(form, 'textbox', { name: t.saveMessage }), { target: { value: 'empty workspace' } }); });
  const controls = ['button', 'textbox', 'combobox'].flatMap(role => queryAllByRole(host, role, { name: /\S/ }));
  expect(controls.length).toBe(host.querySelectorAll('button, input, textarea, select').length);
  for (const control of controls) expect(control.tabIndex).toBeGreaterThanOrEqual(0);
  for (const label of [t.versions, t.diff, t.editorDiff, t.validation]) expect(getByRole(host, 'region', { name: label }).tabIndex).toBe(0);
  const save = getByRole(form, 'button', { name: t.save });
  await waitFor(() => expect(save.hasAttribute('disabled')).toBe(false));
  await act(() => fireEvent.click(save));
  const dialog = getByRole(host, 'dialog', { name: t.save });
  expect(getByRole(dialog, 'button', { name: t.confirmSave }).tabIndex).toBe(0);
  await act(() => fireEvent.click(getByRole(dialog, 'button', { name: t.cancel })));
  expect(queryByRole(host, 'dialog')).toBeNull();
  expect(Object.keys(workspaceCopy['pt-BR']).sort()).toEqual(Object.keys(workspaceCopy.en).sort());
  expect(Object.values(t).every(value => value.trim().length > 0)).toBe(true);
});
