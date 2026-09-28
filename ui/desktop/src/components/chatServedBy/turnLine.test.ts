import { describe, expect, it } from 'vitest';
import { createIntl } from 'react-intl';
import type { NodeServedTurnDto, NodesReadResponse_unstable } from '@aaif/goose-sdk';
import { fellBackOf, fellBackText, servingChatsText } from './turnLine';
import { CONFIG, NODE_CLOUD, NODE_FLASH } from '../nodes/nodeGlance.fixtures';

const intl = createIntl({ locale: 'en', defaultLocale: 'en', messages: {} });
const READ: NodesReadResponse_unstable = {
  config: CONFIG,
  nodes: [NODE_FLASH, NODE_CLOUD],
  stored: true,
  lmStudioHidden: 0,
};

function record(extra: Partial<NodeServedTurnDto>): NodeServedTurnDto {
  const reason = 'passed over for this turn: you asked to answer on Flash · this Mac for now';
  return {
    node: NODE_FLASH.def.id,
    role: 'chat',
    rank: 2,
    reason,
    tried: [{ node: NODE_CLOUD.def.id, reason }],
    atMs: 1,
    ...extra,
  };
}

describe('the nodes.fellBack turn line (Q-381)', () => {
  it('announces a turn the person asked onto the next node, and that the lead is next again', () => {
    const fell = fellBackOf(record({ askedForThisTurn: true }), READ);
    expect(fell?.asked).toBe(true);
    expect(fellBackText(intl, fell!)).toBe(
      'Chat is on Flash · this Mac (2nd) for this turn, as you asked. The next message goes to Claude Sonnet · OpenRouter again.'
    );
  });

  it('a fallback nobody asked for keeps its can’t-run words', () => {
    const fell = fellBackOf(record({ reason: 'not connected', tried: [] }), READ);
    expect(fell).toBeNull();
    const failed = fellBackOf(
      record({
        reason: 'not connected',
        tried: [{ node: NODE_CLOUD.def.id, reason: 'not connected' }],
      }),
      READ
    );
    expect(failed?.asked).toBe(false);
    expect(fellBackText(intl, failed!)).toBe(
      "Chat is on Flash · this Mac (2nd): Claude Sonnet · OpenRouter can't run: not connected"
    );
  });
});

describe('Q-428: the turn line when the 1st was left to the node its Mac serves', () => {
  it('names which node answered and why — never "can’t run"', () => {
    const reason =
      'Work’s Mac Studio is serving 27B · both Macs for chat "Kickoff notes"; Claude Sonnet · OpenRouter is left to it';
    const fell = fellBackOf(
      record({
        reason,
        tried: [{ node: NODE_CLOUD.def.id, reason }],
        servingOther: {
          mac: 'Work’s Mac Studio',
          serving: '27B · both Macs',
          chats: ['Kickoff notes'],
          replies: 0,
        },
      }),
      READ
    );
    expect(fellBackText(intl, fell!)).toBe(
      'Chat is on Flash · this Mac (2nd): Work’s Mac Studio is serving 27B · both Macs for chat "Kickoff notes"'
    );
  });

  it('several chats are listed', () => {
    expect(servingChatsText(intl, ['A', 'B'])).toBe('chats "A" and "B"');
    expect(servingChatsText(intl, [])).toBe('another chat');
  });
});
