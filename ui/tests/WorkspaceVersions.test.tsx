import { readFileSync } from 'node:fs';
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { fireEvent, getByRole, queryByRole, waitFor, within } from '@testing-library/dom';
import { afterEach, beforeAll, expect, test, vi } from 'vitest';
import { init, initSync } from '../wasm/meada_core';
import { Api } from '../src/api';
import { Auth } from '../src/auth';
import { NamedWorkspacesPage } from '../src/NamedWorkspacesPage';
import { guidedCopy } from '../src/guided-i18n';
import { historyRoute, versionRoute, versionsRoute } from '../src/named';
import type { WorkspaceDetail } from '../src/named';
import { namedCopy } from '../src/named-i18n';
import config from './fixtures/config.json';

vi.mock('../src/core', async importOriginal => ({
  ...await importOriginal<typeof import('../src/core')>(),
  loadCore: async (language: 'en' | 'pt-BR') => { init(language); },
}));

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const host = document.createElement('div');
document.body.append(host);
const root = createRoot(host);
beforeAll(() => {
  initSync({ module: readFileSync('wasm/meada_core_bg.wasm') });
  HTMLDialogElement.prototype.showModal = function () { this.open = true; };
  HTMLDialogElement.prototype.close = function () { this.open = false; };
});
afterEach(async () => { await act(() => root.render(null)); vi.unstubAllGlobals(); });

const ID = '0123456789abcdef', NEW = 'aaaaaaaaaaaaaaaa', OLD = 'bbbbbbbbbbbbbbbb', FIRST = 'cccccccccccccccc';
const Y = 'github.com/acme/y', A = 'github.com/acme/a';
const t = namedCopy.en, g = guidedCopy.en;
const detail = (operate: boolean): WorkspaceDetail => ({
  id: ID, name: 'Payments', description: '', created: '2026-10-30T01:00:00Z', version: NEW, base: 3, state: 'applied',
  entries: [{ repo: Y, ingestion: 'ready', blocks: {} }], operate, hidden: 0, ...(operate ? { operators: [{ subject: 'alice', login: 'alice' }] } : {}),
});
const row = (version: string, current: boolean, author?: string) => ({ version, saved: '2026-10-30T01:00:00Z', message: `m ${version}`, current, ...(author ? { author } : {}) });
const document_ = { id: OLD, saved: '2026-10-29T01:00:00Z', message: 'older', current: false, base: 3, entries: [{ repo: Y, blocks: {} }], text: `[[repository]]\nrepo = "${Y}"\n`,
  diff: { against: NEW, added: [], removed: [A], changed: [{ repo: Y, keys: ['peer'] }] } };
const history = { base: 3, history: [{ seq: 3, action: 'workspaces.apply', version: NEW, actor: 'alice', authority: 'operator', time: '2026-10-30T01:00:00Z', message: 'link' }], next: null };
const base = (operate: boolean) => ({
  'GET /api/v1/workspaces?': () => Response.json({ workspaces: [{ id: ID, name: 'Payments', description: '', version: NEW, state: 'applied', entries: 1, operate }], next: null }),
  'GET /api/v1/catalog': () => Response.json({ state: 'ready', reason: null, set: null, repositories: [{ identity: Y }], next: null }),
  [`GET /api/v1/workspaces/${ID}/effective`]: () => Response.json({ workspace: ID, generation: null, entries: [] }),
  [`GET /api/v1/workspaces/${ID}/versions`]: () => Response.json({ base: 3, versions: [row(NEW, true, 'alice'), row(OLD, false, 'alice')], next: null }),
  [`GET /api/v1/workspaces/${ID}/history`]: () => Response.json(history),
  [`GET /api/v1/workspaces/${ID}`]: () => Response.json(detail(operate)),
} as Record<string, () => Response>);
function answers(table: Record<string, () => Response>) {
  return vi.fn(async (route: string, init?: RequestInit) => {
    const key = `${init?.method ?? 'GET'} ${route}`;
    // The longest key wins: `workspaces/<id>/versions` is not `workspaces/<id>`.
    const hit = Object.keys(table).filter(k => key.startsWith(k)).sort((a, b) => b.length - a.length)[0];
    if (!hit) throw new Error(`unexpected ${key}`);
    return table[hit]!();
  });
}
async function open(table: Record<string, () => Response>, operate: boolean) {
  const fetch = answers(table);
  vi.stubGlobal('fetch', fetch);
  const session = { subject: 'alice', role: operate ? 'operator' as const : 'viewer' as const, grant: '*' as const };
  await act(() => root.render(<NamedWorkspacesPage api={new Api(new Auth(config))} locale="en" session={session} maxEntries={5} />));
  await waitFor(() => getByRole(host, 'button', { name: `${t.show} Payments` }));
  await act(async () => { fireEvent.click(getByRole(host, 'button', { name: `${t.show} Payments` })); });
  await waitFor(() => getByRole(host, 'list', { name: t.versionsTitle }));
  return fetch;
}
const press = (element: HTMLElement) => act(async () => { fireEvent.click(element); });
const versions = () => within(getByRole(host, 'region', { name: t.versionsTitle }));
const posts = (fetch: ReturnType<typeof answers>) => fetch.mock.calls.filter(([, init]) => init?.method === 'POST').map(([route]) => route);

