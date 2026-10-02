'use strict';
// Operation lookup and the OAuth2 scope rule, read from the pinned Jira OpenAPI files (DESIGN.md §5.3).
// No hand-written scope table: each operation's alternatives come from `x-atlassian-oauth2-scopes`
// (state Current = classic, Beta = granular) or, where that key is absent (the Agile paths), from the
// `OAuth2` entry of `security`. `basicAuth` and empty `{}` alternatives are ignored: they would allow
// every call. A call is allowed when the declared scopes contain one alternative's full set.
const fs = require('fs');
const path = require('path');

const METHODS = ['get', 'put', 'post', 'delete', 'patch'];
const DEFAULT_DIR = path.join(__dirname, '..', 'kit', 'openapi');

function loadOperations(dir = DEFAULT_DIR) {
  const ops = [];
  for (const file of ['jira.json', 'jsw.json']) {
    const spec = JSON.parse(fs.readFileSync(path.join(dir, file), 'utf8'));
    for (const [template, item] of Object.entries(spec.paths)) {
      for (const method of METHODS) {
        const op = item[method];
        if (!op) continue;
        const segs = template.split('/');
        const regex = new RegExp('^' + segs.map((s) => (/^\{.+\}$/.test(s) ? '([^/]+)' : s.replace(/[.*+?^$()|[\]\\]/g, '\\$&'))).join('/') + '/?$');
        const params = segs.filter((s) => /^\{.+\}$/.test(s)).map((s) => s.slice(1, -1));
        ops.push({ file, method: method.toUpperCase(), template, regex, params, literal: segs.filter((s) => s && !/^\{.+\}$/.test(s)).length,
          alternatives: alternatives(op) });
      }
    }
  }
  // Literal segments outrank parameters (/issue/bulkfetch before /issue/{issueIdOrKey}).
  ops.sort((a, b) => b.literal - a.literal || a.params.length - b.params.length);
  return ops;
}

function alternatives(op) {
  const x = op['x-atlassian-oauth2-scopes'];
  if (Array.isArray(x) && x.length) return x.filter((a) => a.scheme === 'OAuth2').map((a) => ({ state: a.state, scopes: a.scopes ?? [] }));
  const oauth = (op.security ?? []).find((s) => s && Object.prototype.hasOwnProperty.call(s, 'OAuth2'));
  return oauth ? [{ state: 'OAuth2', scopes: oauth.OAuth2 }] : [];
}

function createOpenApi(dir) {
  const ops = loadOperations(dir);
  const match = (method, pathname) => {
    for (const op of ops) {
      if (op.method !== method) continue;
      const m = pathname.match(op.regex);
      if (m) return { op, params: Object.fromEntries(op.params.map((p, i) => [p, decodeURIComponent(m[i + 1])])) };
    }
    return null;
  };
  // Any method on this path? (405 vs 404 distinction)
  const pathExists = (pathname) => ops.some((op) => op.regex.test(pathname));
  // The alternative a call is satisfied by: classic (Current) where it exists, else the full granular set.
  const scopeCheck = (op, declared) => {
    const have = new Set(declared);
    if (!op.alternatives.length) return { ok: false, chosen: null, reason: 'operation declares no OAuth2 alternative' };
    const ordered = [...op.alternatives].sort((a, b) => (a.state === 'Current' ? -1 : 0) - (b.state === 'Current' ? -1 : 0));
    for (const alt of ordered) if (alt.scopes.every((s) => have.has(s))) return { ok: true, chosen: alt };
    return { ok: false, chosen: null, needed: ordered };
  };
  return { ops, match, pathExists, scopeCheck };
}

module.exports = { createOpenApi, loadOperations, DEFAULT_DIR };
