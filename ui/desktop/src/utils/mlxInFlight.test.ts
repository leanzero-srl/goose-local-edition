import { describe, expect, it } from 'vitest';
import {
  BACKGROUND_WORK_EN,
  backgroundWorkCut,
  inFlightWork,
  trayCutLine,
  workCutBy,
  TRAY_ACTION_ENGINES,
} from './mlxInFlight';
import { LIVE_WRITING, factCheckSnapshot, liveSplitSnapshot } from './mlxInFlight.fixtures';
import en from '../i18n/messages/en.json';

describe('inFlightWork — what a stop would cut (Q-148)', () => {
  it('the live split: whose answer, its elapsed, its tokens, and the context it holds', () => {
    const work = inFlightWork(liveSplitSnapshot());
    expect(work).toMatchObject({
      engine: 'distributed',
      requests: 1,
      elapsedS: 2355,
      tokens: 24228,
      reading: false,
      contextTokens: 39996 + 24228,
    });
    expect(work?.clients.map((c) => c.kind)).toEqual(['chat']);
  });

  it('an idle engine, a stale read or no read cut nothing', () => {
    expect(inFlightWork(liveSplitSnapshot([]))).toBeNull();
    expect(inFlightWork({ ...liveSplitSnapshot(), mode: 'unknown' })).toBeNull();
    expect(inFlightWork({ ...liveSplitSnapshot(), stats: null })).toBeNull();
    expect(inFlightWork(null)).toBeNull();
  });

  it('a door cuts only the engine it stops', () => {
    expect(workCutBy(liveSplitSnapshot(), ['distributed'])).not.toBeNull();
    expect(workCutBy(liveSplitSnapshot(), ['single'])).toBeNull();
    expect(workCutBy(liveSplitSnapshot(), TRAY_ACTION_ENGINES['stop-distributed']!)).not.toBeNull();
    expect(workCutBy(liveSplitSnapshot(), TRAY_ACTION_ENGINES.unmount!)).toBeNull();
    expect(TRAY_ACTION_ENGINES.mount).toBeUndefined();
  });

  it('the tray says it in figures; a request still reading says its prompt', () => {
    expect(trayCutLine(inFlightWork(liveSplitSnapshot())!)).toBe(
      'Stopping cuts the answer in flight (39m 15s, 24k tokens written)'
    );
    const reading = inFlightWork(
      liveSplitSnapshot([{ ...LIVE_WRITING, phase: 'prefill', completionTokens: 0, elapsedS: 95 }])
    )!;
    expect(trayCutLine(reading)).toBe(
      'Stopping cuts the answer in flight (1m 35s, reading a 40k-token prompt)'
    );
  });
});

describe('Q-185: goose’s own call for a chat is cut as that, never as "the answer"', () => {
  it('E2E #3i: the fact check reading its 1,094-token prompt is background work, named', () => {
    const work = inFlightWork(factCheckSnapshot())!;
    expect(backgroundWorkCut(work)).toBe('factCheck');
    expect(trayCutLine(work)).toBe(
      'Stopping cuts goose’s background work: Checking the reply (3s, reading a 1.1k-token prompt)'
    );
  });

  it('a turn, several requests or an external client are not background work', () => {
    expect(backgroundWorkCut(inFlightWork(liveSplitSnapshot())!)).toBeNull();
    const two = inFlightWork(liveSplitSnapshot([LIVE_WRITING, { ...LIVE_WRITING, id: 'r2' }]))!;
    expect(backgroundWorkCut({ ...two, clients: factCheckSnapshot().serving!.clients })).toBeNull();
    expect(
      backgroundWorkCut({
        ...inFlightWork(factCheckSnapshot())!,
        clients: [{ key: 'e', kind: 'external', model: 'omlx/x', count: 1 }],
      })
    ).toBeNull();
  });

  it('the tray’s English is the catalog’s English, word for word', () => {
    for (const [kind, words] of Object.entries(BACKGROUND_WORK_EN)) {
      expect(en[`backgroundWork.${kind}` as keyof typeof en]?.defaultMessage).toBe(words);
    }
    const kinds = Object.keys(en)
      .filter((id) => /^backgroundWork\.[a-zA-Z]+$/.test(id) && id !== 'backgroundWork.named')
      .map((id) => id.slice('backgroundWork.'.length))
      .sort();
    expect(kinds).toEqual(Object.keys(BACKGROUND_WORK_EN).sort());
  });
});
