import { describe, expect, it } from 'vitest';
import { createIntl } from 'react-intl';
import type { NodeIfNotLoaded, NodeRole, NodeStrategy, NodeWhen } from './model';
import { sentenceFacts, type SentenceFacts } from './resolve';
import { sentenceFor, type SentenceNodeFacts } from './strategySentence';

const intl = createIntl({ locale: 'en', defaultLocale: 'en', messages: {} });

// A: an MLX way measured at 100 s; B: a cloud node; C: an MLX way never measured.
const NAMES: Record<string, string> = { a: 'A · both Macs', b: 'B · cloud', c: 'C · this Mac' };
const ABOUT: Record<string, SentenceNodeFacts> = {
  a: { loads: true, loadMs: 100_000 },
  b: { loads: false, loadMs: null },
  c: { loads: true, loadMs: null },
};
const about = (id: string) => ABOUT[id];
const WEIGHTS: Record<string, number> = { a: 2, b: 1, c: 1 };

function factsOf(
  when: NodeWhen,
  ifNotLoaded: NodeIfNotLoaded,
  ids: string[],
  role: NodeRole = 'chat'
): SentenceFacts {
  const entries = ids.map((node, i) => ({
    node,
    name: NAMES[node],
    rank: i + 1,
    weight: WEIGHTS[node],
  }));
  return {
    role,
    when,
    ifNotLoaded,
    entries,
    ...(when === 'share' ? { shareTotal: entries.reduce((s, e) => s + e.weight, 0) } : {}),
  };
}

const LOAD = 'If A · both Macs isn’t loaded, it loads and your turn waits (about 1m 40s).';
const NEXT_LAST =
  'If A · both Macs isn’t loaded, no node after it can take the work, so it is refused with the reason.';
const NEXT_B =
  'If A · both Macs isn’t loaded, B · cloud takes the work meanwhile and nothing is loaded.';

/** Every when-rule × if-not-loaded × chain length, each against its exact words. */
const MATRIX: [NodeWhen, NodeIfNotLoaded, string[], string][] = [
  ['failover', 'load', ['a'], `Chat runs on A · both Macs. ${LOAD}`],
  [
    'failover',
    'load',
    ['a', 'b'],
    `Chat runs on A · both Macs. If it can’t run, on B · cloud. ${LOAD}`,
  ],
  [
    'failover',
    'load',
    ['a', 'b', 'c'],
    `Chat runs on A · both Macs. If it can’t run, on B · cloud, then C · this Mac. ${LOAD}`,
  ],
  ['failover', 'useNext', ['a'], `Chat runs on A · both Macs. ${NEXT_LAST}`],
  [
    'failover',
    'useNext',
    ['a', 'b'],
    `Chat runs on A · both Macs. If it can’t run, on B · cloud. ${NEXT_B}`,
  ],
  [
    'failover',
    'useNext',
    ['a', 'b', 'c'],
    `Chat runs on A · both Macs. If it can’t run, on B · cloud, then C · this Mac. ${NEXT_B}`,
  ],
  [
    'overflow',
    'load',
    ['a'],
    `Chat runs on A · both Macs; when it is busy, the work waits for it. ${LOAD}`,
  ],
  [
    'overflow',
    'load',
    ['a', 'b'],
    `Chat runs on A · both Macs; when it is busy, the extra work goes to B · cloud. ${LOAD}`,
  ],
  [
    'overflow',
    'load',
    ['a', 'b', 'c'],
    `Chat runs on A · both Macs; when it is busy, the extra work goes to B · cloud, then C · this Mac. ${LOAD}`,
  ],
  [
    'overflow',
    'useNext',
    ['a'],
    `Chat runs on A · both Macs; when it is busy, the work waits for it. ${NEXT_LAST}`,
  ],
  [
    'overflow',
    'useNext',
    ['a', 'b'],
    `Chat runs on A · both Macs; when it is busy, the extra work goes to B · cloud. ${NEXT_B}`,
  ],
  [
    'overflow',
    'useNext',
    ['a', 'b', 'c'],
    `Chat runs on A · both Macs; when it is busy, the extra work goes to B · cloud, then C · this Mac. ${NEXT_B}`,
  ],
  ['share', 'load', ['a'], `Chat runs on A · both Macs. ${LOAD}`],
  ['share', 'load', ['a', 'b'], `Chat is shared: A · both Macs 2 parts, B · cloud 1 part. ${LOAD}`],
  [
    'share',
    'load',
    ['a', 'b', 'c'],
    `Chat is shared: A · both Macs 2 parts, B · cloud 1 part, C · this Mac 1 part. ${LOAD}`,
  ],
  ['share', 'useNext', ['a'], `Chat runs on A · both Macs. ${NEXT_LAST}`],
  [
    'share',
    'useNext',
    ['a', 'b'],
    `Chat is shared: A · both Macs 2 parts, B · cloud 1 part. ${NEXT_B}`,
  ],
  [
    'share',
    'useNext',
    ['a', 'b', 'c'],
    `Chat is shared: A · both Macs 2 parts, B · cloud 1 part, C · this Mac 1 part. ${NEXT_B}`,
  ],
];

describe('sentenceFor — the rule read back in plain words', () => {
  it('covers the whole matrix: 3 when-rules × 2 if-not-loaded rules × chains of 1, 2 and 3', () => {
    expect(MATRIX.length).toBe(18);
  });

  for (const [when, ifNotLoaded, ids, want] of MATRIX) {
    it(`${when} · ${ifNotLoaded} · chain of ${ids.length}`, () => {
      expect(sentenceFor(intl, factsOf(when, ifNotLoaded, ids), about)).toBe(want);
    });
  }

  it('a build role says the TASK waits, and an unmeasured load says so — never an estimate', () => {
    expect(sentenceFor(intl, factsOf('failover', 'load', ['c'], 'build'), about)).toBe(
      'Build runs on C · this Mac. If C · this Mac isn’t loaded, it loads and the task waits; its first load is not measured yet.'
    );
  });

  it('the clause names the first entry that can be not loaded (a cloud 1st never loads)', () => {
    expect(sentenceFor(intl, factsOf('failover', 'load', ['b', 'a']), about)).toBe(
      `Chat runs on B · cloud. If it can’t run, on A · both Macs. ${LOAD}`
    );
  });

  it('a chain with no MLX way has no if-not-loaded clause at all', () => {
    expect(sentenceFor(intl, factsOf('failover', 'useNext', ['b']), about)).toBe(
      'Chat runs on B · cloud.'
    );
  });

  it('an inherited role is said with its own name, over the chain it follows', () => {
    const strategy: NodeStrategy = {
      id: 's',
      name: 'S',
      roles: {
        chat: {
          chain: [
            { node: 'a', weight: 1 },
            { node: 'b', weight: 1 },
          ],
          when: 'failover',
          ifNotLoaded: 'load',
        },
      },
    };
    const facts = sentenceFacts(strategy, 'build', NAMES);
    expect(facts?.sameAs).toBe('chat');
    expect(sentenceFor(intl, facts as SentenceFacts, about)).toBe(
      'Build runs on A · both Macs. If it can’t run, on B · cloud. If A · both Macs isn’t loaded, it loads and the task waits (about 1m 40s).'
    );
  });

  it('an empty chain says nothing rather than inventing a node', () => {
    expect(sentenceFor(intl, factsOf('failover', 'load', []), about)).toBe('');
  });
});
