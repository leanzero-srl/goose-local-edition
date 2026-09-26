import { describe, expect, it } from 'vitest';
import { inFlightWork, trayCutLine, workCutBy, TRAY_ACTION_ENGINES } from './mlxInFlight';
import { LIVE_WRITING, liveSplitSnapshot } from './mlxInFlight.fixtures';

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