test('the routes are the ones the server takes', () => {
  expect(versionsRoute(ID)).toBe(`workspaces/${ID}/versions`);
  expect(versionsRoute(ID, 'abc')).toBe(`workspaces/${ID}/versions?after=abc`);
  expect(versionRoute(ID, OLD)).toBe(`workspaces/${ID}/versions/${OLD}?against=current`);
  expect(historyRoute(ID)).toBe(`workspaces/${ID}/history`);
  expect(historyRoute(ID, 2)).toBe(`workspaces/${ID}/history?after=2`);
});

test('versions are listed newest first and paged with the server cursor; a version shows its entries, text and diff as the route answers', async () => {
  let page = 0;
  const fetch = await open({
    ...base(true),
    [`GET /api/v1/workspaces/${ID}/versions`]: () => Response.json(page++ === 0
      ? { base: 3, versions: [row(NEW, true, 'alice')], next: 'cursor1' } : { base: 3, versions: [row(FIRST, false, 'bob')], next: null }),
    [`GET /api/v1/workspaces/${ID}/versions/${NEW}`]: () => Response.json({ ...document_, id: NEW, current: true, diff: { against: NEW, added: [], removed: [], changed: [] }, hidden: 2 }),
    [`GET /api/v1/workspaces/${ID}/versions/${FIRST}`]: () => Response.json({ ...document_, id: FIRST, hidden: 2 }),
  }, true);
  const list = () => versions().getByRole('list', { name: t.versionsTitle });
  expect(list().textContent).toContain(`${NEW}`);
  expect(list().textContent).toContain(`(${t.currentVersion})`);
  expect(list().textContent).toContain(`${t.author}: alice`);
  await press(versions().getByRole('button', { name: t.moreVersions }));
  await waitFor(() => expect(list().textContent).toContain(FIRST));
  expect(fetch.mock.calls.map(([r]) => r)).toContain(`/api/v1/workspaces/${ID}/versions?after=cursor1`);
  expect(queryByRole(host, 'button', { name: t.moreVersions })).toBeNull();
  await press(versions().getByRole('button', { name: `${t.showVersion} ${FIRST}` }));
  const shown = await waitFor(() => getByRole(host, 'region', { name: `${t.versionOf} ${FIRST}` }));
  expect(fetch.mock.calls.map(([r]) => r)).toContain(`/api/v1/workspaces/${ID}/versions/${FIRST}?against=current`);
  expect(within(shown).getByLabelText(t.manifestOfVersion)).toHaveProperty('value', document_.text);
  expect(within(shown).getByRole('list', { name: t.versionEntries }).textContent).toContain(Y);
  expect(shown.textContent).toContain(t.hidden(2));
  expect(within(shown).getByRole('list', { name: t.versionDiff }).textContent).toBe(`${g.diffRemoved}: ${A}${g.diffChanged}: ${Y} (${g.keyPeer})`);
});

