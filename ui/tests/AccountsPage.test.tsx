import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { fireEvent, getAllByRole, getByLabelText, getByRole, queryByRole, waitFor, within } from '@testing-library/dom';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { AccountsPage } from '../src/AccountsPage';
import { accountsModel, createCheck, parseGroups } from '../src/accounts';
import type { ManagedAccount } from '../src/accounts';
import { accountsCopy } from '../src/accounts-i18n';
import { Api } from '../src/api';
import { LocalAuth } from '../src/local';
import { Shell, offered } from '../src/pages';
import { copy } from '../src/i18n';
import { auditCopy } from '../src/audit-i18n';
import { auditModel } from '../src/audit';

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
HTMLDialogElement.prototype.showModal ||= function showModal(this: HTMLDialogElement) { this.setAttribute('open', ''); };
HTMLDialogElement.prototype.close ||= function close(this: HTMLDialogElement) { this.removeAttribute('open'); };
const host = document.createElement('div');
document.body.append(host);
const root = createRoot(host);
const config = { version: '0.1.0', operations: true, public_url: location.origin, sign_in: 'accounts' as const };
const refusal = (status: number, code: string, message: string, extra: Record<string, unknown> = {}) =>
  Response.json({ error: { code, message, ...extra } }, { status });

const ana: ManagedAccount = { username: 'ana', role: 'viewer', state: 'active', locked_until: null, change_required: true,
  password_changed: '2026-10-30T10:00:00Z', created: '2026-10-30T10:00:00Z', created_by: 'local:admin', groups: ['team-web', 'team-ui'] };
const bob: ManagedAccount = { ...ana, username: 'bob', role: 'operator', state: 'suspended', change_required: false, groups: [] };
const cy: ManagedAccount = { ...ana, username: 'cy', state: 'locked', locked_until: '2026-11-01T12:15:00Z' };

beforeEach(() => { sessionStorage.clear(); });
afterEach(async () => { await act(() => root.render(null)); vi.unstubAllGlobals(); });

/** The fetch fake: the first answer lists the accounts, every later one is `answers`' next. */
function serve(...answers: Response[]) {
  const fetch = vi.fn().mockResolvedValueOnce(Response.json({ accounts: [ana, bob, cy] }));
  for (const answer of answers) fetch.mockResolvedValueOnce(answer);
  fetch.mockImplementation(() => Promise.resolve(Response.json({ accounts: [ana, bob, cy] })));
  vi.stubGlobal('fetch', fetch);
  return fetch;
}
const sent = (fetch: ReturnType<typeof vi.fn>, call: number) => ({ route: String(fetch.mock.calls[call]![0]), body: JSON.parse(fetch.mock.calls[call]![1].body) as unknown });
async function open(locale: 'en' | 'pt-BR' = 'en') {
  await act(() => root.render(<AccountsPage api={new Api(new LocalAuth(config))} locale={locale} />));
  await waitFor(() => getByRole(host, 'table'));
}
const type = (label: string, value: string) => fireEvent.change(getByLabelText(host, label), { target: { value } });

test('the view model marks suspended accounts in text and says until when a locked one is locked', () => {
  const t = accountsCopy.en;
  expect(accountsModel([ana, bob, cy], t)).toEqual([
    { username: 'ana', role: 'Viewer', groups: 'team-web, team-ui', state: 'Active', change: 'Yes', changed: '2026-10-30T10:00:00Z' },
    { username: 'bob', role: 'Operator', groups: 'None', state: 'Suspended', change: 'No', changed: '2026-10-30T10:00:00Z' },
    { username: 'cy', role: 'Viewer', groups: 'team-web, team-ui', state: 'Locked until 2026-11-01T12:15:00Z', change: 'Yes', changed: '2026-10-30T10:00:00Z' },
  ]);
  expect(accountsModel([cy], accountsCopy['pt-BR'])[0]!.state).toBe('Bloqueada até 2026-11-01T12:15:00Z');
});

test('the create check holds the username pattern and the password length in scalar values', () => {
  const t = accountsCopy.en;
  expect(createCheck('ana.lima_1-x', 'a'.repeat(12), t)).toBeUndefined();
  for (const bad of ['', 'Ana', 'a b', 'ána', 'a'.repeat(65), 'a/b']) expect(createCheck(bad, 'a'.repeat(12), t)).toBe(t.usernameInvalid);
  expect(createCheck('ana', 'a'.repeat(11), t)).toBe(t.passwordLength);
  expect(createCheck('ana', 'a'.repeat(129), t)).toBe(t.passwordLength);
  expect(createCheck('ana', '😀'.repeat(12), t)).toBeUndefined(); // 12 scalar values, 24 UTF-16 units
  expect(parseGroups(' team-a, team-b  team-a,,')).toEqual(['team-a', 'team-b']);
});

