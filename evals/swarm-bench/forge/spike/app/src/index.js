import Resolver from '@forge/resolver';
import api, { route } from '@forge/api';
import { kvs } from '@forge/kvs';
import { Queue } from '@forge/events';

const resolver = new Resolver();

resolver.define('summarise', async ({ payload, context }) => {
  const key = payload.issueKey ?? context.extension?.issue?.key;
  const res = await api.asApp().requestJira(route`/rest/api/3/issue/${key}?fields=summary,status`);
  if (!res.ok) throw new Error(`jira ${res.status}`);
  const issue = await res.json();
  const views = ((await kvs.get(`views:${key}`)) ?? 0) + 1;
  await kvs.set(`views:${key}`, views);
  return { key, summary: issue.fields.summary, status: issue.fields.status.name, views };
});

resolver.define('comment', async ({ payload }) => {
  const res = await api.asUser().requestJira(route`/rest/api/3/issue/${payload.issueKey}/comment`, {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ body: { type: 'doc', version: 1, content: [{ type: 'paragraph', content: [{ type: 'text', text: payload.text }] }] } }),
  });
  return { status: res.status };
});

resolver.define('egressProbe', async () => {
  console.log('egress probe starting');
  try { const r = await fetch('https://example.com/'); return { status: r.status, proxyError: r.headers.get('forge-proxy-error') }; }
  catch (e) { return { threw: String(e.message ?? e) }; }
});

export const handler = resolver.getDefinitions();

export const hourly = async (event, context) => {
  const res = await api.asApp().requestJira(route`/rest/api/3/search/jql`, {
    method: 'POST', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ jql: 'status = "To Do"', fields: ['summary'] }),
  });
  const { issues } = await res.json();
  await kvs.set('last-sweep', { count: issues.length, at: event?.context?.moduleKey ?? 'unknown' });
  await new Queue({ key: 'spike-queue' }).push(issues.map((i) => ({ body: { key: i.key } })));
  return { swept: issues.length };
};

export const hook = async (request) => {
  const body = JSON.parse(request.body || '{}');
  await kvs.set(`hook:${body.id}`, body);
  const stored = await kvs.get(`hook:${body.id}`);
  return { statusCode: 202, headers: { 'Content-Type': ['application/json'] }, body: JSON.stringify({ stored }) };
};

export const consume = async (event) => {
  const key = event.body.key;
  await api.asApp().requestJira(route`/rest/api/3/issue/${key}`, {
    method: 'PUT', headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ fields: { labels: ['swept'] } }),
  });
  return 'ok';
};
