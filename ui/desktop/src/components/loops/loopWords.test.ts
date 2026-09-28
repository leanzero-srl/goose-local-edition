import { createIntl } from 'react-intl';
import { describe, expect, it } from 'vitest';
import raw from '../../../../../crates/goose/src/session_loops/loops.fixture.json';
import { sentenceMessage, sentenceValues } from './loopWords';
import {
  clockTime,
  statusSentence,
  type LoopRecord,
  type LoopStatus,
  type LoopStatusReason,
} from './model';

interface SentenceCase {
  name: string;
  record: LoopRecord;
  status: LoopStatus;
  reason?: LoopStatusReason;
  now: string;
  utcOffsetMinutes: number;
  expect: { key: string; facts: Record<string, string>; text: string };
}

const cases = (raw as unknown as { sentences: SentenceCase[] }).sentences;
const intl = createIntl({ locale: 'en', defaultLocale: 'en', messages: {} });

describe('the NOW sentences in the catalogs', () => {
  it('say, for every fixture case, exactly the English the model says', () => {
    expect(cases.length).toBeGreaterThanOrEqual(35);
    for (const c of cases) {
      const got = statusSentence(
        c.record,
        c.status,
        c.reason,
        Date.parse(c.now),
        c.utcOffsetMinutes,
        clockTime
      );
      expect(got.ok, c.name).toBe(true);
      if (!got.ok) continue;
      const message = sentenceMessage(got.value);
      expect(message, `${c.name}: ${got.value.key} is in the catalogs`).not.toBeNull();
      expect(intl.formatMessage(message!, sentenceValues(got.value)), c.name).toBe(c.expect.text);
    }
  });

  it('names a key the catalogs do not carry instead of guessing words for it', () => {
    expect(sentenceMessage({ key: 'loops.now.unknown', facts: {}, text: 'x' })).toBeNull();
  });
});
