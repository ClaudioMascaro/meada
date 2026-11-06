#!/usr/bin/env node
// #675: real console API, keyboard-driven graph tables and playground, with retained evidence.
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import fs from 'node:fs';
import path from 'node:path';

const run = path.resolve(process.argv[2]);
const require = createRequire(path.join(process.env.PROVE_NODE, 'node_modules', 'x.js'));
const { chromium } = require('playwright-core');
const axe = fs.readFileSync(require.resolve('axe-core/axe.min.js'), 'utf8');
const read = name => JSON.parse(fs.readFileSync(path.join(run, name), 'utf8'));
const write = (name, value) => fs.writeFileSync(path.join(run, name), typeof value === 'string' ? value : JSON.stringify(value, null, 2));
const url = fs.readFileSync(path.join(run, 'url'), 'utf8');
const prefix = 'example.com/console-graph/';
const exports = Object.fromEntries(['app', 'deploy', 'infra', 'monitoring'].map(name => [name, read(`exports/${name}.json`)]));
const worker = 'image::ghcr.io/console/worker';
const queue = 'channel::sqs::console-jobs';

// Source-derived controls accompany API parity: missing roles or links must not pass together.
assert.deepEqual(exports.app.providers.map(row => row.contract).sort(), [worker, 'image::ghcr.io/console/unrun'].sort());
assert.deepEqual(exports.deploy.consumers.map(row => row.contract).sort(), [worker, 'image::ghcr.io/console/missing'].sort());
assert.equal(exports.deploy.links.length, 1);
assert.deepEqual(exports.deploy.links.map(row => [row.contract, row.provider, row.consumer, row.level, row.root]), [[
  worker, prefix + 'app', prefix + 'deploy::compose.yaml::services.worker', 'exact', '.',
]]);
assert.deepEqual(exports.infra.provisioners.map(row => [row.contract, row.root]).sort(), [[queue, 'envs/dev'], [queue, 'envs/prod']]);
assert.deepEqual(exports.app.provisions.map(row => [row.contract, row.root, row.at, row.level]).sort(), [
  [queue, 'envs/dev', 'settings.yaml#queue', 'inferred'], [queue, 'envs/prod', 'settings.yaml#queue', 'inferred'],
]);
assert.equal(exports.monitoring.providers.length, 4);
assert.equal(exports.monitoring.consumers.length, 2);
assert.equal(exports.monitoring.links.length, 3);
assert.deepEqual(exports.monitoring.links.map(row => [row.provider.split('::').at(-1), row.consumer.split('::').at(-1), row.level]).sort(), [
  ['groups[0].rules[0]', 'route.routes[0]', 'exact'], ['groups[0].rules[0]', 'route.routes[1]', 'inferred'],
  ['groups[0].rules[1]', 'route.routes[1]', 'inferred'],
].sort());

