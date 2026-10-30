import { readFileSync } from 'node:fs';
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { fireEvent, getAllByRole, getByLabelText, getByRole, queryByRole, waitFor, within } from '@testing-library/dom';
import { afterEach, beforeAll, expect, test, vi } from 'vitest';
import { init, initSync } from '../wasm/meada_core';
import { Api } from '../src/api';
import { Auth } from '../src/auth';
import { NamedWorkspacesPage } from '../src/NamedWorkspacesPage';
import { copy } from '../src/i18n';
import { createBody, ingestionText, listRoute, pendingText, plain, stateText } from '../src/named';
import type { WorkspaceDetail, WorkspaceList } from '../src/named';
import { namedCopy } from '../src/named-i18n';
import { offered } from '../src/pages';
import config from './fixtures/config.json';

// The bundle is read from disk: a mocked `fetch` answers the page's routes, not the WebAssembly file.
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

const Y = 'github.com/acme/y', A = 'github.com/acme/a';
const catalog = (identities: string[]) => ({
  state: 'ready', reason: null, set: { finished: '2026-10-30T01:00:00.000000Z', age_seconds: 1, max_age_seconds: 5400 },
  repositories: identities.map(identity => ({ identity })), next: null,
});
const workspaces: WorkspaceList = {
  workspaces: [{ id: '0123456789abcdef', name: 'Payments', description: 'Y and A', version: 'fedcba9876543210', state: 'applied', entries: 2, operate: false }],
  next: null,
};
const detail: WorkspaceDetail = {
  id: '0123456789abcdef', name: 'Payments', description: 'Y and A', created: '2026-10-30T01:00:00Z', version: 'fedcba9876543210', base: 1,
  state: 'applied', entries: [{ repo: Y, ingestion: 'pending', blocks: {} }, { repo: A, ingestion: 'excluded', blocks: {} }], operate: false,
};
const session = { subject: 'alice', role: 'viewer' as const, grant: [Y, A] };

/** A `fetch` answering by route, the way the console does: effects of a page may ask in any order. */
function answers(table: Record<string, () => Response>) {
  return vi.fn(async (route: string, init?: RequestInit) => {
    const key = `${init?.method ?? 'GET'} ${route}`;
    const answer = table[key] ?? Object.entries(table).find(([k]) => key.startsWith(k))?.[1];
    if (!answer) throw new Error(`unexpected ${key}`);
    return answer();
  });
}

function page(locale: 'en' | 'pt-BR' = 'en') {
  return act(() => root.render(<NamedWorkspacesPage api={new Api(new Auth(config))} locale={locale} session={session} maxEntries={5} />));
}

test('the routes and bodies are the bounded ones the server takes', () => {
  expect(listRoute()).toBe('workspaces?limit=25');
  expect(listRoute('0123456789abcdef')).toBe('workspaces?limit=25&after=0123456789abcdef');
  expect(JSON.parse(createBody('n', '', '[[repository]]\nrepo = "x"\n', 'm'))).toEqual({ name: 'n', description: '', entries: '[[repository]]\nrepo = "x"\n', message: 'm' });
  expect(plain('Payments', 1, 80)).toBe(true);
  for (const bad of ['', ' x', 'x ', 'a\nb', 'x'.repeat(81)]) expect(plain(bad, 1, 80)).toBe(false);
  // The core's rule exactly: U+FEFF is no whitespace to Rust, U+3000 is.
  expect(plain('\uFEFFx', 1, 80)).toBe(true);
  expect(plain('\u3000x', 1, 80)).toBe(false);
});

test.each(['en', 'pt-BR'] as const)('every state is text in %s, and both languages say the same things', locale => {
  const t = namedCopy[locale];
  expect(stateText('live', t)).not.toBe(stateText('applied', t));
  for (const state of ['pending', 'ready', 'excluded'] as const) expect(ingestionText(state, t).length).toBeGreaterThan(3);
  for (const reason of ['no_set', 'stale_set', 'missing_login']) expect(pendingText(reason, t).length).toBeGreaterThan(20);
  expect(Object.keys(namedCopy['pt-BR']).sort()).toEqual(Object.keys(namedCopy.en).sort());
});

test('the workspaces page is offered to every admitted subject when the console has workspaces', () => {
  for (const role of ['viewer', 'team_operator', 'operator'] as const) {
    expect(offered({ subject: 's', role }, false, false, true).map(r => r.path)).toContain('/workspaces');
    expect(offered({ subject: 's', role }, false, false, false).map(r => r.path)).not.toContain('/workspaces');
  }
});

