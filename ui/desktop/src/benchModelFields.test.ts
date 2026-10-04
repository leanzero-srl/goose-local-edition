import { describe, it, expect } from 'vitest';
import {
  MODEL_FIELDS_ENV,
  describeRunModelFields,
  forgeEffortPin,
  modelFieldsForRun,
  modelFieldsLaunchEnv,
  validBenchModelFields,
} from './benchModelFields';

describe('benchmark model fields', () => {
  it('accepts snake_case ids with string or finite number values only', () => {
    expect(validBenchModelFields({ effort: 'low', top_k: 40, temperature: 0.7 })).toBe(true);
    expect(validBenchModelFields({})).toBe(true);
    for (const bad of [
      null,
      [],
      'effort',
      { Effort: 'low' },
      { effort: '' },
      { effort: { nested: true } },
      { top_k: Number.NaN },
      { 'effort; rm -rf': 'low' },
    ]) {
      expect(validBenchModelFields(bad)).toBe(false);
    }
  });

  it('a single-model SB run sends every saved value', () => {
    expect(modelFieldsForRun({ effort: 'xhigh', top_k: 40 }, false)).toEqual({
      sent: { effort: 'xhigh', top_k: 40 },
      pinnedByTier: [],
    });
  });

  it("Forge records the saved effort as pinned and never sends it, so the tier's pin holds", () => {
    const record = modelFieldsForRun({ effort: 'low', verbosity: 'high' }, true);
    expect(record).toEqual({ sent: { verbosity: 'high' }, pinnedByTier: ['effort'] });
    expect(forgeEffortPin('medium')).toEqual({
      effort: 'Forge runs every model at reasoning effort medium',
    });
  });

  it('always sets the variable benchmark-config reads, {} when nothing is sent', () => {
    expect(
      modelFieldsLaunchEnv('openrouter', 'deepseek/deepseek-v4.1-flash', { effort: 'low' })
    ).toEqual({
      [MODEL_FIELDS_ENV]: '{"openrouter":{"deepseek/deepseek-v4.1-flash":{"effort":"low"}}}',
    });
    expect(modelFieldsLaunchEnv('openrouter', 'm', {})).toEqual({ [MODEL_FIELDS_ENV]: '{}' });
  });

  it('describes what a result ran at', () => {
    expect(describeRunModelFields({ sent: { top_k: 40, effort: 'low' }, pinnedByTier: [] })).toBe(
      'effort low · top_k 40'
    );
    expect(describeRunModelFields({ sent: {}, pinnedByTier: ['effort'] })).toBe(
      "the model's defaults; effort pinned by the tier"
    );
    expect(describeRunModelFields(undefined)).toBe('not recorded (the run predates this record)');
  });
});
