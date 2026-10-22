import { expect, test, vi } from 'vitest';
import { Api, loadConfig } from '../src/api';
import { Auth } from '../src/auth';

test('anonymous configuration and viewer session need no token or cookies', async () => {
  const config = { version: '0.1.0', operations: false, public_url: location.origin };
  const fetch = vi.fn().mockResolvedValueOnce(Response.json(config))
    .mockResolvedValueOnce(Response.json({ subject: null, role: 'viewer' }));
  vi.stubGlobal('fetch', fetch);
  expect(await loadConfig()).toEqual(config);
  expect(await new Api(new Auth(config)).session()).toEqual({ subject: null, role: 'viewer' });
  for (const [, options] of fetch.mock.calls) expect(options.credentials).toBe('omit');
});

test('a 401 starts another login; API errors retain the server catalog message', async () => {
  const auth = new Auth({ version: '0.1.0', operations: true, public_url: location.origin, issuer: 'https://issuer.example' });
  const login = vi.spyOn(auth, 'login').mockResolvedValue();
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('', { status: 401 })));
  await expect(new Api(auth).session()).rejects.toMatchObject({ status: 401, code: 'session_expired' });
  expect(login).toHaveBeenCalledOnce();
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(Response.json({ error: { code: 'forbidden', message: 'operations are disabled' } }, { status: 403 })));
  await expect(new Api(auth).session()).rejects.toMatchObject({ status: 403, message: 'operations are disabled' });
});
