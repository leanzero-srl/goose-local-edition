// Throwaway ACP driver for the hermetic remote-single pass (lives in ui/sdk only so it can import
// the built SDK; deleted after the pass). Usage:
//   node .rs-drive.mjs <serveUrl> ext <method> '<json params>'
//   node .rs-drive.mjs <serveUrl> prompt '<text>'      (new session, streams, prints timing)
import { GooseClient } from './dist/index.js';

const [url, cmd, a, b] = process.argv.slice(2);
// goose serve's ACP needs its secret; the SDK stream has no header hook, so fetch carries it.
const secret = process.env.RS_SECRET;
const rawFetch = globalThis.fetch;
globalThis.fetch = (input, init = {}) => {
  const headers = new Headers(init.headers ?? {});
  if (secret) headers.set('X-Secret-Key', secret);
  return rawFetch(input, { ...init, headers });
};
const chunks = [];
let firstAt = null;
const client = new GooseClient(
  () => ({
    sessionUpdate: async (n) => {
      const u = n.update;
      if (u?.sessionUpdate === 'agent_message_chunk' && u.content?.type === 'text') {
        if (firstAt == null) firstAt = Date.now();
        chunks.push(u.content.text);
      }
    },
    requestPermission: async () => ({ outcome: { outcome: 'cancelled' } }),
  }),
  url
);
await client.initialize({ protocolVersion: 1, clientCapabilities: {} });
if (cmd === 'ext') {
  const out = await client.extMethod(a, JSON.parse(b ?? '{}'));
  console.log(JSON.stringify(out, null, 1));
} else if (cmd === 'prompt') {
  const cwd = process.env.RS_CWD ?? process.cwd();
  const s = await client.newSession({ cwd, mcpServers: [] });
  const t0 = Date.now();
  const r = await client.prompt({ sessionId: s.sessionId, prompt: [{ type: 'text', text: a }] });
  const t1 = Date.now();
  const text = chunks.join('');
  console.log(JSON.stringify({ stopReason: r.stopReason, sessionId: s.sessionId, ms_total: t1 - t0,
    ms_first_chunk: firstAt ? firstAt - t0 : null, chunks: chunks.length, chars: text.length,
    tail: text.slice(-400) }, null, 1));
}
process.exit(0);
