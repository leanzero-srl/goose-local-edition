import { execFileSync } from 'node:child_process';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { pickBenchShots, limitBenchShotsForPublish } from './benchShots';
import { forgePublishBody, forgePublishProblem, forgePublishTiers } from './benchForgePublish';
import {
  FORGE_KIT_MODULE,
  FORGE_KIT_STATUS_SCRIPT,
  parseForgeKitStatus,
  readForgeKitStatus,
} from './benchForgeKit';
import { projectBenchScore } from './benchScoreProjection';
import {
  FORGE_ERAS,
  FORGE_ERA_TIER_ORDER,
  FORGE_TIERS,
  FORGE_TIER_ORDER,
  TIERS,
  TIER_SCORER,
  familyOfScorer,
  forgeEra,
} from './components/benchmark/baselines';
import forgeVerdict from './components/benchmark/forge-alt.fixture.json';
import forge2Verdict from './components/benchmark/forge2-pilot.fixture.json';

/** The main-side Forge seams, each against a real verdict: score_forge.py's for the forge-1.0 alt golden app
 *  (forge-1.0-rc 0.799), and score_forge2.py's for the GPT-6.1 Sol pilot on forge-2.0 (2026-10-10, three
 *  scoring seeds, forge-2.0-rc 0.9618). */
const BENCH_DIR = path.resolve(__dirname, '..', '..', '..', 'evals', 'swarm-bench', 'bench');

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

  it('leads a 380 px-only run (both probes since 2006de559, forge-2.0) with its widget in both themes, each file once', async () => {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'forge2-shots-test-'));
    dirs.push(dir);
    await fs.mkdir(path.join(dir, 'forge-shots', 'seeds'), { recursive: true });
    // Every image forge2_probe.mjs left in the Sol pilot's forge-shots/ (2026-10-10), each with its own
    // bytes so the test can tell which file a pick took.
    for (const name of [
      'contact-sheet.png',
      'not-started-1340.png',
      ...[1173, 1176, 1259, 726, 757, 988].flatMap((sprint) => [
        `sprint-action-${sprint}-dark-800x600.png`,
        `sprint-action-${sprint}-light-800x600.png`,
      ]),
      'sprint-action-resolver-500-726.png',
      ...[129, 134, 51, 7].flatMap((board) => [
        `widget-edit-${board}-dark.png`,
        `widget-edit-${board}-light.png`,
        `widget-view-${board}-dark-380x480.png`,
        `widget-view-${board}-light-380x480.png`,
      ]),
      'widget-view-noconfig.png',
      'widget-view-second-instance.png',
    ])
      await fs.writeFile(path.join(dir, 'forge-shots', name), name);
    const shots = await pickBenchShots(dir);
    const file = (b64: string) => Buffer.from(b64, 'base64').toString();
    // The 380 px widget leads in both themes; the narrow pick would repeat the light lead, so it is skipped.
    expect(shots.map((shot) => [shot.name, shot.caption, file(shot.b64)])).toEqual([
      ['forge-widget-light', 'Dashboard widget · light', 'widget-view-129-light-380x480.png'],
      ['forge-widget-dark', 'Dashboard widget · dark', 'widget-view-129-dark-380x480.png'],
      ['forge-sprint-light', 'Sprint action · light', 'sprint-action-1173-light-800x600.png'],
      ['forge-sprint-dark', 'Sprint action · dark', 'sprint-action-1173-dark-800x600.png'],
      ['forge-edit', 'Widget edit view', 'widget-edit-129-light.png'],
      ['forge-noconfig', 'Widget before configuration', 'widget-view-noconfig.png'],
      ['forge-not-started', 'Sprint not started', 'not-started-1340.png'],
      ['forge-contact-sheet', 'Every captured surface', 'contact-sheet.png'],
    ]);
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

  it('publishes every forge-1.0 tier letter, L K T R S B U V A and E', () => {
    expect(forgePublishTiers(projected.tiers, 'forge-1.0')).toEqual({
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
    expect(forgePublishTiers({ L: 0.5 }, 'forge-1.0')).toMatchObject({ L: 0.5, K: 0, E: 0 });
  });

  it('publishes exactly forge-2.0’s nineteen letters — the v1 ten and R1–R9 — never dropping a v2 family', () => {
    const projected2 = projectBenchScore(forge2Verdict as never);
    const tiers = forgePublishTiers(projected2.tiers, 'forge-2.0');
    expect(Object.keys(tiers)).toEqual(FORGE_ERA_TIER_ORDER['forge-2.0']);
    expect(tiers).toMatchObject({ R: 0.748, E: 0.5039, R1: 1, R5: 0.9333, R7: 0.9889 });
    // A forge-1.0 result never carries a 2.0 letter (the site refuses unknown letters per era).
    expect(Object.keys(forgePublishTiers(projected2.tiers, 'forge-1.0'))).not.toContain('R1');
    expect(forgePublishTiers({ L: 0.5 }, 'forge-2.0')).toMatchObject({ L: 0.5, R1: 0, R9: 0 });
  });

  it('refuses a forge-2.0 rc result in its own era’s words, and a scorer naming no era this app knows', () => {
    const projected2 = projectBenchScore(forge2Verdict as never);
    const stored2 = { scorerVersion: projected2.scorerVersion, verdict: projected2.verdict };
    expect(projected2.scorerVersion).toBe('forge-2.0-rc');
    expect(forgePublishProblem(stored2)).toBe(
      'Scored by forge-2.0-rc: the Forge thresholds are not frozen yet, so the result is not board-grade. Forge results publish once the forge-2.0 freeze pins them.'
    );
    expect(forgePublishProblem({ ...stored2, scorerVersion: 'forge-2.0' })).toBeNull();
    expect(forgePublishProblem({ ...stored2, scorerVersion: 'forge-2.1' })).toBe(
      'forge-2.1 is not a Forge benchmark this app knows, so it cannot publish from here.'
    );
    expect(() =>
      forgePublishBody({ scorerVersion: 'forge-2.1', verdict: projected2.verdict })
    ).toThrow('forge-2.1 is not a Forge benchmark this app knows.');
  });
});

