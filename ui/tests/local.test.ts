import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { Api, ApiError } from '../src/api';
import type { Config } from '../src/api';
import { LocalAuth, SESSION_KEY } from '../src/local';

const config: Config = { version: '0.1.0', operations: true, public_url: location.origin, sign_in: 'accounts' };
const future = () => new Date(Date.now() + 8 * 3600_000).toISOString();
const token = (name: string, change = false) => ({ token: name, expires_at: future(), change_required: change });
const refusal = (status: number, code: string) => Response.json({ error: { code, message: `fixed ${code}` } }, { status });

beforeEach(() => { sessionStorage.clear(); localStorage.clear(); });
afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); });

test('a sign-in posts the credentials to login, keeps the token in sessionStorage only and sends it as the bearer', async () => {
  const fetch = vi.fn()
    .mockResolvedValueOnce(Response.json(token('session-secret')))
    .mockResolvedValueOnce(Response.json({ subject: 'local:admin', role: 'operator' }));
  vi.stubGlobal('fetch', fetch);
  const auth = new LocalAuth(config);
  expect(auth.stage()).toBe('out');
  expect(await auth.signIn('admin', 'a password')).toBe('in');
  const [url, init] = fetch.mock.calls[0]!;
  expect(url).toBe('/api/v1/login');
  expect(init).toMatchObject({ method: 'POST', credentials: 'omit', redirect: 'error', cache: 'no-store' });
  expect(JSON.parse(init.body)).toEqual({ username: 'admin', password: 'a password' });
  expect(new Headers(init.headers).has('Authorization')).toBe(false);
  expect(auth.token).toBe('session-secret');
  expect(JSON.parse(sessionStorage.getItem(SESSION_KEY)!).token).toBe('session-secret');
  expect(localStorage.length).toBe(0);
  expect(document.cookie).toBe('');
  expect(location.href).not.toContain('session-secret');
  await new Api(auth).session();
  expect(fetch.mock.calls[1]![0]).toBe('/api/v1/session');
  expect(new Headers(fetch.mock.calls[1]![1].headers).get('Authorization')).toBe('Bearer session-secret');
});

test('a change-required sign-in is bound to the password change, also after a reload of the tab', async () => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(Response.json(token('t1', true))));
  const auth = new LocalAuth(config);
  expect(await auth.signIn('admin', 'first password here')).toBe('forced');
  expect(new LocalAuth(config).stage()).toBe('forced');
});

test('an expired, malformed or foreign stored session is dropped at load', () => {
  sessionStorage.setItem(SESSION_KEY, JSON.stringify({ token: 'old', expires_at: new Date(Date.now() - 1000).toISOString(), change_required: false }));
  expect(new LocalAuth(config).stage()).toBe('out');
  expect(sessionStorage.getItem(SESSION_KEY)).toBeNull();
  sessionStorage.setItem(SESSION_KEY, '{not json');
  expect(new LocalAuth(config).token).toBeUndefined();
  expect(sessionStorage.getItem(SESSION_KEY)).toBeNull();
});

test('a refused sign-in keeps no token and the server answer is the error', async () => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(refusal(401, 'invalid_credentials')));
  const auth = new LocalAuth(config);
  await expect(auth.signIn('admin', 'wrong')).rejects.toMatchObject({ status: 401, code: 'invalid_credentials' });
  expect(auth.stage()).toBe('out');
  expect(sessionStorage.length).toBe(0);
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(Response.json({ token: 1 })));
  await expect(auth.signIn('admin', 'x')).rejects.toThrow('login');
  expect(auth.token).toBeUndefined();
});

test('sign-out clears the token and notifies subscribers', async () => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(Response.json(token('t'))));
  const auth = new LocalAuth(config);
  const listener = vi.fn();
  auth.subscribe(listener);
  await auth.signIn('admin', 'x');
  listener.mockClear();
  auth.clear();
  expect(auth.token).toBeUndefined();
  expect(sessionStorage.length).toBe(0);
  expect(auth.expired).toBe(false);
  expect(listener).toHaveBeenCalledOnce();
});

