import { describe, expect, it } from 'vitest';
import { collapseRepeats, substantiveChunk } from './useSwarmRun';

describe('collapseRepeats', () => {
  // MEASURED live: a scout's 2000-char thinking tail was one paragraph repeated three times (15 duplicate
  // lines). Rendered verbatim it filled the whole thinking box and read as a UI bug rather than a looping
  // model. Folding it makes the loop the visible fact.
  it('names a repeated BLOCK instead of pasting it again', () => {
    const block = 'This task is straightforward.\nKey libraries needed:\n1. `argparse` for the CLI\nNo lookups needed.';
    const out = collapseRepeats(`${block}\n${block}\n${block}`);
    expect(out).toContain('the model repeated the block above 3×');
    expect(out).toContain('it is looping');
    // The block itself is shown exactly ONCE.
    expect(out.split('Key libraries needed:').length - 1).toBe(1);
    expect(out.length).toBeLessThan(`${block}\n${block}\n${block}`.length);
  });

  it('folds a run of identical LINES and says how many', () => {
    const out = collapseRepeats('start\nsame\nsame\nsame\nend');
    expect(out).toBe('start\nsame  ⟲ ×3\nend');
  });

  it('leaves non-repeating text byte-identical', () => {
    const prose = 'First line.\nSecond line.\n\nThird line after a gap.';
    expect(collapseRepeats(prose)).toBe(prose);
  });

  it('never annotates blank lines (they are just spacing)', () => {
    expect(collapseRepeats('a\n\n\n\nb')).toBe('a\n\n\n\nb');
  });

  it('handles empty input', () => {
    expect(collapseRepeats('')).toBe('');
  });
});

describe('substantiveChunk', () => {
  // OBSERVED live: a node that was actively generating displayed just "m". The text channel emits
  // single-token fragments while the real narration goes to the <think> channel, so an ungated fallback
  // prints a letter and calls it the node's status.
  it('rejects the single-token fragments the text channel emits', () => {
    for (const frag of ['m', '.', ' with', '(group', '', '   ', '42', '```']) {
      expect(substantiveChunk(frag), `"${frag}" must not be shown as a live line`).toBe('');
    }
  });

  it('accepts a real sentence and trims it', () => {
    expect(substantiveChunk('  Let me sketch the module layout:  ')).toBe(
      'Let me sketch the module layout:'
    );
  });

  it('handles null/undefined', () => {
    expect(substantiveChunk(null)).toBe('');
    expect(substantiveChunk(undefined)).toBe('');
  });
});
