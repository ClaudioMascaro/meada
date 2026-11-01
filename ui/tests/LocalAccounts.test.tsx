import { readFileSync } from 'node:fs';
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { fireEvent, getByLabelText, getByRole, queryByRole, waitFor } from '@testing-library/dom';
import { afterEach, beforeAll, beforeEach, expect, test, vi } from 'vitest';
import { initSync } from '../wasm/meada_core';
import { App } from '../src/App';
import { Api } from '../src/api';
import type { Config } from '../src/api';
import { Auth } from '../src/auth';
import { LocalAuth, SESSION_KEY } from '../src/local';
import { accountCopy } from '../src/account-i18n';
import { copy } from '../src/i18n';
import oidc from './fixtures/config.json';

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const host = document.createElement('div');
document.body.append(host);
const root = createRoot(host);
const config: Config = { version: '0.1.0', operations: true, public_url: location.origin, sign_in: 'accounts' };
const t = accountCopy.en;
const issued = (token: string, change = false) => Response.json({ token, expires_at: new Date(Date.now() + 3600_000).toISOString(), change_required: change });
const operator = { subject: 'local:admin', role: 'operator', groups: [], grant: '*', account: true };

beforeAll(() => { initSync({ module: readFileSync('wasm/meada_core_bg.wasm') }); });
beforeEach(() => { sessionStorage.clear(); localStorage.clear(); history.replaceState(null, '', '/config'); });
afterEach(async () => { await act(() => root.render(null)); vi.unstubAllGlobals(); });

function type(label: string, value: string) { fireEvent.change(getByLabelText(host, label), { target: { value } }); }
async function submit() { await act(async () => { fireEvent.submit(host.querySelector('form')!); }); }
const mount = (auth: LocalAuth, language: 'en' | 'pt-BR' = 'en') =>
  act(() => root.render(<App api={new Api(auth)} config={config} initialLanguage={language} />));

test('the first sign-in of the default account goes through the forced change to the workspace', async () => {
  const fetch = vi.fn(async (url: string, init?: RequestInit) => {
    if (url === '/api/v1/login') return issued('t1', true);
    if (url === '/api/v1/account/password') {
      expect(new Headers(init?.headers).get('Authorization')).toBe('Bearer t1');
      return issued('t2');
    }
    expect(new Headers(init?.headers).get('Authorization')).toBe('Bearer t2');
    return Response.json(url === '/api/v1/session' ? operator : { requires: ['snapshot', 'generations'] });
  });
  vi.stubGlobal('fetch', fetch);
  await mount(new LocalAuth(config));
  // Signed out: the sign-in page, and no session was asked for.
  expect(getByRole(host, 'heading', { level: 1 }).textContent).toBe(t.signInTitle);
  expect(queryByRole(host, 'navigation')).toBeNull();
  expect(fetch).not.toHaveBeenCalled();
  type(t.username, 'admin'); type(t.password, 'admin123');
  await submit();
  // Change required: the forced page, with nothing to navigate to and no other request.
  await waitFor(() => expect(getByRole(host, 'heading', { level: 1 }).textContent).toBe(t.forcedTitle));
  expect(queryByRole(host, 'navigation')).toBeNull();
  expect(queryByRole(host, 'link', { name: copy.en.workspace })).toBeNull();
  await act(async () => { history.pushState(null, '', '/workspace'); dispatchEvent(new PopStateEvent('popstate')); });
  expect(getByRole(host, 'heading', { level: 1 }).textContent).toBe(t.forcedTitle);
  expect(fetch.mock.calls.map(call => call[0])).toEqual(['/api/v1/login']);
  type(t.currentPassword, 'admin123'); type(t.newPassword, 'correct horse battery staple'); type(t.repeatPassword, 'correct horse battery staple');
  await submit();
  await waitFor(() => expect(location.pathname).toBe('/workspace'));
  await waitFor(() => expect(host.textContent).toContain('local:admin'));
  expect(getByRole(host, 'link', { name: copy.en.profile })).toBeDefined();
  expect(host.textContent).toContain(copy.en.operator);
  expect(localStorage.length).toBe(0);
});

test('a 401 returns to the sign-in page with the token gone', async () => {
  sessionStorage.setItem(SESSION_KEY, JSON.stringify({ token: 'old', expires_at: new Date(Date.now() + 3600_000).toISOString(), change_required: false }));
  const fetch = vi.fn()
    .mockResolvedValueOnce(Response.json(operator))
    .mockResolvedValueOnce(Response.json({ error: { code: 'unauthorized', message: 'no valid token' } }, { status: 401 }));
  vi.stubGlobal('fetch', fetch);
  await mount(new LocalAuth(config));
  await waitFor(() => expect(host.textContent).toContain('local:admin'));
  await act(() => { fireEvent.click(getByRole(host, 'link', { name: copy.en.manifest })); });
  await waitFor(() => expect(getByRole(host, 'heading', { level: 1 }).textContent).toBe(t.signInTitle));
  expect(getByRole(host, 'status').textContent).toBe(t.sessionEnded);
  expect(sessionStorage.getItem(SESSION_KEY)).toBeNull();
  expect(queryByRole(host, 'navigation')).toBeNull();
  expect(location.pathname).toBe('/config');
});

