import { readFileSync } from 'node:fs';
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { fireEvent, getByRole, queryByRole, waitFor } from '@testing-library/dom';
import { afterEach, beforeAll, expect, test, vi } from 'vitest';
import { initSync } from '../wasm/meada_core';
import { Api } from '../src/api';
import { Auth } from '../src/auth';
import { GraphPage } from '../src/GraphPage';
import { graphCopy } from '../src/graph-i18n';
import config from './fixtures/config.json';

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const host = document.createElement('div'); document.body.append(host);
const root = createRoot(host);
beforeAll(() => { initSync({ module: readFileSync('wasm/meada_core_bg.wasm') }); });
afterEach(async () => { await act(() => root.render(null)); });

test.each(['en', 'pt-BR'] as const)('all five named keyboard-accessible tables derive from one export fetch in %s', async locale => {
  const t = graphCopy[locale];
  const fetch = vi.fn().mockResolvedValue(new Response(readFileSync('../tests/golden/chain-web.decided.export.json', 'utf8')));
  vi.stubGlobal('fetch', fetch);
  await act(() => root.render(<GraphPage api={new Api(new Auth(config))} locale={locale} />));
  await act(() => {
    fireEvent.change(getByRole(host, 'textbox', { name: t.repository }), { target: { value: 'example.com/chain/web' } });
    fireEvent.change(getByRole(host, 'textbox', { name: t.generation }), { target: { value: '0123456789abcdef' } });
    fireEvent.submit(getByRole(host, 'form', { name: t.graph }));
  });
  await waitFor(() => expect(getByRole(host, 'table', { name: t.communities })).toBeDefined());
  for (const view of ['communities', 'contracts', 'links', 'drift', 'decisions'] as const) {
    await act(() => { fireEvent.change(getByRole(host, 'combobox', { name: t.view }), { target: { value: view } }); });
    const region = getByRole(host, 'region', { name: t[view] });
    expect(region.tabIndex).toBe(0);
    expect(getByRole(region, 'table', { name: t[view] })).toBeDefined();
  }
  expect(fetch).toHaveBeenCalledTimes(1);
  expect(fetch.mock.calls[0]![0]).toBe('/api/v1/repositories/example.com%2Fchain%2Fweb/export?generation=0123456789abcdef');
});

test('an invalid export exposes the core parse error and leaves no half-drawn table', async () => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('{"repository":"x","files":[{"path":"a","hash":7}]}')));
  await act(() => root.render(<GraphPage api={new Api(new Auth(config))} locale="en" />));
  await act(() => {
    fireEvent.change(getByRole(host, 'textbox', { name: 'Repository' }), { target: { value: 'example.com/chain/web' } });
    fireEvent.submit(getByRole(host, 'form', { name: 'Code graph' }));
  });
  await waitFor(() => expect(getByRole(host, 'alert').textContent).toContain('files[0].hash'));
  expect(queryByRole(host, 'table')).toBeNull();
});

test('new interface copy has every key in both languages', () => {
  expect(Object.keys(graphCopy.en).sort()).toEqual(Object.keys(graphCopy['pt-BR']).sort());
  expect(Object.values(graphCopy['pt-BR']).every(Boolean)).toBe(true);
});

test('loading the current generation again reads the latest export instead of a cached alias', async () => {
  const first = readFileSync('../tests/golden/chain-web.export.json', 'utf8');
  const next = JSON.parse(first);
  next.communities.push({ name: 'new-generation-community', members: [] });
  const fetch = vi.fn().mockResolvedValueOnce(new Response(first)).mockResolvedValueOnce(new Response(JSON.stringify(next)));
  vi.stubGlobal('fetch', fetch);
  await act(() => root.render(<GraphPage api={new Api(new Auth(config))} locale="en" />));
  await act(() => {
    fireEvent.change(getByRole(host, 'textbox', { name: 'Repository' }), { target: { value: 'example.com/chain/web' } });
    fireEvent.submit(getByRole(host, 'form', { name: 'Code graph' }));
  });
  await waitFor(() => expect(getByRole(host, 'table', { name: 'Communities' })).toBeDefined());
  await act(() => { fireEvent.submit(getByRole(host, 'form', { name: 'Code graph' })); });
  await waitFor(() => expect(host.textContent).toContain('new-generation-community'));
  expect(fetch).toHaveBeenCalledTimes(2);
});
