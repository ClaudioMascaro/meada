import { readFileSync } from 'node:fs';
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { fireEvent, getByRole, queryByRole, waitFor } from '@testing-library/dom';
import { afterEach, beforeAll, expect, test, vi } from 'vitest';
import { initSync } from '../wasm/meada_core';
import { App } from '../src/App';
import { OperationalPage } from '../src/OperationalPage';
import { WorkspacePage } from '../src/WorkspacePage';
import { Api } from '../src/api';
import type { Session } from '../src/api';
import { Auth } from '../src/auth';
import { copy } from '../src/i18n';
import { operationsCopy } from '../src/operations-i18n';
import { workspaceCopy } from '../src/workspace-i18n';
import config from './fixtures/config.json';
import fixtures from './fixtures/operational.json';

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const host = document.createElement('div'); document.body.append(host);
const root = createRoot(host);
beforeAll(() => {
  initSync({ module: readFileSync('wasm/meada_core_bg.wasm') });
  HTMLDialogElement.prototype.showModal = function () { this.open = true; };
  HTMLDialogElement.prototype.close = function () { this.open = false; };
});
afterEach(async () => { await act(() => root.render(null)); vi.unstubAllGlobals(); });

const version = (id: string, repositories: object[]) => ({ id, author: 'alice', time: '2026-10-23T10:00:00Z', message: id, repositories });
// `owner_missing` is on the wire only when true: the entry names a team no directory group carries.
const current = version('1111111111111111', [{ repo: 'a.com/x', path: 'x', owner: 'ghosts', owner_missing: true }, { repo: 'a.com/y', path: 'y', owner: 'team-a' }]);
const older = version('2222222222222222', [{ repo: 'a.com/x', path: 'older', owner: 'ghosts', owner_missing: true }]);
const groups = ['alpha', 'alphabet', 'a"quoted'];
const global: Session = { role: 'operator', subject: 'root' };
const teamOperator: Session = { role: 'team_operator', subject: 'tina', groups: ['team-a'], grant: ['a.com/x', 'a.com/y'], teams: ['team-a'] };
const viewer: Session = { role: 'viewer', subject: 'vera' };

function reads() {
  return vi.fn(async (url: string) => {
    if (url === '/api/v1/workspace') return Response.json({ current: { id: current.id, fence: '7' }, version: current });
    if (url === '/api/v1/workspace/versions') return Response.json({ current: { id: current.id, fence: '7' }, versions: [current, older], malformed: [] });
    if (url === `/api/v1/workspace/versions/${older.id}`) return Response.json(older);
    if (url.startsWith('/api/v1/directory/groups?prefix=')) return Response.json({ groups: groups.filter(name => name.startsWith(decodeURIComponent(url.split('=')[1] ?? ''))) });
    if (url === '/api/v1/manifest') return Response.json({ file: 'ops.toml', manifest: {}, modules: [], errors: [] });
    throw new Error(`unexpected GET ${url}`);
  });
}
const asked = (fetch: ReturnType<typeof vi.fn>) => fetch.mock.calls.map(([url]) => String(url)).filter(url => url.includes('/directory/'));
async function render(session: Session, fetch: ReturnType<typeof vi.fn>, locale: 'en' | 'pt-BR' = 'en') {
  vi.stubGlobal('fetch', fetch);
  await act(() => root.render(<WorkspacePage api={new Api(new Auth(config))} config={{ ...config, operations: true }} session={session} locale={locale} />));
  const form = await waitFor(() => getByRole(host, 'form', { name: workspaceCopy[locale].editor }));
  const entries = getByRole(form, 'textbox', { name: workspaceCopy[locale].editEntries }) as HTMLTextAreaElement;
  await waitFor(() => expect(entries.value).not.toBe(''));
  return { form, entries };
}
/** Types `text` at the end of the entries, the caret where a keystroke leaves it. */
const type = (field: HTMLTextAreaElement, text: string) => act(() => {
  fireEvent.change(field, { target: { value: text } });
  field.setSelectionRange(text.length, text.length); fireEvent.select(field);
});

test.each(['en', 'pt-BR'] as const)('the global operator picks an owner among the directory groups in %s', async locale => {
  const fetch = reads(); const { form, entries } = await render(global, fetch, locale);
  const t = workspaceCopy[locale];
  expect(queryByRole(form, 'group', { name: t.ownerSuggestions })).toBeNull();
  const head = '[[repository]]\nrepo = "a.com/z"\npath = "z"\nowner = ';
  await type(entries, `${head}"al`);
  const picker = await waitFor(() => getByRole(form, 'group', { name: t.ownerSuggestions }));
  expect(asked(fetch)).toEqual(['/api/v1/directory/groups?prefix=al']);
  expect([...picker.querySelectorAll('button')].map(button => button.textContent)).toEqual(['alpha', 'alphabet']);
  await act(() => fireEvent.click(getByRole(picker, 'button', { name: 'alphabet' })));
  expect(entries.value).toBe(`${head}"alphabet`);
  // A quote in a name is escaped into the TOML string, and the prefix reaches the route decoded.
  await type(entries, `${head}"a`);
  await act(() => fireEvent.click(getByRole(form, 'button', { name: 'a"quoted' })));
  expect(entries.value).toBe(`${head}"a\\"quoted`);
  // Leaving the owner string closes the picker.
  await type(entries, `${head}"alpha"\n`);
  await waitFor(() => expect(queryByRole(form, 'group', { name: t.ownerSuggestions })).toBeNull());
});