test('the profile page is reachable from the shell, in Portuguese too, and signing out returns to sign-in', async () => {
  sessionStorage.setItem(SESSION_KEY, JSON.stringify({ token: 'live', expires_at: new Date(Date.now() + 3600_000).toISOString(), change_required: false }));
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(Response.json(operator)));
  await mount(new LocalAuth(config), 'pt-BR');
  await waitFor(() => expect(host.textContent).toContain('local:admin'));
  await act(() => { fireEvent.click(getByRole(host, 'link', { name: copy['pt-BR'].profile })); });
  expect(location.pathname).toBe('/profile');
  expect(getByRole(host, 'heading', { level: 1 }).textContent).toBe(accountCopy['pt-BR'].profileTitle);
  await act(() => { fireEvent.click(getByRole(host, 'button', { name: copy['pt-BR'].signOut })); });
  expect(getByRole(host, 'heading', { level: 1 }).textContent).toBe(accountCopy['pt-BR'].signInTitle);
  expect(sessionStorage.getItem(SESSION_KEY)).toBeNull();
  expect(queryByRole(host, 'status')).toBeNull();
});

test('the identity provider flow is unchanged: no sign-in page, no profile and the bearer stays in memory', async () => {
  const fetch = vi.fn().mockResolvedValue(Response.json({ subject: 'alice', role: 'viewer' }));
  vi.stubGlobal('fetch', fetch);
  await act(() => root.render(<App api={new Api(new Auth(oidc))} config={oidc} initialLanguage="en" />));
  await waitFor(() => expect(host.textContent).toContain('alice'));
  expect(queryByRole(host, 'link', { name: copy.en.profile })).toBeNull();
  expect(queryByRole(host, 'textbox', { name: t.username })).toBeNull();
  expect(getByRole(host, 'navigation', { name: copy.en.navigation })).toBeDefined();
  history.replaceState(null, '', '/profile');
  await act(async () => { dispatchEvent(new PopStateEvent('popstate')); });
  expect(getByRole(host, 'heading', { level: 1 }).textContent).toBe(copy.en.notFound);
  expect(sessionStorage.getItem(SESSION_KEY)).toBeNull();
});

const live = () => sessionStorage.setItem(SESSION_KEY, JSON.stringify({ token: 'live', expires_at: new Date(Date.now() + 3600_000).toISOString(), change_required: false }));
const agent = { token: 'eyJhbGciOiJFZERTQSJ9.agent.signature', expires_at: '2026-11-02T10:00:00Z', surface: 'nodes' };

test('the profile generates an agent token for the nodes, shown once with its expiry and a copy control, gone on navigation', async () => {
  live();
  const writeText = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText } });
  const fetch = vi.fn(async (url: string, init?: RequestInit) => {
    if (url === '/api/v1/account/tokens') {
      expect(init?.method).toBe('POST');
      expect(new Headers(init?.headers).get('Authorization')).toBe('Bearer live');
      expect(JSON.parse(String(init?.body))).toEqual({ surface: 'nodes', hours: 24 });
      return Response.json(agent);
    }
    return Response.json(url === '/api/v1/session' ? operator : { file: 'ops/meada.workspace.toml', manifest: {}, modules: [], errors: [] });
  });
  vi.stubGlobal('fetch', fetch);
  await mount(new LocalAuth(config));
  await waitFor(() => expect(host.textContent).toContain('local:admin'));
  await act(() => { fireEvent.click(getByRole(host, 'link', { name: copy.en.profile })); });
  expect(getByRole(host, 'heading', { level: 2, name: t.tokensTitle })).toBeDefined();
  expect((getByLabelText(host, t.surface) as HTMLSelectElement).value).toBe('nodes');
  // Without a proposal server, the nodes are the one surface offered.
  expect([...(getByLabelText(host, t.surface) as HTMLSelectElement).options].map(o => o.value)).toEqual(['nodes']);
  expect((getByLabelText(host, t.validity) as HTMLInputElement).value).toBe('8');
  type(t.validity, '24');
  await act(async () => { fireEvent.click(getByRole(host, 'button', { name: t.issue })); });
  const shown = await waitFor(() => getByRole(host, 'region', { name: t.issuedToken }));
  expect(shown.querySelector('output')?.textContent).toBe(agent.token);
  expect(shown.querySelector('time')?.getAttribute('dateTime')).toBe(agent.expires_at);
  await act(async () => { fireEvent.click(getByRole(host, 'button', { name: t.copyToken })); });
  expect(writeText).toHaveBeenCalledWith(agent.token);
  expect(host.textContent).toContain(t.copied);
  // Never stored: neither the tab's storage nor the browser's holds it.
  expect(JSON.stringify(sessionStorage)).not.toContain(agent.token);
  expect(sessionStorage.getItem(SESSION_KEY)).not.toContain('agent');
  expect(localStorage.length).toBe(0);
  // Leaving the page clears it; coming back shows the form alone.
  await act(() => { fireEvent.click(getByRole(host, 'link', { name: copy.en.manifest })); });
  expect(host.textContent).not.toContain(agent.token);
  await act(() => { fireEvent.click(getByRole(host, 'link', { name: copy.en.profile })); });
  expect(host.textContent).not.toContain(agent.token);
  expect(queryByRole(host, 'region', { name: t.issuedToken })).toBeNull();
  expect(fetch.mock.calls.filter(call => call[0] === '/api/v1/account/tokens')).toHaveLength(1);
});

