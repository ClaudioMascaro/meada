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

test('Terraform contract sides and provisioning links retain each root and evidence', async () => {
  const text = readFileSync('../tests/golden/provision-infra.export.json', 'utf8');
  const data = JSON.parse(text);
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(text)));
  await act(() => root.render(<GraphPage api={new Api(new Auth(config))} locale="en" />));
  await act(() => {
    fireEvent.change(getByRole(host, 'textbox', { name: 'Repository' }), {
      target: { value: 'example.com/provision/infra' },
    });
    fireEvent.submit(getByRole(host, 'form', { name: 'Code graph' }));
  });
  await waitFor(() => expect(getByRole(host, 'table', { name: 'Communities' })).toBeDefined());
  await act(() => {
    fireEvent.change(getByRole(host, 'combobox', { name: 'Graph view' }), {
      target: { value: 'contracts' },
    });
  });
  const contracts = getByRole(host, 'table', { name: 'Contracts' });
  expect(getByRole(contracts, 'columnheader', { name: 'Provisioners' })).toBeDefined();
  expect(getByRole(contracts, 'columnheader', { name: 'Config uses' })).toBeDefined();
  const useSite = 'example.com/provision/infra envs/prod/terraform.tfvars#queue';
  const queue = getByRole(contracts, 'row', { name: /channel::sqs::orders/ });
  expect(getByRole(queue, 'cell', { name: useSite })).toBeDefined();
  for (const side of data.provisioners) {
    expect(contracts.textContent).toContain(`${side.node} (root ${side.root})`);
  }
  await act(() => {
    fireEvent.change(getByRole(host, 'combobox', { name: 'Graph view' }), {
      target: { value: 'provisions' },
    });
  });
  const links = getByRole(host, 'table', { name: 'Provisioning links' });
  const rows = Array.from(links.querySelectorAll<HTMLTableRowElement>('tbody tr'));
  const configRows = rows.filter(row => queryByRole(row, 'cell', { name: useSite }));
  expect(rows.filter(row => !configRows.includes(row))).toHaveLength(16);
  expect(configRows).toHaveLength(2);
  for (const root of ['envs/dev', 'envs/prod']) {
    expect(configRows.some(row => row.textContent?.includes(
      `example.com/provision/infra::${root}/main.tf::aws_sqs_queue.orders (root ${root})`,
    ))).toBe(true);
  }
  for (const link of data.provisions) {
    expect(links.textContent).toContain(`${link.provisioner} (root ${link.root})`);
    expect(links.textContent).toContain(link.side);
    expect(links.textContent).toContain(link.level);
    expect(links.textContent).toContain(link.reason);
  }
});

test.each(['en', 'pt-BR'] as const)('new kind views keep orphan sides visible and localize their accessible names in %s', async locale => {
  const t = graphCopy[locale];
  const data = JSON.parse(readFileSync('../tests/golden/images.builders.export.json', 'utf8'));
  data.links = [];
  const fetch = vi.fn().mockResolvedValueOnce(new Response(JSON.stringify(data)))
    .mockImplementation(async () => new Response(readFileSync('../tests/golden/chain-web.export.json', 'utf8')));
  vi.stubGlobal('fetch', fetch);
  await act(() => root.render(<GraphPage api={new Api(new Auth(config))} locale={locale} />));
  await act(() => {
    fireEvent.change(getByRole(host, 'textbox', { name: t.repository }), { target: { value: data.repository } });
    fireEvent.submit(getByRole(host, 'form', { name: t.graph }));
  });
  await waitFor(() => expect(getByRole(host, 'table', { name: t.communities })).toBeDefined());
  await act(() => {
    fireEvent.change(getByRole(host, 'combobox', { name: t.view }), { target: { value: 'images' } });
  });
  expect(getByRole(host, 'region', { name: t.legend }).textContent).toContain(`${t.builder} → ${t.runner}`);
  const sides = getByRole(host, 'table', { name: t.imageSides });
  for (const side of data.providers) expect(sides.textContent).toContain(side.at);
  expect(getByRole(host, 'table', { name: t.images }).textContent).toContain(t.noResults);
  await act(() => {
    fireEvent.change(getByRole(host, 'textbox', { name: t.repository }), { target: { value: 'example.com/chain/web' } });
    fireEvent.submit(getByRole(host, 'form', { name: t.graph }));
  });
  await waitFor(() => expect(getByRole(host, 'table', { name: t.communities })).toBeDefined());
  expect((getByRole(host, 'combobox', { name: t.view }) as HTMLSelectElement).value).toBe('communities');
  expect(queryByRole(host, 'option', { name: t.images })).toBeNull();
  expect(queryByRole(host, 'option', { name: t.ci })).toBeNull();
  await act(() => {
    fireEvent.change(getByRole(host, 'combobox', { name: t.view }), { target: { value: 'contracts' } });
    fireEvent.submit(getByRole(host, 'form', { name: t.graph }));
  });
  await waitFor(() => expect(getByRole(host, 'table', { name: t.contracts })).toBeDefined());
  expect((getByRole(host, 'combobox', { name: t.view }) as HTMLSelectElement).value).toBe('contracts');
});

