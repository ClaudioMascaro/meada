import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { fireEvent, getByRole, queryByRole, waitFor } from '@testing-library/dom';
import { afterEach, beforeAll, expect, test, vi } from 'vitest';
import { OperationalPage } from '../src/OperationalPage';
import { Api } from '../src/api';
import { Auth } from '../src/auth';
import config from './fixtures/config.json';
import fleet from './fixtures/fleet.json';
import manifest from './fixtures/manifest.json';
import fixtures from './fixtures/operational.json';
import { actionsCopy } from '../src/actions-i18n';

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const host = document.createElement('div');
document.body.append(host);
const root = createRoot(host);
beforeAll(() => {
  HTMLDialogElement.prototype.showModal = function () { this.open = true; };
  HTMLDialogElement.prototype.close = function () { this.open = false; };
});

test('a refused operation displays the exact holder message and does not reload or dismiss confirmation', async () => {
  const enabled = { ...config, operations: true };
  const message = 'refresh lock held by host writer PID 42 since 2026-10-23T00:00:00Z: snapshot lock';
  const fetch = vi.fn(async (url: string) => {
    if (url === '/api/v1/manifest') return Response.json(manifest);
    if (url === '/api/v1/fleet') return Response.json(fleet);
    return Response.json({ error: { code: 'conflict', message } }, { status: 409 });
  });
  vi.stubGlobal('fetch', fetch);
  await act(() => root.render(<OperationalPage kind="fleet" api={new Api(new Auth(enabled))} locale="en"
    config={enabled} session={{ role: 'operator', subject: 'maintainer' }} />));
  const button = await waitFor(() => getByRole(host, 'button', { name: 'Drop retention aaaaaaaaaaaaaaaa' }));
  await act(() => fireEvent.click(button));
  const dialog = getByRole(host, 'dialog', { name: 'Drop retention' });
  await act(() => fireEvent.click(getByRole(dialog, 'button', { name: 'Confirm' })));
  expect(getByRole(dialog, 'alert').textContent).toBe(message);
  expect(fetch.mock.calls.filter(([url]) => url === '/api/v1/fleet')).toHaveLength(1);
});

test('a store success survives a subsequent fleet refresh failure', async () => {
  const enabled = { ...config, operations: true };
  const fetch = vi.fn(async (url: string) => {
    if (url === '/api/v1/manifest') return Response.json(manifest);
    if (url === '/api/v1/store') return Response.json({ ...fixtures.store, generations: [
      { id: 'cccccccccccccccc', current: false, retained: false, kept: false },
    ] });
    if (url === '/api/v1/pointer') return Response.json({ result: 'retained cccccccccccccccc (fence 17)' });
    return Response.json({ error: { code: 'unavailable', message: 'valkey unavailable' } }, { status: 503 });
  });
  vi.stubGlobal('fetch', fetch);
  await act(() => root.render(<OperationalPage kind="store" api={new Api(new Auth(enabled))} locale="en"
    config={enabled} session={{ role: 'operator', subject: 'maintainer' }} />));
  const button = await waitFor(() => getByRole(host, 'button', { name: 'Retain generation cccccccccccccccc' }));
  await act(() => fireEvent.click(button));
  const dialog = getByRole(host, 'dialog', { name: 'Retain generation' });
  await act(() => fireEvent.click(getByRole(dialog, 'button', { name: 'Confirm' })));
  expect(getByRole(host, 'status').textContent).toBe('retained cccccccccccccccc (fence 17)');
  expect(getByRole(host, 'alert').textContent).toBe(actionsCopy.en.reloadError);
  expect(fetch.mock.calls.filter(([url]) => url === '/api/v1/fleet')).toHaveLength(1);
  expect(fetch.mock.calls.filter(([url]) => url === '/api/v1/store')).toHaveLength(2);
});

test.each(['en', 'pt-BR'] as const)('stuck cases offer named keyboard recovery controls with exact u64 commands in %s', async locale => {
  const enabled = { ...config, operations: true };
  const fetch = vi.fn(async (url: string, init?: RequestInit) => {
    if (url === '/api/v1/manifest') return Response.json(manifest);
    if (url === '/api/v1/fleet') return Response.json(fleet);
    if (url === '/api/v1/fence/recover' && init?.method === 'POST') return Response.json({ result: 'fence 18446744073709551615' });
    throw new Error(`Unexpected request ${url}`);
  });
  vi.stubGlobal('fetch', fetch);
  const t = actionsCopy[locale];
  await act(() => root.render(<OperationalPage kind="fleet" api={new Api(new Auth(enabled))} locale={locale}
    config={enabled} session={{ role: 'operator', subject: 'maintainer' }} />));
  const form = await waitFor(() => getByRole(host, 'form', { name: `${t.recover} node-a` }));
  const input = getByRole(form, 'textbox', { name: t.floor });
  expect(input.tabIndex).toBe(0);
  expect(getByRole(form, 'button', { name: t.recover }).tabIndex).toBe(0);
  await act(() => fireEvent.change(input, { target: { value: '18446744073709551616' } }));
  await act(() => fireEvent.submit(form));
  expect(getByRole(form, 'alert').textContent).toBe(t.floorInvalid);
  expect(queryByRole(host, 'dialog')).toBeNull();
  await act(() => fireEvent.change(input, { target: { value: '18446744073709551615' } }));
  expect(form.textContent).toContain('meada snapshot fence recover --at-least 18446744073709551615 --workspace');
  await act(() => fireEvent.submit(form));
  const dialog = getByRole(host, 'dialog', { name: t.recover });
  expect(fetch.mock.calls.filter(([, init]) => init?.method === 'POST')).toHaveLength(0);
  await act(() => fireEvent.click(getByRole(dialog, 'button', { name: t.confirm })));
  const write = fetch.mock.calls.find(([, init]) => init?.method === 'POST');
  expect(JSON.parse(String(write?.[1]?.body))).toEqual({ at_least: '18446744073709551615' });
  expect(Object.keys(actionsCopy.en).sort()).toEqual(Object.keys(actionsCopy['pt-BR']).sort());
});
afterEach(async () => { await act(() => root.render(null)); vi.unstubAllGlobals(); });

