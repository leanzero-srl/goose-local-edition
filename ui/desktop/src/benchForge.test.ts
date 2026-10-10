import { execFileSync } from 'node:child_process';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import {
  pickBenchShots,
  limitBenchShotsForPublish,
  readBenchShotsSnapshot,
  writeBenchShotsSnapshot,
} from './benchShots';
import { forgePublishBody, forgePublishProblem, forgePublishTiers } from './benchForgePublish';
import { forgeHasReliability, readForgeReliability } from './benchForgeReliability';
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
import forge2Haiku from './components/benchmark/forge2-haiku-reliability.fixture.json';
import forge2Reference from './components/benchmark/forge2-reference-reliability.fixture.json';
import forge2Sol from './components/benchmark/forge2-sol-reliability.fixture.json';
import forge2Sonnet from './components/benchmark/forge2-sonnet-reliability.fixture.json';

/** The main-side Forge seams, each against a real verdict: score_forge.py's for the forge-1.0 alt golden app
 *  (forge-1.0-rc 0.799), and score_forge2.py's for the GPT-6.1 Sol pilot on forge-2.0 (2026-10-10, three
 *  scoring seeds, forge-2.0-rc 0.9618) — kept as scored BEFORE the reliability rule, so it carries no
 *  `reliability` block. The four `*-reliability` fixtures are score_forge2.py recompose() output under the
 *  group rule (branch forge2/final, score_forge2.py sha256 3bc9d9b48961…), each the scorer's whole object:
 *  Sonnet 5.5 on the hardened task (0.9628 → 0.745), GPT-6.1 Sol on the hardened task (0.9793 → 0.8248),
 *  Haiku (0.3378 → 0.0998, unpublishable) and the reference app at seed 0123456789abcdef (0.9907,
 *  reliability 1). */
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
    // No admin-panel file, no admin-panel picture: nothing stands in for it.
    expect(shots.some((shot) => /admin/i.test(shot.name + shot.caption))).toBe(false);
  });

  // The Sol pilot's forge-shots/ again, plus the three pictures the harness adds when the UI Kit admin page
  // rendered (forge2/admin-shot). Each file holds its own name so a pick can be told apart.
  const SOL_SHOTS = [
    'contact-sheet.png',
    'not-started-1340.png',
    'sprint-action-1173-dark-800x600.png',
    'sprint-action-1173-light-800x600.png',
    'widget-edit-129-light.png',
    'widget-view-129-dark-380x480.png',
    'widget-view-129-light-380x480.png',
    'widget-view-noconfig.png',
  ];
  const ADMIN_SHOTS = [
    'admin-panel-dark.png',
    'admin-panel-light.png',
    'admin-panel-saved-light.png',
  ];
  async function shotsDir(files: string[]) {
    const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'forge2-admin-shots-test-'));
    dirs.push(dir);
    await fs.mkdir(path.join(dir, 'forge-shots'));
    for (const name of files) await fs.writeFile(path.join(dir, 'forge-shots', name), name);
    return dir;
  }
  const NOTE =
    "the app's component tree, drawn by the benchmark's UI Kit host. Jira draws the same tree with its own components.";

  it('shows the UI Kit admin panel after the widget and sprint leads, and publishes it in light and dark within the site’s five', async () => {
    const shots = await pickBenchShots(await shotsDir([...SOL_SHOTS, ...ADMIN_SHOTS]));
    const file = (b64: string) => Buffer.from(b64, 'base64').toString();
    expect(shots.map((shot) => [shot.name, file(shot.b64)])).toEqual([
      ['forge-widget-light', 'widget-view-129-light-380x480.png'],
      ['forge-widget-dark', 'widget-view-129-dark-380x480.png'],
      ['forge-sprint-light', 'sprint-action-1173-light-800x600.png'],
      ['forge-sprint-dark', 'sprint-action-1173-dark-800x600.png'],
      ['forge-admin-light', 'admin-panel-light.png'],
      ['forge-admin-dark', 'admin-panel-dark.png'],
      ['forge-admin-saved', 'admin-panel-saved-light.png'],
      ['forge-edit', 'widget-edit-129-light.png'],
      ['forge-noconfig', 'widget-view-noconfig.png'],
      ['forge-not-started', 'not-started-1340.png'],
      ['forge-contact-sheet', 'contact-sheet.png'],
    ]);
    // The caption (the picture's alt text too) says what the picture is: the benchmark's host drew it.
    expect(
      shots.filter((shot) => shot.name.startsWith('forge-admin')).map((s) => s.caption)
    ).toEqual([
      `Admin panel (UI Kit) · light: ${NOTE}`,
      `Admin panel (UI Kit) · dark: ${NOTE}`,
      `Admin panel (UI Kit) · after saving: ${NOTE}`,
    ]);
    // leanzero.net's route takes five pictures, a filename-safe name and a caption of at most 200
    // characters each: one of every surface, the admin panel in both themes.
    const published = limitBenchShotsForPublish(shots);
    expect(published.map((shot) => shot.name)).toEqual([
      'forge-widget-light',
      'forge-widget-dark',
      'forge-sprint-light',
      'forge-admin-light',
      'forge-admin-dark',
    ]);
    for (const shot of shots) {
      expect(shot.caption.length).toBeLessThanOrEqual(200);
      expect(shot.name).toMatch(/^[a-z0-9][a-z0-9._-]{0,59}$/i);
    }
  });

  it('shows and publishes nothing for the admin panel when the run has no admin-panel file', async () => {
    const shots = await pickBenchShots(await shotsDir(SOL_SHOTS));
    expect(shots.map((shot) => shot.name)).toEqual([
      'forge-widget-light',
      'forge-widget-dark',
      'forge-sprint-light',
      'forge-sprint-dark',
      'forge-edit',
      'forge-noconfig',
      'forge-not-started',
      'forge-contact-sheet',
    ]);
    expect(limitBenchShotsForPublish(shots).map((shot) => shot.name)).toEqual([
      'forge-widget-light',
      'forge-widget-dark',
      'forge-sprint-light',
      'forge-sprint-dark',
      'forge-edit',
    ]);
  });

  it('keeps the pick order through the result row’s snapshot, so the latest run publishes its leads too', async () => {
    const dir = await shotsDir([...SOL_SHOTS, ...ADMIN_SHOTS]);
    const shots = await pickBenchShots(dir);
    const names = shots.map((shot) => shot.name);
    // The hazard: by name the contact sheet and the edit view come before every lead.
    expect(names.slice().sort()).not.toEqual(names);
    const snapshot = path.join(dir, 'shots-snapshot');
    await writeBenchShotsSnapshot(snapshot, shots);
    const read = await readBenchShotsSnapshot(snapshot);
    expect(read).toEqual(shots);
    expect(limitBenchShotsForPublish(read)).toEqual(limitBenchShotsForPublish(shots));
    // A snapshot an earlier build wrote records no order: its Forge picks still publish as the leads.
    const legacy = path.join(dir, 'legacy-snapshot');
    await fs.mkdir(legacy);
    for (const shot of shots)
      await fs.writeFile(
        path.join(legacy, `${shot.name}.json`),
        JSON.stringify({ caption: shot.caption, b64: shot.b64 })
      );
    const old = await readBenchShotsSnapshot(legacy);
    expect(old.map((shot) => shot.name).sort()).toEqual(names.slice().sort());
    expect(limitBenchShotsForPublish(old).map((shot) => shot.name)).toEqual([
      'forge-widget-light',
      'forge-widget-dark',
      'forge-sprint-light',
      'forge-admin-light',
      'forge-admin-dark',
    ]);
    // No snapshot at all is an empty read (the publisher then reads the run's own tree), never a throw.
    expect(await readBenchShotsSnapshot(path.join(dir, 'absent'))).toEqual([]);
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
    // The frozen era's identity publishes a verdict scored under the era's current rule (Sol on the hardened
    // task, recomposed with its reliability block); the pre-rule verdict is refused further down.
    const ruled = projectBenchScore(forge2Sol as never);
    expect(forgePublishProblem({ scorerVersion: 'forge-2.0', verdict: ruled.verdict })).toBeNull();
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

  it('builds a forge-2.0 body from a REAL Sol verdict: nineteen tier means and every row, R1–R9 included', () => {
    const projected2 = projectBenchScore(forge2Sol as never);
    const body = forgePublishBody({
      scorerVersion: 'forge-2.0',
      tiers: projected2.tiers,
      verdict: projected2.verdict,
    });
    expect(Object.keys(body.tiers as object)).toEqual(FORGE_ERA_TIER_ORDER['forge-2.0']);
    expect((body.tiers as Record<string, number>).R2).toBe(forge2Sol.tiers.R2.mean);
    // Not one scorer row is dropped: the v2 families ride under their own letters.
    const rows = body.checksSummary as Array<{ check: string; tier: string; score: number }>;
    expect(rows.map((r) => [r.check, r.tier, r.score])).toEqual(
      forge2Sol.checks.map((c) => [c.check, c.tier, c.score])
    );
    expect(rows.filter((r) => /^R\d$/.test(r.tier))).toHaveLength(32);
    expect(rows.find((r) => r.check === 'r5_nonadmin_refused')).toMatchObject({ tier: 'R5' });
    expect(body.admission).toEqual({ ceiling: 1, reasons: [], failedChecksByBand: [] });
    expect(body.rawScore).toBe(forge2Sol.rawScore);
    expect(body.scoreInner).toBe(forge2Sol.inner);
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

// The numbers each recomposed verdict states (read from the fixture files; listed once so a re-composition
// changes one table). `final` is the verdict's score, which for these four is its earned score (no ceiling).
const RELIABILITY_CASES = {
  sonnet: { verdict: forge2Sonnet, inner: 0.9628, reliability: 0.7738, final: 0.745, groups: 8 },
  sol: { verdict: forge2Sol, inner: 0.9793, reliability: 0.8422, final: 0.8248, groups: 9 },
  haiku: { verdict: forge2Haiku, inner: 0.3378, reliability: 0.2954, final: 0.0998, groups: 14 },
  reference: { verdict: forge2Reference, inner: 0.9907, reliability: 1, final: 0.9907, groups: 0 },
} as const;

describe('forge-2.0 publishes the scorer’s reliability evidence (score_forge2.py reliability(); the site’s route v2.9)', () => {
  const stored20 = (verdict: unknown) => {
    const projected = projectBenchScore(verdict as never);
    return { scorerVersion: 'forge-2.0', tiers: projected.tiers, verdict: projected.verdict };
  };
  type Defect = { tier: string; check: string; score: number; factor: number };

  it('keeps the scorer’s block whole on the stored row, and knows which eras carry the rule', () => {
    for (const { verdict } of Object.values(RELIABILITY_CASES))
      expect(
        (projectBenchScore(verdict as never).verdict as { reliability?: unknown }).reliability
      ).toEqual(verdict.reliability);
    expect(readForgeReliability(forge2Sonnet.reliability)).toEqual(forge2Sonnet.reliability);
    expect(forgeHasReliability('forge-2.0')).toBe(true);
    expect(forgeHasReliability('forge-2.0-rc')).toBe(true);
    for (const other of ['forge-1.0', 'forge-1.0-rc', 'sb-7.2', undefined])
      expect(forgeHasReliability(other)).toBe(false);
    // Not the scorer's shape is not a record — never half-read. The per-row shape the rule had before
    // (root, root_score, unexercised; no tier, no priced_as_critical) is refused too.
    const block = forge2Sonnet.reliability;
    const { priced_as_critical: _priced, ...withoutPriced } = block;
    for (const broken of [
      undefined,
      null,
      1,
      { multiplier: 1 },
      { ...block, multiplier: '0.7738' },
      { ...block, defects: [{ check: 'u_widget_live', score: 0, factor: 0.9 }] },
      { ...block, defects: [{ tier: 'U', check: 'u_widget_live', score: 0 }] },
      { ...block, folded: { U: 'u_widget_chart' } },
      withoutPriced,
      { ...withoutPriced, unexercised: [] },
    ])
      expect(readForgeReliability(broken)).toBeNull();
  });

  it.each(Object.entries(RELIABILITY_CASES).filter(([name]) => name !== 'haiku'))(
    'posts what the route requires for %s, each field the verdict’s own',
    (_name, { verdict, inner, reliability, final, groups }) => {
      const stored = stored20(verdict);
      expect(forgePublishProblem(stored)).toBeNull();
      const body = forgePublishBody(stored);
      // Exactly these keys: the route refuses any other (and the block's k, floor, folded and
      // priced_as_critical lists are the site's own to derive from the posted rows, never posted).
      expect(Object.keys(body).sort()).toEqual(
        [
          'admission',
          'checksSummary',
          'criticalFloor',
          'criticalMultiplier',
          'criticalRows',
          'excellenceEMean',
          'excellenceFraction',
          'gateConditions',
          'preSeverityScore',
          'rawScore',
          'reliability',
          'reliabilityDefects',
          'scoreInner',
          'tiers',
        ].sort()
      );
      expect(body.criticalMultiplier).toBe(verdict.critical.multiplier);
      expect(body.rawScore).toBe(verdict.rawScore);
      expect(body.rawScore).toBe(final);
      expect(body.scoreInner).toBe(inner);
      expect(body.reliability).toBe(verdict.reliability.multiplier);
      expect(body.reliability).toBe(reliability);
      // The scorer's defects passed through: one per group that multiplies, in its order, its four keys.
      const defects = body.reliabilityDefects as Defect[];
      expect(defects).toEqual(verdict.reliability.defects);
      expect(defects).toHaveLength(groups);
      for (const defect of defects)
        expect(Object.keys(defect)).toEqual(['tier', 'check', 'score', 'factor']);
      // The rows carry the scorer's detail from its first character, cut only at the route's 220-character
      // limit; a group's worst test is a posted row with that score.
      const rows = body.checksSummary as Array<{ check: string; score: number; detail?: string }>;
      expect(rows).toHaveLength(verdict.checks.length);
      rows.forEach((row, i) => {
        expect(row.check).toBe(verdict.checks[i].check);
        expect(row.detail).toBe(verdict.checks[i].detail.slice(0, 220));
      });
      for (const defect of defects)
        expect(rows.find((row) => row.check === defect.check)?.score).toBe(defect.score);
      // The route's own equation on the posted numbers: weighted tier means × critical multiplier ×
      // reliability is the earned score it ties the final to (its tolerance, 2e-4).
      const tiers = body.tiers as Record<string, number>;
      const weights = verdict.tiers as Record<string, { weight: number }>;
      const tests = Object.keys(tiers).reduce((sum, t) => sum + tiers[t] * weights[t].weight, 0);
      expect(Math.abs(tests - inner)).toBeLessThan(1e-3);
      expect(
        Math.abs(tests * (body.criticalMultiplier as number) * reliability - final)
      ).toBeLessThan(2e-4);
    }
  );

  it('posts Sonnet’s widget group once, by its worst test — the group’s other failed tests are not posted again', () => {
    const defects = forgePublishBody(stored20(forge2Sonnet)).reliabilityDefects as Defect[];
    const widget = defects.filter((d) => d.tier === 'U');
    expect(widget).toHaveLength(1);
    expect(widget[0]).toEqual(forge2Sonnet.reliability.defects.find((d) => d.tier === 'U'));
    for (const other of forge2Sonnet.reliability.folded.U ?? [])
      expect(defects.some((d) => d.check === other)).toBe(false);
    // One entry per group: no tier twice, and none for the excellence tier.
    expect(new Set(defects.map((d) => d.tier)).size).toBe(defects.length);
    expect(defects.some((d) => d.tier === 'E')).toBe(false);
  });

  it('refuses a forge-2.0 verdict without `reliability`, naming the field — and never builds a body with a default factor', () => {
    // The pilot verdict as scored before the rule: publishable by its scorer, no reliability block.
    const before = stored20(forge2Verdict);
    expect(before.verdict).not.toHaveProperty('reliability');
    expect(forgePublishProblem(before)).toBe(
      'This Forge 2.0 result has no `reliability` record: it was scored before failed tests multiplied the score. Re-score the saved build to publish it.'
    );
    expect(() => forgePublishBody(before)).toThrow(/has no `reliability` record/);
    // The same refusal for a ruled verdict that lost its block, a block that is not the scorer's shape,
    // and a verdict without the critical multiplier the route requires beside it.
    const ruled = stored20(forge2Sonnet);
    const without = (patch: Record<string, unknown>) => ({
      ...ruled,
      verdict: { ...ruled.verdict, ...patch },
    });
    expect(forgePublishProblem(without({ reliability: undefined }))).toMatch(
      /has no `reliability` record/
    );
    expect(forgePublishProblem(without({ reliability: { multiplier: 1 } }))).toBe(
      "This Forge 2.0 result's `reliability` record is not what its scorer writes (the multiplier, and each group's worst test with its factor). Re-score the saved build to publish it."
    );
    expect(() => forgePublishBody(without({ reliability: { multiplier: 1 } }))).toThrow(
      /is not what its scorer writes/
    );
    expect(forgePublishProblem(without({ critical: { floor: 0.6 } }))).toMatch(
      /has no `critical\.multiplier`, which leanzero\.net requires/
    );
    expect(() => forgePublishBody(without({ critical: { floor: 0.6 } }))).toThrow(
      /critical\.multiplier/
    );
    // An rc result is refused as rc first: a re-score under an rc scorer stays rc.
    expect(forgePublishProblem({ ...before, scorerVersion: 'forge-2.0-rc' })).toMatch(
      /^Scored by forge-2\.0-rc: the Forge thresholds are not frozen yet/
    );
    expect(() => forgePublishBody({ ...before, scorerVersion: 'forge-2.0-rc' })).toThrow(
      /has no `reliability` record/
    );
  });

  it('refuses Haiku in its scorer’s own words (rows the harness could not run)', () => {
    expect(forgePublishProblem(stored20(forge2Haiku))).toBe(
      `The scorer marked this Forge result unpublishable: ${forge2Haiku.unpublishable_reasons.join('; ')}.`
    );
    expect(forge2Haiku.unpublishable_reasons).toEqual(['9 unavailable row(s)']);
  });

  it('posts no reliability key for forge-1.0, which has no such rule (the site refuses them there)', () => {
    const projected = projectBenchScore(forgeVerdict as never);
    const body = forgePublishBody({
      scorerVersion: 'forge-1.0',
      tiers: projected.tiers,
      verdict: { ...projected.verdict, reliability: forge2Sonnet.reliability },
    });
    expect(body).not.toHaveProperty('reliability');
    expect(body).not.toHaveProperty('reliabilityDefects');
    expect(
      forgePublishProblem({ scorerVersion: 'forge-1.0', verdict: projected.verdict })
    ).toBeNull();
  });

  it('omits a row’s detail when the scorer wrote none — the route refuses an empty one', () => {
    const ruled = stored20(forge2Reference);
    const checks = (ruled.verdict.checks as Array<Record<string, unknown>>).map((row, i) =>
      i === 0 ? { ...row, detail: '' } : i === 1 ? { ...row, detail: undefined } : row
    );
    const rows = forgePublishBody({ ...ruled, verdict: { ...ruled.verdict, checks } })
      .checksSummary as Array<Record<string, unknown>>;
    expect(rows[0]).toEqual({ check: 'l_deployable', tier: 'L', score: 1 });
    expect(rows[1]).not.toHaveProperty('detail');
    expect(rows[2]).toHaveProperty('detail');
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