test('the page is offered to the global operator alone, and only with local accounts', () => {
  const paths = (role?: 'viewer' | 'team_operator' | 'operator', accounts = true) =>
    offered(role && { subject: 's', role }, false, false, false, true, accounts).map(route => route.path);
  expect(paths('operator')).toContain('/accounts');
  expect(paths('operator', false)).not.toContain('/accounts');
  for (const role of ['viewer', 'team_operator', undefined] as const) expect(paths(role)).not.toContain('/accounts');
});

test.each(['en', 'pt-BR'] as const)('the list shows every account with its state and labelled, keyboard reachable actions in %s', async locale => {
  const t = accountsCopy[locale];
  serve();
  await act(() => root.render(<Shell path="/accounts" locale={locale} onLanguage={() => {}} t={copy[locale]} accounts session={{ subject: 'local:admin', role: 'operator' }}><AccountsPage api={new Api(new LocalAuth(config))} locale={locale} /></Shell>));
  await waitFor(() => getByRole(host, 'table'));
  expect(getByRole(host, 'link', { name: copy[locale].accounts }).getAttribute('href')).toBe('/accounts');
  const rows = getAllByRole(host, 'row').slice(1);
  expect(rows).toHaveLength(3);
  expect(rows[1]!.textContent).toContain(t.suspended);
  expect(rows[2]!.textContent).toContain(`${t.lockedUntil} 2026-11-01T12:15:00Z`);
  expect(within(rows[1]!).queryByRole('button', { name: `${t.suspend}: bob` })).toBeNull(); // a suspended account offers reactivation
  for (const name of [`${t.setRole}: ana`, `${t.setGroups}: ana`, `${t.suspend}: ana`, `${t.resetPassword}: ana`, `${t.reactivate}: bob`]) {
    expect(getByRole(host, 'button', { name }).tabIndex).toBeGreaterThanOrEqual(0);
  }
  expect(getByRole(host, 'region', { name: t.list }).tabIndex).toBe(0);
});

test('the create form starts with the change-required box checked and asks the server only once its own checks pass', async () => {
  const t = accountsCopy.en;
  const fetch = serve(Response.json({ account: ana }));
  await open();
  const box = getByLabelText(host, t.changeRequired) as HTMLInputElement;
  expect(box.checked).toBe(true);
  expect((getByLabelText(host, t.temporaryPassword) as HTMLInputElement).type).toBe('password');
  type(t.username, 'Ana'); type(t.temporaryPassword, 'a long enough password');
  await act(async () => { fireEvent.submit(getByRole(host, 'button', { name: t.create }).closest('form')!); });
  expect(getByRole(host, 'alert').textContent).toBe(t.usernameInvalid);
  type(t.username, 'ana'); type(t.temporaryPassword, 'short');
  await act(async () => { fireEvent.submit(getByRole(host, 'button', { name: t.create }).closest('form')!); });
  expect(getByRole(host, 'alert').textContent).toBe(t.passwordLength);
  expect(fetch).toHaveBeenCalledTimes(1); // the list alone
  type(t.groupsLabel, 'team-web, team-ui'); type(t.temporaryPassword, 'a long enough password');
  fireEvent.click(box);
  await act(async () => { fireEvent.submit(getByRole(host, 'button', { name: t.create }).closest('form')!); });
  expect(sent(fetch, 1)).toEqual({ route: '/api/v1/accounts', body: { username: 'ana', role: 'viewer', groups: ['team-web', 'team-ui'], password: 'a long enough password', change_required: false } });
  await waitFor(() => expect(getByRole(host, 'status').textContent).toBe(t.created));
  expect(fetch).toHaveBeenCalledTimes(3); // the list again
});

test('a refused creation shows the server rule in the page language, or the server message unchanged', async () => {
  const t = accountsCopy['pt-BR'];
  serve(refusal(422, 'password_policy', 'common', { rule: 'common_password' }), refusal(409, 'username_taken', 'that username is taken'));
  await open('pt-BR');
  type(t.username, 'ana'); type(t.temporaryPassword, 'password1234');
  await act(async () => { fireEvent.submit(getByRole(host, 'button', { name: t.create }).closest('form')!); });
  expect(getByRole(host, 'alert').textContent).toBe(t.rule_common_password);
  await act(async () => { fireEvent.submit(getByRole(host, 'button', { name: t.create }).closest('form')!); });
  await waitFor(() => expect(getByRole(host, 'alert').textContent).toBe('that username is taken'));
});

test('a role change is confirmed in the dialog, sends nothing before and reloads after', async () => {
  const t = accountsCopy.en;
  const fetch = serve(Response.json({ account: { ...ana, role: 'operator' } }));
  await open();
  await act(async () => { fireEvent.click(getByRole(host, 'button', { name: `${t.setRole}: ana` })); });
  const dialog = getByRole(host, 'dialog', { hidden: true });
  expect(within(dialog).getByRole('heading', { hidden: true }).textContent).toBe(`${t.roleTitle} ana`);
  expect(fetch).toHaveBeenCalledTimes(1);
  fireEvent.change(within(dialog).getByLabelText(t.role), { target: { value: 'operator' } });
  await act(async () => { fireEvent.click(within(dialog).getByRole('button', { name: t.confirm, hidden: true })); });
  expect(sent(fetch, 1)).toEqual({ route: '/api/v1/accounts/ana', body: { action: 'role', role: 'operator' } });
  await waitFor(() => expect(queryByRole(host, 'dialog', { hidden: true })).toBeNull());
  expect(getByRole(host, 'status').textContent).toBe(t.done);
});

