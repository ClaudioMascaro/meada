import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { fireEvent, getAllByRole, getByLabelText, getByRole, queryByRole, waitFor } from '@testing-library/dom';
import { afterEach, expect, test, vi } from 'vitest';
import { Api } from '../src/api';
import { Auth } from '../src/auth';
import { CatalogPage } from '../src/CatalogPage';
import { catalogRoute, checkBody, checkText, reasonText, setText } from '../src/catalog';
import type { CatalogAnswer, CheckAnswer } from '../src/catalog';
import { catalogCopy } from '../src/catalog-i18n';
import { copy } from '../src/i18n';
import { Shell, offered } from '../src/pages';
import config from './fixtures/config.json';

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const host = document.createElement('div');
document.body.append(host);
const root = createRoot(host);
afterEach(async () => { await act(() => root.render(null)); vi.unstubAllGlobals(); });

const set = { finished: '2026-10-30T01:00:00.000000Z', age_seconds: 12, max_age_seconds: 5400 };
const ready = (identities: string[], next: string | null = null): CatalogAnswer => ({
  state: 'ready', reason: null, set, repositories: identities.map(identity => ({ identity })), next,
});
const pendingAnswer = (reason: CatalogAnswer['reason'], withSet: boolean): CatalogAnswer => ({
  state: 'pending', reason, set: withSet ? set : null, repositories: [], next: null,
});

function page(locale: 'en' | 'pt-BR' = 'en') {
  return act(() => root.render(<CatalogPage api={new Api(new Auth(config))} locale={locale} />));
}

test('a route asks for a bounded page, a prefix and the cursor the last page gave, nothing else', () => {
  expect(catalogRoute()).toBe('catalog?limit=50');
  expect(catalogRoute('github.com/acme', 'Z2l0')).toBe('catalog?limit=50&q=github.com%2Facme&after=Z2l0');
  expect(JSON.parse(checkBody(['a/b/c']))).toEqual({ identities: ['a/b/c'] });
  expect(JSON.parse(checkBody(Array.from({ length: 80 }, (_, i) => `h/o/r${i}`))).identities).toHaveLength(50);
});

test.each(['en', 'pt-BR'] as const)('every state is text, in %s, and both languages say the same things', locale => {
  const t = catalogCopy[locale];
  for (const reason of ['no_set', 'stale_set', 'missing_login'] as const) expect(reasonText(reason, t).length).toBeGreaterThan(20);
  expect(reasonText(null, t)).toBe('');
  expect(setText(null, t)).toBe(t.noSet);
  expect(setText(set, t)).toContain('5400');
  for (const state of ['selectable', 'unavailable', 'pending'] as const) expect(checkText(state, t)).toBeTruthy();
  expect(Object.keys(catalogCopy['pt-BR']).sort()).toEqual(Object.keys(catalogCopy.en).sort());
});

test('the catalog is offered to every admitted subject, and only when the console has one', () => {
  const paths = (role?: 'viewer' | 'team_operator' | 'operator', catalog = true) => offered(role && { subject: 's', role }, false, catalog).map(route => route.path);
  for (const role of ['viewer', 'team_operator', 'operator', undefined] as const) {
    expect(paths(role)).toContain('/catalog');
    expect(paths(role, false)).not.toContain('/catalog');
  }
});

test.each(['en', 'pt-BR'] as const)('the picker lists what the server lists, pages with the cursor and selects locally in %s', async locale => {
  const t = catalogCopy[locale];
  const fetch = vi.fn()
    .mockResolvedValueOnce(Response.json(ready(['github.com/acme/a', 'github.com/acme/y'], 'Z2l0aHViLmNvbS9hY21lL3k')))
    .mockResolvedValueOnce(Response.json(ready(['github.com/acme/z'])));
  vi.stubGlobal('fetch', fetch);
  await act(() => root.render(
    <Shell path="/catalog" locale={locale} onLanguage={() => {}} session={{ subject: 'v', role: 'viewer' }} t={copy[locale]} catalog>
      <CatalogPage api={new Api(new Auth(config))} locale={locale} />
    </Shell>,
  ));
  await waitFor(() => expect(getByRole(host, 'list', { name: t.repositories })).toBeDefined());
  expect(getByRole(host, 'link', { name: copy[locale].catalog }).getAttribute('aria-current')).toBe('page');
  expect(fetch).toHaveBeenNthCalledWith(1, '/api/v1/catalog?limit=50', expect.anything());
  expect(getAllByRole(host, 'checkbox')).toHaveLength(2);
  expect(host.textContent).toContain(t.ready);
  await act(() => { fireEvent.click(getByRole(host, 'button', { name: t.more })); });
  await waitFor(() => expect(getAllByRole(host, 'checkbox')).toHaveLength(3));
  expect(fetch).toHaveBeenNthCalledWith(2, '/api/v1/catalog?limit=50&after=Z2l0aHViLmNvbS9hY21lL3k', expect.anything());
  expect(queryByRole(host, 'button', { name: t.more })).toBeNull();
  await act(() => { fireEvent.click(getByLabelText(host, `${t.select} github.com/acme/y`)); });
  expect(getByRole(host, 'list', { name: t.selected }).textContent).toBe('github.com/acme/y');
  for (const control of [...getAllByRole(host, 'button'), ...getAllByRole(host, 'checkbox')]) expect(control.tabIndex).toBeGreaterThanOrEqual(0);
});

