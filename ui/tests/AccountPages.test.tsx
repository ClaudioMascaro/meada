import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { fireEvent, getByLabelText, getByRole, queryByRole, waitFor } from '@testing-library/dom';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { ForcedChangePage, ProfilePage, SignInPage } from '../src/AccountPages';
import { Shell } from '../src/pages';
import { accountCopy } from '../src/account-i18n';
import { copy } from '../src/i18n';
import { LocalAuth } from '../src/local';
import { Api } from '../src/api';
import type { Config, Session } from '../src/api';

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const host = document.createElement('div');
document.body.append(host);
const root = createRoot(host);
const config: Config = { version: '0.1.0', operations: true, public_url: location.origin, sign_in: 'accounts' };
const issued = (token: string, change = false) => Response.json({ token, expires_at: new Date(Date.now() + 3600_000).toISOString(), change_required: change });
const refusal = (status: number, code: string, extra: Record<string, unknown> = {}) => Response.json({ error: { code, message: `server ${code}`, ...extra } }, { status });

beforeEach(() => { sessionStorage.clear(); localStorage.clear(); });
afterEach(async () => { await act(() => root.render(null)); vi.unstubAllGlobals(); });

function type(label: string, value: string) { fireEvent.change(getByLabelText(host, label), { target: { value } }); }
async function submit() { await act(async () => { fireEvent.submit(host.querySelector('form')!); }); }

test.each(['en', 'pt-BR'] as const)('the sign-in page has labelled username and password fields and a submit in %s', async locale => {
  const t = accountCopy[locale];
  await act(() => root.render(<SignInPage auth={new LocalAuth(config)} locale={locale} expired={false} />));
  expect(getByRole(host, 'heading', { level: 1 }).textContent).toBe(t.signInTitle);
  const username = getByLabelText(host, t.username) as HTMLInputElement;
  const password = getByLabelText(host, t.password) as HTMLInputElement;
  expect(username.autocomplete).toBe('username');
  expect(password.type).toBe('password');
  expect(password.autocomplete).toBe('current-password');
  for (const element of [username, password, getByRole(host, 'button', { name: t.signInTitle })]) expect(element.tabIndex).toBeGreaterThanOrEqual(0);
  expect(queryByRole(host, 'alert')).toBeNull();
});

test('a sign-in sends the typed credentials; a refusal is one generic message and the page stays', async () => {
  const t = accountCopy.en;
  const fetch = vi.fn().mockResolvedValue(refusal(401, 'invalid_credentials'));
  vi.stubGlobal('fetch', fetch);
  const auth = new LocalAuth(config);
  await act(() => root.render(<SignInPage auth={auth} locale="en" expired={false} />));
  type(t.username, 'admin'); type(t.password, 'nope');
  await submit();
  expect(JSON.parse(fetch.mock.calls[0]![1].body)).toEqual({ username: 'admin', password: 'nope' });
  expect(getByRole(host, 'alert').textContent).toBe(t.signInInvalid);
  expect((getByRole(host, 'button', { name: t.signInTitle }) as HTMLButtonElement).disabled).toBe(false);
  expect(auth.stage()).toBe('out');
  expect(host.textContent).not.toContain('nope');
});

test.each([['busy', 'signInBusy'], ['audit_unavailable', 'signInAudit']] as const)('%s is shown as its own message', async (code, key) => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(refusal(503, code)));
  await act(() => root.render(<SignInPage auth={new LocalAuth(config)} locale="pt-BR" expired={false} />));
  type(accountCopy['pt-BR'].username, 'admin'); type(accountCopy['pt-BR'].password, 'x');
  await submit();
  expect(getByRole(host, 'alert').textContent).toBe(accountCopy['pt-BR'][key]);
});

test('a session that ended says so above the sign-in form', async () => {
  await act(() => root.render(<SignInPage auth={new LocalAuth(config)} locale="en" expired />));
  expect(getByRole(host, 'status').textContent).toBe(accountCopy.en.sessionEnded);
});

test.each(['en', 'pt-BR'] as const)('the forced change page asks for the current and the new password twice in %s and links nowhere', async locale => {
  const t = accountCopy[locale];
  const auth = new LocalAuth(config);
  await act(() => root.render(<Shell path="/profile" locale={locale} onLanguage={() => {}} t={copy[locale]} locked><ForcedChangePage auth={auth} locale={locale} /></Shell>));
  expect(getByRole(host, 'heading', { level: 1 }).textContent).toBe(t.forcedTitle);
  for (const label of [t.currentPassword, t.newPassword, t.repeatPassword]) {
    const input = getByLabelText(host, label) as HTMLInputElement;
    expect(input.type).toBe('password');
  }
  expect((getByLabelText(host, t.newPassword) as HTMLInputElement).autocomplete).toBe('new-password');
  expect(host.querySelectorAll('a')).toHaveLength(1); // the skip link alone
  expect(queryByRole(host, 'navigation')).toBeNull();
  expect(queryByRole(host, 'button', { name: copy[locale].signOut })).toBeNull();
});

