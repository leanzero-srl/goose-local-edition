import { describe, it, expect, beforeEach } from 'vitest';
import {
  SAMPLING_DEFAULTS_KEY,
  clampKnob,
  hasAnySampling,
  loadSamplingDefaults,
  sanitizeSampling,
  saveSamplingDefaults,
} from './sampling';

describe('clampKnob', () => {
  it('clamps to the engine-honored range per knob', () => {
    expect(clampKnob('temperature', 5)).toBe(2);
    expect(clampKnob('temperature', -1)).toBe(0);
    expect(clampKnob('topP', 1.4)).toBe(1);
    expect(clampKnob('minP', 0.05)).toBe(0.05);
    expect(clampKnob('repeatPenalty', 0.1)).toBe(0.5);
    expect(clampKnob('repeatPenalty', 3)).toBe(2);
  });

  it('rounds topK to an integer — the engine parses it as i32', () => {
    expect(clampKnob('topK', 40.6)).toBe(41);
    expect(clampKnob('topK', 2000)).toBe(1000);
  });

  it('maps unset/garbage to undefined (= model default), never to a number', () => {
    expect(clampKnob('temperature', null)).toBeUndefined();
    expect(clampKnob('temperature', undefined)).toBeUndefined();
    expect(clampKnob('temperature', NaN)).toBeUndefined();
    expect(clampKnob('temperature', Infinity)).toBeUndefined();
  });
});

describe('sanitizeSampling', () => {
  it('keeps only known knobs, clamped', () => {
    expect(
      sanitizeSampling({ temperature: 0.7, topK: 40, bogus: 9, repeatPenalty: 9 })
    ).toEqual({ temperature: 0.7, topK: 40, repeatPenalty: 2 });
  });

  it('coerces numeric strings and drops non-numbers', () => {
    expect(sanitizeSampling({ temperature: '0.3', topP: 'high' })).toEqual({ temperature: 0.3 });
  });

  it('treats null/undefined/non-objects as empty (= all model defaults)', () => {
    expect(sanitizeSampling(null)).toEqual({});
    expect(sanitizeSampling(undefined)).toEqual({});
    expect(sanitizeSampling('x')).toEqual({});
    expect(hasAnySampling(sanitizeSampling(null))).toBe(false);
  });
});

describe('defaults persistence (swarmSamplingDefaults)', () => {
  beforeEach(() => localStorage.removeItem(SAMPLING_DEFAULTS_KEY));

  it('round-trips through localStorage, sanitized on both ends', () => {
    saveSamplingDefaults({ temperature: 0.2, topK: 40.4 });
    expect(loadSamplingDefaults()).toEqual({ temperature: 0.2, topK: 40 });
  });

  it('an absent or corrupt key loads as empty, never throws', () => {
    expect(loadSamplingDefaults()).toEqual({});
    localStorage.setItem(SAMPLING_DEFAULTS_KEY, '{not json');
    expect(loadSamplingDefaults()).toEqual({});
  });
});
