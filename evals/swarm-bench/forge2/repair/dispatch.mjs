#!/usr/bin/env node
// Only UI clicks launch entrants. Read-only Electron methods observe their own session receipts.
const [command, ...args] = process.argv.slice(2);
const options = Object.fromEntries(args.filter((x) => x.includes('=')).map((x) => { const at = x.indexOf('='); return [x.slice(0, at), x.slice(at + 1)]; }));
if (!['inspect', 'result', 'configure', 'launch', 'cancel'].includes(command)) throw new Error('Use inspect, result, configure, launch or cancel.');
if (['configure', 'cancel'].includes(command) && !args.includes('--execute-mutations')) throw new Error('Configuration requires --execute-mutations after freeze.');
if (command === 'launch' && !args.includes('--execute-paid')) throw new Error('Launch requires --execute-paid after freeze.');
const targets = await (await fetch('http://127.0.0.1:9897/json')).json();
const target = targets.find((page) => page.type === 'page' && page.url.includes('/Applications/Goose%20Swarm.app/'));
if (!target) throw new Error('Installed Goose Swarm app page is absent on CDP9897.');
const ws = new WebSocket(target.webSocketDebuggerUrl);
const pending = new Map(); let seq = 0;
ws.addEventListener('message', (event) => { const msg = JSON.parse(event.data); if (pending.has(msg.id)) { pending.get(msg.id)(msg); pending.delete(msg.id); } });
await new Promise((resolve, reject) => { ws.addEventListener('open', resolve, { once: true }); ws.addEventListener('error', reject, { once: true }); });
const send = (method, params = {}) => new Promise((resolve) => { const id = ++seq; pending.set(id, resolve); ws.send(JSON.stringify({ id, method, params })); });
const evaluate = async (expression) => {
  const answer = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
  if (answer.error || answer.result?.exceptionDetails) throw new Error('Benchmark page evaluation failed.');
  return answer.result.result.value;
};
const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const until = async (predicate) => {
  const end = Date.now() + 60000; // local UI/IPC transport; never a deadline for model work
  while (Date.now() < end) { const value = await predicate(); if (value) return value; await wait(250); }
  throw new Error('Benchmark UI did not reach the requested form state.');
};
const key = async (value, code, virtual) => {
  for (const type of ['keyDown', 'keyUp']) await send('Input.dispatchKeyEvent', { type, key: value, code, windowsVirtualKeyCode: virtual });
};
const click = async (selector, text) => {
  const xy = await evaluate(`(() => { const all = [...document.querySelectorAll(${JSON.stringify(selector)})];
    const matches = all.filter(e => !e.disabled && ${text === undefined ? 'true' : `(e.innerText || '').trim() === ${JSON.stringify(text)}`});
    if(matches.length !== 1) return null; const e = matches[0]; e.scrollIntoView({block:'center'}); const r=e.getBoundingClientRect(); return [r.x+r.width/2,r.y+r.height/2]; })()`);
  if (!xy) throw new Error('Expected one enabled Benchmark control: ' + (text ?? selector));
  for (const type of ['mouseMoved', 'mousePressed', 'mouseReleased']) await send('Input.dispatchMouseEvent', { type, x: xy[0], y: xy[1], button: 'left', clickCount: 1 });
};
const input = async (label, text) => {
  const selector = `input[aria-label=${JSON.stringify(label)}]`;
  await click(selector);
  await send('Input.dispatchKeyEvent', { type: 'keyDown', key: 'a', code: 'KeyA', windowsVirtualKeyCode: 65, modifiers: 4 });
  await send('Input.dispatchKeyEvent', { type: 'keyUp', key: 'a', code: 'KeyA', windowsVirtualKeyCode: 65, modifiers: 4 });
  await key('Backspace', 'Backspace', 8);
  if (text) await send('Input.insertText', { text });
  await key('Tab', 'Tab', 9);
};
const sessions = () => evaluate(`window.electron.benchmarkSessions().then(r => r.sessions.map(s => ({runId:s.runId,dataDir:s.dataDir,startedAt:s.startedAt,scorerVersion:s.scorerVersion,outcome:s.outcome,score:s.score,publishable:s.publishable,retryScoring:s.retryScoring})))`);
const form = () => evaluate(`({url:location.href,provider:document.querySelector('[aria-label="Model provider"]')?.innerText,
  host:document.querySelector('[aria-label="OpenRouter host"]')?.innerText,
  policy:document.querySelector('[data-testid="forge-run-policy"]')?.innerText,
  fieldsLoading:!!document.querySelector('[data-testid="model-custom-fields-loading"]'),
  fieldsUnlisted:!!document.querySelector('[data-testid="model-custom-fields-unlisted"]'),
  fieldsSaving:(document.querySelector('[data-testid="model-custom-fields"]')?.innerText||'').includes('Saving…'),
  fields:[...document.querySelectorAll('[data-testid^="model-field-row-"]')].map(e=>({id:e.dataset.testid.replace('model-field-row-',''),pinned:!!e.querySelector('[data-testid$="-pinned"]'),number:e.querySelector('input')?.getAttribute('aria-label'),trigger:e.querySelector('[data-testid$="-trigger"]')?.dataset.testid})),
  inputs:[...document.querySelectorAll('input[aria-label]')].filter(e=>e.getAttribute('aria-label') !== 'Filter projects by name and sessions by title').map(e=>({label:e.getAttribute('aria-label'),value:e.value,disabled:e.disabled})),
  alerts:[...document.querySelectorAll('[role="alert"]')].map(e=>e.innerText),
  running:[...document.querySelectorAll('button')].some(e=>(e.innerText||'').trim()==='Cancel run')})`);