test('the forced change checks the repeat locally, shows the server rule on a refusal and unbinds the session on success', async () => {
  const t = accountCopy.en;
  const fetch = vi.fn()
    .mockResolvedValueOnce(issued('t1', true))
    .mockResolvedValueOnce(refusal(422, 'password_policy', { rule: 'too_short' }))
    .mockResolvedValueOnce(issued('t2'));
  vi.stubGlobal('fetch', fetch);
  const auth = new LocalAuth(config);
  await auth.signIn('admin', 'first password here');
  await act(() => root.render(<ForcedChangePage auth={auth} locale="en" />));
  type(t.currentPassword, 'first password here'); type(t.newPassword, 'one password here'); type(t.repeatPassword, 'two');
  await submit();
  expect(getByRole(host, 'alert').textContent).toBe(t.mismatch);
  expect(fetch).toHaveBeenCalledTimes(1);
  type(t.newPassword, 'short'); type(t.repeatPassword, 'short');
  await submit();
  expect(getByRole(host, 'alert').textContent).toBe(t.rule_too_short);
  expect(auth.stage()).toBe('forced');
  type(t.newPassword, 'correct horse battery staple'); type(t.repeatPassword, 'correct horse battery staple');
  await submit();
  expect(auth.stage()).toBe('in');
  expect(JSON.parse(fetch.mock.calls[2]![1].body)).toEqual({ current: 'first password here', new: 'correct horse battery staple' });
});

test('the profile shows the subject and role, changes the password voluntarily and signs out', async () => {
  const t = accountCopy.en;
  const fetch = vi.fn().mockResolvedValueOnce(issued('t1')).mockResolvedValueOnce(issued('t2'));
  vi.stubGlobal('fetch', fetch);
  const auth = new LocalAuth(config);
  await auth.signIn('admin', 'x');
  await act(() => root.render(<ProfilePage api={new Api(auth)} auth={auth} locale="en" session={{ subject: 'local:admin', role: 'operator', account: true }} onSession={() => {}} />));
  expect(getByRole(host, 'heading', { level: 1 }).textContent).toBe(t.profileTitle);
  expect(host.textContent).toContain('local:admin');
  expect(host.textContent).toContain(copy.en.operator);
  type(t.currentPassword, 'x'); type(t.newPassword, 'correct horse battery staple'); type(t.repeatPassword, 'correct horse battery staple');
  await submit();
  await waitFor(() => expect(getByRole(host, 'status').textContent).toBe(t.changed));
  expect(auth.token).toBe('t2');
  await act(() => { fireEvent.click(getByRole(host, 'button', { name: copy.en.signOut })); });
  expect(auth.stage()).toBe('out');
  expect(sessionStorage.length).toBe(0);
});

test('under break glass the profile offers no agent token, which does not exist there', async () => {
  const auth = new LocalAuth({ ...config, break_glass: true });
  const session = { subject: 'local:admin', role: 'operator', account: true, github_host: 'github.com', github_grant: 'current' } as const;
  await act(() => root.render(<ProfilePage api={new Api(auth)} auth={auth} locale="en" session={session} onSession={() => {}} />));
  expect(getByRole(host, 'heading', { level: 2, name: accountCopy.en.changeTitle })).toBeDefined();
  expect(queryByRole(host, 'heading', { name: accountCopy.en.tokensTitle })).toBeNull();
  expect(queryByRole(host, 'heading', { name: accountCopy.en.githubTitle })).toBeNull();
});

const viewer: Session = { subject: 'local:ana', role: 'viewer', account: true, groups: [], grant: [], github_host: 'github.com', github_grant: 'current' };
const verifiedAnswer = (token: string, login: string | null) =>
  Response.json({ login, token, expires_at: new Date(Date.now() + 3600_000).toISOString(), change_required: false });
const tokenForm = (locale: 'en' | 'pt-BR' = 'en') => getByLabelText(host, accountCopy[locale].githubToken).closest('form')!;

test('without the GitHub grant provider the profile offers no login verification', async () => {
  const auth = new LocalAuth(config);
  await act(() => root.render(<ProfilePage api={new Api(auth)} auth={auth} locale="en" session={{ subject: 'local:ana', role: 'viewer', account: true }} onSession={() => {}} />));
  expect(queryByRole(host, 'heading', { name: accountCopy.en.githubTitle })).toBeNull();
});

