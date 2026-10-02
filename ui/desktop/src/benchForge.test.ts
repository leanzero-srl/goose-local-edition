import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { pickBenchShots, limitBenchShotsForPublish } from './benchShots';
import { forgePublishBody, forgePublishProblem, forgePublishTiers } from './benchForgePublish';
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

describe('the forge publish body (INTEGRATION.md, as implemented on the site)', () => {
  it('builds tiers, checksSummary, admission + rawScore and share-valued gateConditions from the REAL verdict, no composition', () => {
    const projected = projectBenchScore(forgeVerdict as never);
    const stored = {
      scorerVersion: 'forge-1.0',
      tiers: projected.tiers,
      verdict: projected.verdict,
    };
    const body = forgePublishBody(stored);
    // Exactly the ten Forge letters, per-tier means, E the excellence slice (fraction × e_mean).
    expect(Object.keys(body.tiers as object)).toEqual([
      'L',
      'K',
      'T',
      'R',
      'S',
      'B',
      'U',
      'V',
      'A',
      'E',
    ]);
    expect((body.tiers as Record<string, number>).E).toBe(forgeVerdict.tiers.E.mean);
    // Every scorer row, each a forge check under its own tier letter, numbers verbatim.
    const rows = body.checksSummary as Array<{ check: string; tier: string; score: number }>;
    expect(rows).toHaveLength(forgeVerdict.checks.length);
    expect(rows.map((r) => [r.check, r.tier, r.score])).toEqual(
      forgeVerdict.checks.map((c) => [c.check, c.tier, c.score])
    );
    expect(rows.find((r) => r.check === 'k_widget_edit_bridge')).toMatchObject({
      tier: 'K',
      score: 0,
    });
    // The admission record and the earned score exactly as score_forge.py recorded them.
    expect(body.admission).toEqual({
      ceiling: 0.799,
      reasons: ['current platform, complete surfaces: k_widget_edit_bridge (maximum 0.799)'],
      failedChecksByBand: [
        {
          ceiling: 0.799,
          band: 'current platform, complete surfaces',
          checks: ['k_widget_edit_bridge'],
        },
      ],
    });
    expect(body.rawScore).toBe(forgeVerdict.rawScore);
    // Excellence conditions publish their share as the value.
    const gates = body.gateConditions as Array<{ name: string; ok: boolean; value: number }>;
    expect(gates).toHaveLength(forgeVerdict.excellence.conditions.length);
    expect(gates.find((g) => g.name === 'k_widget_edit_bridge')).toEqual({
      name: 'k_widget_edit_bridge',
      ok: false,
      value: 0,
    });
    expect(gates.find((g) => g.name === 't_event_rows')).toEqual({
      name: 't_event_rows',
      ok: true,
      value: 1,
    });
    expect(body.scoreInner).toBe(forgeVerdict.inner);
    expect(body.criticalMultiplier).toBe(forgeVerdict.critical.multiplier);
    expect(body).not.toHaveProperty('composition');
    expect(body).not.toHaveProperty('repairRounds');
    // The same stored row passes the local refusals once its identity is the frozen era's.
    expect(forgePublishProblem(stored)).toBeNull();
  });

  it('refuses a body the site would refuse: no admission, no rawScore or no rows', () => {
    const projected = projectBenchScore(forgeVerdict as never);
    const base = { scorerVersion: 'forge-1.0', verdict: projected.verdict };
    for (const drop of ['admission', 'rawScore', 'checks'])
      expect(
        forgePublishProblem({
          ...base,
          verdict: { ...projected.verdict, [drop]: drop === 'checks' ? [] : undefined },
        })
      ).toMatch(/lacks its admission record, earned score or check rows/);
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