test('a verification posts the selection and shows the server\'s state for each, in text', async () => {
  const t = catalogCopy.en;
  const verified: CheckAnswer = {
    state: 'ready', reason: null,
    results: [{ identity: 'github.com/acme/a', state: 'selectable' }, { identity: 'github.com/acme/y', state: 'unavailable' }],
  };
  const fetch = vi.fn()
    .mockResolvedValueOnce(Response.json(ready(['github.com/acme/a', 'github.com/acme/y'])))
    .mockResolvedValueOnce(Response.json(verified));
  vi.stubGlobal('fetch', fetch);
  await page();
  await waitFor(() => expect(getAllByRole(host, 'checkbox')).toHaveLength(2));
  expect((getByRole(host, 'button', { name: t.verify }) as HTMLButtonElement).disabled).toBe(true);
  for (const identity of ['github.com/acme/a', 'github.com/acme/y']) await act(() => { fireEvent.click(getByLabelText(host, `${t.select} ${identity}`)); });
  await act(() => { fireEvent.click(getByRole(host, 'button', { name: t.verify })); });
  await waitFor(() => expect(getByRole(host, 'region', { name: t.verified })).toBeDefined());
  const [route, init] = fetch.mock.calls[1] as [string, RequestInit];
  expect(route).toBe('/api/v1/catalog/check');
  expect(init.method).toBe('POST');
  expect(JSON.parse(init.body as string)).toEqual({ identities: ['github.com/acme/a', 'github.com/acme/y'] });
  const text = getByRole(host, 'region', { name: t.verified }).textContent;
  expect(text).toContain(`github.com/acme/a: ${t.selectable}`);
  expect(text).toContain(`github.com/acme/y: ${t.unavailable}`);
  expect(text).not.toContain('reason');
});

test.each([['no_set', false], ['stale_set', true], ['missing_login', true]] as const)('a pending catalog (%s) lists nothing and says why', async (reason, withSet) => {
  const t = catalogCopy.en;
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(Response.json(pendingAnswer(reason, withSet))));
  await page();
  await waitFor(() => expect(host.textContent).toContain(t.pendingState));
  expect(host.textContent).toContain(t[reason]);
  expect(queryByRole(host, 'checkbox')).toBeNull();
  expect(queryByRole(host, 'list', { name: t.repositories })).toBeNull();
  expect(host.textContent).toContain(withSet ? '5400' : t.noSet);
});

test('a search asks the server for the prefix and an empty answer says so', async () => {
  const t = catalogCopy.en;
  const fetch = vi.fn()
    .mockResolvedValueOnce(Response.json(ready(['github.com/acme/a'])))
    .mockResolvedValueOnce(Response.json(ready([])));
  vi.stubGlobal('fetch', fetch);
  await page();
  await waitFor(() => expect(getAllByRole(host, 'checkbox')).toHaveLength(1));
  await act(() => { fireEvent.change(getByLabelText(host, t.search), { target: { value: 'github.com/other' } }); });
  await act(() => { fireEvent.click(getByRole(host, 'button', { name: t.searchButton })); });
  await waitFor(() => expect(host.textContent).toContain(t.empty));
  expect(fetch).toHaveBeenNthCalledWith(2, '/api/v1/catalog?limit=50&q=github.com%2Fother', expect.anything());
});

test('a refusal is shown as the server wrote it, and nothing is listed', async () => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(Response.json({ error: { code: 'deprovisioned', message: 'This account is deactivated.' } }, { status: 403 })));
  await page();
  await waitFor(() => expect(getByRole(host, 'alert').textContent).toBe('This account is deactivated.'));
  expect(queryByRole(host, 'checkbox')).toBeNull();
});
