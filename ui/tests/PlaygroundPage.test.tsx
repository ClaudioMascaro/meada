import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { fireEvent, getByRole, queryByRole, waitFor } from '@testing-library/dom';
import { afterEach, expect, test, vi } from 'vitest';
import { Api } from '../src/api';
import { Auth } from '../src/auth';
import { PlaygroundPage } from '../src/PlaygroundPage';
import config from './fixtures/config.json';
import { graphCopy } from '../src/graph-i18n';

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const host = document.createElement('div'); document.body.append(host);
const root = createRoot(host);
afterEach(async () => { await act(() => root.render(null)); });

test.each(['en', 'pt-BR'] as const)('playground sends the served shape and displays the exact answer in %s', async locale => {
  const t = graphCopy[locale];
  const answer = 'caller\n  -> provider [exact]\n\n';
  const fetch = vi.fn().mockResolvedValue(Response.json({ text: answer, generation: '0123456789abcdef' }));
  vi.stubGlobal('fetch', fetch);
  await act(() => root.render(<PlaygroundPage api={new Api(new Auth(config))} locale={locale} />));
  await act(() => {
    fireEvent.change(getByRole(host, 'textbox', { name: t.repository }), { target: { value: 'example.com/chain/web' } });
    fireEvent.change(getByRole(host, 'textbox', { name: t.generation }), { target: { value: '0123456789abcdef' } });
    fireEvent.change(getByRole(host, 'combobox', { name: t.tool }), { target: { value: 'trace' } });
  });
  await act(() => {
    fireEvent.change(getByRole(host, 'textbox', { name: t.start }), { target: { value: 'dashboard' } });
    fireEvent.submit(getByRole(host, 'form', { name: t.playground }));
  });
  await waitFor(() => expect(getByRole(host, 'region', { name: t.answer }).textContent).toBe(answer));
  expect(fetch.mock.calls[0]![0]).toBe('/api/v1/query');
  expect(JSON.parse(fetch.mock.calls[0]![1].body)).toEqual({
    repository: 'example.com/chain/web', generation: '0123456789abcdef', tool: 'trace',
    arguments: { from: 'dashboard', direction: 'down' },
  });
  expect(host.textContent).toContain('0123456789abcdef');
  expect(queryByRole(host, 'option', { name: /uncommitted/i })).toBeNull();
  expect(getByRole(host, 'button', { name: t.run }).tabIndex).toBe(0);
});

test('a query refusal is shown as the API words it', async () => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(Response.json({ error: { code: 'not_found', message: 'generation is not held' } }, { status: 404 })));
  await act(() => root.render(<PlaygroundPage api={new Api(new Auth(config))} locale="en" />));
  await act(() => {
    fireEvent.change(getByRole(host, 'textbox', { name: 'Repository' }), { target: { value: 'example.com/chain/web' } });
    fireEvent.change(getByRole(host, 'textbox', { name: 'Query' }), { target: { value: 'orders' } });
    fireEvent.submit(getByRole(host, 'form', { name: 'Query playground' }));
  });
  await waitFor(() => expect(getByRole(host, 'alert').textContent).toBe('generation is not held'));
});


test.each(['en', 'pt-BR'] as const)('playground controls have names and keyboard access in %s', async locale => {
  const t = graphCopy[locale];
  await act(() => root.render(<PlaygroundPage api={new Api(new Auth(config))} locale={locale} />));
  for (const name of [t.repository, t.generation, t.query]) expect(getByRole(host, 'textbox', { name }).tabIndex).toBe(0);
  expect(getByRole(host, 'combobox', { name: t.tool }).tabIndex).toBe(0);
  expect(getByRole(host, 'combobox', { name: t.kind }).tabIndex).toBe(0);
  expect(getByRole(host, 'spinbutton', { name: t.budget }).tabIndex).toBe(0);
  expect(getByRole(host, 'button', { name: t.run }).tabIndex).toBe(0);
});
