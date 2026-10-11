'use strict';
const test=require('node:test');
const assert=require('node:assert/strict');
const {createSite}=require('./site.cjs');
const {TOKEN_TTL_S}=require('./realtime.cjs');
const SEED='0123456789abcdef';
const MODEL='claude-sonnet-4-6';
async function rig() {
  const site=await createSite({seed:SEED});
  const call=async(op,body={})=>{
    const res=await fetch(`${site.adminUrl}/${op}`,{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify(body)});
    return {status:res.status,body:await res.json()};
  };
  const llm=async(atMs,extra={})=>(await call('llm',{method:'POST',model:MODEL,body:{messages:[{role:'user',content:'hello'}]},
    ...(atMs===undefined?{}:{atMs,completedAtMs:atMs+200}),...extra})).body;
  return {site,call,llm,t:site.state.now()};
}
test('real site HTTP exact LLM rate window survives global clock ahead and expires exactly at the boundary',async()=>{
  const {site,call,llm,t}=await rig();
  try {
    await call('llmphase',{phase:'boundary',script:['ratelimited','clean']});
    site.state.advanceTo(t+2*86_400_000);
    for (const at of [t,t+59_999]) assert.equal((await llm(at)).status,429);
    assert.equal((await llm(t+60_000)).status,200);
    const {body:log}=await call('llmlog');
    assert.deepEqual(log.entries.map(e=>Date.parse(e.t)),[t,t+59_999,t+60_000]);
    assert.deepEqual(log.entries.map(e=>e.completed_ms),[t+200,t+60_199,t+60_200]);
    assert.equal(Date.parse(log.state.rateLimitedUntil),t+60_000);
    assert.equal(site.state.now(),t+2*86_400_000);
  } finally {await site.stop();}
});
test('real site HTTP LLM tokens belong to the exact request UTC day across midnight and a later global day',async()=>{
  const {site,call,llm,t}=await rig();
  try {
    await call('llmphase',{phase:'days',script:['clean']});
    const midnight=Date.parse(new Date(t+86_400_000).toISOString().slice(0,10)+'T00:00:00.000Z');
    site.state.advanceTo(midnight+2*86_400_000);
    for (const at of [midnight-1,midnight]) assert.equal((await llm(at)).status,200);
    const {body:log}=await call('llmlog');
    const days=log.state.tokensByDay;
    assert.deepEqual(Object.keys(days),[new Date(midnight-1).toISOString().slice(0,10),new Date(midnight).toISOString().slice(0,10)]);
    assert.deepEqual(Object.values(days).map(x=>x.calls),[1,1]);
    assert.equal(Object.values(days).reduce((n,x)=>n+x.total_tokens,0),log.entries.reduce((n,e)=>n+e.usage.total_tokens,0));
    assert.equal(site.state.now(),midnight+2*86_400_000);
  } finally {await site.stop();}
});
test('direct control LLM requests without timing retain site time and omit unknown completion',async()=>{
  const {site,call,llm,t}=await rig();
  try {
    await call('llmphase',{phase:'default',script:['clean']});
    site.state.advanceTo(t+12345);
    assert.equal((await llm()).status,200);
    const {body:log}=await call('llmlog');
    assert.equal(Date.parse(log.entries[0].t),site.state.now());
    assert.equal(Object.hasOwn(log.entries[0],'completed_ms'),false);
  } finally {await site.stop();}
});
test('private LLM metadata rejects invalid request or impossible completion instead of inventing time',async()=>{
  const {site,call,t}=await rig();
  try {
    for (const times of [{atMs:'invalid'},{atMs:t,completedAtMs:t-1},{atMs:t,completedAtMs:'invalid'}]) {
      const result=await call('llm',{method:'POST',model:MODEL,body:{messages:[]},...times});
      assert.equal(result.status,500);
      assert.match(result.body.error,/Platform/);
    }
    const {body:log}=await call('llmlog');
    assert.equal(log.entries.length,0);
  } finally {await site.stop();}
});
test('app LLM body cannot override the proxy-owned timing metadata',async()=>{
  const {site,call,llm,t}=await rig();
  try {
    await call('llmphase',{phase:'body-clock',script:['clean']});
    await llm(t,{body:{messages:[],atMs:t+86_400_000,completedAtMs:t+86_400_100}});
    const {body:log}=await call('llmlog');
    assert.equal(Date.parse(log.entries[0].t),t);
    assert.equal(log.entries[0].completed_ms,t+200);
  } finally {await site.stop();}
});
test('real site Realtime signs and validates at exact invocation time while global clock is later',async()=>{
  const {site,call,t}=await rig();
  try {
    const {body:signed}=await call('rtsign',{channelName:'clock',claims:{sprint:7},atMs:t});
    const expires=signed.expiresAt*1000;
    assert.equal(signed.expiresAt,Math.floor(t/1000)+TOKEN_TTL_S);
    await call('rtsubscribe',{channelName:'clock',isGlobal:true,token:signed.jwt});
    site.state.advanceTo(expires+86_400_000);
    const {body:before}=await call('rtpublish',{channelName:'clock',isGlobal:true,token:signed.jwt,payload:'{}',atMs:expires-1});
    assert.ok(before.eventId);
    assert.equal(Number(before.eventTimestamp),expires-1);
    const {body:at}=await call('rtpublish',{channelName:'clock',isGlobal:true,token:signed.jwt,payload:'{}',atMs:expires});
    assert.match(at.errors[0].message,/TOKEN_EXPIRED/);
    const {body:logs}=await call('rtlog');
    assert.deepEqual(logs.events.map(e=>e.ms),[expires-1,expires]);
    const {body:deliveries}=await call('rtdeliveries');
    assert.equal(Date.parse(deliveries.deliveries[0].at),expires-1);
    assert.equal(site.state.now(),expires+86_400_000);
  } finally {await site.stop();}
});
test('direct Realtime default timing, replay delivery and invalid supplied timestamps stay explicit',async()=>{
  const {site,call,t}=await rig();
  try {
    const {body:signed}=await call('rtsign',{channelName:'clock'});
    assert.equal(signed.expiresAt,Math.floor(t/1000)+TOKEN_TTL_S);
    await call('rtsubscribe',{channelName:'clock',isGlobal:true});
    await call('rtpublish',{channelName:'clock',isGlobal:true,payload:'{}'});
    site.state.advanceTo(t+5000);
    await call('rtsubscribe',{channelName:'clock',isGlobal:true,replaySeconds:10});
    const {body:log}=await call('rtlog');
    assert.equal(log.events[0].ms,t);
    const {body:deliveries}=await call('rtdeliveries');
    assert.deepEqual(deliveries.deliveries.map(d=>Date.parse(d.at)),[t,t+5000]);
    for (const op of ['rtsign','rtpublish']) {
      const invalid=await call(op,{channelName:'clock',isGlobal:true,payload:'{}',atMs:'invalid'});
      assert.equal(invalid.status,500);
    }
  } finally {await site.stop();}
});

