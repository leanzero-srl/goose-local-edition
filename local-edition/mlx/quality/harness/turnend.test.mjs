// node --test local-edition/mlx/quality/harness/turnend.test.mjs — Q-527's pure half: the turn-end verdict.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { turnEndVerdict, NOTICE_TESTIDS } from './turnend.mjs';
import { playwrightCandidates, resolvePlaywright } from './pw.mjs';

test('a done turn whose reply is an error notice is `error`, with the notice text', () => {
  const v = turnEndVerdict('done', [{ testid: 'provider-error-notice', cls: 'execution', text: 'goose could not send this request\tswarm chat: no swarm block' }]);
  assert.deepEqual(v, { ended: 'error', notice: '[provider-error-notice:execution] goose could not send this request swarm chat: no swarm block' });
});
test('no notice keeps done; a hang stays a hang', () => {
  assert.deepEqual(turnEndVerdict('done', []), { ended: 'done', notice: '' });
  assert.deepEqual(turnEndVerdict('hang', [{ testid: 'no-node-notice', cls: '', text: 'x' }]), { ended: 'hang', notice: '' });
});
test('every failure notice GooseMessage draws is read', () => {
  for (const id of ['provider-error-notice', 'no-node-notice', 'no-node-split-stopped', 'link-drop-notice', 'network-cut-raw', 'tool-bounds-notice']) assert.ok(NOTICE_TESTIDS.includes(id), id);
});
test('playwright resolves from this checkout or $HOME, never a hard-coded user', () => {
  const c = playwrightCandidates({ HOME: '/Users/workhorse' }, '/Users/workhorse', '/Users/x/Projects/goose/local-edition/mlx/quality/harness/');
  assert.deepEqual(c, ['/Users/x/Projects/goose/local-edition/mlx/quality/harness/../../../../ui/node_modules/playwright-core/index.mjs', '/Users/workhorse/Projects/goose/ui/node_modules/playwright-core/index.mjs']);
  assert.equal(resolvePlaywright({ env: { HOME: '/h' }, home: '/h', here: '/r/a/b/c/d', exists: (p) => p === '/h/Projects/goose/ui/node_modules/playwright-core/index.mjs' }), '/h/Projects/goose/ui/node_modules/playwright-core/index.mjs');
  assert.equal(playwrightCandidates({ PLAYWRIGHT_CORE: '/pw' }, '/h', '/r')[0], '/pw/index.mjs');
});