test('the groups dialog starts from the current list and replaces it', async () => {
  const t = accountsCopy.en;
  const fetch = serve(Response.json({ account: ana }));
  await open();
  await act(async () => { fireEvent.click(getByRole(host, 'button', { name: `${t.setGroups}: ana` })); });
  const dialog = getByRole(host, 'dialog', { hidden: true });
  const field = within(dialog).getByLabelText(t.groupsLabel) as HTMLInputElement;
  expect(field.value).toBe('team-web, team-ui');
  fireEvent.change(field, { target: { value: 'team-ops' } });
  await act(async () => { fireEvent.click(within(dialog).getByRole('button', { name: t.confirm, hidden: true })); });
  expect(sent(fetch, 1).body).toEqual({ action: 'groups', groups: ['team-ops'] });
});

test('suspend, reactivate and reset password each go through the dialog; last_operator is shown unchanged and the dialog stays', async () => {
  const t = accountsCopy.en;
  const fetch = serve(
    refusal(409, 'last_operator', 'the last active operator cannot be suspended or demoted'),
    Response.json({ account: bob }), Response.json({ accounts: [ana, bob, cy] }), Response.json({ account: ana }),
  );
  await open();
  await act(async () => { fireEvent.click(getByRole(host, 'button', { name: `${t.suspend}: ana` })); });
  let dialog = getByRole(host, 'dialog', { hidden: true });
  await act(async () => { fireEvent.click(within(dialog).getByRole('button', { name: t.confirm, hidden: true })); });
  expect(sent(fetch, 1)).toEqual({ route: '/api/v1/accounts/ana', body: { action: 'suspend' } });
  expect(within(dialog).getByRole('alert', { hidden: true }).textContent).toBe('the last active operator cannot be suspended or demoted');
  await act(async () => { fireEvent.click(within(dialog).getByRole('button', { name: t.cancel, hidden: true })); });
  expect(queryByRole(host, 'dialog', { hidden: true })).toBeNull();

  await act(async () => { fireEvent.click(getByRole(host, 'button', { name: `${t.reactivate}: bob` })); });
  dialog = getByRole(host, 'dialog', { hidden: true });
  await act(async () => { fireEvent.click(within(dialog).getByRole('button', { name: t.confirm, hidden: true })); });
  expect(sent(fetch, 2).body).toEqual({ action: 'reactivate' });
  await waitFor(() => expect(queryByRole(host, 'dialog', { hidden: true })).toBeNull());

  await act(async () => { fireEvent.click(getByRole(host, 'button', { name: `${t.resetPassword}: ana` })); });
  dialog = getByRole(host, 'dialog', { hidden: true });
  expect((within(dialog).getByLabelText(t.changeRequired) as HTMLInputElement).checked).toBe(true);
  fireEvent.change(within(dialog).getByLabelText(t.temporaryPassword), { target: { value: 'short' } });
  await act(async () => { fireEvent.click(within(dialog).getByRole('button', { name: t.confirm, hidden: true })); });
  expect(within(dialog).getByRole('alert', { hidden: true }).textContent).toBe(t.passwordLength);
  expect(fetch).toHaveBeenCalledTimes(4); // list, suspend, reactivate, list again: the short password never left
  fireEvent.change(within(dialog).getByLabelText(t.temporaryPassword), { target: { value: 'a new temporary password' } });
  fireEvent.click(within(dialog).getByLabelText(t.changeRequired));
  await act(async () => { fireEvent.click(within(dialog).getByRole('button', { name: t.confirm, hidden: true })); });
  expect(sent(fetch, 4)).toEqual({ route: '/api/v1/accounts/ana', body: { action: 'reset_password', password: 'a new temporary password', change_required: false } });
});

test('the audit page projects every account action in both languages and the role none', () => {
  const actions = ['account.login', 'account.password', 'account.reset_admin', 'account.create', 'account.role', 'account.groups', 'account.suspend', 'account.reactivate', 'account.reset_password', 'account.remove_github', 'account.github', 'account.token', 'account'];
  const records = actions.map((action, id) => ({ id, time: 't', via: 'console' as const, subject: null, login: null, role: 'none' as const, owners: null, action, arguments: {}, outcome: 'refused' as const, message: null }));
  for (const locale of ['en', 'pt-BR'] as const) {
    const rows = auditModel({ records, next: null }, auditCopy[locale]);
    rows.forEach((row, i) => { expect(row.action).toBe(`${auditCopy[locale].actions[actions[i]!]} (${actions[i]})`); expect(row.role).toBe(auditCopy[locale].none); });
  }
  expect(auditModel({ records: [{ ...records[0]!, action: 'workspace.save' }], next: null }, auditCopy.en)[0]!.action).toBe('workspace.save');
});
