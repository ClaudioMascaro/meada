import { readFileSync } from 'node:fs';
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { fireEvent, getAllByRole, getByLabelText, getByRole, queryAllByRole, queryByRole, waitFor, within } from '@testing-library/dom';
import { afterEach, beforeAll, expect, test, vi } from 'vitest';
import { init, initSync, namedEntriesText } from '../wasm/meada_core';
import { Api } from '../src/api';
import { Auth } from '../src/auth';
import { NamedWorkspacesPage } from '../src/NamedWorkspacesPage';
import { entriesJson } from '../src/guided';
import type { Entry } from '../src/guided';
import { guidedCopy } from '../src/guided-i18n';
import type { WorkspaceDetail } from '../src/named';
import { namedCopy } from '../src/named-i18n';
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

const Y = 'github.com/acme/y', A = 'github.com/acme/a', B = 'github.com/acme/b';
const ID = '0123456789abcdef';
const catalog = (identities: string[]) => ({
  state: 'ready', reason: null, set: { finished: '2026-10-30T01:00:00.000000Z', age_seconds: 1, max_age_seconds: 5400 },
  repositories: identities.map(identity => ({ identity })), next: null,
});
const operated: WorkspaceDetail = {
  id: ID, name: 'Payments', description: 'Y', created: '2026-10-30T01:00:00Z', version: 'fedcba9876543210', base: 1,
  state: 'applied', entries: [{ repo: Y, ingestion: 'ready', blocks: {} }], operate: true, hidden: 0, operators: [{ subject: 'alice', login: 'alice' }],
};
const viewed: WorkspaceDetail = { ...operated, operate: false, operators: undefined };
const view = (diagnostics: object[] = []) => ({
  workspace: ID, generation: 'cac067a808cf689d',
  entries: [{ repo: Y, source: 'context', blocks: {}, ignored: [], coverage: 'indexed', diagnostics }],
});
const summary = (operate: boolean) => ({ workspaces: [{ id: ID, name: 'Payments', description: 'Y', version: 'fedcba9876543210', state: 'applied', entries: 1, operate }], next: null });

/** A `fetch` answering by route, the way the console does: effects of a page may ask in any order. */
function answers(table: Record<string, () => Response>) {
  return vi.fn(async (route: string, init?: RequestInit) => {
    const key = `${init?.method ?? 'GET'} ${route}`;
    const answer = table[key] ?? Object.entries(table).find(([k]) => key.startsWith(k))?.[1];
    if (!answer) throw new Error(`unexpected ${key}`);
    return answer();
  });
}

async function open(table: Record<string, () => Response>, locale: 'en' | 'pt-BR' = 'en', detail = operated) {
  const fetch = answers({
    'GET /api/v1/workspaces?': () => Response.json(summary(detail.operate)),
    'GET /api/v1/catalog': () => Response.json(catalog([Y, A])),
    [`GET /api/v1/workspaces/${ID}/effective`]: () => Response.json(view()),
    [`GET /api/v1/workspaces/${ID}`]: () => Response.json(detail),
    ...table,
  });
  vi.stubGlobal('fetch', fetch);
  const session = { subject: 'alice', role: 'viewer' as const, grant: [Y, A] };
  await act(() => root.render(<NamedWorkspacesPage api={new Api(new Auth(config))} locale={locale} session={session} maxEntries={5} />));
  const show = namedCopy[locale].show;
  await waitFor(() => expect(getByRole(host, 'button', { name: `${show} Payments` })).toBeDefined());
  await act(() => { fireEvent.click(getByRole(host, 'button', { name: `${show} Payments` })); });
  await waitFor(() => expect(getByRole(host, 'region', { name: namedCopy[locale].edit })).toBeDefined());
  return fetch;
}
const editor = (locale: 'en' | 'pt-BR' = 'en') => within(getByRole(host, 'region', { name: namedCopy[locale].edit }));
const entry = (identity: string) => within(getByRole(host, 'group', { name: identity }));
const text = () => (getByLabelText(host, namedCopy.en.manifestText) as HTMLTextAreaElement).value;
const posts = (fetch: ReturnType<typeof answers>) => fetch.mock.calls.filter(([, init]) => (init as RequestInit | undefined)?.method === 'POST').map(([route]) => route);
const type = (element: HTMLElement, value: string) => act(() => { fireEvent.change(element, { target: { value } }); });
const press = (element: HTMLElement) => act(() => { fireEvent.click(element); });

