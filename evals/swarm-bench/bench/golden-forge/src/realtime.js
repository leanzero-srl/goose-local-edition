import { publishGlobal, signRealtimeToken } from '@forge/realtime';

// One global channel: the queue consumer and the scheduled run are async contexts, where the realtime
// docs allow only publishGlobal/subscribeGlobal, authorised by tokens signed with the same claims on
// both sides. Payloads carry sprint ids only — subscribers re-read the numbers through their resolver.
export const CHANNEL = 'scope-ledger-updates';
const CLAIMS = { ledger: 'scope-ledger' };

export async function subscribeToken() {
  const { token, expiresAt, errors } = await signRealtimeToken(CHANNEL, CLAIMS, ['subscribe']);
  if (!token) throw new Error(`realtime token refused: ${JSON.stringify(errors)}`);
  return { channel: CHANNEL, token, expiresAt };
}

// A failed announcement must not undo or retry the ledger write it follows: it is logged loudly and the
// widget catches up on its next load.
export async function announce(sprintIds) {
  const ids = [...new Set(sprintIds.map(String))].sort();
  if (!ids.length) return;
  try {
    await publishSprintIds(ids);
  } catch (e) {
    console.error(`realtime: publish threw for sprints ${ids.join(',')}: ${e.message}`);
  }
}

async function publishSprintIds(ids) {
  const signed = await signRealtimeToken(CHANNEL, CLAIMS, ['publish']);
  if (!signed.token) {
    console.error(`realtime: publish token refused for sprints ${ids.join(',')}: ${JSON.stringify(signed.errors)}`);
    return;
  }
  const res = await publishGlobal(CHANNEL, { sprintIds: ids }, { token: signed.token });
  if (res.errors) console.error(`realtime: publish failed for sprints ${ids.join(',')}: ${JSON.stringify(res.errors)}`);
}
