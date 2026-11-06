// Browser parity with the CLI/console export, never the feature implementation or fixtures.
// Invoked by aws-sdk.py while its own console is running. Uses existing graph representation.
import { createRequire } from 'node:module';
import fs from 'node:fs';
import path from 'node:path';

const run = path.resolve(process.argv[2]);
const require = createRequire(path.join(process.env.PROVE_NODE, 'node_modules', 'x.js'));
const { chromium } = require('playwright-core');
const axe = fs.readFileSync(require.resolve('axe-core/axe.min.js'), 'utf8');
const url = fs.readFileSync(path.join(run, 'console.url'), 'utf8');
const browser = await chromium.launch({ executablePath: process.env.PROVE_CHROMIUM });
const context = await browser.newContext({ bypassCSP: true });
await context.addInitScript(() => {
  window.__sdkCls = 0;
  new PerformanceObserver(list => {
    for (const entry of list.getEntries()) if (!entry.hadRecentInput) window.__sdkCls += entry.value;
  }).observe({ type: 'layout-shift', buffered: true });
});
const page = await context.newPage();
const errors = [], checks = [];
page.on('console', message => { if (message.type() === 'error') errors.push(message.text()); });
page.on('pageerror', error => errors.push(error.message));
const equal = (actual, expected) => JSON.stringify(actual) === JSON.stringify(expected);
function check(name, passed, detail) {
  checks.push({ name, passed: Boolean(passed), detail });
  fs.writeFileSync(path.join(run, 'console-graph-checks.json'), JSON.stringify(checks, null, 2));
  console.log(`${passed ? 'PASS' : 'FAIL'} ${name}`);
}
try {
  await page.goto(url + '/graph', { waitUntil: 'networkidle' });
  for (const repository of ['app', 'infra', 'peer']) {
    const data = JSON.parse(fs.readFileSync(path.join(run, `console-${repository}-export.json`), 'utf8'));
    const reply = page.waitForResponse(response => response.url().includes('/export') && response.status() === 200);
    const form = page.locator('form.graph-form');
    await form.locator('input[required]').fill(data.repository);
    await form.locator('button[type=submit]').click();
    const loaded = await reply;
    check(`${repository} graph loaded exact console API export`, (await loaded.text()) === JSON.stringify(data, null, 2) + '\n'
      || equal(JSON.parse(await loaded.text()), data));
    const picker = page.locator('.graph-view-picker select');
    await picker.waitFor();
    const contracts = new Map();
    const row = contract => {
      if (!contracts.has(contract)) contracts.set(contract, [contract, [], [], []]);
      return contracts.get(contract);
    };
    for (const [key, column] of [['providers', 1], ['consumers', 2]]) {
      for (const side of data[key]) if (side.contract !== null) row(side.contract)[column].push(side.site);
    }
    for (const provisioner of data.provisioners ?? []) row(provisioner.contract)[3].push(`${provisioner.node} (root ${provisioner.root})`);
    for (const link of data.provisions ?? []) {
      const values = row(link.contract)[3], place = `${link.provisioner} (root ${link.root})`;
      if (!values.includes(place)) values.push(place);
    }
    const expectedContracts = [...contracts.values()].sort((left, right) => left[0] < right[0] ? -1 : left[0] > right[0] ? 1 : 0);
    const views = {
      contracts: expectedContracts,
      provisions: (data.provisions ?? []).map(link => [link.contract, link.side,
        `${link.provisioner} (root ${link.root})`, link.level, link.reason]),
      links: data.links.filter(link => link.consumer.split('::')[0] !== link.provider.split('::')[0])
        .map(link => [link.contract, link.consumer, link.provider, link.level]),
    };
    if (repository === 'app') check('app cross-repository link table is nonempty', views.links.length > 0, views.links.length);
    for (const [view, expected] of Object.entries(views)) {
      if (view === 'provisions' && !expected.length) continue;
      await picker.selectOption(view);
      const actual = await page.locator('table tbody tr').evaluateAll(rows => rows.map(row =>
        [...row.querySelectorAll('td')].map(cell => cell.querySelector('ul')
          ? [...cell.querySelectorAll('li')].map(item => item.textContent)
          : cell.textContent)));
      // The established empty table has one translated placeholder row.
      const normalized = !expected.length && actual.length === 1 && actual[0].length === 1 ? [] : actual;
      check(`${repository} ${view}: complete table parity`, equal(normalized, expected), { actual: normalized, expected });
      await page.screenshot({ path: path.join(run, `graph-${repository}-${view}.png`), fullPage: true });
    }
  }
  const metrics = await page.evaluate(() => ({ cls: window.__sdkCls,
    loadMs: performance.getEntriesByType('navigation')[0].loadEventEnd }));
  await page.addScriptTag({ content: axe });
  const violations = await page.evaluate(async () => (await window.axe.run()).violations
    .filter(entry => ['serious', 'critical'].includes(entry.impact)).map(entry => ({ id: entry.id, count: entry.nodes.length })));
  check('graph layout shift <= 0.1', metrics.cls <= 0.1, metrics.cls);
  check('graph load time <= 2000 ms', metrics.loadMs <= 2000, metrics.loadMs);
  check('graph zero serious/critical accessibility violations', !violations.length, violations);
  check('graph zero console errors', !errors.length, errors);
} finally {
  fs.writeFileSync(path.join(run, 'console-graph-last-page.txt'), await page.locator('body').innerText().catch(() => 'page unavailable'));
  await page.screenshot({ path: path.join(run, 'console-graph-last-page.png'), fullPage: true }).catch(() => {});
  await browser.close();
}
fs.writeFileSync(path.join(run, 'console-graph-result.json'), JSON.stringify({
  result: checks.every(check => check.passed) ? 'checks passed' : 'BROKEN', checks: checks.length,
}, null, 2));
process.exit(checks.every(check => check.passed) ? 0 : 1);