test('delayed out-of-order calls retain both observed rate windows without creating a limit before its start',async()=>{
  const {site,call,llm,t}=await rig();
  try {
    await call('llmphase',{phase:'out-of-order',script:['ratelimited','clean','ratelimited','clean']});
    site.state.advanceTo(t+2*86_400_000);
    for (const [at,status] of [[t,429],[t+60_000,200],[t+120_000,429],[t+179_999,429],[t+59_999,429],[t-1,200]]) {
      assert.equal((await llm(at)).status,status,String(at));
    }
    const {body:log}=await call('llmlog');
    assert.equal(Date.parse(log.state.rateLimitedUntil),t+180_000);
    assert.equal(site.state.now(),t+2*86_400_000);
  } finally {await site.stop();}
});
test('delayed cross-midnight LLM usage preserves newer and older token buckets',async()=>{
  const {site,call,llm,t}=await rig();
  try {
    await call('llmphase',{phase:'out-of-order-days',script:['clean']});
    const midnight=Date.parse(new Date(t+86_400_000).toISOString().slice(0,10)+'T00:00:00.000Z');
    site.state.advanceTo(midnight+2*86_400_000);
    for (const at of [midnight+1,midnight-1,midnight+2,midnight-2]) await llm(at);
    const {body:log}=await call('llmlog');
    const days=log.state.tokensByDay;
    assert.equal(days[new Date(midnight).toISOString().slice(0,10)].calls,2);
    assert.equal(days[new Date(midnight-1).toISOString().slice(0,10)].calls,2);
    assert.equal(Object.values(days).reduce((n,x)=>n+x.total_tokens,0),log.entries.reduce((n,e)=>n+e.usage.total_tokens,0));
    assert.equal(site.state.now(),midnight+2*86_400_000);
  } finally {await site.stop();}
});
