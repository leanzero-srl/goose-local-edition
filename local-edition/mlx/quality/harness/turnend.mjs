// turnend.mjs — Q-527: a turn whose reply is the app's own error notice ended in an ERROR, not `done`.
// RU-2026-10-31-0 (Gabee, qwen3.5-9b) graded three turns `done` while every reply was "Ran into this error:
// Execution error: swarm chat: no `swarm` block in config.yaml …": r1's FAIL regex reads the message's innerText,
// and the desktop never shows those words — GooseMessage.tsx swaps the agent's "Ran into this error: <detail>"
// for a notice component that draws a headline in plain words (ProviderErrorNotice: "goose could not send this
// request", the detail behind Details). So the turn end reads the notice ELEMENTS, by their data-testids:
//   provider-error-notice  ProviderErrorNotice.tsx (role=alert, data-class = the error's class)
//   no-node-notice         NoNodeNotice.tsx (no node can serve the turn)
//   no-node-split-stopped  NoNodeNotice.tsx SplitStopped (the split stopped mid-turn)
//   link-drop-notice       LinkDropNotice.tsx (the Link peer dropped the turn)
//   network-cut-raw        SplitCutNotice.tsx (the network cut a split turn)
//   tool-bounds-notice     ToolBoundsNotice.tsx (a failure kind of its own in GooseMessage)
// plus any other [role=alert] inside an assistant message, so a new notice component is not missed silently.
export const NOTICE_TESTIDS = ['provider-error-notice', 'no-node-notice', 'no-node-split-stopped', 'link-drop-notice', 'network-cut-raw', 'tool-bounds-notice'];

// The notices drawn in the assistant messages after the first n0 (this turn's), as a user sees them.
export const readNotices = (p, n0) => p.evaluate(({ n0, ids }) => {
  const sel = [...ids.map((id) => `[data-testid="${id}"]`), '[role=alert]'].join(', ');
  const out = [];
  for (const m of [...document.querySelectorAll('.goose-message')].filter((m) => m.offsetParent).slice(n0)) {
    const seen = new Set();
    for (const el of m.querySelectorAll(sel)) {
      const top = el.closest(ids.map((id) => `[data-testid="${id}"]`).join(', ')) ?? el;
      if (seen.has(top)) continue; seen.add(top);
      out.push({ testid: top.dataset.testid ?? `role=${top.getAttribute('role')}`, cls: top.dataset.class ?? '', text: top.innerText.replace(/\s+/g, ' ').trim() });
    }
  }
  return out;
}, { n0, ids: NOTICE_TESTIDS });

// Pure: the turn's final verdict. A turn that would be `done` but drew an error notice is `error`; a hang stays
// a hang (it ended for another reason). `notice` = the turns.tsv notice cell for that case.
export function turnEndVerdict(ended, notices) {
  if (ended !== 'done' || !notices?.length) return { ended, notice: '' };
  const notice = notices.map((n) => `[${n.testid}${n.cls ? `:${n.cls}` : ''}] ${n.text}`).join(' | ').replace(/\t/g, ' ').slice(0, 400);
  return { ended: 'error', notice };
}