test.each(['en', 'pt-BR'] as const)('the profile verifies a GitHub login with a pasted token sent once, then shows the login, in %s', async locale => {
  const t = accountCopy[locale];
  const verified: Session = { ...viewer, login: 'ana-souza', grant: ['github.com/acme/web'] };
  const fetch = vi.fn().mockResolvedValueOnce(issued('t1')).mockResolvedValueOnce(verifiedAnswer('t2', 'ana-souza'))
    .mockResolvedValueOnce(Response.json(verified));
  vi.stubGlobal('fetch', fetch);
  const auth = new LocalAuth(config);
  await auth.signIn('ana', 'x');
  const onSession = vi.fn();
  await act(() => root.render(<ProfilePage api={new Api(auth)} auth={auth} locale={locale} session={viewer} onSession={onSession} />));
  expect(getByRole(host, 'heading', { level: 2, name: t.githubTitle })).toBeDefined();
  expect(host.textContent).toContain(t.githubNone);
  const input = getByLabelText(host, t.githubToken) as HTMLInputElement;
  expect(input.type).toBe('password');
  expect(input.autocomplete).toBe('off');
  expect(queryByRole(host, 'button', { name: t.githubRemove })).toBeNull();
  fireEvent.change(input, { target: { value: 'ghp_secret0123' } });
  await act(async () => { fireEvent.submit(tokenForm(locale)); });
  await waitFor(() => expect(onSession).toHaveBeenCalledWith(verified));
  const [url, init] = fetch.mock.calls[1]!;
  expect(url).toBe('/api/v1/account/github');
  expect(init.method).toBe('POST');
  expect(JSON.parse(init.body)).toEqual({ token: 'ghp_secret0123' });
  expect(new Headers(init.headers).get('Authorization')).toBe('Bearer t1');
  expect(fetch.mock.calls[2]![0]).toBe('/api/v1/session');
  // The fresh session token carrying the login replaces the old one; the pasted token is kept nowhere.
  expect(auth.token).toBe('t2');
  expect(input.value).toBe('');
  expect(host.textContent).toContain(t.githubVerified);
  expect(host.textContent).not.toContain('ghp_secret0123');
  expect(JSON.stringify(sessionStorage)).not.toContain('ghp_secret0123');
  expect(JSON.stringify(localStorage)).not.toContain('ghp_secret0123');
});

test.each([
  [409, 'login_taken', 'githubTaken'], [422, 'github_refused', 'githubRefused'], [422, 'invalid_token', 'githubInvalidToken'],
  [422, 'invalid_login', 'githubInvalidLogin'], [503, 'github_unavailable', 'githubUnavailable'], [403, 'forbidden', 'githubForbidden'],
  [500, 'other', 'githubFailed'],
] as const)('a refused verification (%s %s) is the page\'s own message, never the server\'s', async (status, code, key) => {
  const fetch = vi.fn().mockResolvedValueOnce(issued('t1')).mockResolvedValueOnce(refusal(status, code));
  vi.stubGlobal('fetch', fetch);
  const auth = new LocalAuth(config);
  await auth.signIn('ana', 'x');
  const onSession = vi.fn();
  await act(() => root.render(<ProfilePage api={new Api(auth)} auth={auth} locale="en" session={viewer} onSession={onSession} />));
  type(accountCopy.en.githubToken, 'ghp_secret0123');
  await act(async () => { fireEvent.submit(tokenForm()); });
  await waitFor(() => expect(getByRole(host, 'alert').textContent).toBe(accountCopy.en[key]));
  expect(onSession).not.toHaveBeenCalled();
  expect(auth.token).toBe('t1');
  expect(host.textContent).not.toContain(`server ${code}`);
});

test('a 401 on the verification ends the session', async () => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(issued('t1')).mockResolvedValueOnce(refusal(401, 'unauthorized')));
  const auth = new LocalAuth(config);
  await auth.signIn('ana', 'x');
  await act(() => root.render(<ProfilePage api={new Api(auth)} auth={auth} locale="en" session={viewer} onSession={() => {}} />));
  type(accountCopy.en.githubToken, 'ghp_secret0123');
  await act(async () => { fireEvent.submit(tokenForm()); });
  await waitFor(() => expect(auth.stage()).toBe('out'));
  expect(auth.expired).toBe(true);
});

test('the profile removes its own verified login and reads the session again', async () => {
  const t = accountCopy.en;
  const fetch = vi.fn().mockResolvedValueOnce(issued('t1')).mockResolvedValueOnce(verifiedAnswer('t3', null))
    .mockResolvedValueOnce(Response.json(viewer));
  vi.stubGlobal('fetch', fetch);
  const auth = new LocalAuth(config);
  await auth.signIn('ana', 'x');
  const onSession = vi.fn();
  await act(() => root.render(<ProfilePage api={new Api(auth)} auth={auth} locale="en" session={{ ...viewer, login: 'ana-souza' }} onSession={onSession} />));
  expect(host.textContent).toContain(`${t.githubVerifiedAs} ana-souza`);
  await act(async () => { fireEvent.click(getByRole(host, 'button', { name: t.githubRemove })); });
  await waitFor(() => expect(onSession).toHaveBeenCalledWith(viewer));
  expect(fetch.mock.calls[1]![0]).toBe('/api/v1/account/github/remove');
  expect(auth.token).toBe('t3');
  expect(host.textContent).toContain(t.githubRemoved);
});
