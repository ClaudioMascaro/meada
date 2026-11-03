import { readFileSync } from 'node:fs';
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { fireEvent, getByLabelText, getByRole, queryByRole, waitFor } from '@testing-library/dom';
import { afterEach, beforeAll, beforeEach, expect, test, vi } from 'vitest';
import { initSync } from '../wasm/meada_core';
import { App } from '../src/App';
import { SignInChoicePage } from '../src/AccountPages';
import { Api } from '../src/api';
import type { Config } from '../src/api';
import { BREAK_GLASS_PATH, LocalAuth, SESSION_KEY, breakGlassTab } from '../src/local';
import { accountCopy } from '../src/account-i18n';
import { copy } from '../src/i18n';
import oidc from './fixtures/config.json';

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const host = document.createElement('div');
document.body.append(host);
const root = createRoot(host);
// The identity provider's config, with the break-glass mark (#563).
const config: Config = { ...oidc, sign_in: 'oidc', break_glass: true };
const t = accountCopy.en;
const issued = (token: string, change = false) => Response.json({ token, expires_at: new Date(Date.now() + 3600_000).toISOString(), change_required: change });
const operator = { subject: 'local:admin', role: 'operator', groups: [], grant: '*', account: true };
const live = () => JSON.stringify({ token: 'live', expires_at: new Date(Date.now() + 3600_000).toISOString(), change_required: false });

beforeAll(() => { initSync({ module: readFileSync('wasm/meada_core_bg.wasm') }); });
beforeEach(() => { sessionStorage.clear(); localStorage.clear(); history.replaceState(null, '', '/config'); });
afterEach(async () => { await act(() => root.render(null)); vi.unstubAllGlobals(); });

function type(label: string, value: string) { fireEvent.change(getByLabelText(host, label), { target: { value } }); }
async function submit() { await act(async () => { fireEvent.submit(host.querySelector('form')!); }); }

test('a tab signs in on the console\'s own page only under break glass, on that page or holding its session', () => {
  expect(breakGlassTab(config, BREAK_GLASS_PATH)).toBe(true);
  expect(breakGlassTab(config, '/config')).toBe(false);
  expect(breakGlassTab({ ...config, break_glass: undefined }, BREAK_GLASS_PATH)).toBe(false);
  expect(breakGlassTab(oidc as Config, BREAK_GLASS_PATH)).toBe(false);
  sessionStorage.setItem(SESSION_KEY, live());
  expect(breakGlassTab(config, '/workspace')).toBe(true);
  expect(breakGlassTab(oidc as Config, '/workspace')).toBe(false);
  // An expired session is dropped, and the tab is the identity provider's again.
  sessionStorage.setItem(SESSION_KEY, JSON.stringify({ token: 'old', expires_at: new Date(Date.now() - 1000).toISOString(), change_required: false }));
  expect(breakGlassTab(config, '/workspace')).toBe(false);
  expect(sessionStorage.getItem(SESSION_KEY)).toBeNull();
});

test('the choice offers the identity provider first and a discreet break-glass link, which stays when the provider fails', async () => {
  const signIn = vi.fn().mockRejectedValueOnce(new Error('config')).mockResolvedValueOnce(undefined);
  await act(() => root.render(<SignInChoicePage locale="en" onIdentityProvider={signIn} />));
  expect(getByRole(host, 'heading', { level: 1 }).textContent).toBe(t.chooseTitle);
  const link = getByRole(host, 'link', { name: t.breakGlassLink });
  expect(link.getAttribute('href')).toBe(BREAK_GLASS_PATH);
  expect(queryByRole(host, 'textbox')).toBeNull();
  await act(async () => { fireEvent.click(getByRole(host, 'button', { name: t.identityProvider })); });
  expect(signIn).toHaveBeenCalledTimes(1);
  expect(getByRole(host, 'alert').textContent).toBe(t.identityProviderFailed);
  expect(getByRole(host, 'link', { name: t.breakGlassLink })).toBeDefined();
  await act(async () => { fireEvent.click(getByRole(host, 'button', { name: t.identityProvider })); });
  expect(signIn).toHaveBeenCalledTimes(2);
  // A sign-in that went on to the redirect leaves the button waiting.
  expect(getByRole(host, 'button', { name: t.redirecting }).hasAttribute('disabled')).toBe(true);
  await act(() => root.render(null));
  await act(() => root.render(<SignInChoicePage locale="pt-BR" onIdentityProvider={signIn} />));
  expect(getByRole(host, 'link', { name: accountCopy['pt-BR'].breakGlassLink })).toBeDefined();
  expect(getByRole(host, 'button', { name: accountCopy['pt-BR'].identityProvider })).toBeDefined();
});