const g = guidedCopy.en;
const t = namedCopy.en;
const full: Entry[] = [
  { repo: Y, serve: { hosts: ['y.example'], prefix: '/api' }, resource: [{ kind: 'postgres', id: 'orders', role: 'read-write' }], peer: [{ repo: A, base_url_env: ['A_URL'] }] },
  { repo: A },
];

async function assemble() {
  await type(editor().getByLabelText(g.addSelect), A);
  await press(editor().getByRole('button', { name: g.addButton }));
  await press(entry(Y).getByRole('checkbox', { name: g.declareServe }));
  await type(entry(Y).getByLabelText(g.hosts), 'y.example');
  await type(entry(Y).getByLabelText(g.prefix), '/api');
  await press(entry(Y).getByRole('button', { name: g.addResource }));
  await type(entry(Y).getByLabelText(g.kind), 'postgres');
  await type(entry(Y).getByLabelText(g.id), 'orders');
  await type(entry(Y).getByLabelText(g.role), 'read-write');
  await press(entry(Y).getByRole('button', { name: g.addPeer }));
  await type(entry(Y).getByLabelText(g.envRefs), 'A_URL');
}

test('the guided controls and the text are one canonical document, and computing them opens no connection', async () => {
  const fetch = await open({});
  const before = fetch.mock.calls.length;
  await assemble();
  // The form wrote exactly the text the core writes for those entries, and nothing but a route read happened.
  expect(text()).toBe(namedEntriesText(entriesJson(full)));
  expect(fetch.mock.calls.length).toBe(before);
  // The same document typed as text fills the form with the same values.
  const typed = text();
  await type(getByLabelText(host, t.manifestText), `[[repository]]\nrepo = "${Y}"\n[repository.serve]\nhosts = ["other.example"]\n`);
  expect((entry(Y).getByLabelText(g.hosts) as HTMLTextAreaElement).value).toBe('other.example');
  expect(queryByRole(host, 'group', { name: A })).toBeNull();
  await type(getByLabelText(host, t.manifestText), typed);
  expect((entry(Y).getByLabelText(g.prefix) as HTMLInputElement).value).toBe('/api');
  expect((entry(Y).getByLabelText(g.kind) as HTMLInputElement).value).toBe('postgres');
  expect((entry(Y).getByLabelText(g.envRefs) as HTMLTextAreaElement).value).toBe('A_URL');
  expect(text()).toBe(typed);
  expect(fetch.mock.calls.length).toBe(before);
});

test('a value the core refuses is shown as its message and blocks the save; text that does not parse makes the form read-only', async () => {
  await open({});
  await assemble();
  await type(entry(Y).getByLabelText(g.envRefs), 'A_URL=https://secret.example');
  await waitFor(() => expect(getAllByRole(host, 'alert').some(a => /A_URL=https:/.test(a.textContent ?? ''))).toBe(true));
  await type(editor().getByLabelText(t.message), 'msg');
  expect((editor().getByRole('button', { name: t.saveApply }) as HTMLButtonElement).disabled).toBe(true);
  await type(getByLabelText(host, t.manifestText), '[[repository\nrepo =');
  expect(host.textContent).toContain(g.textInvalid);
  expect((getByRole(host, 'group', { name: Y }) as HTMLFieldSetElement).disabled).toBe(true);
});

