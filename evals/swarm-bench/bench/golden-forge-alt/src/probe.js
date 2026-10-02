import api, { route } from '@forge/api';
export async function run() {
  const K = 'key in (PAY-199, PAY-203, PAY-224, SRCH-319)';
  const q = async (jql) => { const r = await api.asApp().requestJira(route`/rest/api/3/search/jql?jql=${jql}&fields=created&maxResults=100`); const b = await r.json(); return r.status + ' ' + JSON.stringify((b.issues || []).map((i) => i.key)); };
  const g = async (r) => { const res = await api.asApp().requestJira(r); const b = await res.json(); return { status: res.status, keys: Object.keys(b).join(','), isLast: b.isLast, tok: !!b.nextPageToken, n: (b.issues || []).length }; };
  return {
    inOpen: await q(`${K} AND sprint in openSprints()`),
    notOpen: await q(`${K} AND sprint not in openSprints()`),
    sw1: await g(route`/rest/software/1.0/sprint/365/issue?fields=created&maxResults=5`),
    sw2: await g(route`/rest/software/1.0/sprint/365/issue?fields=created&maxResults=5&startAt=5`),
    ag: await g(route`/rest/agile/1.0/sprint/365/issue?fields=created&maxResults=5&startAt=5`),
  };
}
