// 30-line mock Jira Cloud REST v3: enough surface for the spike, every call recorded.
const http = require('http');
const issues = {
  'SPK-1': { key: 'SPK-1', id: '10001', fields: { summary: 'Login button misaligned', status: { name: 'To Do' }, labels: [] } },
  'SPK-2': { key: 'SPK-2', id: '10002', fields: { summary: 'Export CSV times out', status: { name: 'In Progress' }, labels: [] } },
};
const calls = [];
function startMockJira(port) {
  return new Promise((ok) => http.createServer((req, res) => {
    let raw = ''; req.on('data', (c) => (raw += c)); req.on('end', () => {
      const u = new URL(req.url, 'http://x'); const body = raw ? JSON.parse(raw) : undefined;
      calls.push({ method: req.method, path: u.pathname + u.search, as: req.headers['x-forge-as'], body });
      const send = (s, j) => { res.writeHead(s, { 'content-type': 'application/json' }); res.end(j === undefined ? '' : JSON.stringify(j)); };
      let m;
      if ((m = u.pathname.match(/^\/rest\/api\/3\/issue\/([A-Z]+-\d+)$/))) {
        const i = issues[m[1]]; if (!i) return send(404, { errorMessages: ['Issue does not exist'] });
        if (req.method === 'GET') return send(200, i);
        if (req.method === 'PUT') { Object.assign(i.fields, body.fields); return send(204); }
      }
      if ((m = u.pathname.match(/^\/rest\/api\/3\/issue\/([A-Z]+-\d+)\/comment$/)) && req.method === 'POST')
        return send(201, { id: String(calls.length), body: body.body });
      if (u.pathname === '/rest/api/3/search/jql' && req.method === 'POST') {
        const want = /status = "([^"]+)"/.exec(body.jql)?.[1];
        return send(200, { issues: Object.values(issues).filter((i) => !want || i.fields.status.name === want) });
      }
      send(404, { errorMessages: [`mock has no ${req.method} ${u.pathname}`] });
    });
  }).listen(port, '127.0.0.1', ok));
}
module.exports = { startMockJira, calls, issues };