test('a password change sends the bearer and keeps the fresh token without the mark', async () => {
  const fetch = vi.fn()
    .mockResolvedValueOnce(Response.json(token('t1', true)))
    .mockResolvedValueOnce(Response.json(token('t2')));
  vi.stubGlobal('fetch', fetch);
  const auth = new LocalAuth(config);
  await auth.signIn('admin', 'first password here');
  await auth.changePassword('first password here', 'correct horse battery staple');
  const [url, init] = fetch.mock.calls[1]!;
  expect(url).toBe('/api/v1/account/password');
  expect(new Headers(init.headers).get('Authorization')).toBe('Bearer t1');
  expect(JSON.parse(init.body)).toEqual({ current: 'first password here', new: 'correct horse battery staple' });
  expect(auth.stage()).toBe('in');
  expect(auth.token).toBe('t2');
  expect(JSON.parse(sessionStorage.getItem(SESSION_KEY)!)).toMatchObject({ token: 't2', change_required: false });
});

test('a refused current password keeps the session; a 401 of the session itself ends it', async () => {
  const fetch = vi.fn().mockResolvedValueOnce(Response.json(token('t1', true)))
    .mockResolvedValueOnce(refusal(401, 'invalid_credentials'))
    .mockResolvedValueOnce(refusal(422, 'password_policy'))
    .mockResolvedValueOnce(refusal(401, 'unauthorized'));
  vi.stubGlobal('fetch', fetch);
  const auth = new LocalAuth(config);
  await auth.signIn('admin', 'first password here');
  await expect(auth.changePassword('typo', 'long enough password')).rejects.toBeInstanceOf(ApiError);
  expect(auth.stage()).toBe('forced');
  await expect(auth.changePassword('first password here', 'short')).rejects.toMatchObject({ code: 'password_policy' });
  expect(auth.stage()).toBe('forced');
  await expect(auth.changePassword('first password here', 'long enough password')).rejects.toMatchObject({ status: 401 });
  expect(auth.stage()).toBe('out');
  expect(auth.expired).toBe(true);
  expect(sessionStorage.length).toBe(0);
});

test('any 401 of the API clears the token and reports an ended session', async () => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(Response.json(token('t'))));
  const auth = new LocalAuth(config);
  await auth.signIn('admin', 'x');
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(new Response('', { status: 401 })));
  await expect(new Api(auth).session()).rejects.toMatchObject({ status: 401, code: 'session_expired' });
  expect(auth.token).toBeUndefined();
  expect(auth.stage()).toBe('out');
  expect(auth.expired).toBe(true);
  expect(sessionStorage.length).toBe(0);
});

test('a 403 password_change_required binds the tab to the forced change, kept across a reload', async () => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(Response.json(token('t'))));
  const auth = new LocalAuth(config);
  await auth.signIn('admin', 'x');
  expect(auth.stage()).toBe('in');
  vi.stubGlobal('fetch', vi.fn()
    .mockResolvedValueOnce(refusal(403, 'password_change_required'))
    .mockResolvedValueOnce(refusal(403, 'forbidden')));
  await expect(new Api(auth).session()).rejects.toMatchObject({ status: 403, code: 'password_change_required' });
  expect(auth.stage()).toBe('forced');
  expect(auth.token).toBe('t');
  expect(new LocalAuth(config).stage()).toBe('forced');
  // Another 403 changes nothing.
  const other = new LocalAuth(config);
  await expect(new Api(other).session()).rejects.toMatchObject({ status: 403, code: 'forbidden' });
}  );

test('the token is never logged', async () => {
  const spies = (['log', 'info', 'warn', 'error', 'debug'] as const).map(name => vi.spyOn(console, name).mockImplementation(() => {}));
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(Response.json(token('secret-token'))).mockResolvedValueOnce(refusal(401, 'unauthorized')));
  const auth = new LocalAuth(config);
  await auth.signIn('admin', 'x');
  await new Api(auth).session().catch(() => {});
  for (const spy of spies) expect(spy).not.toHaveBeenCalled();
});
