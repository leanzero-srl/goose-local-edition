import api, { route } from '@forge/api';

export class RetryLater extends Error {
  constructor(seconds) {
    super(`Jira asked to retry after ${seconds}s`);
    this.seconds = seconds;
  }
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function retryAfterSeconds(res) {
  const raw = res.headers.get('retry-after');
  if (!raw) return 1;
  const n = Number(raw);
  if (Number.isFinite(n)) return Math.max(n, 0);
  const at = Date.parse(raw);
  return Number.isFinite(at) ? Math.max((at - Date.now()) / 1000, 0) : 1;
}

// One budget per invocation: a wait that would not fit is handed back to the caller
// (the consumer turns it into an InvocationError with the same retryAfter).
export function makeClient(principal, { waitBudgetSeconds }) {
  let waited = 0;
  async function send(r, init) {
    for (;;) {
      const res = await principal.requestJira(r, init);
      if (res.status !== 429) return res;
      const seconds = retryAfterSeconds(res);
      if (waited + seconds > waitBudgetSeconds) throw new RetryLater(seconds);
      waited += seconds;
      await sleep(Math.ceil(seconds * 1000) + 50);
    }
  }
  async function json(r, init) {
    const res = await send(r, init);
    if (res.status === 404) return null;
    if (!res.ok) throw new Error(`Jira ${res.status} on ${r.value || r}: ${(await res.text()).slice(0, 300)}`);
    return res.status === 204 ? {} : res.json();
  }
  return { send, json };
}

export const appClient = (opts) => makeClient(api.asApp(), opts);

export async function scrumBoards(jira) {
  const boards = [];
  for (let startAt = 0; ; ) {
    const page = await jira.json(route`/rest/agile/1.0/board?type=scrum&startAt=${startAt}&maxResults=50`);
    for (const b of page.values || []) boards.push({ id: String(b.id), name: b.name, projectId: b.location && b.location.projectId != null ? String(b.location.projectId) : null });
    if (page.isLast || !(page.values || []).length) break;
    startAt += page.values.length;
  }
  return boards;
}

export async function boardSprints(jira, boardId, state) {
  const sprints = [];
  for (let startAt = 0; ; ) {
    const page = await jira.json(route`/rest/agile/1.0/board/${boardId}/sprint?state=${state}&startAt=${startAt}&maxResults=50`);
    if (!page) break;
    sprints.push(...(page.values || []));
    if (page.isLast || !(page.values || []).length) break;
    startAt += page.values.length;
  }
  return sprints;
}

export async function boardEstimationField(jira, boardId) {
  const cfg = await jira.json(route`/rest/agile/1.0/board/${boardId}/configuration`);
  const est = cfg && cfg.estimation;
  return est && est.type === 'field' && est.field ? est.field.fieldId : null;
}

export const getSprint = (jira, sprintId) => jira.json(route`/rest/agile/1.0/sprint/${sprintId}`);

export async function sprintFieldId(jira) {
  const fields = await jira.json(route`/rest/api/3/field`);
  const f = fields.find((x) => x.schema && x.schema.custom === 'com.pyxis.greenhopper.jira:gh-sprint');
  if (!f) throw new Error('This site has no Sprint field (com.pyxis.greenhopper.jira:gh-sprint)');
  return f.id;
}

// expand=changelog embeds at most one page of histories; the rest is paged here.
async function completeChangelog(jira, issue) {
  const cl = issue.changelog || { histories: [], total: 0 };
  const histories = [...(cl.histories || [])];
  if ((cl.total || 0) > histories.length) {
    histories.length = 0;
    for (let startAt = 0; ; ) {
      const page = await jira.json(route`/rest/api/3/issue/${issue.id}/changelog?startAt=${startAt}&maxResults=100`);
      histories.push(...(page.values || []));
      if (page.isLast || !(page.values || []).length) break;
      startAt += page.values.length;
    }
  }
  return { ...issue, changelog: { histories } };
}

export async function issueWithChangelog(jira, issueIdOrKey, fields) {
  const issue = await jira.json(route`/rest/api/3/issue/${issueIdOrKey}?fields=${fields.join(',')}&expand=changelog`);
  return issue ? completeChangelog(jira, issue) : null;
}

export async function sprintIssues(jira, sprintId, fields) {
  const issues = [];
  for (let startAt = 0; ; ) {
    const page = await jira.json(route`/rest/software/1.0/sprint/${sprintId}/issue?fields=${fields.join(',')}&expand=changelog&startAt=${startAt}&maxResults=100`);
    if (!page) break;
    for (const i of page.issues || []) issues.push(await completeChangelog(jira, i));
    const n = (page.issues || []).length;
    if (!n || startAt + n >= (page.total || 0)) break;
    startAt += n;
  }
  return issues;
}

export async function searchIssueIds(jira, jql) {
  const ids = [];
  let token;
  do {
    const page = token
      ? await jira.json(route`/rest/api/3/search/jql?jql=${jql}&fields=created&maxResults=100&nextPageToken=${token}`)
      : await jira.json(route`/rest/api/3/search/jql?jql=${jql}&fields=created&maxResults=100`);
    for (const i of page.issues || []) ids.push(String(i.id));
    token = page.isLast ? undefined : page.nextPageToken;
  } while (token);
  return ids;
}

// asApp + an explicit check for the person: BROWSE_PROJECTS on each issue id.
export async function browsableIssueIds(jira, accountId, issueIds) {
  const allowed = new Set();
  const ids = [...new Set(issueIds)].map(Number).filter(Number.isFinite);
  for (let i = 0; i < ids.length; i += 1000) {
    const res = await jira.json(route`/rest/api/3/permissions/check`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', Accept: 'application/json' },
      body: JSON.stringify({ accountId, projectPermissions: [{ permissions: ['BROWSE_PROJECTS'], issues: ids.slice(i, i + 1000) }] }),
    });
    for (const grant of (res && res.projectPermissions) || []) {
      if (grant.permission === 'BROWSE_PROJECTS') for (const id of grant.issues || []) allowed.add(String(id));
    }
  }
  return allowed;
}

export async function searchIssues(jira, jql, fields) {
  const issues = [];
  let token;
  do {
    const page = token
      ? await jira.json(route`/rest/api/3/search/jql?jql=${jql}&fields=${fields.join(',')}&expand=changelog&maxResults=100&nextPageToken=${token}`)
      : await jira.json(route`/rest/api/3/search/jql?jql=${jql}&fields=${fields.join(',')}&expand=changelog&maxResults=100`);
    for (const i of page.issues || []) issues.push(await completeChangelog(jira, i));
    token = page.isLast ? undefined : page.nextPageToken;
  } while (token);
  return issues;
}
