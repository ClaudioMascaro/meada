import { webcrypto } from 'node:crypto';
import { beforeEach, expect, test, vi } from 'vitest';
import { Auth, challenge } from '../src/auth';
import type { Config } from '../src/api';

const config: Config = {
  version: '0.1.0', operations: false, public_url: 'https://console.example', issuer: 'https://identity.example',
  client_id: 'meada-console', scopes: ['meada:read'], audience: 'https://console.example',
};
const metadata = {
  issuer: config.issuer, authorization_endpoint: 'https://identity.example/authorize',
  token_endpoint: 'https://identity.example/token', code_challenge_methods_supported: ['S256'],
};

beforeEach(() => {
  sessionStorage.clear(); localStorage.clear();
  vi.stubGlobal('crypto', webcrypto);
});

test('PKCE matches the RFC 7636 S256 example', async () => {
  expect(await challenge('dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk'))
    .toBe('E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM');
});

test('login redirects to the configured issuer with its public client and PKCE', async () => {
  const redirected = vi.fn();
  const auth = new Auth(config, redirected);
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(Response.json(metadata)));
  await auth.login();
  const url = new URL(redirected.mock.calls[0]![0]);
  expect(url.origin + url.pathname).toBe('https://identity.example/authorize');
  expect(Object.fromEntries(url.searchParams)).toMatchObject({
    client_id: 'meada-console', redirect_uri: 'https://console.example/auth/callback',
    response_type: 'code', code_challenge_method: 'S256', scope: 'openid meada:read',
    audience: 'https://console.example',
  });
  expect(url.searchParams.get('state')).toBe(sessionStorage.getItem('meada.pkce.state'));
  expect(sessionStorage.length).toBe(2);
  expect(sessionStorage.getItem('meada.pkce.verifier')).toHaveLength(43);
  expect(url.searchParams.get('code_challenge')).toHaveLength(43);
});

test('a callback with another state is refused and removes the redirect secrets', async () => {
  sessionStorage.setItem('meada.pkce.state', 'expected');
  sessionStorage.setItem('meada.pkce.verifier', 'verifier');
  const auth = new Auth(config, vi.fn());
  const fetch = vi.fn(); vi.stubGlobal('fetch', fetch);
  await expect(auth.callback(new URLSearchParams('code=code&state=wrong'))).rejects.toThrow('login');
  expect(fetch).not.toHaveBeenCalled();
  expect(sessionStorage.length).toBe(0);
});

test('a successful login exchanges the code directly and keeps the bearer only in memory', async () => {
  const auth = new Auth(config, vi.fn());
  sessionStorage.setItem('meada.pkce.state', 'expected');
  sessionStorage.setItem('meada.pkce.verifier', 'verifier');
  const fetch = vi.fn()
    .mockResolvedValueOnce(Response.json(metadata))
    .mockResolvedValueOnce(Response.json({ access_token: 'bearer-secret', token_type: 'Bearer' }));
  vi.stubGlobal('fetch', fetch);
  await auth.callback(new URLSearchParams('code=one-time-code&state=expected'));
  expect(fetch.mock.calls[1]![0]).toBe('https://identity.example/token');
  const options = fetch.mock.calls[1]![1];
  expect(options.credentials).toBe('omit');
  expect(Object.fromEntries(options.body)).toEqual({
    grant_type: 'authorization_code', client_id: 'meada-console', code: 'one-time-code',
    redirect_uri: 'https://console.example/auth/callback', code_verifier: 'verifier',
  });
  expect(auth.token).toBe('bearer-secret');
  expect(document.cookie).toBe('');
  expect(localStorage.length).toBe(0);
  expect(sessionStorage.length).toBe(0);
  auth.clear();
  expect(auth.token).toBeUndefined();
});

test('discovery cannot move the code exchange to another origin or disable S256', async () => {
  const auth = new Auth(config, vi.fn());
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(Response.json({ ...metadata, token_endpoint: 'https://foreign.example/token' })));
  await expect(auth.login()).rejects.toThrow('config');
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(Response.json({ ...metadata, code_challenge_methods_supported: ['plain'] })));
  await expect(auth.login()).rejects.toThrow('config');
  expect(sessionStorage.length).toBe(0);
});

test('the callback exchanges no code when the provider returned an error', async () => {
  sessionStorage.setItem('meada.pkce.state', 'expected');
  sessionStorage.setItem('meada.pkce.verifier', 'verifier');
  const auth = new Auth(config, vi.fn());
  const fetch = vi.fn(); vi.stubGlobal('fetch', fetch);
  await expect(auth.callback(new URLSearchParams('error=access_denied&state=expected'))).rejects.toThrow('login');
  expect(sessionStorage.length).toBe(0);
  expect(fetch).not.toHaveBeenCalled();
});
test('login preserves issuer query parameters while replacing client OAuth parameters', async () => {
  const redirected = vi.fn();
  const auth = new Auth(config, redirected);
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(Response.json({
    ...metadata,
    authorization_endpoint: 'https://identity.example/authorize?tenant=agents&tenant=second&prompt=login&client_id=old&state=old&state=duplicate&response_type=token',
  })));
  await auth.login();
  const url = new URL(redirected.mock.calls[0]![0]);
  expect(url.searchParams.getAll('tenant')).toEqual(['agents', 'second']);
  expect(url.searchParams.get('prompt')).toBe('login');
  expect(url.searchParams.getAll('client_id')).toEqual(['meada-console']);
  expect(url.searchParams.getAll('state')).toEqual([sessionStorage.getItem('meada.pkce.state')]);
  expect(url.searchParams.getAll('response_type')).toEqual(['code']);
  expect(url.searchParams.get('code_challenge_method')).toBe('S256');
});