test.each([['team operator', teamOperator], ['viewer', viewer]] as const)('the %s is offered no directory values and asks the route nothing', async (_name, session) => {
  const fetch = reads();
  if (session.role === 'viewer') {
    vi.stubGlobal('fetch', fetch);
    await act(() => root.render(<WorkspacePage api={new Api(new Auth(config))} config={{ ...config, operations: true }} session={session} locale="en" />));
    await waitFor(() => expect(getByRole(host, 'heading', { name: 'Workspace versions' })).toBeTruthy());
  } else {
    const { entries, form } = await render(session, fetch);
    await type(entries, `${entries.value}owner = "al`);
    expect(queryByRole(form, 'group', { name: workspaceCopy.en.ownerSuggestions })).toBeNull();
  }
  expect(asked(fetch)).toEqual([]);
});

test.each(['en', 'pt-BR'] as const)('the editor and the comparison flag owner_missing entries with text in %s, and a save never sends the flag', async locale => {
  const t = workspaceCopy[locale];
  const fetch = reads(); const { form, entries } = await render(global, fetch, locale);
  const status = getByRole(form, 'status');
  expect(status.textContent).toContain('a.com/x'); expect(status.textContent).toContain(t.ownerMissing);
  expect(status.textContent).not.toContain('a.com/y');
  expect(entries.value).not.toContain('owner_missing');
  const from = getByRole(host, 'combobox', { name: t.from });
  await act(() => fireEvent.change(from, { target: { value: older.id } }));
  await waitFor(() => expect(getByRole(host, 'region', { name: t.diff }).textContent).toContain('repository.path'));
  const table = [...host.querySelectorAll('table')].find(candidate => candidate.textContent?.includes('a.com/x') && candidate.textContent.includes(t.ownerMissing));
  expect(table).toBeTruthy();
});

test('a team operator sees the flag next to the owner of its entries', async () => {
  const fetch = reads(); const { form } = await render(teamOperator, fetch);
  const row = [...form.querySelectorAll('tr')].find(candidate => candidate.textContent?.includes('a.com/x'))!;
  expect(row.textContent).toContain(workspaceCopy.en.ownerMissing);
  expect([...form.querySelectorAll('tr')].find(candidate => candidate.textContent?.includes('a.com/y'))!.textContent).not.toContain(workspaceCopy.en.ownerMissing);
});

test.each(['en', 'pt-BR'] as const)('the admission page shows the directory counts and the last SCIM write in %s', async locale => {
  const t = operationsCopy[locale];
  const render = async (directory: object | null) => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(Response.json({ ...fixtures.admission, directory })));
    await act(() => root.render(<OperationalPage kind="admission" api={new Api(new Auth(config))} locale={locale} config={config} session={{ role: 'viewer', subject: 'vera' }} />));
    await waitFor(() => expect(host.textContent).toContain(directory ? t.directoryUsers : t.directory));
  };
  await render({ users: 12, groups: 3, last_write: '2026-10-28T09:00:00Z' });
  for (const text of [t.directoryUsers, '12', t.directoryGroups, '3', t.directoryLastWrite, '2026-10-28T09:00:00Z']) expect(host.textContent).toContain(text);
  await render(null);
  expect(host.textContent).toContain(t.directoryUnavailable);
  await act(() => root.render(null));
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(Response.json(fixtures.admission)));
  await act(() => root.render(<OperationalPage kind="admission" api={new Api(new Auth(config))} locale={locale} config={config} session={{ role: 'viewer', subject: 'vera' }} />));
  await waitFor(() => expect(getByRole(host, 'heading', { level: 1 }).textContent).toBe(t.admission));
  expect(host.textContent).not.toContain(t.directoryUsers);
});

test.each(['en', 'pt-BR'] as const)('a deprovisioned session shows the deactivated page in %s', async locale => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(Response.json({ error: { code: 'deprovisioned', message: 'deactivated' } }, { status: 403 })));
  await act(() => root.render(<App api={new Api(new Auth(config))} config={config} initialLanguage={locale} />));
  await waitFor(() => expect(getByRole(host, 'heading', { level: 1 }).textContent).toBe(copy[locale].deprovisionedTitle));
  expect(getByRole(host, 'alert').textContent).toBe(copy[locale].deprovisioned);
});