test('the list shows what the server lists, and a workspace shows pending and excluded entries as text, not ready', async () => {
  const t = namedCopy.en;
  const fetch = answers({
    'GET /api/v1/workspaces?': () => Response.json(workspaces),
    'GET /api/v1/catalog': () => Response.json(catalog([Y, A])),
    'GET /api/v1/workspaces/0123456789abcdef/effective': () => Response.json(effectiveView([])),
    'GET /api/v1/workspaces/0123456789abcdef': () => Response.json(detail),
  });
  vi.stubGlobal('fetch', fetch);
  await page();
  await waitFor(() => expect(getByRole(host, 'button', { name: `${t.show} Payments` })).toBeDefined());
  expect(fetch).toHaveBeenCalledWith('/api/v1/workspaces?limit=25', expect.anything());
  await act(() => { fireEvent.click(getByRole(host, 'button', { name: `${t.show} Payments` })); });
  await waitFor(() => expect(getByRole(host, 'list', { name: t.entries })).toBeDefined());
  const entries = getByRole(host, 'list', { name: t.entries }).textContent ?? '';
  expect(entries).toContain(`${Y}: ${t.pending}`);
  expect(entries).toContain(`${A}: ${t.excluded}`);
  expect(entries).not.toContain(`${Y}: ${t.ready}`);
  expect(host.textContent).toContain(t.viewOnly);
  expect(queryByRole(host, 'list', { name: t.operators })).toBeNull();
});

test('a selection is written as the canonical manifest, reviewed, frozen and posted; the created workspace is shown', async () => {
  const t = namedCopy.en;
  const created = { id: '0123456789abcdef', version: 'fedcba9876543210', created: true };
  const fetch = answers({
    'GET /api/v1/workspaces?': () => Response.json({ workspaces: [], next: null }),
    'GET /api/v1/catalog': () => Response.json(catalog([Y, A])),
    'POST /api/v1/workspaces': () => Response.json(created, { status: 201 }),
    'GET /api/v1/workspaces/0123456789abcdef/effective': () => Response.json(effectiveView([])),
    'GET /api/v1/workspaces/0123456789abcdef': () => Response.json(detail),
  });
  vi.stubGlobal('fetch', fetch);
  await page();
  await waitFor(() => expect(getAllByRole(host, 'checkbox')).toHaveLength(2));
  expect((getByRole(host, 'button', { name: t.preview }) as HTMLButtonElement).disabled).toBe(true);
  await act(() => { fireEvent.change(getByLabelText(host, t.name), { target: { value: 'Payments' } }); });
  await act(() => { fireEvent.change(getByLabelText(host, t.message), { target: { value: 'first' } }); });
  for (const identity of [Y, A]) await act(() => { fireEvent.click(getByLabelText(host, `${t.select} ${identity}`)); });
  const manifest = getByRole(host, 'generic', { name: t.manifest }).textContent ?? '';
  expect(manifest).toContain(`repo = "${Y}"`);
  expect(manifest).not.toContain('path');
  await act(() => { fireEvent.click(getByRole(host, 'button', { name: t.preview })); });
  await act(() => { fireEvent.click(getByRole(host, 'button', { name: t.create })); });
  await waitFor(() => expect(fetch.mock.calls.some(([route]) => route === '/api/v1/workspaces')).toBe(true));
  const post = fetch.mock.calls.find(([route, init]) => route === '/api/v1/workspaces' && (init as RequestInit).method === 'POST')!;
  const body = JSON.parse((post[1] as RequestInit).body as string) as Record<string, string>;
  expect(Object.keys(body).sort()).toEqual(['description', 'entries', 'message', 'name']);
  expect(body.entries).toBe(manifest);
  expect(body.name).toBe('Payments');
});