const browser = await chromium.launch({ executablePath: process.env.PROVE_CHROMIUM });
const budgets = [];
const errors = [];
try {
  const context = await browser.newContext({ bypassCSP: true, locale: 'en-US' });
  await context.addInitScript(() => {
    window.__cls = 0;
    new PerformanceObserver(list => {
      for (const entry of list.getEntries()) if (!entry.hadRecentInput) window.__cls += entry.value;
    }).observe({ type: 'layout-shift', buffered: true });
  });
  const page = await context.newPage();
  page.on('pageerror', error => errors.push(error.message));
  page.on('console', message => { if (message.type() === 'error') errors.push(message.text()); });
  async function open(route) {
    await page.goto(url + route, { waitUntil: 'networkidle' });
    const result = await page.evaluate(() => ({ cls: window.__cls, loadMs: performance.getEntriesByType('navigation')[0].loadEventEnd }));
    budgets.push({ route, ...result });
    assert.ok(result.cls <= .1 && result.loadMs <= 2000, `page budgets ${JSON.stringify(result)}`);
  }
  async function load(name) {
    const selector = page.getByRole('combobox', { name: 'Graph view' });
    const previous = await selector.count() ? await selector.inputValue() : 'communities';
    const repository = page.getByRole('textbox', { name: 'Repository', exact: true });
    await repository.fill(prefix + name);
    const response = page.waitForResponse(reply => reply.url().includes('/export'));
    await repository.press('Enter');
    assert.equal((await response).status(), 200);
    await page.waitForFunction(identity => document.querySelector('.file-path code')?.textContent === identity, prefix + name);
    const options = await selector.locator('option').evaluateAll(options => options.map(option => option.value));
    assert.equal(await selector.inputValue(), options.includes(previous) ? previous : 'communities');
    await choose('communities');
  }
  async function choose(value) {
    const selector = page.getByRole('combobox', { name: 'Graph view' });
    const values = await selector.locator('option').evaluateAll(options => options.map(option => option.value));
    assert.ok(values.includes(value), `view available: ${value}`);
    await selector.focus();
    await selector.press('Home');
    for (let i = 0; i < values.indexOf(value); i++) await selector.press('ArrowDown');
    await selector.press('Enter');
    assert.equal(await selector.inputValue(), value);
    await selector.press('Tab');
    assert.equal(await page.evaluate(() => document.activeElement?.getAttribute('role')), 'region');
    await page.keyboard.press('ArrowRight');
  }
  async function table(name, expected, capture) {
    const table = page.getByRole('table', { name, exact: true });
    await table.waitFor();
    const rows = await table.locator('tbody tr').evaluateAll(rows => rows.map(row => [...row.querySelectorAll('td')].map(cell => cell.textContent)));
    write(`${capture}.rows.json`, rows);
    assert.deepEqual(rows, expected.length ? expected : [['No entries in this view.']], name);
    const region = page.getByRole('region', { name, exact: true });
    assert.equal(await region.getAttribute('tabindex'), '0');
    await region.focus();
    assert.ok(await region.evaluate(element => document.activeElement === element), `${name} receives keyboard focus`);
    await page.addScriptTag({ content: axe });
    const violations = await page.evaluate(async () => (await window.axe.run()).violations
      .filter(row => row.impact === 'serious' || row.impact === 'critical'));
    write(`${capture}.axe.json`, violations);
    assert.equal(violations.length, 0, `${name}: serious/critical accessibility violations`);
    write(`${capture}.txt`, await table.innerText());
    await page.screenshot({ path: path.join(run, `${capture}.png`), fullPage: true });
  }
  function links(data, kind) {
    return data.links.filter(row => row.contract.startsWith(kind + '::')).map(row => [
      row.contract, row.provider, row.consumer + (row.root ? ` (root ${row.root})` : ''), row.level, row.reason,
    ]);
  }
  function sides(data, kind) {
    const rows = new Map();
    for (const [field, column] of [['providers', 1], ['consumers', 2]]) {
      for (const row of data[field].filter(row => row.contract?.startsWith(kind + '::'))) {
        if (!rows.has(row.contract)) rows.set(row.contract, [row.contract, [], []]);
        rows.get(row.contract)[column].push(row.site + (row.at ? ` (${row.at})` : '') + (row.root ? ` (root ${row.root})` : ''));
      }
    }
    return [...rows.values()].sort((a, b) => a[0].localeCompare(b[0])).map(row => [row[0], row[1].join(''), row[2].join('')]);
  }
  await open('/graph');
  await load('app');
  const legend = page.getByRole('region', { name: 'Graph legend' });
  for (const text of ['Contract side → Provisioner', 'Builder → Runner', 'Emitter → Matcher']) assert.ok((await legend.innerText()).includes(text));
  await choose('images');
  await table('Image contracts', sides(exports.app, 'image'), 'app-image-sides');
  await table('Image links', links(exports.app, 'image'), 'app-image-links');
  await choose('provisions');
  await table('Provisioning links', exports.app.provisions.map(row => [row.contract, row.side + (row.at ? ` ${row.at}` : ''),
    `${row.provisioner} (root ${row.root})`, row.level, row.reason]), 'app-provisions');
  await choose('contracts');
  const contracts = page.getByRole('table', { name: 'Contracts', exact: true });
  assert.equal(await contracts.locator('tbody tr').count(), 3);
  for (const root of ['envs/dev', 'envs/prod']) assert.ok((await contracts.innerText()).includes(`(root ${root})`));
  assert.ok((await contracts.innerText()).includes('settings.yaml#queue'));
  await load('deploy');
  await choose('images');
  await table('Image contracts', sides(exports.deploy, 'image'), 'deploy-image-sides');
  await table('Image links', links(exports.deploy, 'image'), 'deploy-image-links');
  await load('monitoring');
  await choose('alerts');
  await table('Alert contracts', sides(exports.monitoring, 'alert'), 'alert-sides');
  await table('Alert links', links(exports.monitoring, 'alert'), 'alert-links');
  await choose('links');
  await table('Cross-repository links', [], 'alert-cross-repository-control');
  await load('infra');
  // Loading a repository without image/alert sides removes those unavailable views.
  const options = await page.getByRole('combobox', { name: 'Graph view' }).locator('option').evaluateAll(rows => rows.map(row => row.value));
  assert.ok(!options.includes('images') && !options.includes('alerts'));
  await choose('contracts');
  assert.ok((await page.getByRole('table', { name: 'Contracts', exact: true }).innerText()).includes(queue));
  for (const root of ['envs/dev', 'envs/prod']) assert.ok((await page.getByRole('table', { name: 'Contracts', exact: true }).innerText()).includes(`(root ${root})`));
  await choose('provisions');
  await table('Provisioning links', exports.infra.provisions.map(row => [row.contract, row.side + (row.at ? ` ${row.at}` : ''),
    `${row.provisioner} (root ${row.root})`, row.level, row.reason]), 'infra-provisions');

  await open('/playground');
  for (const { body, answer } of read('queries.json')) {
    await page.getByRole('textbox', { name: 'Repository', exact: true }).fill(body.repository);
    await page.getByRole('textbox', { name: 'Query', exact: true }).fill(body.arguments.query);
    await page.getByRole('combobox', { name: 'Kind', exact: true }).selectOption(body.arguments.kind);
    const response = page.waitForResponse(reply => reply.url().endsWith('/query'));
    await page.getByRole('textbox', { name: 'Query', exact: true }).press('Enter');
    assert.equal((await response).status(), 200);
    const region = page.getByRole('region', { name: 'Answer', exact: true });
    await region.waitFor();
    assert.equal(await region.textContent(), answer, body.arguments.kind + ' playground/API answer parity');
    write(`playground-${body.arguments.kind}.txt`, await region.textContent());
  }
  await page.screenshot({ path: path.join(run, 'playground-alert.png'), fullPage: true });
  assert.equal(errors.length, 0, 'console errors');
  console.log('Keyboard-selected image, alert and provisioning views; exact/inferred links and roots; orphan sides; same-repository alerts; reload reset; value/iac/alert playground API parity; page and accessibility budgets passed.');
} finally {
  write('budgets.json', budgets);
  write('browser-errors.json', errors);
  await browser.close();
}
