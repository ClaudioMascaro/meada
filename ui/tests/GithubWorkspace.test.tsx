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

// The workspace views under the GitHub grant provider (#333): the editor attributes and authorizes
// additions with the core's functions given the session's login and host, shows each entry's
// `added_by`, and every view marks the entries the last role check left out (`excluded`).
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const host = document.createElement('div'); document.body.append(host);
const root = createRoot(host);
beforeAll(() => {
  initSync({ module: readFileSync('wasm/meada_core_bg.wasm') });
  HTMLDialogElement.prototype.showModal = function () { this.open = true; };
  HTMLDialogElement.prototype.close = function () { this.open = false; };
});
afterEach(async () => { await act(() => root.render(null)); vi.unstubAllGlobals(); });

const x = { repo: 'g.com/t/x', path: 'x', owner: 'team-a', added_by: 'ana-souza' };
const y = { repo: 'g.com/t/y', path: 'y', owner: 'team-b' };
const w = { repo: 'a.com/t/w', path: 'w', owner: 'team-a' };
const version = (id: string, repositories: object[]) => ({ id, author: 'alice', time: '2026-10-23T10:00:00Z', message: id, repositories });
// What the routes answer: the current version marks `x` as left out of the last refresh, an older one carries no mark.
const current = version('1111111111111111', [{ ...x, excluded: true }, y, w]);
const older = version('2222222222222222', [x, y, w]);
const github = { login: 'ana-souza', github_host: 'g.com', github_grant: 'current' as const };
const team: Session = { subject: 'ana', role: 'team_operator', groups: ['team-a'], grant: ['a.com/t/w', 'g.com/t/x', 'g.com/t/y'], teams: ['team-a'], ...github };
const anonymous: Session = { ...team, login: undefined };
const operator: Session = { subject: 'root', role: 'operator', ...github };

function reads() {
  return vi.fn(async (url: string, init?: RequestInit) => {
    if (init?.method === 'POST') throw new Error(`unexpected POST ${url}`);
    if (url === '/api/v1/workspace') return Response.json({ current: { id: current.id, fence: '7' }, version: current });
    if (url === '/api/v1/workspace/versions') return Response.json({ current: { id: current.id, fence: '7' }, versions: [current, older], malformed: [] });
    if (url === '/api/v1/manifest') return Response.json({ file: 'ops.toml', manifest: {}, modules: [], errors: [] });
    const found = [current, older].find(row => url === `/api/v1/workspace/versions/${row.id}`);
    if (found) return Response.json(found);
    throw new Error(`unexpected GET ${url}`);
  });
}
async function render(session: Session, locale: 'en' | 'pt-BR', fetch: ReturnType<typeof vi.fn>) {
  vi.stubGlobal('fetch', fetch);
  await act(() => root.render(<WorkspacePage api={new Api(new Auth(config))} config={{ ...config, operations: true }} session={session} locale={locale} />));
}
async function editor(locale: 'en' | 'pt-BR') {
  const t = workspaceCopy[locale];
  const form = await waitFor(() => getByRole(host, 'form', { name: t.editor }));
  const entries = getByRole(form, 'textbox', { name: t.editEntries }) as HTMLTextAreaElement;
  await waitFor(() => expect(entries.value).not.toBe(''));
  await act(() => { fireEvent.change(getByRole(form, 'textbox', { name: t.saveMessage }), { target: { value: 'm' } }); });
  return { t, form, entries, save: getByRole(form, 'button', { name: t.save }) };
}
const type = (field: Element, value: string) => act(() => { fireEvent.change(field, { target: { value } }); });
const addition = (repo: string, extra = '') => `\n[[repository]]\nrepo = "${repo}"\npath = "n"\nowner = "team-a"\n${extra}`;

const marks = [
  { name: 'an addition on the host by a subject without a login', session: anonymous, edit: (text: string) => text + addition('g.com/t/new'), repo: 'g.com/t/new', reason: 'unattributed' as const },
  { name: 'an edit of added_by', session: team, edit: (text: string) => text.replace('added_by = "ana-souza"', 'added_by = "bia"'), repo: 'g.com/t/x', reason: 'added_by_change' as const },
  { name: 'an added_by set on an entry', session: team, edit: (text: string) => text.replace('path = "w"', 'path = "w"\nadded_by = "ana-souza"'), repo: 'a.com/t/w', reason: 'added_by_change' as const },
];
test.each(marks.flatMap(row => (['pt-BR', 'en'] as const).map(locale => ({ ...row, locale }))))('$name is marked $reason with its text in $locale, with no request, and disables the save', async ({ session, edit, repo, reason, locale }) => {
  const fetch = reads(); await render(session, locale, fetch);
  const { t, form, entries, save } = await editor(locale);
  await waitFor(() => expect(save.hasAttribute('disabled')).toBe(false));
  const count = fetch.mock.calls.length;
  const original = entries.value;
  await type(entries, edit(original));
  const diff = getByRole(form, 'region', { name: t.editorDiff });
  expect(diff.textContent).toContain(repo);
  expect(diff.textContent).toContain(`${t.refusedMark}: `); expect(diff.textContent).toContain(t[`reason_${reason}`]);
  expect(getByRole(form, 'status').textContent).toBe(t.refusedSummary);
  expect(save.hasAttribute('disabled')).toBe(true);
  await type(entries, original);
  expect(save.hasAttribute('disabled')).toBe(false);
  expect(fetch.mock.calls.length).toBe(count);
});