test('the break-glass page signs the default account in through its forced change to the workspace and back out to the same page', async () => {
  history.replaceState(null, '', BREAK_GLASS_PATH);
  const fetch = vi.fn(async (url: string) => {
    if (url === '/api/v1/login') return issued('t1', true);
    if (url === '/api/v1/account/password') return issued('t2');
    return Response.json(url === '/api/v1/session' ? operator : { requires: ['snapshot', 'generations'] });
  });
  vi.stubGlobal('fetch', fetch);
  await act(() => root.render(<App api={new Api(new LocalAuth(config))} config={config} initialLanguage="en" />));
  expect(getByRole(host, 'heading', { level: 1 }).textContent).toBe(t.breakGlassTitle);
  expect(host.textContent).toContain(t.breakGlassIntro);
  expect(getByRole(host, 'link', { name: t.backToIdentityProvider }).getAttribute('href')).toBe('/config');
  expect(queryByRole(host, 'navigation')).toBeNull();
  expect(document.title).toBe(`${t.breakGlassTitle} · Meada`);
  expect(fetch).not.toHaveBeenCalled();
  type(t.username, 'admin'); type(t.password, 'first password here');
  await submit();
  await waitFor(() => expect(getByRole(host, 'heading', { level: 1 }).textContent).toBe(t.forcedTitle));
  type(t.currentPassword, 'first password here'); type(t.newPassword, 'correct horse battery staple'); type(t.repeatPassword, 'correct horse battery staple');
  await submit();
  await waitFor(() => expect(location.pathname).toBe('/workspace'));
  await waitFor(() => expect(host.textContent).toContain('local:admin'));
  expect(host.textContent).toContain(copy.en.operator);
  expect(getByRole(host, 'link', { name: copy.en.profile })).toBeDefined();
  await act(() => { fireEvent.click(getByRole(host, 'link', { name: copy.en.profile })); });
  await act(() => { fireEvent.click(getByRole(host, 'button', { name: copy.en.signOut })); });
  expect(getByRole(host, 'heading', { level: 1 }).textContent).toBe(t.breakGlassTitle);
  expect(location.pathname).toBe(BREAK_GLASS_PATH);
  expect(sessionStorage.getItem(SESSION_KEY)).toBeNull();
  expect(localStorage.length).toBe(0);
});

test('a break-glass session already held opens the console, and its sign-in page is never the local accounts one', async () => {
  sessionStorage.setItem(SESSION_KEY, live());
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(Response.json(operator)));
  await act(() => root.render(<App api={new Api(new LocalAuth(config))} config={config} initialLanguage="pt-BR" />));
  await waitFor(() => expect(host.textContent).toContain('local:admin'));
  expect(location.pathname).toBe('/config');
  await act(() => { fireEvent.click(getByRole(host, 'link', { name: copy['pt-BR'].profile })); });
  await act(() => { fireEvent.click(getByRole(host, 'button', { name: copy['pt-BR'].signOut })); });
  expect(getByRole(host, 'heading', { level: 1 }).textContent).toBe(accountCopy['pt-BR'].breakGlassTitle);
  expect(host.textContent).not.toContain(accountCopy['pt-BR'].signInIntro);
});
