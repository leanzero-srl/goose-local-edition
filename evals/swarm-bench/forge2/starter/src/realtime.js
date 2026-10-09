import { publishGlobal } from '@forge/realtime';

// One global channel. The realtime docs: "The publish API is only supported for functions invoked from
// the app frontend. This is not currently available for async events and web triggers. Use publishGlobal
// instead". Tokens are signed in resolvers only, so the async publishers (queue consumer, scheduled run)
// publish without one and the widget subscribes without one. Payloads carry sprint ids only — nothing a
// viewer could not see — and subscribers re-read the numbers through their own resolver.
export const CHANNEL = 'scope-ledger-updates';

// A failed announcement must not undo or retry the ledger write it follows: it is logged loudly and the
// widget catches up on its next load.
export async function announce(sprintIds) {
  const ids = [...new Set(sprintIds.map(String))].sort();
  if (!ids.length) return;
  try {
    const res = await publishGlobal(CHANNEL, { sprintIds: ids });
    if (res.errors) console.error(`realtime: publish failed for sprints ${ids.join(',')}: ${JSON.stringify(res.errors)}`);
  } catch (e) {
    console.error(`realtime: publish threw for sprints ${ids.join(',')}: ${e.message}`);
  }
}