test.each(['fleet', 'store'] as const)('the %s page hides actions from viewers, team operators and when operations are off', async kind => {
  for (const role of ['viewer', 'team_operator', 'operator'] as const) {
    const settings = { ...config, operations: role !== 'operator' };
    const fetch = vi.fn().mockResolvedValue(Response.json(kind === 'fleet' ? fleet : fixtures.store));
    vi.stubGlobal('fetch', fetch);
    await act(() => root.render(<OperationalPage kind={kind} api={new Api(new Auth(settings))} locale="en"
      config={settings} session={{ role, subject: 'maintainer' }} />));
    await waitFor(() => getByRole(host, 'table', { name: kind === 'fleet' ? 'Live nodes' : 'Generation documents' }));
    expect(queryByRole(host, 'button', { name: /Set pointer|Retain generation|Drop retention|Recover fence/ })).toBeNull();
    expect(queryByRole(host, 'form')).toBeNull();
    expect(fetch).toHaveBeenCalledOnce();
    expect(fetch.mock.calls[0]?.[0]).toBe(`/api/v1/${kind}`);
    await act(() => root.render(null));
  }
});

test('a missing bootstrap filename leaves fleet reads available and does not offer incomplete commands', async () => {
  const enabled = { ...config, operations: true };
  const fetch = vi.fn(async (url: string) => url === '/api/v1/fleet' ? Response.json(fleet)
    : Response.json({ error: { code: 'unavailable', message: 'manifest unavailable' } }, { status: 503 }));
  vi.stubGlobal('fetch', fetch);
  await act(() => root.render(<OperationalPage kind="fleet" api={new Api(new Auth(enabled))} locale="en"
    config={enabled} session={{ role: 'operator', subject: 'maintainer' }} />));
  await waitFor(() => getByRole(host, 'table', { name: 'Live nodes' }));
  expect(getByRole(host, 'alert').textContent).toBe(actionsCopy.en.manifestError);
  expect(queryByRole(host, 'button', { name: /Set pointer|Recover fence/ })).toBeNull();
});

test('a retained generation shows the exact command, Escape sends nothing, and confirmation reloads the fleet', async () => {
  const enabled = { ...config, operations: true };
  const file = "ops/team's console.toml";
  const fetch = vi.fn(async (url: string, init?: RequestInit) => {
    if (url === '/api/v1/manifest') return Response.json({ ...manifest, file });
    if (url === '/api/v1/fleet') return Response.json(fleet);
    if (url === '/api/v1/pointer' && init?.method === 'POST') return Response.json({ result: 'current aaaaaaaaaaaaaaaa (fence 16)' });
    throw new Error(`Unexpected request ${url}`);
  });
  vi.stubGlobal('fetch', fetch);
  await act(() => root.render(<OperationalPage kind="fleet" api={new Api(new Auth(enabled))} locale="en"
    config={enabled} session={{ role: 'operator', subject: 'maintainer' }} />));
  const button = await waitFor(() => getByRole(host, 'button', { name: 'Set pointer aaaaaaaaaaaaaaaa' }));
  const command = `meada snapshot pointer set aaaaaaaaaaaaaaaa --workspace 'ops/team'"'"'s console.toml'`;
  expect(button.parentElement?.textContent).toContain(command);
  expect(button.parentElement?.textContent).toContain('next publish');
  const writes = () => fetch.mock.calls.filter(([, init]) => init?.method === 'POST');
  button.focus();
  await act(() => fireEvent.click(button));
  expect(writes()).toHaveLength(0);
  let dialog = getByRole(host, 'dialog', { name: 'Set pointer' });
  expect(dialog.textContent).toContain(command);
  await act(() => fireEvent(dialog, new Event('cancel', { cancelable: true })));
  expect(writes()).toHaveLength(0);
  expect(queryByRole(host, 'dialog')).toBeNull();
  expect(document.activeElement).toBe(button);
  await act(() => fireEvent.click(button));
  dialog = getByRole(host, 'dialog', { name: 'Set pointer' });
  await act(() => fireEvent.click(getByRole(dialog, 'button', { name: 'Confirm' })));
  await waitFor(() => expect(getByRole(host, 'status').textContent).toContain('current aaaaaaaaaaaaaaaa (fence 16)'));
  expect(writes()).toHaveLength(1);
  expect(JSON.parse(String(writes()[0]?.[1]?.body))).toEqual({ action: 'set', generation: 'aaaaaaaaaaaaaaaa' });
  expect(fetch.mock.calls.filter(([url]) => url === '/api/v1/fleet')).toHaveLength(2);
});