describe('the Forge eras this app knows', () => {
  it('maps every recorded Forge scorer to its era — rc to its tier — and nothing else', () => {
    expect(FORGE_ERAS).toEqual(
      TIERS.filter((tier) => familyOfScorer(TIER_SCORER[tier]) === 'forge')
    );
    expect(forgeEra('forge-1.0')).toBe('forge-1.0');
    expect(forgeEra('forge-1.0-rc')).toBe('forge-1.0');
    expect(forgeEra('forge-2.0')).toBe('forge-2.0');
    expect(forgeEra('forge-2.0-rc')).toBe('forge-2.0');
    for (const other of ['forge-2.1', 'forge-2.0-rc2', 'forge-2', 'sb-7.2', '', undefined])
      expect(forgeEra(other)).toBeNull();
  });

  it('carries each era’s tier letters exactly as its real scorer recorded them, every letter named', () => {
    expect(Object.keys(forgeVerdict.tiers)).toEqual(FORGE_ERA_TIER_ORDER['forge-1.0']);
    expect(Object.keys(forge2Verdict.tiers)).toEqual(FORGE_ERA_TIER_ORDER['forge-2.0']);
    for (const [verdict, era] of [
      [forgeVerdict, 'forge-1.0'],
      [forge2Verdict, 'forge-2.0'],
    ] as const)
      for (const row of verdict.checks) expect(FORGE_ERA_TIER_ORDER[era]).toContain(row.tier);
    // The view's order lists every era's letters once, each with a name a person reads.
    expect(FORGE_TIER_ORDER).toEqual(FORGE_ERA_TIER_ORDER['forge-2.0']);
    for (const tier of FORGE_TIER_ORDER) expect(FORGE_TIERS[tier]?.name).toBeTruthy();
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

  it('builds a forge-2.0 body from the REAL Sol verdict: nineteen tier means and every row, R1–R9 included', () => {
    const projected2 = projectBenchScore(forge2Verdict as never);
    const body = forgePublishBody({
      scorerVersion: 'forge-2.0',
      tiers: projected2.tiers,
      verdict: projected2.verdict,
    });
    expect(Object.keys(body.tiers as object)).toEqual(FORGE_ERA_TIER_ORDER['forge-2.0']);
    expect((body.tiers as Record<string, number>).R2).toBe(forge2Verdict.tiers.R2.mean);
    // Not one scorer row is dropped: the v2 families ride under their own letters.
    const rows = body.checksSummary as Array<{ check: string; tier: string; score: number }>;
    expect(rows.map((r) => [r.check, r.tier, r.score])).toEqual(
      forge2Verdict.checks.map((c) => [c.check, c.tier, c.score])
    );
    expect(rows.filter((r) => /^R\d$/.test(r.tier))).toHaveLength(32);
    expect(rows.find((r) => r.check === 'r5_nonadmin_refused')).toMatchObject({ tier: 'R5' });
    expect(body.admission).toEqual({ ceiling: 1, reasons: [], failedChecksByBand: [] });
    expect(body.rawScore).toBe(forge2Verdict.rawScore);
    expect(body.scoreInner).toBe(forge2Verdict.inner);
    expect(body.criticalMultiplier).toBe(1);
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
  it('runs against the real payload: the forge-2.0 kit (not 1.0’s) and the forge-2.0 tier’s own policy', async () => {
    const cache = await fs.mkdtemp(path.join(os.tmpdir(), 'forge-kit-cache-'));
    dirs.push(cache);
    const status = await readForgeKitStatus(
      path.dirname(BENCH_DIR),
      { python: 'python3', node: process.execPath, env: {} },
      cache
    );
    const lockSha = (module: string) =>
      execFileSync('python3', ['-B', `${module}.py`, 'lock-sha'], { cwd: BENCH_DIR })
        .toString()
        .trim();
    // An empty cache: nothing prepared, read without the network and without writing.
    expect(status).toMatchObject({ state: 'missing', reasoningEffort: 'medium' });
    expect(status.missing).toContain('app-modules');
    expect(await fs.readdir(cache)).toEqual([]);
    // The kit identity is forge2_kit's, which differs from forge-1.0's kit.
    expect(status.kitLockSha256).toBe(lockSha(FORGE_KIT_MODULE));
    expect(status.kitLockSha256).not.toBe(lockSha('forge_kit'));
    // The budget is the forge-2.0 tier's own (isolated_tiers FORGE20), not the shared 150.
    const tiers = await fs.readFile(path.join(BENCH_DIR, 'isolated_tiers.py'), 'utf8');
    const stated = Number(/FORGE20 = IsolatedTier\([\s\S]*?call_budget=(\d+)\)/.exec(tiers)?.[1]);
    expect(stated).toBeGreaterThan(150);
    expect(status.callBudget).toBe(stated);
  });

  it('reads the bundled tier’s kit module and policy from the payload, never restating them', () => {
    expect(FORGE_KIT_STATUS_SCRIPT).toContain(`${FORGE_KIT_MODULE}.status()`);
    expect(FORGE_KIT_STATUS_SCRIPT).toContain("isolated_tiers.BY_VERSION['forge-2.0']");
    expect(FORGE_KIT_STATUS_SCRIPT).toContain('tier.call_budget');
    expect(FORGE_KIT_STATUS_SCRIPT).toContain('tier.reasoning_effort');
    expect(FORGE_KIT_STATUS_SCRIPT).not.toContain('FORGE10');
    // No default dollar stop is read or shown (owner 2026-10-02).
    expect(FORGE_KIT_STATUS_SCRIPT).not.toContain('wallet');
    expect(
      parseForgeKitStatus(
        'noise\n{"ready": true, "missing": [], "kit_lock_sha256": "0a0b", "call_budget": 150, "reasoning_effort": "medium"}\n'
      )
    ).toEqual({
      state: 'ready',
      missing: [],
      kitLockSha256: '0a0b',
      callBudget: 150,
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
