'use strict';
// The fenced child process a UI Kit bundle runs in (index.cjs spawns it under a deny-default sandbox: it reads only
// its own directory and the Node runtime, has no network and cannot start processes). JSON lines both ways:
//   stdin (parent -> child): {t:'start', filename, context, moduleKey, startTime} once, then {t:'cmd', id, name, args}
//                            and {t:'reply', id, ok, value, error} (the answer to one of the child's calls)
//   fd 3  (child -> parent): {t:'call', id, kind, args}, {t:'ready', state}, {t:'done', id, ok, value, error, state}
// Every 'ready' and 'done' carries the state as of that moment, so the parent's reads are exact as of the last command.
const fs = require('fs');
const path = require('path');
const readline = require('readline');
const { createHost } = require('./host.cjs');

const out = fs.createWriteStream(null, { fd: 3 });
const send = (m) => out.write(`${JSON.stringify(m)}\n`);
const calls = new Map();
let nextCall = 0;
let host = null;
let sentDocs = 0;

const call = (kind, args) => new Promise((resolve, reject) => {
  const id = ++nextCall;
  calls.set(id, { resolve, reject });
  send({ t: 'call', id, kind, args });
});

function state() {
  const s = { docsFrom: sentDocs, docs: host.docs.slice(sentDocs), log: host.log, invokes: host.invokes, flags: host.flags, errors: host.errors,
    console: host.console, harnessMissing: host.harnessMissing, typed: [...host.typed], now: host.now() };
  sentDocs = host.docs.length;
  return s;
}

// In this process every unhandled rejection is the app's (the host's own promises are all handled): a console error,
// as in a browser, never the end of the page.
process.on('unhandledRejection', (reason) => host?.recordError('unhandledRejection', reason));

const COMMANDS = {
  setValue: (label, value) => host.setValue(label, value),
  click: (label) => host.click(label),
  waitIdle: () => host.waitIdle(),
  flush: () => host.flush(),
  advance: (ms) => host.advance(ms),
};

readline.createInterface({ input: process.stdin }).on('line', async (line) => {
  const m = JSON.parse(line);
  if (m.t === 'start') {
    host = createHost({ code: fs.readFileSync(path.join(__dirname, 'ui.js'), 'utf8'), filename: m.filename, context: m.context, moduleKey: m.moduleKey, startTime: m.startTime, call });
    send({ t: 'ready', state: state() });
  } else if (m.t === 'reply') {
    const c = calls.get(m.id);
    calls.delete(m.id);
    if (m.ok) c.resolve(m.value);
    else c.reject(Object.assign(new Error(m.error.message), { errorType: m.error.errorType ?? null }));
  } else if (m.t === 'cmd') {
    try {
      if (!COMMANDS[m.name]) throw Object.assign(new Error(`unknown command '${m.name}'`), { code: 'HARNESS' });
      const value = await COMMANDS[m.name](...m.args);
      send({ t: 'done', id: m.id, ok: true, value, state: state() });
    } catch (e) {
      send({ t: 'done', id: m.id, ok: false, error: { code: e.code ?? 'HARNESS', message: String(e.message) }, state: state() });
    }
  }
}).on('close', () => { host?.close(); process.exit(0); });
