// Drive #662's image kinds through the built console, retaining loaded rows and screenshots.
import { createRequire } from 'node:module';
import fs from 'node:fs';
import path from 'node:path';

const dir = path.resolve(process.argv[2]);
const repository = process.argv[3];
const direct = repository.endsWith('/direct');
const builder = repository.endsWith('/app');
const require = createRequire(path.join(process.env.PROVE_NODE, 'node_modules', 'x.js'));
const { chromium } = require('playwright-core');
const axe = fs.readFileSync(require.resolve('axe-core/axe.min.js'),'utf8');
const url = fs.readFileSync(path.join(dir, 'url'), 'utf8');
const browser = await chromium.launch({ executablePath: process.env.PROVE_CHROMIUM });
try {
  const context = await browser.newContext({ bypassCSP: true });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', e => errors.push(e.message));
  page.on('console', m => { if (m.type() === 'error') errors.push(m.text()); });
  await page.goto(url + '/graph', { waitUntil: 'networkidle' });
  await page.locator('form input[required]').fill(repository);
  await page.locator('form button[type=submit]').click();
  const picker = page.locator('.graph-view-picker select');
  await picker.waitFor();
  for (const view of (direct ? ['executions'] : builder ? ['contracts'] : ['contracts', 'links'])) {
    await picker.selectOption(view);
    await page.getByRole('region').filter({ hasText: direct ? 'services.noimage' : 'image::ghcr.io/parcel/worker' }).waitFor();
    const text = await page.locator('.graph-table').innerText();
    await page.addScriptTag({ content: axe });
    const violations = await page.evaluate(async () => (await window.axe.run()).violations
      .filter(v => v.impact === 'serious' || v.impact === 'critical').map(v => v.id));
    fs.writeFileSync(path.join(dir, `graph-${builder ? 'builder-' : ''}${view}-axe.json`), JSON.stringify(violations,null,2));
    fs.writeFileSync(path.join(dir, `graph-${builder ? 'builder-' : ''}${view}.txt`), text);
    await page.screenshot({ path: path.join(dir, `graph-${builder ? 'builder-' : ''}${view}.png`), fullPage: true });
    if (violations.length) throw new Error(view + ': accessibility violations ' + violations.join(', '));
    if (direct && (!text.includes('src') || !text.includes('exact') || !text.includes('services.worker'))) throw new Error('direct execution rows missing');
    if (!direct && !text.includes('image::ghcr.io/parcel/worker')) throw new Error(view + ': image contract missing');
    if (builder && !text.includes('.github/workflows/publish.yml#')) throw new Error('builder location missing');
    if (view === 'links' && (!text.includes('example.com/image-proof/app') || !text.includes('inferred') || !text.includes('exact'))) {
      throw new Error('links: builder or evaluated levels missing');
    }
  }
  fs.writeFileSync(path.join(dir, 'graph-errors.json'), JSON.stringify(errors, null, 2));
  if (errors.length) throw new Error('console page errors: ' + errors.join('; '));
  console.log('Image contracts and links, builder repository, exact and inferred levels rendered.');
} finally {
  await browser.close();
}
