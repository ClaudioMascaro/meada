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
