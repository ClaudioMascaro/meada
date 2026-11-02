// The console's pages in Chromium, against the budgets the maintainer set (2026-11-02): layout
// shift <= 0.1, load <= 2 s on loopback, zero console errors, zero axe violations of impact
// serious or critical. Then one playground query, compared with the head's MCP stdio answer.
//
//   node console-pages.mjs <RUN>
//
// Needs playwright-core and axe-core under PROVE_NODE (references/console.md installs them) and a
// Chromium: PROVE_CHROMIUM, or the newest under PLAYWRIGHT_BROWSERS_PATH / ~/.cache/ms-playwright.
import { createRequire } from 'node:module';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const run = path.resolve(process.argv[2]);
const require = createRequire(path.join(process.env.PROVE_NODE, 'node_modules', 'x.js'));
const { chromium } = require('playwright-core');
const axe = fs.readFileSync(require.resolve('axe-core/axe.min.js'), 'utf8');
const dir = path.join(run, 'console');
const url = fs.readFileSync(path.join(dir, 'url'), 'utf8');
const shots = path.join(dir, 'shots');
fs.mkdirSync(shots, { recursive: true });
const BUDGET = { cls: 0.1, loadMs: 2000 };

function chrome() {
  if (process.env.PROVE_CHROMIUM) return process.env.PROVE_CHROMIUM;
  const root = process.env.PLAYWRIGHT_BROWSERS_PATH || path.join(os.homedir(), '.cache/ms-playwright');
  const found = (fs.existsSync(root) ? fs.readdirSync(root) : []).filter(d => /^chromium-\d+$/.test(d))
    .sort((a, b) => Number(b.split('-')[1]) - Number(a.split('-')[1]))
    .map(d => ['chrome-linux64/chrome', 'chrome-linux/chrome'].map(p => path.join(root, d, p)).find(fs.existsSync))
    .filter(Boolean);
  if (!found.length) throw new Error(`no Chromium under ${root}: set PROVE_CHROMIUM or see references/console.md`);
  return found[0];
}

const browser = await chromium.launch({ executablePath: chrome() });
const context = await browser.newContext();
await context.addInitScript(() => {
  window.__cls = 0;
  new PerformanceObserver(list => {
    for (const e of list.getEntries()) if (!e.hadRecentInput) window.__cls += e.value;
  }).observe({ type: 'layout-shift', buffered: true });
});
const page = await context.newPage();
const errors = [];
page.on('console', m => { if (m.type() === 'error') errors.push(`${page.url()}: ${m.text()}`); });
page.on('pageerror', e => errors.push(`${page.url()}: uncaught ${e.message}`));

const lines = [];
let failures = 0;
// The admin password is derived from the run's signing key, as console.py's admin_password does.
const key = path.join(dir, 'private.pem');
if (fs.existsSync(key)) {
  const password = crypto.createHmac('sha256', fs.readFileSync(key)).update('prove-meada admin').digest().subarray(0, 18).toString('base64url');
  // The sign-in page: the first text input is the user name, the password input the password.
  await page.goto(url + '/', { waitUntil: 'networkidle' });
  await page.locator('input:not([type=password])').first().fill('admin');
  await page.locator('input[type=password]').first().fill(password);
  await page.locator('button[type=submit]').first().click();
  await page.waitForLoadState('networkidle');
  await page.screenshot({ path: path.join(shots, 'signed-in.png') });
  lines.push('signed in as admin through the page');
}
for (const route of ['/config', '/session', '/manifest', '/playground', '/graph']) {
  const before = errors.length;
  await page.goto(url + route, { waitUntil: 'networkidle' });
  const heading = await page.locator('h1').first().textContent({ timeout: 5000 }).catch(() => null);
  const { cls, loadMs } = await page.evaluate(() => {
    const nav = performance.getEntriesByType('navigation')[0];
    return { cls: window.__cls, loadMs: nav ? nav.loadEventEnd : -1 };
  });
  await page.addScriptTag({ content: axe });
  const violations = await page.evaluate(async () => (await window.axe.run()).violations
    .filter(v => v.impact === 'serious' || v.impact === 'critical').map(v => `${v.id} (${v.nodes.length})`));
  await page.screenshot({ path: path.join(shots, route.slice(1) + '.png'), fullPage: true });
  const bad = !heading || cls > BUDGET.cls || loadMs > BUDGET.loadMs || violations.length || errors.length > before;
  failures += bad ? 1 : 0;
  lines.push(`${route}: h1 ${JSON.stringify(heading)}, CLS ${cls.toFixed(3)} (<= ${BUDGET.cls}), load ${Math.round(loadMs)} ms ` +
    `(<= ${BUDGET.loadMs}), axe ${violations.length ? violations.join(', ') : 'none'}, console errors ${errors.length - before}` +
    (bad ? '  FAIL' : ''));
}

// One playground query: the repository and value inputs are the form's required ones, in order.
const [name] = fs.readdirSync(path.join(run, 'head')).sort();
const rec = JSON.parse(fs.readFileSync(path.join(run, 'head', name, 'record.json'), 'utf8'));
const identity = JSON.parse(fs.readFileSync(path.join(run, 'head', name, 'export1.json'), 'utf8')).repository;
const i = rec.queries.findIndex(q => q.tool === 'context');
const q = rec.queries[i];
await page.goto(url + '/playground', { waitUntil: 'networkidle' });
const form = page.locator('form').first();
await form.locator('input[required]').nth(0).fill(identity);
await form.locator('select').first().selectOption(q.tool);
await form.locator('input[required]').nth(1).fill(q.arguments.symbol);
await form.locator('button[type=submit]').click();
const answer = await page.locator('pre[role=region]').textContent({ timeout: 30000 }).catch(() => null);
await page.screenshot({ path: path.join(shots, 'playground-answer.png'), fullPage: true });
const same = answer === rec.answers[i];
failures += same ? 0 : 1;
lines.push(`playground context ${q.arguments.symbol} on ${identity}: ${same ? 'byte-equal to' : 'DIFFERS from'} MCP stdio`);
fs.writeFileSync(path.join(dir, 'playground-answer.txt'), answer ?? '');
lines.push(...errors.map(e => `console error: ${e}`));
await browser.close();

const verdict = failures ? `differences found: ${failures}` : 'checks passed';
fs.writeFileSync(path.join(dir, 'pages.md'), `# console-pages\n\n${verdict}\n\n${lines.map(l => `- ${l}`).join('\n')}\n`);
console.log([verdict, ...lines].join('\n'));
process.exit(failures ? 1 : 0);
