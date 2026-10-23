import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { fireEvent, getByRole, queryAllByRole, waitFor } from '@testing-library/dom';
import { afterEach, expect, test, vi } from 'vitest';
import { OperationalPage } from '../src/OperationalPage';
import { Api } from '../src/api';
import { Auth } from '../src/auth';
import { operationsCopy } from '../src/operations-i18n';
import fleet from './fixtures/fleet.json';
import fixtures from './fixtures/operational.json';
import config from './fixtures/config.json';
import session from './fixtures/session.json';

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const host = document.createElement('div');
document.body.append(host);
const root = createRoot(host);
afterEach(async () => { await act(() => root.render(null)); vi.unstubAllGlobals(); });

test.each(['en', 'pt-BR'] as const)('the five operational pages label every keyboard control in %s', async locale => {
  for (const kind of ['fleet', 'store', 'repositories', 'credentials', 'admission'] as const) {
    const data = kind === 'fleet' ? fleet : fixtures[kind];
    const fetch = vi.fn().mockResolvedValue(Response.json(data));
    vi.stubGlobal('fetch', fetch);
    await act(() => root.render(<OperationalPage kind={kind} api={new Api(new Auth(config))} locale={locale} config={config} session={{ ...session, role: 'viewer' }} />));
    await waitFor(() => expect(fetch).toHaveBeenCalledOnce());
    await waitFor(() => expect(getByRole(host, 'heading', { level: 1 }).textContent).toBe(operationsCopy[locale][kind]));
    await waitFor(() => expect(queryAllByRole(host, 'table').length).toBeGreaterThan(0));
    const controls = ['button', 'textbox', 'combobox', 'link'].flatMap(role => queryAllByRole(host, role, { name: /\S/ }));
    expect(controls.length).toBe(host.querySelectorAll('button, input, textarea, select, a[href]').length);
    expect(controls.length).toBeGreaterThan(0);
    for (const control of controls) expect(control.tabIndex).toBeGreaterThanOrEqual(0);
    await act(() => root.render(null));
  }
  expect(Object.keys(operationsCopy.en).sort()).toEqual(Object.keys(operationsCopy['pt-BR']).sort());
});

test.each([200, 400])('the named token-check form clears the pasted token after HTTP %i without storing it', async status => {
  localStorage.clear(); sessionStorage.clear();
  history.replaceState(null, '', '/admission');
  let finish: (response: Response) => void = () => { throw new Error('check not started'); };
  const pending = new Promise<Response>(resolve => { finish = resolve; });
  const fetch = vi.fn().mockResolvedValueOnce(Response.json(fixtures.admission)).mockReturnValueOnce(pending);
  vi.stubGlobal('fetch', fetch);
  const api = new Api(new Auth(config));
  await act(() => root.render(<OperationalPage kind="admission" api={api} locale="en" config={config} session={{ ...session, role: 'viewer' }} />));
  const form = await waitFor(() => getByRole(host, 'form', { name: operationsCopy.en.checkToken }));
  const input = getByRole(host, 'textbox', { name: operationsCopy.en.token }) as HTMLTextAreaElement;
  const pasted = 'pasted.jwt.secret-42';
  await act(() => { fireEvent.input(input, { target: { value: pasted } }); fireEvent.submit(form); });
  expect(fetch.mock.calls[1]?.[0]).toBe('/api/v1/admission/check');
  expect(JSON.parse(fetch.mock.calls[1]?.[1].body)).toEqual({ token: pasted });
  await act(() => finish(Response.json(status === 200 ? {
    checks: { signature: false, issuer: null, audience: null, lifetime: null, scopes: null }, identities: null,
  } : { error: { code: 'bad_request', message: 'Invalid token check body' } }, { status })));
  expect(input.value).toBe('');
  expect(host.textContent).not.toContain(pasted);
  await act(() => root.render(<OperationalPage kind="admission" api={api} locale="pt-BR" config={config} session={{ ...session, role: 'viewer' }} />));
  expect((getByRole(host, 'textbox', { name: operationsCopy['pt-BR'].token }) as HTMLTextAreaElement).value).toBe('');
  expect(localStorage.length).toBe(0);
  expect(sessionStorage.length).toBe(0);
  expect(location.href).not.toContain(pasted);
  expect(fetch).toHaveBeenCalledTimes(2);
});
