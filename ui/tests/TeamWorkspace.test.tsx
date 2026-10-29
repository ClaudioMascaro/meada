import { readFileSync } from 'node:fs';
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { fireEvent, getByRole, queryByRole, waitFor, within } from '@testing-library/dom';
import { afterEach, beforeAll, expect, test, vi } from 'vitest';
import { entriesText, initSync } from '../wasm/meada_core';
import { WorkspacePage } from '../src/WorkspacePage';
import { Api } from '../src/api';
import type { Session } from '../src/api';
import { Auth } from '../src/auth';
import config from './fixtures/config.json';
import { workspaceCopy } from '../src/workspace-i18n';

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const host = document.createElement('div'); document.body.append(host);
const root = createRoot(host);
beforeAll(() => {
  initSync({ module: readFileSync('wasm/meada_core_bg.wasm') });
  HTMLDialogElement.prototype.showModal = function () { this.open = true; };
  HTMLDialogElement.prototype.close = function () { this.open = false; };
});
afterEach(async () => { await act(() => root.render(null)); vi.unstubAllGlobals(); });

// What `GET /api/v1/workspace/versions/<id>` returns a team operator: the entries its grant reads.
const x = { repo: 'a.com/x', path: 'x', owner: 'team-a' };
const q = { repo: 'a.com/q', path: 'q', owner: 'team-a' };
const y = { repo: 'a.com/y', path: 'y', owner: 'team-b' };
const z = { repo: 'a.com/z', path: 'z' };
const version = (id: string, message: string, repositories: object[]) => ({ id, author: 'alice', time: '2026-10-23T10:00:00Z', message, repositories });
const current = version('1111111111111111', 'Current', [x, q, y, z]);
const older = version('2222222222222222', 'Older', [{ ...x, path: 'older-x' }, q, y, z]);
const foreign = version('3333333333333333', 'Touches team-b', [x, q, { ...y, path: 'changed' }, z]);
const ownerChanged = version('4444444444444444', 'Owner changed', [x, q, { ...y, owner: 'team-c' }, z]);
const versions = [current, older, foreign, ownerChanged];
const team: Session = { subject: 'tina', role: 'team_operator', groups: ['team-a'], grant: ['a.com/q', 'a.com/w', 'a.com/x', 'a.com/y', 'a.com/z'], teams: ['team-a'] };

function reads() {
  return vi.fn(async (url: string) => {
    if (url === '/api/v1/workspace') return Response.json({ current: { id: current.id, fence: '7' }, version: current });
    if (url === '/api/v1/workspace/versions') return Response.json({ current: { id: current.id, fence: '7' }, versions, malformed: [] });
    const found = versions.find(row => url === `/api/v1/workspace/versions/${row.id}`);
    if (found) return Response.json(found);
    throw new Error(`unexpected GET ${url}`);
  });
}
const posts = (fetch: ReturnType<typeof vi.fn>) => fetch.mock.calls.filter(([, init]) => init?.method === 'POST');
async function render(session: Session, locale: 'en' | 'pt-BR', fetch: ReturnType<typeof vi.fn>, operations = true) {
  vi.stubGlobal('fetch', fetch);
  await act(() => root.render(<WorkspacePage api={new Api(new Auth(config))} config={{ ...config, operations }} session={session} locale={locale} />));
}
async function editor(locale: 'en' | 'pt-BR') {
  const t = workspaceCopy[locale];
  const form = await waitFor(() => getByRole(host, 'form', { name: t.editor }));
  const entries = getByRole(form, 'textbox', { name: t.editEntries }) as HTMLTextAreaElement;
  await waitFor(() => expect(entries.value).not.toBe(''));
  return { t, form, entries, save: getByRole(form, 'button', { name: t.save }) };
}
const type = (field: Element, value: string) => act(() => { fireEvent.change(field, { target: { value } }); });

