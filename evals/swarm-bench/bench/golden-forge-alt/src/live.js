import { publishGlobal } from '@forge/realtime';

// One global channel per sprint; the payload names the sprint and nothing else. The widget
// subscribes to the channels of the sprints it shows and re-reads its numbers on a message.
// publishGlobal, because publish() is not available to async-event functions.
export const sprintChannel = (sprintId) => `scope-ledger-sprint-${sprintId}`;

export async function announce(sprintIds) {
  const failures = [];
  for (const id of new Set([...sprintIds].map(String))) {
    try {
      const r = await publishGlobal(sprintChannel(id), { sprintId: id });
      if (r && r.errors) failures.push(`${sprintChannel(id)}: ${JSON.stringify(r.errors)}`);
    } catch (err) {
      failures.push(`${sprintChannel(id)}: ${err && err.message ? err.message : String(err)}`);
    }
  }
  // The rows are already written; a lost announcement must not redeliver the write, so it is
  // reported in the function log instead of thrown.
  if (failures.length) console.error(`realtime publish failed for ${failures.length} channel(s): ${failures.join('; ')}`);
}
