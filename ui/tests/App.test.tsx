import { readFileSync } from 'node:fs';
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { fireEvent, getByRole, waitFor } from '@testing-library/dom';
import { afterEach, beforeAll, expect, test, vi } from 'vitest';
import { initSync } from '../wasm/meada_core';
import { App } from '../src/App';
import { Api } from '../src/api';
import { Auth } from '../src/auth';
import config from './fixtures/config.json';
import session from './fixtures/session.json';
import { copy } from '../src/i18n';

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const host = document.createElement('div');
document.body.append(host);
const root = createRoot(host);
beforeAll(() => { initSync({ module: readFileSync('wasm/meada_core_bg.wasm') }); });
afterEach(async () => { await act(() => root.render(null)); });

test('page navigation stays in the console and configuration is reachable after a failed manifest read', async () => {
  history.replaceState(null, '', '/config');
  const fetch = vi.fn().mockResolvedValueOnce(Response.json(session)).mockResolvedValueOnce(
    Response.json({ error: { code: 'unavailable', message: 'snapshot store unavailable' } }, { status: 503 }),
  );
  vi.stubGlobal('fetch', fetch);
  await act(() => root.render(<App api={new Api(new Auth(config))} config={config} initialLanguage="en" />));
  await waitFor(() => expect(host.textContent).toContain(session.subject));
  await act(() => { fireEvent.click(getByRole(host, 'link', { name: copy.en.manifest })); });
  await waitFor(() => expect(getByRole(host, 'alert').textContent).toBe('snapshot store unavailable'));
  expect(location.pathname).toBe('/manifest');
  await act(() => { fireEvent.click(getByRole(host, 'link', { name: copy.en.config })); });
  expect(getByRole(host, 'heading', { level: 1 }).textContent).toBe(copy.en.config);
  expect(location.pathname).toBe('/config');
  expect(fetch).toHaveBeenCalledTimes(2);
});

test('the shell opens playground and graph without another session or export request', async () => {
  history.replaceState(null, '', '/config');
  const fetch = vi.fn().mockResolvedValue(Response.json(session));
  vi.stubGlobal('fetch', fetch);
  await act(() => root.render(<App api={new Api(new Auth(config))} config={config} initialLanguage="en" />));
  await waitFor(() => expect(host.textContent).toContain(session.subject));
  await act(() => { fireEvent.click(getByRole(host, 'link', { name: 'Query playground' })); });
  expect(getByRole(host, 'heading', { level: 1 }).textContent).toBe('Query playground');
  await act(() => { fireEvent.click(getByRole(host, 'link', { name: 'Code graph' })); });
  expect(getByRole(host, 'heading', { level: 1 }).textContent).toBe('Code graph');
  expect(fetch).toHaveBeenCalledTimes(1);
});

test('the shell opens workspace versions and returns to config when its modules are absent', async () => {
  history.replaceState(null, '', '/config');
  const fetch = vi.fn(async (url: string) => Response.json(url === '/api/v1/session' ? session : { requires: ['snapshot', 'generations'] }));
  vi.stubGlobal('fetch', fetch);
  await act(() => root.render(<App api={new Api(new Auth(config))} config={config} initialLanguage="en" />));
  await waitFor(() => expect(host.textContent).toContain(session.subject));
  await act(() => fireEvent.click(getByRole(host, 'link', { name: 'Workspace' })));
  expect(location.pathname).toBe('/workspace');
  await waitFor(() => expect(host.textContent).toContain('[snapshot]'));
  expect(getByRole(host, 'heading', { level: 1 }).textContent).toBe('Workspace versions');
  await act(() => fireEvent.click(getByRole(host, 'link', { name: copy.en.config })));
  expect(getByRole(host, 'heading', { level: 1 }).textContent).toBe(copy.en.config);
  expect(fetch).toHaveBeenCalledTimes(3);
});