test('eligibility, ingestion and the effective configuration of each entry are text; a repository outside the catalog must be verified, and is refused in words', async () => {
  const fetch = await open({
    'POST /api/v1/catalog/check': () => Response.json({ state: 'ready', reason: null, results: [{ identity: B, state: 'unavailable' }] }),
    'POST /api/v1/workspaces/effective': () => Response.json({
      ...view(), entries: [{ repo: Y, source: 'repository', blocks: {}, ignored: ['[[repository.peer]] ' + A], coverage: 'indexed', diagnostics: [{ code: 'ignored_by_repository', detail: '[[repository.peer]] ' + A }] }],
    }),
  });
  await type(editor().getByLabelText(g.addSelect), A);
  await press(editor().getByRole('button', { name: g.addButton }));
  const list = (identity: string) => getByRole(host, 'list', { name: `${identity}: ${g.eligibility}` }).textContent ?? '';
  expect(list(Y)).toContain(g.unchanged);
  expect(list(Y)).toContain(`${g.ingestion}: ${t.ready}`);
  expect(list(A)).toContain(g.selectable);
  expect(list(A)).toContain(g.newEntry);
  // B is typed in the text: the catalog never listed it, so it is not verified and the change cannot be saved.
  await type(getByLabelText(host, t.manifestText), `${text()}\n[[repository]]\nrepo = "${B}"\n`);
  await type(editor().getByLabelText(t.message), 'add b');
  expect(list(B)).toContain(g.notVerified);
  expect((editor().getByRole('button', { name: t.saveApply }) as HTMLButtonElement).disabled).toBe(true);
  expect(getByRole(host, 'alert', { name: g.notEligible }).textContent).toContain(g.notEligibleHidden(1));
  await press(editor().getByRole('button', { name: g.verify }));
  await waitFor(() => expect(list(B)).toContain(g.unavailable));
  expect(JSON.parse((fetch.mock.calls.find(([r]) => r === '/api/v1/catalog/check')![1] as RequestInit).body as string)).toEqual({ identities: [B] });
  // The effective configuration of the edits names what the repository's own file overrides, inside the entry.
  await press(entry(B).getByRole('button', { name: `${g.removeRepository} ${B}` }));
  await press(editor().getByRole('button', { name: t.checkEffective }));
  await waitFor(() => expect(list(Y)).toContain(t.diagnostic_ignored_by_repository));
  expect(list(Y)).toContain(t.source_repository);
  expect(host.textContent).toContain(g.previewOf);
});

test('the diff is concrete and the reviewed payload is frozen until the person confirms, then saved at the base the page read', async () => {
  const fetch = await open({
    [`POST /api/v1/workspaces/${ID}/versions`]: () => Response.json({ base: 1, version: 'aaaaaaaaaaaaaaaa', saved: true }, { status: 201 }),
    [`POST /api/v1/workspaces/${ID}/apply`]: () => Response.json({ base: 2, version: 'aaaaaaaaaaaaaaaa', action: 'apply' }),
  });
  await assemble();
  expect(getByRole(host, 'list', { name: g.diffTitle }).textContent).toContain(`${g.diffAdded}: ${A}`);
  expect(getByRole(host, 'list', { name: g.diffTitle }).textContent).toContain(`${g.diffChanged}: ${Y} (${g.keyPeer}, ${g.keyResource}, ${g.keyServe})`);
  await type(editor().getByLabelText(t.message), 'link a');
  const reviewed = text();
  await press(editor().getByRole('button', { name: t.saveApply }));
  expect(posts(fetch)).toEqual([]);
  const dialog = getByRole(host, 'dialog');
  expect(within(dialog).getByRole('list', { name: `${g.diffTitle} (${g.payload})` }).textContent).toContain(`${g.diffAdded}: ${A}`);
  expect(dialog.textContent).toContain('link a');
  // Whatever happens to the draft behind the dialog, only the reviewed payload is sent.
  await type(getByLabelText(host, t.manifestText), `[[repository]]\nrepo = "${B}"\n`);
  await type(editor().getByLabelText(t.message), 'changed behind the dialog');
  expect(dialog.textContent).toContain('link a');
  expect(dialog.textContent).not.toContain('changed behind the dialog');
  await press(within(dialog).getByRole('button', { name: t.saveApply }));
  await waitFor(() => expect(posts(fetch)).toEqual([`/api/v1/workspaces/${ID}/versions`, `/api/v1/workspaces/${ID}/apply`]));
  const body = (route: string) => JSON.parse((fetch.mock.calls.find(([r]) => r === route)![1] as RequestInit).body as string) as Record<string, unknown>;
  expect(body(`/api/v1/workspaces/${ID}/versions`)).toEqual({ entries: reviewed, message: 'link a', base: 1 });
  expect(body(`/api/v1/workspaces/${ID}/apply`)).toEqual({ version: 'aaaaaaaaaaaaaaaa', base: 1 });
  await waitFor(() => expect(host.textContent).toContain(t.savedNote));
});