test.each([
  { view: 'images', golden: 'images.workflow', title: 'Image links', from: 'Builder', to: 'Runner',
    contract: 'image::ghcr.io/acme/orders', provider: 'example.com/acme/app::.github/workflows/build.yml::jobs.image.steps[0]',
    consumer: 'example.com/acme/deploy::deployment.yaml::Deployment.apps/shop/orders (root .)' },
  { view: 'alerts', golden: 'alerts.equality', title: 'Alert links', from: 'Emitter', to: 'Matcher',
    contract: 'alert::support::team-b', provider: 'example.com/acme/monitoring::rules.yaml::groups[0].rules[0]',
    consumer: 'example.com/acme/monitoring::alertmanager.yaml::route.routes[0]' },
])('$view show directed, named roles including links inside one repository', async example => {
  const text = readFileSync(`../tests/golden/${example.golden}.export.json`, 'utf8');
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(text)));
  await act(() => root.render(<GraphPage api={new Api(new Auth(config))} locale="en" />));
  await act(() => {
    fireEvent.change(getByRole(host, 'textbox', { name: 'Repository' }), {
      target: { value: JSON.parse(text).repository },
    });
    fireEvent.submit(getByRole(host, 'form', { name: 'Code graph' }));
  });
  await waitFor(() => expect(getByRole(host, 'table', { name: 'Communities' })).toBeDefined());
  const legend = getByRole(host, 'region', { name: 'Graph legend' });
  expect(legend.textContent).toContain('Builder → Runner');
  expect(legend.textContent).toContain('Emitter → Matcher');
  expect(legend.textContent).toContain('Contract side → Provisioner');
  await act(() => {
    fireEvent.change(getByRole(host, 'combobox', { name: 'Graph view' }), { target: { value: example.view } });
  });
  const region = getByRole(host, 'region', { name: example.title });
  expect(region.tabIndex).toBe(0);
  region.focus();
  expect(document.activeElement).toBe(region);
  const table = getByRole(region, 'table', { name: example.title });
  expect(Array.from(table.querySelectorAll('thead th')).map(th => th.textContent)).toEqual([
    'Contract', example.from, example.to, 'Level', 'Reason',
  ]);
  expect(Array.from(table.querySelectorAll('tbody tr')).map(row =>
    Array.from(row.querySelectorAll('td')).map(cell => cell.textContent))).toEqual([
    [example.contract, example.provider, example.consumer, 'exact', JSON.parse(text).links[0].reason],
  ]);
});

test.each(['en', 'pt-BR'] as const)('CI links are a group of their own, with its legend and the absent mark, in %s', async locale => {
  const t = graphCopy[locale];
  const text = readFileSync('../tests/golden/ci-calls.app-a.export.json', 'utf8');
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(text)));
  await act(() => root.render(<GraphPage api={new Api(new Auth(config))} locale={locale} />));
  await act(() => {
    fireEvent.change(getByRole(host, 'textbox', { name: t.repository }), { target: { value: 'github.com/acme/app-a' } });
    fireEvent.submit(getByRole(host, 'form', { name: t.graph }));
  });
  await waitFor(() => expect(getByRole(host, 'table', { name: t.communities })).toBeDefined());
  expect(getByRole(host, 'region', { name: t.legend }).textContent).toContain(`${t.called} → ${t.caller}`);
  await act(() => {
    fireEvent.change(getByRole(host, 'combobox', { name: t.view }), { target: { value: 'ci' } });
  });
  const links = getByRole(host, 'table', { name: t.ci });
  expect(Array.from(links.querySelectorAll('thead th')).map(th => th.textContent)).toEqual([
    t.contract, t.called, t.caller, t.level, t.reason,
  ]);
  const rows = Array.from(links.querySelectorAll('tbody tr')).map(row =>
    Array.from(row.querySelectorAll('td')).map(cell => cell.textContent));
  // A call to another repository, with the Git ref as written and the indexed commit.
  expect(rows[0]).toEqual([
    'ci::github.com/acme/workflows/.github/workflows/lint.yml',
    'github.com/acme/workflows::.github/workflows/lint.yml::.',
    'github.com/acme/app-a::.github/workflows/ci.yml::jobs.lint',
    'exact', '@main (indexed 2344e45)',
  ]);
  // A call inside the repository is in the group too.
  expect(rows[6]).toEqual([
    'ci::github.com/acme/app-a/tools/local',
    'github.com/acme/app-a::tools/local/action.yml::.',
    'github.com/acme/app-a::.github/workflows/forms.yml::jobs.actions.steps[4]',
    'exact', 'own repository',
  ]);
  expect(rows).toHaveLength(7);
  const sides = getByRole(host, 'table', { name: t.ciSides });
  expect(Array.from(sides.querySelectorAll('thead th')).map(th => th.textContent)).toEqual([
    t.contract, t.calledFiles, t.callers,
  ]);
  const internal = Array.from(sides.querySelectorAll('tbody tr')).map(row =>
    Array.from(row.querySelectorAll('td')).map(cell => cell.textContent))
    .find(row => row[0] === 'ci::github.com/acme/workflows/.github/workflows/internal.yml');
  expect(internal).toEqual([
    'ci::github.com/acme/workflows/.github/workflows/internal.yml', '',
    'github.com/acme/app-a::.github/workflows/ci.yml::jobs.internal (absent at the indexed commit)',
  ]);
});
