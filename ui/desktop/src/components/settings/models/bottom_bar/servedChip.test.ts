import { describe, expect, it } from 'vitest';
import { createIntl } from 'react-intl';
import type { ChatRoute, ChatServedBy } from '../../../chatServedBy/chatServedBy';
import { servedChipWords } from './servedChip';

const intl = createIntl({ locale: 'en', defaultLocale: 'en', messages: {} });

function servedOn(route: ChatRoute): ChatServedBy {
  return {
    engine: 'none',
    model: null,
    where: [],
    readiness: { kind: 'ready' },
    route,
  } as unknown as ChatServedBy;
}

describe('the chip names a chat’s own node set (Q-379)', () => {
  it('as "This chat’s nodes" — never the name goosed generates for the set', () => {
    const words = servedChipWords(
      intl,
      servedOn({
        kind: 'strategy',
        name: 'This chat’s nodes (20260928_4)',
        own: true,
        node: 'Flash · this Mac',
        nodeId: 'flash-here',
      }),
      false
    );
    expect(words.chipLabel).toBe('This chat’s nodes · Flash · this Mac');
    expect(words.chipLabel).not.toContain('20260928_4');
    const before = servedChipWords(
      intl,
      servedOn({
        kind: 'strategy',
        name: 'This chat’s nodes (20260928_4)',
        own: true,
        node: null,
        nodeId: null,
      }),
      false
    );
    expect(before.chipLabel).toBe('This chat’s nodes');
  });

  it('a named strategy keeps its own name', () => {
    const words = servedChipWords(
      intl,
      servedOn({
        kind: 'strategy',
        name: 'Everyday',
        own: false,
        node: 'Flash · this Mac',
        nodeId: 'flash-here',
      }),
      false
    );
    expect(words.chipLabel).toBe('Everyday · Flash · this Mac');
  });
});