test('a grant lost between review and apply is refused as the server words it, the roles are read again and the person recovers by removing the entry', async () => {
  let listed = [Y, A];
  const fetch = await open({
    'GET /api/v1/catalog': () => Response.json(catalog(listed)),
    [`POST /api/v1/workspaces/${ID}/versions`]: () => {
      listed = [Y];
      return Response.json({ error: { code: 'entries_refused', message: 'refused', refused: [{ identity: A, change: 'add', reason: 'not_eligible' }], hidden: 1 } }, { status: 403 });
    },
  });
  await type(editor().getByLabelText(g.addSelect), A);
  await press(editor().getByRole('button', { name: g.addButton }));
  await type(editor().getByLabelText(t.message), 'add a');
  const catalogReads = () => fetch.mock.calls.filter(([r]) => String(r).startsWith('/api/v1/catalog?')).length;
  const before = catalogReads();
  const save = () => editor().getByRole('button', { name: t.saveApply }) as HTMLButtonElement;
  const eligibility = () => getByRole(host, 'list', { name: `${A}: ${g.eligibility}` }).textContent ?? '';
  expect(eligibility()).toContain(g.selectable);
  await press(save());
  await press(within(getByRole(host, 'dialog')).getByRole('button', { name: t.saveApply }));
  const region = await waitFor(() => getByRole(host, 'alert', { name: g.refusedSave }));
  expect(region.textContent).toContain(A);
  expect(region.textContent).toContain(g.refusedHidden(1));
  expect(queryByRole(host, 'dialog')).toBeNull();
  await waitFor(() => expect(catalogReads()).toBeGreaterThan(before));
  // The draft is kept, but A is no longer selectable and the change cannot be saved again as it is.
  await waitFor(() => expect(eligibility()).toContain(g.notVerified));
  expect(save().disabled).toBe(true);
  await press(entry(A).getByRole('button', { name: `${g.removeRepository} ${A}` }));
  expect(text()).toBe(namedEntriesText(entriesJson([{ repo: Y }])));
});

