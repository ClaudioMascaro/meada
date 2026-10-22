import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { getAllByRole, getByRole } from '@testing-library/dom';
import { afterEach, expect, test } from 'vitest';
import { Shell, ConfigPage, SessionPage, ManifestPage } from '../src/pages';
import { copy } from '../src/i18n';
import config from './fixtures/config.json';
import manifest from './fixtures/manifest.json';

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const host = document.createElement('div');
document.body.append(host);
const root = createRoot(host);
afterEach(async () => { await act(() => root.render(null)); });

test.each(['en', 'pt-BR'] as const)('the shell and three pages are named and keyboard reachable in %s', async locale => {
  const t = copy[locale];
  for (const page of [
    <ConfigPage config={config} t={t} />,
    <SessionPage session={{ subject: 'alice', role: 'viewer' }} t={t} />,
    <ManifestPage manifest={manifest} t={t} />,
  ]) {
    await act(() => root.render(
      <Shell path="/config" locale={locale} onLanguage={() => {}} session={{ subject: 'alice', role: 'viewer' }} t={t}>
        {page}
      </Shell>,
    ));
    expect(getByRole(host, 'navigation', { name: t.navigation })).toBeDefined();
    expect(getByRole(host, 'main')).toBeDefined();
    for (const element of [
      ...getAllByRole(host, 'link'), ...getAllByRole(host, 'combobox'),
    ]) {
      expect(element.tabIndex).toBeGreaterThanOrEqual(0);
      expect(element.getAttribute('aria-label') || element.textContent?.trim()).toBeTruthy();
    }
    expect(getByRole(host, 'combobox', { name: t.language })).toBeDefined();
    expect(getByRole(host, 'link', { name: t.skip }).getAttribute('href')).toBe('#main');
    expect(getByRole(host, 'link', { name: t.config }).getAttribute('aria-current')).toBe('page');
    expect(host.querySelector('button')).toBeNull();
    for (const region of host.querySelectorAll<HTMLElement>('[role=region]')) {
      expect(region.getAttribute('aria-label')).toBeTruthy();
      expect(region.tabIndex).toBe(0);
    }
  }
});

test('an authenticated session can end its in-memory session with a named keyboard control', async () => {
  await act(() => root.render(<SessionPage session={{ subject: 'alice', role: 'operator' }} t={copy.en} onSignOut={() => {}} />));
  expect(getByRole(host, 'button', { name: copy.en.signOut }).tabIndex).toBe(0);
});