test('an operator reads the history, a page at a time; a viewer has no history control and never asks for it', async () => {
  const fetch = await open({
    ...base(true),
    [`GET /api/v1/workspaces/${ID}/history`]: () => Response.json({ ...history, next: 2 }),
  }, true);
  await press(getByRole(host, 'button', { name: t.showHistory }));
  const table = await waitFor(() => getByRole(host, 'table', { name: t.history }));
  for (const word of ['3', 'workspaces.apply', 'alice', 'operator', '2026-10-30T01:00:00Z', 'link']) expect(table.textContent).toContain(word);
  await press(getByRole(host, 'button', { name: t.moreHistory }));
  expect(fetch.mock.calls.map(([r]) => r)).toContain(`/api/v1/workspaces/${ID}/history?after=2`);

  await act(() => root.render(null));
  const viewer = await open({
    ...base(false),
    [`GET /api/v1/workspaces/${ID}/versions`]: () => Response.json({ base: 3, versions: [row(NEW, true), row(OLD, false)], next: null }),
    // This viewer reads the older version whole but not the current one: the diff is refused, the version is not.
    [`GET /api/v1/workspaces/${ID}/versions/${OLD}?against=current`]: () => Response.json({ error: { code: 'not_found', message: 'unknown version' } }, { status: 404 }),
    [`GET /api/v1/workspaces/${ID}/versions/${OLD}`]: () => Response.json({ ...document_, diff: undefined }),
  }, false);
  expect(queryByRole(host, 'button', { name: t.showHistory })).toBeNull();
  expect(queryByRole(host, 'region', { name: t.history })).toBeNull();
  expect(viewer.mock.calls.map(([r]) => String(r)).filter(r => r.includes('/history'))).toEqual([]);
  // A viewer's list carries no author, and it cannot apply.
  expect(versions().getByRole('list', { name: t.versionsTitle }).textContent).not.toContain(t.author);
  await press(versions().getByRole('button', { name: `${t.showVersion} ${OLD}` }));
  await waitFor(() => getByRole(host, 'region', { name: `${t.versionOf} ${OLD}` }));
  expect(queryByRole(host, 'button', { name: t.applyVersion })).toBeNull();
  expect(queryByRole(host, 'region', { name: t.versionDiff })).toBeNull();
});

const choose = async () => {
  await press(versions().getByRole('button', { name: `${t.showVersion} ${OLD}` }));
  await waitFor(() => getByRole(host, 'region', { name: `${t.versionOf} ${OLD}` }));
};
const onVersion = { [`GET /api/v1/workspaces/${ID}/versions/${OLD}`]: () => Response.json(document_) };

test('applying a listed version waits for the confirmation and is sent at the base the page read', async () => {
  const fetch = await open({ ...base(true), ...onVersion, [`POST /api/v1/workspaces/${ID}/apply`]: () => Response.json({ base: 4, version: OLD }) }, true);
  await choose();
  await press(getByRole(host, 'button', { name: t.applyVersion }));
  expect(posts(fetch)).toEqual([]);
  const dialog = getByRole(host, 'dialog');
  expect(within(dialog).getByRole('list', { name: `${t.versionDiff} (${t.applyConfirm})` }).textContent).toContain(`${g.diffRemoved}: ${A}`);
  await press(within(dialog).getByRole('button', { name: t.applyVersion }));
  await waitFor(() => expect(posts(fetch)).toEqual([`/api/v1/workspaces/${ID}/apply`]));
  const init = fetch.mock.calls.find(([, i]) => i?.method === 'POST')![1] as RequestInit;
  expect(JSON.parse(init.body as string)).toEqual({ version: OLD, base: 3 });
  await waitFor(() => expect(host.textContent).toContain(t.savedNote));
  expect(queryByRole(host, 'dialog')).toBeNull();
});

test('a stale base shows the server message and the versions are read again', async () => {
  const fetch = await open({
    ...base(true), ...onVersion,
    [`POST /api/v1/workspaces/${ID}/apply`]: () => Response.json({ error: { code: 'stale_base', message: 'changed since seq 3', base: 5 } }, { status: 409 }),
  }, true);
  const reads = () => fetch.mock.calls.filter(([r]) => r === `/api/v1/workspaces/${ID}/versions`).length;
  const before = reads();
  await choose();
  await press(getByRole(host, 'button', { name: t.applyVersion }));
  await press(within(getByRole(host, 'dialog')).getByRole('button', { name: t.applyVersion }));
  await waitFor(() => expect(host.textContent).toContain('changed since seq 3'));
  expect(queryByRole(host, 'dialog')).toBeNull();
  await waitFor(() => expect(reads()).toBeGreaterThan(before));
});

test('a refused entry is shown as the server worded it and the versions are read again', async () => {
  const fetch = await open({
    ...base(true), ...onVersion,
    [`POST /api/v1/workspaces/${ID}/apply`]: () => Response.json({ error: { code: 'entries_refused', message: 'refused by the grant', refused: [{ identity: A, change: 'add', reason: 'not_eligible' }], hidden: 1 } }, { status: 403 }),
  }, true);
  const reads = () => fetch.mock.calls.filter(([r]) => r === `/api/v1/workspaces/${ID}/versions`).length;
  const before = reads();
  await choose();
  await press(getByRole(host, 'button', { name: t.applyVersion }));
  await press(within(getByRole(host, 'dialog')).getByRole('button', { name: t.applyVersion }));
  const region = await waitFor(() => getByRole(host, 'alert', { name: g.refusedSave }));
  expect(region.textContent).toContain(A);
  expect(region.textContent).toContain(g.refusedHidden(1));
  expect(host.textContent).toContain('refused by the grant');
  await waitFor(() => expect(reads()).toBeGreaterThan(before));
});
