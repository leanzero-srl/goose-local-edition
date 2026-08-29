import { describe, expect, it } from 'vitest';
import real from './__fixtures__/realLaneDigest.json';

// THE PROVENANCE NOTE IS CHECKED AGAINST THE DATA, NOT TRUSTED.
//
// `_why` quotes three sizes in two different units, and a reader who takes the byte figure for a
// character count concludes the fixture contradicts itself and "corrects" a number that was right.
// Every figure in the note is therefore parsed back out of the note and re-derived from the strings,
// so a note that stops matching the payload fails here instead of misleading the next reader.
describe('the real-digest fixture describes itself accurately', () => {
  const note = real._why;
  const figure = (unit: RegExp): number => {
    const m = note.match(unit);
    expect(m, `\`_why\` no longer states a figure matching ${unit}`).not.toBeNull();
    return Number(m![1].replace(/,/g, ''));
  };
  const bytes = (s: string) => new TextEncoder().encode(s).length;

  it('counts the durable log in the units each field actually uses', () => {
    expect(real.thinking_chars).toBe(real.full_thinking.length);
    expect(figure(/([\d,]+) chars\b/)).toBe(real.full_thinking.length);
    expect(figure(/([\d,]+) UTF-8 bytes\b/)).toBe(bytes(real.full_thinking));
  });

  it('states the rolling window size the capture actually has', () => {
    expect(figure(/([\d,]+)-char ROLLING WINDOW/)).toBe(real.last_thinking.length);
  });

  it('keeps the two sizes distinct, which is the whole point of the capture', () => {
    expect(bytes(real.full_thinking)).not.toBe(real.full_thinking.length);
    expect(real.last_thinking.length).toBeLessThan(real.full_thinking.length);
  });
});