try {
  if (command === 'inspect') { console.log(JSON.stringify({ form: await form(), sessions: await sessions() })); }
  else if (command === 'result') {
    if (!options.run) throw new Error('Original run ID is required.');
    const r = await evaluate(`window.electron.benchmarkRunResult(${JSON.stringify(options.run)})`);
    console.log(JSON.stringify(r ? {runId:r.runId,workdir:r.workdir,modelId:r.modelId,verdict:r.verdict,modelBilledCost:r.modelBilledCost} : {}));
  }
  else if (command === 'cancel') {
    if (!options.run) throw new Error('Original run ID is required for cancellation.');
    const active = (await sessions()).filter(s => s.outcome === 'running');
    if (active.length === 0) { console.log(JSON.stringify({runId:options.run,status:'already-ended'})); }
    else {
      if (active.length !== 1 || active[0].runId !== options.run) throw new Error('Active session is not the owned original launch; cancellation refused.');
      if (!(await form()).running) throw new Error('Owned native Cancel control is not visible; cancellation unproven.');
      await click('button', 'Cancel run');
      await until(()=>evaluate(`!!document.querySelector('[role="dialog"]') && [...document.querySelectorAll('[role="dialog"] button')].some(e=>e.innerText.trim()==='Cancel the run')`));
      // Confirm identity again at the destructive control boundary.
      const current = (await sessions()).filter(s=>s.outcome==='running');
      if (current.length !== 1 || current[0].runId !== options.run) throw new Error('Active ownership changed before cancellation.');
      await click('[role="dialog"] button', 'Cancel the run');
      await until(async()=>!(await sessions()).some(s=>s.runId===options.run && s.outcome==='running'));
      console.log(JSON.stringify({runId:options.run,status:'cancelled',surface:'native Benchmark Cancel dialog'}));
    }
  }
  else if (command === 'configure') {
    if (!options.model || !Number.isFinite(Number(options.wallet)) || Number(options.wallet) <= 0) throw new Error('Exact model and positive wallet amount are required.');
    if ((await sessions()).some((session) => session.outcome === 'running')) throw new Error('An entrant/scorer is already running.');
    await evaluate(`location.hash='#/benchmark?era=forge-2.0'; true`);
    await until(async () => (await form()).policy?.includes('300'));
    const current = await form();
    if (current.provider !== 'OpenRouter') {
      await click('[aria-label="Model provider"]');
      await until(() => evaluate(`!![...document.querySelectorAll('[role="menuitem"]')].find(e=>e.innerText.trim()==='OpenRouter')`));
      await click('[role="menuitem"]', 'OpenRouter');
    }
    await input('Model ID', options.model);
    await until(async () => { const f=await form(); return f.host && !f.fieldsLoading && !f.inputs.find(e=>e.label==='Stop a run at (US dollars, OpenRouter)')?.disabled; });
    if ((await form()).fieldsUnlisted) throw new Error('Exact requested route is absent from the native catalog.');
    if ((await form()).host !== 'Any host (OpenRouter routes)') {
      await click('[aria-label="OpenRouter host"]');
      await until(() => evaluate(`!![...document.querySelectorAll('[role="option"],[role="menuitem"]')].find(e=>e.innerText.trim()==='Any host (OpenRouter routes)')`));
      await click('[role="option"],[role="menuitem"]', 'Any host (OpenRouter routes)');
    }
    if (await evaluate(`document.querySelector('[data-testid="model-custom-fields-sampling"]')?.dataset.state === 'closed'`))
      await click('[data-testid="model-custom-fields-sampling"] button[aria-expanded]');
    for (const field of (await form()).fields.filter(e=>!e.pinned)) {
      if (field.number) await input(field.number, '');
      else {
        const defaultSelector = `[data-testid="model-field-${field.id}-default"]`;
        if (field.trigger) await click(`[data-testid="${field.trigger}"]`);
        await until(()=>evaluate(`!!document.querySelector(${JSON.stringify(defaultSelector)})`));
        await click(defaultSelector);
      }
      await until(async()=>!(await form()).fieldsSaving);
    }
    await input('Stop a run at (US dollars, OpenRouter)', options.wallet);
    await until(async () => { const value = await form(); return value.inputs.find((entry) => entry.label === 'Model ID')?.value === options.model
      && value.inputs.find((entry) => entry.label === 'Stop a run at (US dollars, OpenRouter)')?.value === options.wallet && !value.alerts.length && !value.fieldsLoading && !value.fieldsSaving; });
    console.log(JSON.stringify(await form()));
  } else {
    const before = await sessions(), value = await form();
    if (before.some((session) => session.outcome === 'running') || value.running) throw new Error('One entrant at a time.');
    if (value.provider !== 'OpenRouter' || value.host !== 'Any host (OpenRouter routes)' || !value.policy?.includes('300') || !value.policy?.includes('medium')) throw new Error('Matched Forge2 settings are not present.');
    if (value.inputs.find((entry) => entry.label === 'Model ID')?.value !== options.model
        || value.inputs.find((entry) => entry.label === 'Stop a run at (US dollars, OpenRouter)')?.value !== options.wallet
        || value.alerts.length || value.fieldsLoading || value.fieldsSaving || value.fieldsUnlisted) throw new Error('Model/wallet form differs from the authorized unit.');
    const known = new Set(before.map((session) => session.runId));
    await click('button', 'Run benchmark');
    const started = await until(async () => { const found = (await sessions()).filter((session) => !known.has(session.runId)); return found.length === 1 ? found[0] : null; });
    console.log(JSON.stringify({ ...started, launchSurface: 'installed Benchmark view', settings: value }));
  }
} finally { ws.close(); }
