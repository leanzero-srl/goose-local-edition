import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { pickBenchShots, limitBenchShotsForPublish } from './benchShots';
import { forgePublishProblem, forgePublishTiers } from './benchForgePublish';
import { FORGE_KIT_STATUS_SCRIPT, parseForgeKitStatus } from './benchForgeKit';
import { projectBenchScore } from './benchScoreProjection';
import forgeVerdict from './components/benchmark/forge-alt.fixture.json';

/** The main-side Forge seams, each against the real verdict score_forge.py wrote for the alt golden app. */

const dirs: string[] = [];
const PNG = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLbtAAAAABJRU5ErkJggg==',
  'base64'
);
afterEach(async () => {
  await Promise.all(dirs.splice(0).map((dir) => fs.rm(dir, { recursive: true, force: true })));
});

describe('forge screenshots', () => {
  it('reads forge_probe.mjs captures from forge-shots/, widget and sprint action in both themes first', async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'forge-shots-test-'));
    dirs.push(dir);
    await fs.mkdir(path.join(dir, 'forge-shots'));
    // The names forge_probe.mjs writes (forge/DESIGN.md §6.6), as a real run left them.
    for (const name of [
      'contact-sheet.png',
      'not-started-799.png',
      'sprint-action-663-dark-800x600.png',
      'sprint-action-644-dark-800x600.png',
      'sprint-action-644-light-800x600.png',
      'widget-edit-170-light.png',
      'widget-view-66-light-1180x480.png',
      'widget-view-170-light-1180x480.png',
      'widget-view-170-dark-1180x480.png',
      'widget-view-170-light-380x480.png',
      'widget-view-noconfig.png',
    ])
      await fs.writeFile(path.join(dir, 'forge-shots', name), PNG);
    const shots = await pickBenchShots(dir);
    expect(shots.map((shot) => [shot.name, shot.caption])).toEqual([
      ['forge-widget-light', 'Dashboard widget · light'],
      ['forge-widget-dark', 'Dashboard widget · dark'],
      ['forge-sprint-light', 'Sprint action · light'],
      ['forge-sprint-dark', 'Sprint action · dark'],
      ['forge-edit', 'Widget edit view'],
      ['forge-widget-narrow', 'Dashboard widget · 380 px'],
      ['forge-noconfig', 'Widget before configuration'],
      ['forge-not-started', 'Sprint not started'],
      ['forge-contact-sheet', 'Every captured surface'],
    ]);
    // The lowest name within a kind wins — the same pick on every read.
    expect(shots[0].b64).toBe(PNG.toString('base64'));
    // Publishing keeps the site's five-image limit: the four theme shots and the edit view.
    expect(limitBenchShotsForPublish(shots).map((shot) => shot.name)).toEqual([
      'forge-widget-light',
      'forge-widget-dark',
      'forge-sprint-light',
      'forge-sprint-dark',
      'forge-edit',
    ]);
  });
});

describe('forge publishing', () => {
  const projected = projectBenchScore(forgeVerdict as never);
  const stored = { scorerVersion: projected.scorerVersion, verdict: projected.verdict };

  it('keeps the verdict’s own publishability on the stored row, for Forge only', () => {
    expect(projected.scorerVersion).toBe('forge-1.0-rc');
    expect(projected.verdict).toMatchObject({
      family: 'forge',
      status: 'scored',
      publishable: true,
      unpublishable_reasons: [],
      runtime: 'wrapper',
      admission: { ceiling: 0.799 },
    });
    const sb = projectBenchScore({ scorerVersion: 'sb-7.2', status: 'x', publishable: false });
    expect(sb.verdict).not.toHaveProperty('status');
    expect(sb.verdict).not.toHaveProperty('publishable');
  });

  it('refuses what the scorer marks not board-grade, in its own words — and nothing else', () => {
    // The real verdict is rc: a measurement until the thresholds freeze.
    expect(forgePublishProblem(stored)).toBe(
      'Scored by forge-1.0-rc: the Forge thresholds are not frozen yet, so the result is not board-grade. Forge results publish once the forge-1.0 freeze pins them.'
    );
    const frozen = { ...stored, scorerVersion: 'forge-1.0' };
    expect(forgePublishProblem(frozen)).toBeNull();
    expect(
      forgePublishProblem({
        ...frozen,
        verdict: {
          ...projected.verdict,
          publishable: false,
          unpublishable_reasons: ['3 unavailable row(s)'],
        },
      })
    ).toBe('The scorer marked this Forge result unpublishable: 3 unavailable row(s).');
    expect(
      forgePublishProblem({
        ...frozen,
        verdict: { ...projected.verdict, status: 'held', harness_missing: ['GET /x', 'GET /y'] },
      })
    ).toMatch(/^This Forge result is held: the emulator met 2 calls it does not model/);
    // A verdict with no publishability at all is not assumed publishable.
    expect(forgePublishProblem({ scorerVersion: 'forge-1.0', verdict: {} })).toMatch(
      /carries no publishability verdict/
    );
  });

  it('publishes every Forge tier letter, L K T R S B U V A and E', () => {
    expect(forgePublishTiers(projected.tiers)).toEqual({
      L: 1,
      K: 0.8333,
      T: 1,
      R: 1,
      S: 1,
      B: 1,
      U: 1,
      V: 1,
      A: 1,
      E: 0.9286,
    });
    expect(forgePublishTiers({ L: 0.5 })).toMatchObject({ L: 0.5, K: 0, E: 0 });
  });
});

describe('the Forge kit status', () => {
  it('reads forge_kit.status() and the tier policy from the payload, never restating them', () => {
    expect(FORGE_KIT_STATUS_SCRIPT).toContain('forge_kit.status()');
    expect(FORGE_KIT_STATUS_SCRIPT).toContain('bench_budget.CALL_BUDGET');
    expect(FORGE_KIT_STATUS_SCRIPT).toContain('isolated_tiers.FORGE10');
    expect(
      parseForgeKitStatus(
        'noise\n{"ready": true, "missing": [], "kit_lock_sha256": "0a0b", "call_budget": 150, "wallet_usd": "50", "reasoning_effort": "medium"}\n'
      )
    ).toEqual({
      state: 'ready',
      missing: [],
      kitLockSha256: '0a0b',
      callBudget: 150,
      walletDefaultUsd: '50',
      reasoningEffort: 'medium',
    });
    expect(
      parseForgeKitStatus('{"ready": false, "missing": ["app-modules"], "call_budget": 150}')
    ).toMatchObject({ state: 'missing', missing: ['app-modules'] });
    // A malformed answer is an error — never "ready".
    expect(parseForgeKitStatus('Traceback (most recent call last)').state).toBe('error');
    expect(parseForgeKitStatus('{"missing": []}').state).toBe('error');
  });
});
