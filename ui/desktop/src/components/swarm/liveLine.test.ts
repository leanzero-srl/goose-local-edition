import { describe, it, expect } from 'vitest';
import {
  lastSubstantiveLine,
  laneLiveLine,
  thinkingCaption,
  INLINE_TAIL_CHARS,
} from './SwarmRunPanel';

/**
 * THE LIVE LINE MUST BE LIVE.
 *
 * It took the last 2,400 characters as a BLOCK and handed them to a single-line row, which renders the
 * block's beginning — so a node's "what it is doing right now" showed narration from 2,400 characters
 * ago and only moved when the whole block rolled past. Mihai's "the output rolls", in the two surfaces
 * he looks at first, after it had already been fixed in the expanded view.
 */
describe('lastSubstantiveLine', () => {
  it('returns the LAST line, not the first', () => {
    expect(lastSubstantiveLine('reading the spec\nwriting ledger.py\nrunning the tests')).toBe(
      'running the tests'
    );
  });

  it('skips trailing fragments the substance gate rejects', () => {
    expect(lastSubstantiveLine('writing ledger.py\n m\n.\n with')).toBe('writing ledger.py');
  });

  it('falls back to the block when no single line qualifies', () => {
    expect(lastSubstantiveLine('appending to the ledger file now')).toBe(
      'appending to the ledger file now'
    );
  });

  it('returns empty for pure noise', () => {
    expect(lastSubstantiveLine('.\n,\n-')).toBe('');
  });
});

describe('laneLiveLine', () => {
  it('advances as the transcript grows instead of lagging a whole block behind', () => {
    const old = 'x'.repeat(INLINE_TAIL_CHARS) + '\nearly narration line\n';
    const lane = { fullTranscript: old + 'THE NEWEST LINE' } as never;
    expect(laneLiveLine(lane)).toBe('THE NEWEST LINE');
  });

  it('still falls back to the digest fields when there is no durable log', () => {
    expect(laneLiveLine({ reasoning: 'considering the schema' } as never)).toBe(
      'considering the schema'
    );
  });
});

describe('thinkingCaption', () => {
  it('admits a rolling window when there is no durable log', () => {
    expect(thinkingCaption('x'.repeat(2400), undefined, undefined, 22150)).toBe(
      '2,400 of 22,150 chars · rolling window, no durable log yet'
    );
  });

  it('says nothing extra when the window IS the whole stream', () => {
    expect(thinkingCaption('x'.repeat(120), undefined, undefined, 120)).toBe('120 chars');
  });

  it('prefers the durable tail note once the log exists', () => {
    const durable = 'y'.repeat(1000);
    expect(thinkingCaption(durable, durable, 50_000)).toContain('tail of');
  });

  it('does not invent a denominator when the engine counter is absent', () => {
    expect(thinkingCaption('hello there')).toBe('11 chars');
  });
});

describe('the THINKING path advances too', () => {
  it('shows the newest reasoning line, not the start of a 2,400-char block', () => {
    const lane = {
      fullThinking: 'older reasoning line\n'.repeat(200) + 'THE NEWEST THOUGHT',
      thinkingChars: 5000,
    } as never;
    expect(laneLiveLine(lane)).toBe('💭 THE NEWEST THOUGHT');
  });

  it('advances when the thinking grows — the exact defect the tick named', () => {
    const base = 'reasoning about the ledger\n'.repeat(150);
    const before = laneLiveLine({ fullThinking: base + 'step one', thinkingChars: 4000 } as never);
    const after = laneLiveLine({ fullThinking: base + 'step one\nstep two', thinkingChars: 4200 } as never);
    expect(before).not.toBe(after);
    expect(after).toBe('💭 step two');
  });

  it('still yields nothing when there is no reasoning at all', () => {
    expect(laneLiveLine({ thinkingChars: 0 } as never)).toBe('');
  });
});