test.each(['pt-BR', 'en'] as const)('a team operator edits only the entries its grant reads, each with its owner and state in %s', async locale => {
  const fetch = reads(); await render(team, locale, fetch);
  const { t, form, entries } = await editor(locale);
  expect(entries.value).toBe(entriesText(JSON.stringify([x, q, y, z])));
  const table = getByRole(form, 'region', { name: t.entryState });
  expect(table.tabIndex).toBe(0);
  const row = (repo: string) => within(table).getByText(repo).closest('tr')!.textContent!;
  expect(row('a.com/x')).toContain('team-a'); expect(row('a.com/x')).toContain(t.mayChange);
  expect(row('a.com/y')).toContain('team-b'); expect(row('a.com/y')).toContain(`${t.readOnly}: ${t.reason_not_your_team}`);
  expect(row('a.com/z')).toContain(t.noOwner); expect(row('a.com/z')).toContain(`${t.readOnly}: ${t.reason_ownerless}`);
  expect(fetch.mock.calls.some(([url]) => url === '/api/v1/manifest')).toBe(false);
});

const refusals = [
  { name: 'another owner’s entry', edit: (text: string) => text.replace('path = "y"', 'path = "changed"'), repo: 'a.com/y', reason: 'not_your_team' as const },
  { name: 'an owner', edit: (text: string) => text.replace('owner = "team-a"\nrepo = "a.com/x"', 'owner = "team-b"\nrepo = "a.com/x"'), repo: 'a.com/x', reason: 'owner_change' as const },
  { name: 'an ownerless entry', edit: (text: string) => text.replace('path = "z"', 'path = "changed"'), repo: 'a.com/z', reason: 'ownerless' as const },
  { name: 'an addition outside the grant', edit: (text: string) => `${text}\n[[repository]]\npath = "v"\nowner = "team-a"\nrepo = "a.com/v"\n`, repo: 'a.com/v', reason: 'outside_grant' as const },
];
test.each(refusals.flatMap(row => (['pt-BR', 'en'] as const).map(locale => ({ ...row, locale }))))('$name is marked with its reason in $locale and disables the save until undone', async ({ edit, repo, reason, locale }) => {
  const fetch = reads(); await render(team, locale, fetch);
  const { t, form, entries, save } = await editor(locale);
  const original = entries.value;
  await type(getByRole(form, 'textbox', { name: t.saveMessage }), 'a message');
  await waitFor(() => expect(save.hasAttribute('disabled')).toBe(false));
  const count = fetch.mock.calls.length;
  await type(entries, edit(original));
  const diff = getByRole(form, 'region', { name: t.editorDiff });
  expect(diff.textContent).toContain(repo);
  expect(diff.textContent).toContain(`${t.refusedMark}: `); expect(diff.textContent).toContain(t[`reason_${reason}`]);
  expect(getByRole(form, 'status').textContent).toBe(t.refusedSummary);
  expect(diff.tabIndex).toBe(0); expect(diff.getAttribute('aria-label')).toBe(t.editorDiff); // the mark is text inside a focusable, labelled region
  expect(save.hasAttribute('disabled')).toBe(true);
  await type(entries, original);
  expect(getByRole(form, 'region', { name: t.editorDiff }).textContent).not.toContain(t.refusedMark);
  expect(queryByRole(form, 'status')).toBeNull();
  expect(save.hasAttribute('disabled')).toBe(false);
  expect(fetch.mock.calls.length).toBe(count);
});