test('losing the operator role turns the editor away and says why; a stale base offers a reload', async () => {
  let operate = true;
  const fetch = answers({
    'GET /api/v1/workspaces?': () => Response.json(summary(operate)),
    'GET /api/v1/catalog': () => Response.json(catalog([Y, A])),
    [`GET /api/v1/workspaces/${ID}/effective`]: () => Response.json(view()),
    [`GET /api/v1/workspaces/${ID}`]: () => Response.json(operate ? operated : viewed),
    [`POST /api/v1/workspaces/${ID}/versions`]: () => Response.json({ error: { code: 'not_operator', message: 'no' } }, { status: 403 }),
  });
  vi.stubGlobal('fetch', fetch);
  await act(() => root.render(<NamedWorkspacesPage api={new Api(new Auth(config))} locale="en" session={{ subject: 'alice', role: 'viewer', grant: [Y, A] }} maxEntries={5} />));
  await waitFor(() => expect(getByRole(host, 'button', { name: `${t.show} Payments` })).toBeDefined());
  await press(getByRole(host, 'button', { name: `${t.show} Payments` }));
  await waitFor(() => expect(getByRole(host, 'region', { name: t.edit })).toBeDefined());
  await type(editor().getByLabelText(g.addSelect), A);
  await press(editor().getByRole('button', { name: g.addButton }));
  await type(editor().getByLabelText(t.message), 'x');
  await press(editor().getByRole('button', { name: t.saveApply }));
  operate = false;
  await press(within(getByRole(host, 'dialog')).getByRole('button', { name: t.saveApply }));
  await waitFor(() => expect(queryByRole(host, 'region', { name: t.edit })).toBeNull());
  await waitFor(() => expect(host.textContent).toContain(t.notOperator));
  expect(host.textContent).toContain(t.viewOnly);
});

test('a stale base is refused in words, keeps the draft and offers to reload', async () => {
  let reads = 0;
  await open({
    [`POST /api/v1/workspaces/${ID}/versions`]: () => Response.json({ base: 1, version: 'aaaaaaaaaaaaaaaa', saved: true }, { status: 201 }),
    [`POST /api/v1/workspaces/${ID}/apply`]: () => Response.json({ error: { code: 'stale_base', message: 'changed', base: 2 } }, { status: 409 }),
    [`GET /api/v1/workspaces/${ID}`]: () => { reads += 1; return Response.json(operated); },
  });
  await type(editor().getByLabelText(g.addSelect), A);
  await press(editor().getByRole('button', { name: g.addButton }));
  await type(editor().getByLabelText(t.message), 'm');
  await press(editor().getByRole('button', { name: t.saveApply }));
  await press(within(getByRole(host, 'dialog')).getByRole('button', { name: t.saveApply }));
  await waitFor(() => expect(host.textContent).toContain(t.staleBase));
  expect(host.textContent).toContain(g.storedNotApplied);
  const first = reads;
  await press(within(getByRole(host, 'dialog')).getByRole('button', { name: t.cancel }));
  await press(editor().getByRole('button', { name: g.reload }));
  await waitFor(() => expect(reads).toBeGreaterThan(first));
});

test('a viewer sees the entries and their state but no mutation control', async () => {
  const fetch = answers({
    'GET /api/v1/workspaces?': () => Response.json(summary(false)),
    'GET /api/v1/catalog': () => Response.json(catalog([Y, A])),
    [`GET /api/v1/workspaces/${ID}/effective`]: () => Response.json(view()),
    [`GET /api/v1/workspaces/${ID}`]: () => Response.json(viewed),
  });
  vi.stubGlobal('fetch', fetch);
  await act(() => root.render(<NamedWorkspacesPage api={new Api(new Auth(config))} locale="en" session={{ subject: 'bob', role: 'viewer', grant: [Y, A] }} maxEntries={5} />));
  await waitFor(() => expect(getByRole(host, 'button', { name: `${t.show} Payments` })).toBeDefined());
  await press(getByRole(host, 'button', { name: `${t.show} Payments` }));
  await waitFor(() => expect(getByRole(host, 'list', { name: t.effective })).toBeDefined());
  const detail = within(getByRole(host, 'region', { name: 'Payments' }));
  expect(detail.queryAllByRole('button')).toEqual([]);
  expect(detail.queryAllByRole('textbox')).toEqual([]);
  expect(queryByRole(host, 'region', { name: t.edit })).toBeNull();
  expect(queryByRole(host, 'group', { name: Y })).toBeNull();
  expect(detail.getByText(t.viewOnly)).toBeDefined();
});

