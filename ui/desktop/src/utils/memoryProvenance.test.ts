import { describe, expect, it } from 'vitest';
import {
  describeSourceTag,
  indexSavedProposals,
  isSessionKey,
  originOf,
  splitSourceTags,
} from './memoryProvenance';

const proposalFile = (rows: unknown[]) => JSON.stringify(rows);

describe('memory provenance — only what the proposal store recorded', () => {
  it('a SAVED proposal matches the memory with the same scope, category and text', () => {
    const { index, unreadable } = indexSavedProposals([
      {
        key: '20260921_30',
        json: proposalFile([
          {
            id: 'p-1',
            kind: 'memory',
            polarity: 'positive',
            text: 'Webhook tests need WEBHOOK_SECRET=dev.',
            why: 'Lost an hour. ',
            category: 'lessons',
            tags: ['feedback'],
            is_global: false,
            created_at: 1790012779,
            state: 'saved',
          },
          {
            id: 'p-2',
            kind: 'memory',
            text: 'Declined idea',
            category: 'lessons',
            is_global: false,
            created_at: 1,
            state: 'declined',
          },
        ]),
      },
    ]);
    expect(unreadable).toEqual([]);
    const origin = originOf(index, {
      category: 'lessons',
      scope: 'local',
      content: 'Webhook tests need WEBHOOK_SECRET=dev.',
    });
    expect(origin).toEqual({
      key: '20260921_30',
      proposedAt: 1790012779,
      why: 'Lost an hour.',
      polarity: 'positive',
    });
    // same text, other scope → not this proposal's memory
    expect(
      originOf(index, {
        category: 'lessons',
        scope: 'global',
        content: 'Webhook tests need WEBHOOK_SECRET=dev.',
      })
    ).toBeUndefined();
    // a declined proposal never becomes an origin
    expect(originOf(index, { category: 'lessons', scope: 'local', content: 'Declined idea' })).toBe(
      undefined
    );
    // a memory edited after saving no longer claims the proposal
    expect(
      originOf(index, { category: 'lessons', scope: 'local', content: 'Webhook tests, edited.' })
    ).toBeUndefined();
  });

  it('the same lesson saved from two sessions credits the session that proposed it first', () => {
    const row = (created_at: number) => ({
      text: 'Same lesson.',
      category: 'lessons',
      is_global: false,
      created_at,
      state: 'saved',
    });
    const { index } = indexSavedProposals([
      { key: 'later', json: proposalFile([row(200)]) },
      { key: 'first', json: proposalFile([row(100)]) },
    ]);
    expect(
      originOf(index, { category: 'lessons', scope: 'local', content: 'Same lesson.' })?.key
    ).toBe('first');
  });

  it('a file that is not a proposal list is named, not read as empty', () => {
    const { index, unreadable } = indexSavedProposals([
      { key: 'broken', json: '{not json' },
      { key: 'object', json: '{"a":1}' },
    ]);
    expect(index.size).toBe(0);
    expect(unreadable).toEqual(['broken', 'object']);
  });

  it('session keys vs working-directory keys; source tags apart from the rest', () => {
    expect(isSessionKey('20260921_30')).toBe(true);
    expect(isSessionKey('wd-00ff00ff00ff00ff')).toBe(false);
    expect(splitSourceTags(['feedback', 'imported:claude-code'])).toEqual({
      sources: ['imported:claude-code'],
      rest: ['feedback'],
    });
    expect(describeSourceTag('imported:claude-code')).toBe('Imported from Claude Code');
  });
});