test('a save of an edit, a removal and an addition sends the submitted entries after naming the teams that authorize it', async () => {
  const read = reads();
  const fetch = vi.fn(async (url: string, init?: RequestInit) => {
    if (init?.method === 'POST') return Response.json({ result: 'abcdef0123456789' });
    if (url === '/api/v1/workspace/versions/abcdef0123456789') return Response.json({ ...current, id: 'abcdef0123456789', author: 'tina', message: 'team save' });
    return read(url);
  });
  await render(team, 'en', fetch);
  const { t, form, entries, save } = await editor('en');
  const edited = entriesText(JSON.stringify([{ ...x, path: 'edited' }, y, z, { repo: 'a.com/w', path: 'w', owner: 'team-a' }]));
  await type(entries, edited); await type(getByRole(form, 'textbox', { name: t.saveMessage }), 'team save');
  const diff = getByRole(form, 'region', { name: t.editorDiff }).textContent!;
  expect(diff).toContain('a.com/w'); expect(diff).toContain('a.com/q'); expect(diff).toContain('repository.path');
  expect(diff).not.toContain('a.com/y'); expect(diff).not.toContain('a.com/z'); expect(diff).not.toContain(t.refusedMark);
  await act(() => fireEvent.click(save));
  const dialog = getByRole(host, 'dialog', { name: t.save });
  expect(dialog.textContent).toContain(`${t.authorizedBy}: team-a`);
  expect(dialog.textContent).not.toContain('meada workspace');
  expect(posts(fetch)).toHaveLength(0);
  for (const name of [t.cancel, t.confirmSave]) expect(getByRole(dialog, 'button', { name }).tabIndex).toBe(0);
  expect(dialog.getAttribute('aria-labelledby')).toBeTruthy(); expect(dialog.getAttribute('aria-describedby')).toBeTruthy();
  await act(() => fireEvent.click(getByRole(dialog, 'button', { name: t.confirmSave })));
  expect(posts(fetch)).toHaveLength(1);
  expect(JSON.parse(String(posts(fetch)[0]?.[1]?.body))).toEqual({ entries: edited, message: 'team save' });
  await waitFor(() => expect(getByRole(host, 'region', { name: t.saved }).textContent).toContain('abcdef0123456789'));
});

test('a 403 entries_refused lists each refused entry with its change and reason and the hidden count', async () => {
  const read = reads();
  const fetch = vi.fn(async (url: string, init?: RequestInit) => init?.method === 'POST'
    ? Response.json({ error: { code: 'entries_refused', message: 'refused', refused: [{ identity: 'a.com/y', change: 'edit', reason: 'not_your_team' }], hidden: 2 } }, { status: 403 })
    : read(url));
  await render(team, 'en', fetch);
  const { t, form, entries, save } = await editor('en');
  await type(entries, entries.value.replace('path = "x"', 'path = "edited"')); await type(getByRole(form, 'textbox', { name: t.saveMessage }), 'm');
  await act(() => fireEvent.click(save));
  await act(() => fireEvent.click(getByRole(getByRole(host, 'dialog'), 'button', { name: t.confirmSave })));
  const alert = getByRole(getByRole(host, 'dialog'), 'alert');
  expect(alert.textContent).toContain('a.com/y'); expect(alert.textContent).toContain(t.change_edit); expect(alert.textContent).toContain(t.reason_not_your_team);
  expect(alert.textContent).toContain(`${t.hiddenRefused}: 2`);
});

test('a viewer refusal reloads the session and turns the editor read-only', async () => {
  const read = reads(); const reload = vi.fn(async () => Response.json({ subject: 'tina', role: 'viewer', groups: ['team-a'], grant: team.grant }));
  const fetch = vi.fn(async (url: string, init?: RequestInit) => init?.method === 'POST'
    ? Response.json({ error: { code: 'forbidden', message: 'operator required' } }, { status: 403 })
    : url === '/api/v1/session' ? reload() : read(url));
  await render(team, 'en', fetch);
  const { t, form, entries, save } = await editor('en');
  await type(entries, entries.value.replace('path = "x"', 'path = "edited"')); await type(getByRole(form, 'textbox', { name: t.saveMessage }), 'm');
  await act(() => fireEvent.click(save));
  await act(() => fireEvent.click(getByRole(getByRole(host, 'dialog'), 'button', { name: t.confirmSave })));
  await waitFor(() => expect(queryByRole(host, 'dialog')).toBeNull());
  expect(reload).toHaveBeenCalledTimes(1);
  const readOnly = getByRole(host, 'region', { name: t.editor });
  expect(readOnly.textContent).toContain(t.roleLost);
  expect(getByRole(readOnly, 'textbox', { name: t.editEntries }).getAttribute('readonly')).not.toBeNull();
  expect(queryByRole(host, 'button', { name: t.save })).toBeNull();
});

