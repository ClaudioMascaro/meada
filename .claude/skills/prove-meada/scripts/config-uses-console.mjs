#!/usr/bin/env node
// Browser mechanics for config_uses.py: source-derived oracle precedes all app answers.
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
const require=createRequire(import.meta.url);
const nodeRoot=process.env.PROVE_NODE;
if (!nodeRoot) throw new Error('PROVE_NODE required (playwright-core installation)');
const { chromium }=require(path.join(nodeRoot,'node_modules/playwright-core'));
const run=process.env.CONFIG_USES_RUN;
const url=process.env.CONFIG_USES_URL;
const oracle=JSON.parse(fs.readFileSync(path.join(run,'../oracle.json'),'utf8'));
const expected=JSON.parse(fs.readFileSync(path.join(run,'api-export.json'),'utf8'));
const browser=await chromium.launch({executablePath:process.env.PROVE_CHROMIUM || '/usr/bin/chromium',args:['--no-sandbox']});
let failures=0;
const checks=[];
function check(name,ok,detail='') {checks.push({name,ok,detail});failures+=!ok;console.log(`${ok?'PASS':'FAIL'} ${name}`);}
try {
 const page=await browser.newPage({viewport:{width:1600,height:1100}});
 const errors=[];page.on('pageerror',e=>errors.push(String(e)));
 page.on('console',m=>{if(m.type()==='error')errors.push(m.text());});
 const infra=JSON.parse(fs.readFileSync(path.join(run,'api-infra-export.json'),'utf8'));
 for(const [name,data] of [['app',expected],['infra',infra]]) {
   await page.goto(url+'/graph',{waitUntil:'networkidle'});
   const form=page.locator('form.graph-form');
   await form.locator('input[required]').fill(data.repository);
   await form.locator('button[type=submit]').click();
   const select=page.locator('.graph-view-picker select');
   await select.waitFor();await select.selectOption('contracts');
   const body=page.locator('tbody');
   const contracts=await body.locator('tr').allTextContents();
   if(name==='app') for(const use of oracle.uses) {
     const row=contracts.find(t=>t.includes(use.contract));
     check(`contracts use ${use.at}`,!!row && row.includes(use.at),row || 'missing row');
   }
   const actualIds=await body.locator('tr').evaluateAll(rows=>rows.map(row=>row.querySelector('td')?.textContent?.trim() ?? ''));
   const expectedIds=[...new Set([...data.providers,...data.consumers,...(data.provisioners||[]),...(data.uses||[]),...(data.provisions||[])].map(r=>r.contract).filter(Boolean))].sort();
   const sameIds=(a,b)=>JSON.stringify([...a].sort())===JSON.stringify([...b].sort());
   check(name+' whole contract identity set/count matches export',sameIds(actualIds,expectedIds),{actualIds,expectedIds});
   fs.writeFileSync(path.join(run,name+'-graph-contract-identities.json'),JSON.stringify({actualIds,expectedIds},null,2));
   if(name==='infra') {
     // Mutate an unused provisioner row without changing the count. The app's export
     // intentionally cannot carry these unrelated infrastructure declarations.
     const wrongIds=[...actualIds];
     const idle=wrongIds.findIndex(id=>id.includes('::idle-'));
     if(idle>=0) wrongIds[idle]='channel::sqs::injected-wrong-identity';
     check('negative control rejects unused provisioner identity substitution',idle>=0&&!sameIds(wrongIds,expectedIds),{wrongIds,expectedIds});
     fs.writeFileSync(path.join(run,'graph-wrong-identity-control.json'),JSON.stringify({wrongIds,expectedIds},null,2));
   }
   await page.screenshot({path:path.join(run,name+'-graph-contracts.png'),fullPage:true});
   await select.selectOption('provisions');
   const provisions=await body.locator('tr').allTextContents();
   const cells=await body.locator('tr').evaluateAll(rows=>rows.map(row=>[...row.querySelectorAll('td')].map(cell=>cell.textContent?.trim() ?? '')));
   check(name+' whole provision count',provisions.length===(data.provisions||[]).length,provisions.length);
   for(const link of data.provisions||[]) {
     check(`${name} displayed provision fields ${link.provisioner} ${link.at||link.side}`,cells.some(c=>
       c[0]===link.contract && c[1]?.includes(link.side) && c[2]?.includes(link.provisioner) &&
       c[2]?.includes(`root ${link.root}`) && c[3]===link.level && c[4]===link.reason &&
       (!link.at || c.join(' ').includes(link.at))));
   }
   for(const link of oracle.provisions) {
     check(`${name} provision ${link.at} ${link.root}`,provisions.some(t=>t.includes(link.contract)&&t.includes(link.at)&&t.includes(link.provisioner)&&t.includes(link.level)));
   }
   await page.screenshot({path:path.join(run,name+'-graph-provisions.png'),fullPage:true});
 }
 await page.goto(url+'/playground',{waitUntil:'networkidle'});
 const query=page.locator('form').first();
 await query.locator('input[required]').nth(0).fill(expected.repository);
 await query.locator('select').first().selectOption('context');
 await query.locator('input[required]').nth(1).fill('channel::sqs::parcel');
 await query.locator('button[type=submit]').click();
 const text=await page.locator('pre[role=region]').textContent({timeout:30000});
 fs.writeFileSync(path.join(run,'playground-answer.txt'),text);
 for(const use of oracle.uses.filter(u=>u.contract==='channel::sqs::parcel')) check('playground source key '+use.at,text.includes(use.at));
 await page.screenshot({path:path.join(run,'playground.png'),fullPage:true});
 check('no browser errors',errors.length===0,errors);
} finally {
 await browser.close();
 fs.writeFileSync(path.join(run,'browser-checks.json'),JSON.stringify(checks,null,2));
}
process.exit(failures?1:0);
