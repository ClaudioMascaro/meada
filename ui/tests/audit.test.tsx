import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { fireEvent, getAllByRole, getByRole, queryByRole, waitFor } from '@testing-library/dom';
import { afterEach, expect, test, vi } from 'vitest';
import { AuditPage } from '../src/AuditPage';
import { auditModel, auditRoute } from '../src/audit';
import type { AuditPage as Page } from '../src/audit';
import { auditCopy } from '../src/audit-i18n';
import { Api } from '../src/api';
import { Auth } from '../src/auth';
import { copy } from '../src/i18n';
import { Shell, offered } from '../src/pages';
import config from './fixtures/config.json';
import fixture from './fixtures/audit.json';

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const host = document.createElement('div');
document.body.append(host);
const root = createRoot(host);
afterEach(async () => { await act(() => root.render(null)); vi.unstubAllGlobals(); });
const page = fixture as Page;

test('the view model shows every field of a record, absent ones as none', () => {
  const [refused, pending] = auditModel(page, auditCopy.en);
  expect(refused).toEqual({
    id: 12, time: '2026-10-29T12:00:01.123456Z', via: 'Console', subject: 'ana', login: 'None',
    role: 'Team operator', owners: 'team-billing, team-search', action: 'workspace.save',
    arguments: 'version: 0123456789abcdef, message: move billing', outcome: 'Refused',
    message: '1 entry is outside your teams (e.test/team/joint; hidden 1)',
  });
  expect(pending).toMatchObject({ via: 'CLI', login: 'octo-ops', role: 'Operator', owners: 'None', arguments: 'None', outcome: 'Pending', message: 'None' });
  expect(auditModel(page, auditCopy['pt-BR'])[0]).toMatchObject({ role: 'Operador de time', outcome: 'Recusada' });
  expect(auditRoute()).toBe('audit');
  expect(auditRoute(11)).toBe('audit?before=11');
});

test('only the global operator is offered the audit page', () => {
  const paths = (role?: 'viewer' | 'team_operator' | 'operator') => offered(role && { subject: 's', role }).map(route => route.path);
  expect(paths('operator')).toContain('/audit');
  for (const role of ['viewer', 'team_operator', undefined] as const) expect(paths(role)).not.toContain('/audit');
});

test.each(['en', 'pt-BR'] as const)('the page pages with next, labelled and keyboard reachable in %s', async locale => {
  const t = auditCopy[locale];
  const fetch = vi.fn()
    .mockResolvedValueOnce(Response.json(page))
    .mockResolvedValueOnce(Response.json({ records: [{ ...page.records[1], id: 10 }], next: null }));
  vi.stubGlobal('fetch', fetch);
  const session = { subject: 'root', role: 'operator' } as const;
  await act(() => root.render(
    <Shell path="/audit" locale={locale} onLanguage={() => {}} session={session} t={copy[locale]}>
      <AuditPage api={new Api(new Auth(config))} locale={locale} />
    </Shell>,
  ));
  await waitFor(() => expect(getByRole(host, 'table', { name: t.records })).toBeDefined());
  expect(getByRole(host, 'link', { name: copy[locale].audit }).getAttribute('aria-current')).toBe('page');
  const headers = getAllByRole(host, 'columnheader').map(header => header.textContent);
  expect(headers).toEqual([t.time, t.via, t.subject, t.login, t.role, t.owners, t.action, t.arguments, t.outcome, t.message]);
  expect(getAllByRole(host, 'row')).toHaveLength(3);
  expect(host.textContent).toContain('e.test/team/joint; hidden 1');
  expect(getByRole(host, 'region', { name: t.records }).tabIndex).toBe(0);
  for (const control of [...getAllByRole(host, 'button'), ...getAllByRole(host, 'link')]) {
    expect(control.tabIndex).toBeGreaterThanOrEqual(0);
    expect(control.textContent?.trim()).toBeTruthy();
  }
  expect(fetch).toHaveBeenNthCalledWith(1, '/api/v1/audit', expect.anything());
  await act(() => { fireEvent.click(getByRole(host, 'button', { name: t.older })); });
  await waitFor(() => expect(getAllByRole(host, 'row')).toHaveLength(2));
  expect(fetch).toHaveBeenNthCalledWith(2, '/api/v1/audit?before=11', expect.anything());
  expect(queryByRole(host, 'button', { name: t.older })).toBeNull();
  expect(getByRole(host, 'button', { name: t.newest })).toBeDefined();
});

test('a 503 shows the console\'s message', async () => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(Response.json(
    { error: { code: 'audit_unavailable', message: 'the audit in Postgres is unavailable' } }, { status: 503 },
  )));
  await act(() => root.render(<AuditPage api={new Api(new Auth(config))} locale="en" />));
  await waitFor(() => expect(getByRole(host, 'alert').textContent).toBe('the audit in Postgres is unavailable'));
});

test('both languages provide every audit copy key', () => {
  expect(Object.keys(auditCopy['pt-BR']).sort()).toEqual(Object.keys(auditCopy.en).sort());
  expect(Object.values(auditCopy['pt-BR']).every(Boolean)).toBe(true);
});