test.each(['pt-BR', 'en'] as const)('apply is offered only when the entries the session reads authorize it, an older one as a rollback, in %s', async locale => {
  const t = workspaceCopy[locale]; const fetch = reads(); await render(team, locale, fetch);
  const rollback = await waitFor(() => getByRole(host, 'button', { name: `${t.rollback} ${older.id}` }));
  expect(queryByRole(host, 'button', { name: new RegExp(foreign.id) })).toBeNull();
  expect(queryByRole(host, 'button', { name: new RegExp(ownerChanged.id) })).toBeNull();
  expect(getByRole(host, 'button', { name: `${t.apply} ${current.id}` })).toBeTruthy();
  await act(() => fireEvent.click(rollback));
  const dialog = getByRole(host, 'dialog', { name: `${t.rollbackTitle}: ${older.id}` });
  expect(dialog.textContent).toContain(`${t.authorizedBy}: team-a`); expect(dialog.textContent).not.toContain('meada workspace');
  expect(posts(fetch)).toHaveLength(0);
});

test.each([{ session: { subject: 'v', role: 'viewer' as const }, name: 'a viewer' }, { session: team, name: 'a team operator' }])('$name sees each entry’s owner and an owner change in the diff of two versions', async ({ session }) => {
  const fetch = reads(); await render(session, 'en', fetch);
  await waitFor(() => expect(getByRole(host, 'region', { name: 'Entry owners' })).toBeTruthy());
  const owners = getByRole(host, 'region', { name: 'Entry owners' });
  expect(owners.tabIndex).toBe(0); expect(owners.textContent).toContain('team-b'); expect(owners.textContent).toContain('No owner');
  await act(() => fireEvent.change(getByRole(host, 'combobox', { name: 'To version' }), { target: { value: ownerChanged.id } }));
  await waitFor(() => expect(getByRole(host, 'region', { name: 'Version diff' }).textContent).toContain('repository.owner'));
  expect(getByRole(host, 'region', { name: 'Version diff' }).textContent).toContain('a.com/y');
});

test.each([{ session: { subject: 'v', role: 'viewer' as const }, operations: true }, { session: team, operations: false }])('$session.role is offered no save or apply (operations $operations)', async ({ session, operations }) => {
  const fetch = reads(); await render(session, 'en', fetch, operations);
  await waitFor(() => expect(getByRole(host, 'heading', { name: 'Saved versions' })).toBeTruthy());
  expect(queryByRole(host, 'form', { name: 'Workspace editor' })).toBeNull();
  expect(queryByRole(host, 'button', { name: /Apply|Roll back|Save/ })).toBeNull();
});

test('the global operator keeps the whole editor: the full text, no marks and the CLI command', async () => {
  const read = reads();
  const fetch = vi.fn(async (url: string) => url === '/api/v1/manifest' ? Response.json({ file: 'ops.toml', manifest: {}, modules: [], errors: [] }) : read(url));
  await render({ subject: 'root', role: 'operator' }, 'en', fetch);
  const { t, form, entries, save } = await editor('en');
  expect(queryByRole(form, 'region', { name: t.entryState })).toBeNull();
  await type(entries, entries.value.replace('path = "y"', 'path = "changed"')); await type(getByRole(form, 'textbox', { name: t.saveMessage }), 'm');
  expect(getByRole(form, 'region', { name: t.editorDiff }).textContent).not.toContain(t.refusedMark);
  await act(() => fireEvent.click(save));
  const dialog = getByRole(host, 'dialog', { name: t.save });
  expect(dialog.textContent).toContain('meada workspace import <file> --message'); expect(dialog.textContent).not.toContain(t.authorizedBy);
  expect(posts(fetch)).toHaveLength(0);
});

test('one stored version failing to read leaves the other applies offered', async () => {
  const read = reads();
  const fetch = vi.fn(async (url: string) => url === `/api/v1/workspace/versions/${foreign.id}`
    ? Response.json({ error: { code: 'unavailable', message: 'version unavailable' } }, { status: 503 }) : read(url));
  await render(team, 'en', fetch);
  await waitFor(() => expect(getByRole(host, 'button', { name: `Roll back workspace ${older.id}` })).toBeTruthy());
  expect(getByRole(host, 'alert').textContent).toBe('version unavailable');
  expect(queryByRole(host, 'button', { name: new RegExp(foreign.id) })).toBeNull();
});
