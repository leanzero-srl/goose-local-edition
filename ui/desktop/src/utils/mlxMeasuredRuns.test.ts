import { describe, expect, it } from 'vitest';
import {
  contextBucketOf,
  parseMeasuredRuns,
  pickMeasured,
  readingForPrompt,
  type MeasuredRunsFetch,
} from './mlxMeasuredRuns';

/**
 * goosed's `GET /mlx-engine/measured-runs` for the Studio way, in the shape its serde writes (the
 * Rust test `the_tray_reads_the_same_figure_the_plan_counts` pins the keys), with the real store's
 * figures (2026-09-26: 303 of the 428 timed turns count; reading measured at 64k prompts).
 */
const STUDIO_BODY = {
  way: {
    placementId: 'single:link:worksmacstudio-lan-6a972f',
    placement: { kind: 'single', nodes: ['link:worksmacstudio-lan-6a972f'] },
    modelId: 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx',
    nodeNames: ['WorksMacStudio.lan'],
  },
  wayError: null,
  recorded: 468,
  writing: {
    estimate: { value: 26.96, low: 24.15, high: 30.74 },
    measured: true,
    runs: 303,
    lastMeasuredMs: 1790409907845,
  },
  writingBasis:
    "writing: the median of 303 of this way's 428 timed runs — 125 wrote too few tokens to time finer than the runs' own spread (±15%)",
  reading: null,
  readingByBucket: [
    {
      bucket: 65536,
      figure: { estimate: { value: 131.2, low: 110, high: 155 }, measured: true, runs: 123 },
    },
  ],
  storeErrors: [],
};

describe('goose’s measured runs as MAIN reads them (Q-129)', () => {
  it('parses goosed’s answer and refuses a malformed one with the reason', () => {
    const parsed = parseMeasuredRuns(STUDIO_BODY);
    expect(typeof parsed).not.toBe('string');
    expect(parseMeasuredRuns({ ...STUDIO_BODY, writing: { runs: 3 } })).toBe(
      'a measured-runs figure is malformed'
    );
    expect(parseMeasuredRuns({ ...STUDIO_BODY, way: { placementId: 1 } })).toBe(
      'the measured-runs way is malformed'
    );
  });

  it('picks the backend whose way IS the engine read — a linked Mac’s runs never stand for this Mac’s', () => {
    const studio = parseMeasuredRuns(STUDIO_BODY);
    if (typeof studio === 'string') throw new Error(studio);
    const answers: MeasuredRunsFetch[] = [{ ok: true, answer: studio }];
    expect(pickMeasured(answers, 'remote')).toMatchObject({ kind: 'read' });
    expect(pickMeasured(answers, 'single')).toEqual({
      kind: 'unread',
      detail:
        "goose records this Mac's chat on single:link:worksmacstudio-lan-6a972f, not on the engine read here",
    });
    expect(pickMeasured([], 'single')).toEqual({
      kind: 'unread',
      detail: 'no goose backend is running',
    });
    expect(
      pickMeasured(
        [{ ok: true, answer: { ...studio, way: null, wayError: 'no MLX engine serves this Mac' } }],
        'single'
      )
    ).toEqual({ kind: 'unread', detail: 'no MLX engine serves this Mac' });
  });

  it('a prompt’s reading estimate comes from runs of ITS size only', () => {
    const studio = parseMeasuredRuns(STUDIO_BODY);
    if (typeof studio === 'string') throw new Error(studio);
    const read = { kind: 'read' as const, answer: studio };
    expect(contextBucketOf(48_154)).toBe(65_536);
    expect(contextBucketOf(65_536)).toBe(65_536);
    expect(contextBucketOf(0)).toBe(1);
    expect(readingForPrompt(read, 48_154)).toBe(131.2);
    expect(readingForPrompt(read, 1_900)).toBeNull();
    expect(readingForPrompt({ kind: 'pending' }, 48_154)).toBeNull();
  });
});