test('a refusal names what the session reads and counts the rest, and nothing is shown as created', async () => {
  const t = namedCopy.en;
  const refusal = { error: { code: 'entries_refused', message: 'refused', refused: [{ identity: A, change: 'add', reason: 'not_eligible' }], hidden: 2 } };
  vi.stubGlobal('fetch', answers({
    'GET /api/v1/workspaces?': () => Response.json({ workspaces: [], next: null }),
    'GET /api/v1/catalog': () => Response.json(catalog([Y, A])),
    'POST /api/v1/workspaces': () => Response.json(refusal, { status: 403 }),
  }));
  await page();
  await waitFor(() => expect(getAllByRole(host, 'checkbox')).toHaveLength(2));
  await act(() => { fireEvent.change(getByLabelText(host, t.name), { target: { value: 'W' } }); });
  await act(() => { fireEvent.change(getByLabelText(host, t.message), { target: { value: 'm' } }); });
  await act(() => { fireEvent.click(getByLabelText(host, `${t.select} ${Y}`)); });
  await act(() => { fireEvent.click(getByRole(host, 'button', { name: t.preview })); });
  await act(() => { fireEvent.click(getByRole(host, 'button', { name: t.create })); });
  await waitFor(() => expect(getByRole(host, 'alert', { name: t.refusedTitle })).toBeDefined());
  const text = getByRole(host, 'alert', { name: t.refusedTitle }).textContent ?? '';
  expect(text).toContain(A);
  expect(text).toContain(t.refusedHidden(2));
  expect(queryByRole(host, 'status', { name: t.createdNote('W') })).toBeNull();
});

test('a pending catalog says why and offers nothing to select', async () => {
  const t = namedCopy.en;
  vi.stubGlobal('fetch', answers({
    'GET /api/v1/workspaces?': () => Response.json({ workspaces: [], next: null }),
    'GET /api/v1/catalog': () => Response.json({ state: 'pending', reason: 'stale_set', set: null, repositories: [], next: null }),
  }));
  await page();
  await waitFor(() => expect(host.textContent).toContain(t.stale_set));
  expect(queryByRole(host, 'checkbox')).toBeNull();
  expect(copy.en.workspaces).toBe('Named workspaces');
});

const operated: WorkspaceDetail = { ...detail, entries: [{ repo: Y, ingestion: 'pending', blocks: {} }], operate: true, hidden: 0, operators: [{ subject: 'alice', login: 'alice' }] };
const ignored = { code: 'ignored_by_repository', detail: '[[repository.peer]] github.com/acme/a' };
const effectiveView = (diagnostics: object[], source = 'repository') => ({
  workspace: '0123456789abcdef', generation: 'cac067a808cf689d',
  entries: [{ repo: Y, source, blocks: {}, ignored: ['[[repository.peer]] github.com/acme/a'], coverage: 'indexed', diagnostics }],
});
const editorBox = () => within(getByRole(host, 'region', { name: namedCopy.en.edit }));
const peerText = `[[repository]]\nrepo = "${Y}"\n\n[[repository.peer]]\nrepo = "${A}"\nbase_url_env = ["GATEWAY_URL"]\n`;

async function open(table: Record<string, () => Response>) {
  const fetch = answers({
    'GET /api/v1/workspaces?': () => Response.json({ ...workspaces, workspaces: [{ ...workspaces.workspaces[0]!, operate: true }] }),
    'GET /api/v1/catalog': () => Response.json(catalog([Y, A])),
    'GET /api/v1/workspaces/0123456789abcdef/effective': () => Response.json(effectiveView([])),
    'GET /api/v1/workspaces/0123456789abcdef': () => Response.json(operated),
    ...table,
  });
  vi.stubGlobal('fetch', fetch);
  await page();
  await waitFor(() => expect(getByRole(host, 'button', { name: `${namedCopy.en.show} Payments` })).toBeDefined());
  await act(() => { fireEvent.click(getByRole(host, 'button', { name: `${namedCopy.en.show} Payments` })); });
  await waitFor(() => expect(getByLabelText(host, namedCopy.en.manifestText)).toBeDefined());
  return fetch;
}