test('an addition on the host outside the grant is accepted with a login whatever its text says, and the save sends the text as typed', async () => {
  const read = reads();
  const fetch = vi.fn(async (url: string, init?: RequestInit) => init?.method === 'POST' ? Response.json({ result: 'abcdef0123456789' })
    : url === '/api/v1/workspace/versions/abcdef0123456789' ? Response.json(version('abcdef0123456789', [x, y, w])) : read(url, init));
  await render(team, 'en', fetch);
  const { t, form, entries, save } = await editor('en');
  const typed = entries.value + addition('g.com/t/new', 'added_by = "mallory"\n');
  await type(entries, typed);
  const diff = getByRole(form, 'region', { name: t.editorDiff }).textContent!;
  expect(diff).toContain('g.com/t/new'); expect(diff).not.toContain(t.refusedMark);
  // Off the host, the same addition is outside the grant.
  await type(entries, entries.value.replace('g.com/t/new', 'a.com/t/new'));
  expect(getByRole(form, 'region', { name: t.editorDiff }).textContent).toContain(t.reason_outside_grant);
  await type(entries, typed);
  await act(() => fireEvent.click(save));
  await act(() => fireEvent.click(getByRole(getByRole(host, 'dialog'), 'button', { name: t.confirmSave })));
  const posted = fetch.mock.calls.filter(([, init]) => init?.method === 'POST');
  expect(posted).toHaveLength(1);
  expect(JSON.parse(String(posted[0]?.[1]?.body))).toEqual({ entries: typed, message: 'm' });
  expect(typed).not.toContain('excluded');
});

test.each(['pt-BR', 'en'] as const)('a server outside_grant for an identity already present is listed like any refusal in %s', async locale => {
  const read = reads();
  const fetch = vi.fn(async (url: string, init?: RequestInit) => init?.method === 'POST'
    ? Response.json({ error: { code: 'entries_refused', message: 'refused', refused: [{ identity: 'g.com/t/vault', change: 'add', reason: 'outside_grant' }], hidden: 0 } }, { status: 403 })
    : read(url, init));
  await render(team, locale, fetch);
  const { t, entries, save } = await editor(locale);
  await type(entries, entries.value + addition('g.com/t/vault'));
  expect(save.hasAttribute('disabled')).toBe(false); // the editor cannot predict it
  await act(() => fireEvent.click(save));
  await act(() => fireEvent.click(getByRole(getByRole(host, 'dialog'), 'button', { name: t.confirmSave })));
  const alert = getByRole(getByRole(host, 'dialog'), 'alert');
  expect(alert.textContent).toContain('g.com/t/vault'); expect(alert.textContent).toContain(t.change_add);
  expect(alert.textContent).toContain(`${t.reasonLabel}: ${t.reason_outside_grant}`);
  expect(alert.textContent).not.toContain(t.hiddenRefused);
});

test.each(['pt-BR', 'en'] as const)('the team editor shows each entry’s added_by and marks the excluded one with its reason, in text, in %s', async locale => {
  const fetch = reads(); await render(team, locale, fetch);
  const { t, form, entries } = await editor(locale);
  const table = getByRole(form, 'region', { name: t.entryState });
  expect(table.tabIndex).toBe(0);
  expect(within(table).getByRole('columnheader', { name: t.addedBy })).toBeTruthy();
  const row = (repo: string) => within(table).getByText(repo).closest('tr')!.textContent!;
  expect(row('g.com/t/x')).toContain('ana-souza'); expect(row('g.com/t/x')).toContain(t.excluded);
  expect(row('g.com/t/y')).toContain(t.noAddedBy); expect(row('g.com/t/y')).not.toContain(t.excluded);
  // `excluded` is response metadata: never in the text, so never in a save.
  expect(entries.value).toBe(entriesText(JSON.stringify([x, y, w])));
  expect(entries.value).not.toContain('excluded');
});

test.each(['pt-BR', 'en'] as const)('the global operator’s editor lists the excluded entries with the reason in a labelled status, in %s', async locale => {
  const fetch = reads(); await render(operator, locale, fetch);
  const { t, form, entries } = await editor(locale);
  const status = getByRole(form, 'status', { name: t.excluded });
  expect(status.textContent).toContain(t.excludedIntro);
  expect(status.textContent).toContain('g.com/t/x'); expect(status.textContent).toContain('ana-souza'); expect(status.textContent).not.toContain('g.com/t/y');
  expect(entries.value).not.toContain('excluded');
});

test.each(['pt-BR', 'en'] as const)('the versions page marks the excluded entries of the version that carries the mark only, in %s', async locale => {
  const t = workspaceCopy[locale];
  const fetch = reads(); await render({ subject: 'v', role: 'viewer', ...github }, locale, fetch);
  const owners = await waitFor(() => getByRole(host, 'region', { name: t.owners }));
  expect(owners.tabIndex).toBe(0);
  const row = (repo: string) => within(getByRole(host, 'region', { name: t.owners })).getByText(repo).closest('tr')!.textContent!;
  expect(row('g.com/t/x')).toContain(t.excluded); expect(row('g.com/t/x')).toContain('ana-souza');
  expect(row('a.com/t/w')).not.toContain(t.excluded);
  const mark = within(owners).getByText(new RegExp(t.excluded));
  expect(mark.tagName).toBe('STRONG'); expect(mark.className).toBe('workspace-excluded');
  await act(() => fireEvent.change(getByRole(host, 'combobox', { name: t.to }), { target: { value: older.id } }));
  await waitFor(() => expect(within(getByRole(host, 'region', { name: t.owners })).queryByText(new RegExp(t.excluded))).toBeNull());
  expect(row('g.com/t/x')).toContain('ana-souza');
  expect(queryByRole(host, 'form', { name: t.editor })).toBeNull();
  const versionEntries = getByRole(host, 'textbox', { name: t.entries }) as HTMLTextAreaElement;
  expect(versionEntries.value).not.toContain('excluded');
});