test('every control has a name and is reachable by keyboard', async () => {
  await open({});
  await assemble();
  const controls = [...getByRole(host, 'region', { name: t.edit }).querySelectorAll<HTMLElement>('input, select, textarea, button')];
  expect(controls.length).toBeGreaterThan(15);
  for (const control of controls) {
    const label = control.getAttribute('aria-label') ?? (control as HTMLInputElement).labels?.[0]?.textContent ?? control.textContent;
    expect(label?.trim(), control.outerHTML).toBeTruthy();
    expect(control.getAttribute('tabindex'), control.outerHTML).not.toBe('-1');
    if ((control as HTMLInputElement).disabled) continue;
    act(() => control.focus());
    expect(document.activeElement, control.outerHTML).toBe(control);
  }
  // Groups are named by their legends, so a screen reader announces which entry a field belongs to.
  expect(queryAllByRole(host, 'group', { name: Y })).toHaveLength(1);
  expect(entry(Y).getByRole('group', { name: g.peer(1) })).toBeDefined();
});

test('the guided controls are in pt-BR with the same structure', async () => {
  const p = guidedCopy['pt-BR'], n = namedCopy['pt-BR'];
  const fetch = await open({}, 'pt-BR');
  const box = within(getByRole(host, 'region', { name: n.edit }));
  expect(box.getByRole('region', { name: p.scope }).textContent).toContain(p.scopeOperate(1));
  await type(box.getByLabelText(p.addSelect), A);
  await press(box.getByRole('button', { name: p.addButton }));
  await press(within(getByRole(host, 'group', { name: Y })).getByRole('button', { name: p.addPeer }));
  const list = getByRole(host, 'list', { name: `${A}: ${p.eligibility}` }).textContent ?? '';
  expect(list).toContain(p.selectable);
  expect(list).toContain(p.newEntry);
  expect(getByRole(host, 'list', { name: p.diffTitle }).textContent).toContain(`${p.diffAdded}: ${A}`);
  expect(posts(fetch)).toEqual([]);
});

test('a catalog that cannot decide says why in the editor and offers nothing to add, while the text stays editable for removals', async () => {
  await open({ 'GET /api/v1/catalog': () => Response.json({ state: 'pending', reason: 'stale_set', set: null, repositories: [], next: null }) });
  expect(editor().getAllByRole('status').map(e => e.textContent).join('\n')).toContain(t.stale_set);
  expect(editor().getAllByRole('status').map(e => e.textContent).join('\n')).toContain(g.catalogPending);
  expect(queryByRole(host, 'combobox', { name: g.addSelect })).toBeNull();
  expect((editor().getByRole('group', { name: g.add }) as HTMLFieldSetElement).disabled).toBe(true);
  await press(entry(Y).getByRole('button', { name: `${g.removeRepository} ${Y}` }));
  expect(queryByRole(host, 'group', { name: Y })).toBeNull();
  expect(host.textContent).toContain(g.formEmpty);
});

test('a verification the permissions set could not decide can be asked again', async () => {
  let answer = 'pending';
  const fetch = await open({
    'POST /api/v1/catalog/check': () => Response.json(answer === 'pending'
      ? { state: 'pending', reason: 'stale_set', results: [{ identity: B, state: 'pending' }] }
      : { state: 'ready', reason: null, results: [{ identity: B, state: 'selectable' }] }),
  });
  await type(getByLabelText(host, t.manifestText), `${text()}\n[[repository]]\nrepo = "${B}"\n`);
  const verify = () => editor().getByRole('button', { name: g.verify }) as HTMLButtonElement;
  await press(verify());
  await waitFor(() => expect(getByRole(host, 'list', { name: `${B}: ${g.eligibility}` }).textContent).toContain(g.pendingVerification));
  expect(verify().disabled).toBe(false);
  answer = 'ready';
  await press(verify());
  await waitFor(() => expect(getByRole(host, 'list', { name: `${B}: ${g.eligibility}` }).textContent).toContain(g.selectable));
  expect(fetch.mock.calls.filter(([r]) => r === '/api/v1/catalog/check')).toHaveLength(2);
});
