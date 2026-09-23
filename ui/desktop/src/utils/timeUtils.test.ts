import { describe, expect, it } from 'vitest';
import { formatMessageTimestamp } from './timeUtils';

const NOW = new Date(2026, 8, 23, 9, 30);
const at = (d: Date) => Math.floor(d.getTime() / 1000);

/** UX audit C7: one transcript read "5:55 AM" beside "09/21/2026 8:46 PM". One rule, one voice. */
describe('formatMessageTimestamp', () => {
  it('today is the time alone', () => {
    expect(formatMessageTimestamp(at(new Date(2026, 8, 23, 5, 55)), NOW, 'en-US')).toBe('5:55 AM');
  });

  it('another day this year is the date in words and the time — never a numeric date', () => {
    const text = formatMessageTimestamp(at(new Date(2026, 8, 21, 20, 46)), NOW, 'en-US');
    expect(text).toBe('Sep 21, 8:46 PM');
    expect(text).not.toMatch(/\d{2}\/\d{2}\/\d{4}/);
  });

  it('another year adds the year', () => {
    expect(formatMessageTimestamp(at(new Date(2025, 11, 31, 23, 5)), NOW, 'en-US')).toBe(
      'Dec 31, 2025, 11:05 PM'
    );
  });
});
