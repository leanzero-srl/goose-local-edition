'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { FORGE, ensureKit, scratch } = require('./helpers.cjs');
const { createSite } = require(path.join(FORGE, 'site/site.cjs'));
const { TOKEN_TTL_S } = require(path.join(FORGE, 'site/realtime.cjs'));
const SEED = '0123456789abcdef';
const SOURCE = `import Resolver from '@forge/resolver';
import api, { route } from '@forge/api';
import { chat, list } from '@forge/llm';
import { publishGlobal, signRealtimeToken } from '@forge/realtime';
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
const resolver = new Resolver();
async function llm(payload) {
  const model = (await list()).models[0].model;
  const out = [];
  for (let i=0; i<(payload.waits.length+1); i++) {
    try { const answer = await chat({model, messages:[{role:'user',content:'hello'}]}); out.push({status:200, answer}); }
    catch (e) { out.push({status:e.status}); }
    if (i<payload.waits.length) {
      if (payload.waits[i]>0) await sleep(payload.waits[i]);
      if (payload.jira) await api.asApp().requestJira(route\`/rest/api/3/myself\`);
    }
  }
  return out;
}
resolver.define('llm', ({payload})=>llm(payload));
resolver.define('rt', async () => {
  await sleep(1000);
  const {token} = await signRealtimeToken('clock', {sprint:7});
  await sleep(2000);
  return {token, publication:await publishGlobal('clock', {value:'published'}, {token})};
});
export const handler = resolver.getDefinitions();
export const long = event => llm(event.body);
`;
async function rig({ remote = false, ahead = false } = {}) {
  const kit = ensureKit();
  const { createEmulator } = require(path.join(kit.kit_dir, 'lib/emulator.cjs'));
  const appDir = path.join(scratch('platform-clock'), 'app');
  fs.mkdirSync(path.join(appDir, 'src'), {recursive:true});
  fs.mkdirSync(path.join(appDir, 'static/view'), {recursive:true});
  fs.writeFileSync(path.join(appDir, 'static/view/index.html'), '<!doctype html><html><body></body></html>');
  fs.writeFileSync(path.join(appDir, 'src/index.js'), SOURCE);
  fs.writeFileSync(path.join(appDir, 'manifest.yml'), `modules:
  jira:sprintAction:
    - key: clock
      title: Clock
      resource: view
      resolver:
        function: resolver
  llm:
    - key: llm
      model:
        - claude
  consumer:
    - key: long-clock
      queue: long-clock
      function: long
  function:
    - key: resolver
      handler: index.handler
    - key: long
      handler: index.long
      timeoutSeconds: 90
resources:
  - key: view
    path: static/view
permissions:
  scopes:
    - read:jira-work
app:
  id: ari:cloud:ecosystem::app/00000000-0000-0000-0000-000000000000
  runtime:
    name: nodejs22.x
`);
  const site = await createSite({seed:SEED});
  if (ahead) {
    const call = site.control.llm;
    let once = true;
    site.control.llm = req => {
      if (req.method==='POST' && once) { once=false; site.state.advanceTo(Date.parse(site.pack.now)+2*86_400_000); }
      return call(req);
    };
  }
  const emu = await createEmulator({appDir, kitDir:kit.kit_dir, site:remote?{url:site.url,adminUrl:site.adminUrl}:site, runtime:'wrapper'});
  return {site,emu,kit,close:async()=>{await emu.close();await site.stop();fs.rmSync(path.dirname(appDir),{recursive:true,force:true});}};
}
const callsOf = emu => emu.log.filter(e=>e.service==='llm' && e.op==='chat');
async function assertTimes(emu) {
  const proxy = emu.log.filter(e=>e.service==='llm');
  const logged = (await emu.llm.log()).entries;
  assert.equal(proxy.length,logged.length);
  for (let i=0;i<proxy.length;i++) {
    assert.equal(Date.parse(logged[i].t),Date.parse(proxy[i].t_virtual));
    assert.equal(logged[i].completed_ms,Date.parse(proxy[i].t_virtual)+proxy[i].vcostMs);
  }
}
function idleGaps(calls) {
  return calls.slice(1).map((c,i)=>Date.parse(c.t_virtual)-(Date.parse(calls[i].t_virtual)+calls[i].vcostMs));
}
for (const remote of [false,true]) {
  test(`actual SDK pure awaited retries preserve send/completion and positive idle (${remote?'HTTP dev':'local scoring'} site)`,async()=>{
    const r=await rig({remote});
    try {
      await r.emu.llm.phase('awaited',['ratelimited']);
      const out=await r.emu.invokeResolver('clock','llm',{waits:[1000,2000]},{extension:{}},r.site.pack.viewer);
      assert.equal(out.ok,true,JSON.stringify(out.error));
      assert.deepEqual(out.result.map(x=>x.status),[429,429,429]);
      await assertTimes(r.emu);
      assert.deepEqual(idleGaps(callsOf(r.emu)),[1000,2000]);
      assert.equal(r.site.state.now(),Date.parse(out.t1));
    } finally {await r.close();}
  });
}
test('actual SDK immediate retries have rising sends but zero idle and no Retry-After',async()=>{
  const r=await rig();
  try {
    await r.emu.llm.phase('immediate',['ratelimited']);
    const out=await r.emu.invokeResolver('clock','llm',{waits:[0,0]},{extension:{}},r.site.pack.viewer);
    assert.equal(out.ok,true);
    assert.deepEqual(out.result.map(x=>x.status),[429,429,429]);
    await assertTimes(r.emu);
    assert.deepEqual(idleGaps(callsOf(r.emu)),[0,0]);
    assert.ok(callsOf(r.emu).every(c=>c.vcostMs>0 && c.retryAfter===undefined));
  } finally {await r.close();}
});
test('actual SDK intervening Jira reads no longer determine LLM observation timestamps',async()=>{
  const r=await rig();
  try {
    await r.emu.llm.phase('jira-control',['ratelimited']);
    const out=await r.emu.invokeResolver('clock','llm',{waits:[1000,2000],jira:true},{extension:{}},r.site.pack.viewer);
    assert.equal(out.ok,true);
    await assertTimes(r.emu);
    assert.deepEqual(idleGaps(callsOf(r.emu)),[1120,2120]);
    assert.equal(r.emu.log.filter(c=>c.service==='jira').length,2);
  } finally {await r.close();}
});
test('actual SDK exact invocation times survive a concurrently advanced global clock',async()=>{
  const r=await rig({ahead:true});
  try {
    await r.emu.llm.phase('ahead',['ratelimited']);
    const out=await r.emu.invokeResolver('clock','llm',{waits:[1000,2000]},{extension:{}},r.site.pack.viewer);
    assert.equal(out.ok,true);
    assert.deepEqual(out.result.map(x=>x.status),[429,429,429]);
    await assertTimes(r.emu);
    assert.deepEqual(idleGaps(callsOf(r.emu)),[1000,2000]);
    assert.ok(r.site.state.now()>Date.parse(out.t1)+86_400_000);
    const log=await r.emu.llm.log();
    assert.equal(Date.parse(log.state.rateLimitedUntil),Date.parse(log.entries.find(e=>e.op==='chat').t)+60_000);
  } finally {await r.close();}
});
test('actual SDK rate window closes at the request instant without an intervening Jira read',async()=>{
  const r=await rig();
  try {
    await r.emu.llm.phase('window',['ratelimited','clean']);
    const out=await r.emu.invoke('long',{moduleKey:'long-clock',event:{body:{waits:[59_799,1]}}});
    assert.equal(out.ok,true,JSON.stringify(out.error));
    assert.deepEqual(callsOf(r.emu).map(x=>x.status),[429,429,200]);
    await assertTimes(r.emu);
    const calls=callsOf(r.emu);const start=Date.parse(calls[0].t_virtual);
    assert.equal(Date.parse(calls[1].t_virtual)-start,59_999);
    assert.equal(Date.parse(calls[2].t_virtual)-start,60_200);
  } finally {await r.close();}
});
test('actual SDK Realtime sign/publish uses invocation times for expiry, events and deliveries',async()=>{
  const r=await rig();
  try {
    const before=Date.parse(r.site.pack.now);
    const subscriptionToken=await r.site.control.rtsign({channelName:'clock',claims:{sprint:7},atMs:before});
    const sub=await r.site.control.rtsubscribe({channelName:'clock',isGlobal:true,token:subscriptionToken.jwt});
    assert.ok(sub.subscriptionId);
    const out=await r.emu.invokeResolver('clock','rt',{}, {extension:{}},r.site.pack.viewer);
    assert.equal(out.ok,true,JSON.stringify(out.error));
    const sign=r.emu.log.find(c=>c.service==='realtime' && c.op==='signRealtimeToken');
    const publish=r.emu.log.find(c=>c.service==='realtime' && c.op==='publishGlobal');
    const jwt=JSON.parse(Buffer.from(out.result.token.split('.')[1],'base64url'));
    assert.equal(jwt.exp,Math.floor(Date.parse(sign.t_virtual)/1000)+TOKEN_TTL_S);
    assert.equal(Date.parse(sign.t_virtual)-before,1000);
    assert.equal(Date.parse(publish.t_virtual)-before,3200);
    const events=(await r.emu.realtime.log()).events;
    assert.equal(events.length,1);
    assert.equal(events[0].ms,Date.parse(publish.t_virtual));
    assert.equal(Number(out.result.publication.eventTimestamp),Date.parse(publish.t_virtual));
    const deliveries=(await r.site.control.rtdeliveries({})).deliveries;
    assert.equal(deliveries.length,1);
    assert.equal(Date.parse(deliveries[0].at),Date.parse(publish.t_virtual));
  } finally {await r.close();}
});