test('beside a proposal server the profile also issues agent tokens for it', async () => {
  live();
  const fetch = vi.fn(async (url: string, init?: RequestInit) => {
    if (url === '/api/v1/account/tokens') {
      expect(JSON.parse(String(init?.body))).toEqual({ surface: 'proposals', hours: 8 });
      return Response.json({ ...agent, surface: 'proposals' });
    }
    return Response.json(url === '/api/v1/session' ? operator : { file: 'ops/meada.workspace.toml', manifest: {}, modules: [], errors: [] });
  });
  vi.stubGlobal('fetch', fetch);
  await mount(new LocalAuth({ ...config, proposals: true }));
  await waitFor(() => expect(host.textContent).toContain('local:admin'));
  await act(() => { fireEvent.click(getByRole(host, 'link', { name: copy.en.profile })); });
  const surface = getByLabelText(host, t.surface) as HTMLSelectElement;
  expect([...surface.options].map(o => [o.value, o.textContent])).toEqual([['nodes', t.surfaceNodes], ['proposals', t.surfaceProposals]]);
  fireEvent.change(surface, { target: { value: 'proposals' } });
  await act(async () => { fireEvent.click(getByRole(host, 'button', { name: t.issue })); });
  const shown = await waitFor(() => getByRole(host, 'region', { name: t.issuedToken }));
  expect(shown.querySelector('output')?.textContent).toBe(agent.token);
});

test('a refused agent token says why, in Portuguese too, and shows no token', async () => {
  live();
  const answers = [
    Response.json({ error: { code: 'invalid_hours', message: 'an agent token is valid from 1 to 168 hours' } }, { status: 422 }),
    Response.json({ error: { code: 'forbidden', message: 'suspended' } }, { status: 403 }),
    Response.json({ error: { code: 'unavailable', message: 'down' } }, { status: 503 }),
  ];
  vi.stubGlobal('fetch', vi.fn(async (url: string) => url === '/api/v1/account/tokens' ? answers.shift()! : Response.json(operator)));
  await mount(new LocalAuth(config), 'pt-BR');
  const pt = accountCopy['pt-BR'];
  await waitFor(() => expect(host.textContent).toContain('local:admin'));
  await act(() => { fireEvent.click(getByRole(host, 'link', { name: copy['pt-BR'].profile })); });
  for (const expected of [pt.tokenHours, pt.tokenRefused, pt.tokenFailed]) {
    await act(async () => { fireEvent.click(getByRole(host, 'button', { name: pt.issue })); });
    await waitFor(() => expect(getByRole(host, 'alert').textContent).toBe(expected));
    expect(queryByRole(host, 'region', { name: pt.issuedToken })).toBeNull();
  }
  // A refusal of the token keeps the session.
  expect(sessionStorage.getItem(SESSION_KEY)).toContain('live');
});

test('the accounts page is the global operator\'s: listed and open for an operator, the not-found page for a viewer and under the identity provider', async () => {
  const live = JSON.stringify({ token: 'live', expires_at: new Date(Date.now() + 3600_000).toISOString(), change_required: false });
  const answers = (session: object) => vi.fn(async (url: string) => Response.json(url === '/api/v1/session' ? session : { accounts: [] }));
  sessionStorage.setItem(SESSION_KEY, live);
  vi.stubGlobal('fetch', answers(operator));
  await mount(new LocalAuth(config));
  await waitFor(() => expect(host.textContent).toContain('local:admin'));
  await act(() => { fireEvent.click(getByRole(host, 'link', { name: copy.en.accounts })); });
  expect(location.pathname).toBe('/accounts');
  await waitFor(() => expect(getByRole(host, 'heading', { level: 1 }).textContent).toBe('Accounts'));
  await act(() => root.render(null));

  vi.stubGlobal('fetch', answers({ subject: 'local:ana', role: 'viewer', account: true }));
  await mount(new LocalAuth(config));
  await waitFor(() => expect(host.textContent).toContain('local:ana'));
  expect(queryByRole(host, 'link', { name: copy.en.accounts })).toBeNull();
  expect(getByRole(host, 'heading', { level: 1 }).textContent).toBe(copy.en.notFound);
  await act(() => root.render(null));

  vi.stubGlobal('fetch', answers({ subject: 'alice', role: 'operator' }));
  await act(() => root.render(<App api={new Api(new Auth(oidc))} config={oidc} initialLanguage="en" />));
  await waitFor(() => expect(host.textContent).toContain('alice'));
  expect(queryByRole(host, 'link', { name: copy.en.accounts })).toBeNull();
  expect(getByRole(host, 'heading', { level: 1 }).textContent).toBe(copy.en.notFound);
});
