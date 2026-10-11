import { kvs } from '@forge/kvs';
import { remaining, RateLimited } from './jira';

export const REFRESH_WORK = 'scope-refresh-work';

// Every writer and refresher runs behind the same installation-wide queue concurrency key.
// A transaction commits the mutation and its refresh obligation together; resetting the cursor
// also invalidates any cached page from an earlier, unfinished refresh.
export async function commitSprintWrite(sprintId, transaction) {
  if (remaining() < 3000) throw new RateLimited(1);
  const pending = await kvs.get(REFRESH_WORK);
  const sprintIds = [...new Set([...(pending?.sprintIds ?? []), String(sprintId)])];
  await transaction.set(REFRESH_WORK, { sprintIds, startAt: 0 }).execute();
}