test('an operator edits the manifest text, reads the effective configuration before saving and nothing is sent until it confirms', async () => {
  const t = namedCopy.en;
  const fetch = await open({
    'POST /api/v1/workspaces/effective': () => Response.json(effectiveView([ignored])),
    'POST /api/v1/workspaces/0123456789abcdef/versions': () => Response.json({ base: 1, version: 'aaaaaaaaaaaaaaaa', saved: true, diagnostics: [{ repo: Y, ...ignored }] }, { status: 201 }),
    'POST /api/v1/workspaces/0123456789abcdef/apply': () => Response.json({ base: 2, version: 'aaaaaaaaaaaaaaaa', action: 'apply', diagnostics: [{ repo: Y, ...ignored }] }),
  });
  const editor = getByLabelText(host, t.manifestText) as HTMLTextAreaElement;
  expect(editor.value).toContain(`repo = "${Y}"`);
  expect((getByRole(host, 'button', { name: t.saveApply }) as HTMLButtonElement).disabled).toBe(true);
  await act(() => { fireEvent.change(editor, { target: { value: peerText } }); });
  await act(() => { fireEvent.change(editorBox().getByLabelText(t.message), { target: { value: 'link the gateway' } }); });
  // The effective configuration is shown before anything is saved, in text: the repository's own file wins.
  await act(() => { fireEvent.click(getByRole(host, 'button', { name: t.checkEffective })); });
  await waitFor(() => expect(host.textContent).toContain(t.diagnostic_ignored_by_repository));
  expect(host.textContent).toContain(t.source_repository);
  expect(host.textContent).toContain(t.effectiveNote);
  const posts = () => fetch.mock.calls.filter(([, init]) => (init as RequestInit | undefined)?.method === 'POST').map(([route]) => route);
  expect(posts()).toEqual(['/api/v1/workspaces/effective']);
  const preview = fetch.mock.calls.find(([route]) => route === '/api/v1/workspaces/effective')![1] as RequestInit;
  expect(JSON.parse(preview.body as string)).toEqual({ entries: peerText });
  // Review, then confirm: the saved payload is the reviewed text at the base the page read.
  await act(() => { fireEvent.click(getByRole(host, 'button', { name: t.saveApply })); });
  expect(posts()).toEqual(['/api/v1/workspaces/effective']);
  await act(() => { fireEvent.click(getAllByRole(host, 'button', { name: t.saveApply }).at(-1)!); });
  await waitFor(() => expect(posts()).toHaveLength(3));
  const body = (route: string) => JSON.parse((fetch.mock.calls.find(([r]) => r === route)![1] as RequestInit).body as string) as Record<string, unknown>;
  expect(body('/api/v1/workspaces/0123456789abcdef/versions')).toEqual({ entries: peerText, message: 'link the gateway', base: 1 });
  expect(body('/api/v1/workspaces/0123456789abcdef/apply')).toEqual({ version: 'aaaaaaaaaaaaaaaa', base: 1 });
  // An applied change that something overrides is not shown as plainly applied: the finding is read beside it.
  await waitFor(() => expect(getByRole(host, 'alert', { name: t.findings }).textContent).toContain(`${Y}: ${t.diagnostic_ignored_by_repository}`));
});

test('a configuration the held generation has not recorded says so in text', async () => {
  const t = namedCopy.en;
  await open({ 'GET /api/v1/workspaces/0123456789abcdef/effective': () => Response.json({ ...effectiveView([]), recorded: false }) });
  await waitFor(() => expect(host.textContent).toContain(t.notRecorded));
  expect(getByRole(host, 'list', { name: t.effective })).toBeDefined();
});

test('a stale base is refused in words and a viewer gets no editor', async () => {
  const t = namedCopy.en;
  await open({
    'POST /api/v1/workspaces/0123456789abcdef/versions': () => Response.json({ base: 1, version: 'aaaaaaaaaaaaaaaa', saved: true }, { status: 201 }),
    'POST /api/v1/workspaces/0123456789abcdef/apply': () => Response.json({ error: { code: 'stale_base', message: 'changed', base: 2 } }, { status: 409 }),
  });
  await act(() => { fireEvent.change(getByLabelText(host, t.manifestText), { target: { value: peerText } }); });
  await act(() => { fireEvent.change(editorBox().getByLabelText(t.message), { target: { value: 'm' } }); });
  await act(() => { fireEvent.click(getByRole(host, 'button', { name: t.saveApply })); });
  await act(() => { fireEvent.click(getAllByRole(host, 'button', { name: t.saveApply }).at(-1)!); });
  await waitFor(() => expect(host.textContent).toContain(t.staleBase));
  await act(() => root.render(null));
  const viewing = answers({
    'GET /api/v1/workspaces?': () => Response.json(workspaces),
    'GET /api/v1/catalog': () => Response.json(catalog([Y, A])),
    'GET /api/v1/workspaces/0123456789abcdef/effective': () => Response.json(effectiveView([])),
    'GET /api/v1/workspaces/0123456789abcdef': () => Response.json(detail),
  });
  vi.stubGlobal('fetch', viewing);
  await page();
  await waitFor(() => expect(getByRole(host, 'button', { name: `${t.show} Payments` })).toBeDefined());
  await act(() => { fireEvent.click(getByRole(host, 'button', { name: `${t.show} Payments` })); });
  await waitFor(() => expect(getByRole(host, 'list', { name: t.effective })).toBeDefined());
  expect(queryByRole(host, 'region', { name: t.edit })).toBeNull();
  expect(host.textContent).not.toContain(t.manifestText);
});
