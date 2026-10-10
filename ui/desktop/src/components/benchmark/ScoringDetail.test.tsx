import { render, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { ScoringDetail, type VerdictDetail } from './ScoringDetail';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';

/** lucide stamps `lucide lucide-<name>` identifiers on its svgs — names, not utilities. */
const utilitiesOf = (classes: string[]) => classes.filter((c) => !c.startsWith('lucide'));

// Distilled from a REAL verdict (evals/swarm-bench/runs/nodeloop/baseline-n3-r3/verdict.json,
// score 0.8645) so the composition arithmetic is checked against scorer truth, not an invented
// fixture: 0.60·0.827 + 0.15·0.9 + 0.10·0.8333 + 0.05·1.0 + 0.10·1.0 = 0.8645.
const verdict: VerdictDetail = {
  checks: [
    {
      check: 'modules_present',
      tier: 'A',
      score: 1.0,
      detail: '5/5 named files',
      consequence: 'files the spec names by path are missing',
      parts: { 'meridian.py': true, 'store.py': true },
    },
    {
      check: 'sync_completeness',
      tier: 'B',
      score: 0.5,
      detail: '123/247 payments after one sync',
      consequence: 'the tool does not actually sync the vendor data',
      parts: { synced: 123, expected: 247 },
    },
    {
      check: 'second_sync_cost',
      tier: 'C',
      score: 0.0,
      detail: 'second sync re-fetched every page',
      consequence: 'every sync repays the full cost',
    },
    { check: 'journey_loads', tier: 'J', score: 1.0, detail: 'rows render in a real browser' },
    { check: 'visual_typography', tier: 'V', score: 0.8333, detail: 'system font stack present' },
    { check: 'perf_list_p95', tier: 'P', score: 1.0, detail: 'p95 0.59ms (budget 150)' },
  ],
  tiers: {
    A: { mean: 1.0, checks: 6, weight: 0.25 },
    B: { mean: 0.7316, checks: 16, weight: 0.3 },
    C: { mean: 0.8, checks: 10, weight: 0.25 },
    D: { mean: 0.7875, checks: 8, weight: 0.2 },
    HARD: { mean: 1.0, checks: 6, weight: 0.1 },
    J: { mean: 0.9, checks: 5, weight: 0.15 },
    V: { mean: 0.8333, checks: 6, weight: 0.1 },
    P: { mean: 1.0, checks: 3, weight: 0.05 },
  },
  core: 0.827,
  hard: 1.0,
  root_causes: { sync_completeness: ['total_field', 'summary_accuracy'] },
  findingsHeld: ['the served page renders NO data rows in a real browser'],
  repairRounds: [
    { round: 0, findings: 2 },
    { round: 1, findings: 1 },
    { round: 2, findings: 0 },
  ],
};

describe('ScoringDetail', () => {
  it('shows the composition with each component weight and the exact final score', () => {
    const { getByText, getAllByText } = render(<ScoringDetail verdict={verdict} score={0.8645} />);
    getByText('Core build');
    // 'Journey' appears in BOTH the composition table and its tier group header — by design.
    expect(getAllByText('Journey').length).toBeGreaterThanOrEqual(2);
    getByText('Hard block');
    // Contributions of 100: core 0.827×60 = 49.6, hard 1.0×10 = 10.0, final 86.5 (scorer truth).
    getByText('49.6');
    getByText('86.5');
  });

  it('renders every check with its evidence verbatim, and consequence only on lost points', () => {
    const { getByText, queryByText } = render(<ScoringDetail verdict={verdict} score={0.8645} />);
    // The worst imperfect tier (B, 0.7316) auto-opens; its rows carry detail + consequence.
    getByText('123/247 payments after one sync');
    getByText(/the tool does not actually sync/);
    // A perfect check's consequence never renders as a cost (A group is collapsed AND score=1).
    expect(queryByText(/files the spec names by path are missing/)).toBeNull();
  });

  it('marks hard-block checks and tells the repair story', () => {
    const { getByText, getAllByText } = render(<ScoringDetail verdict={verdict} score={0.8645} />);
    getByText(/Findings that held/);
    getByText('the served page renders NO data rows in a real browser');
    getByText('Round 0 · 2 findings');
    getByText('Round 2 · 0 findings');
    // Root-cause attribution names the root and the count it zeroed.
    getByText(/failed at the root and zeroed 2 downstream check/);
    expect(getAllByText(/lost point/).length).toBeGreaterThan(0);
  });

  it('scores read as the status triad (full ok, partial warn, nothing err) on Studio tokens only — no node hue, no hex', async () => {
    const { container } = render(<ScoringDetail verdict={verdict} score={0.8645} />);
    const tone = (chip: Element) => chip.getAttribute('data-tone');
    const chips = [...container.querySelectorAll('[data-testid="score-chip"]')];
    // The open B group (0.7316) is a warn chip; its 0.5 row warn; the collapsed A group's 1.0 is ok.
    expect(chips.some((c) => tone(c) === 'ok' && c.classList.contains('bg-lz-ok-solid'))).toBe(
      true
    );
    expect(chips.some((c) => tone(c) === 'warn' && c.classList.contains('bg-lz-warn-solid'))).toBe(
      true
    );
    // The held findings sit under a solid err header, and the composition earned-fill is the accent.
    expect(
      container.querySelector('[data-testid="findings-held"] .bg-lz-err-solid')
    ).not.toBeNull();
    expect(container.querySelectorAll('svg rect.fill-lz-accent').length).toBeGreaterThan(0);
    expect(container.innerHTML).not.toMatch(/color-node-|color-block|#[0-9a-f]{6}/i);
    assertStudioClean(container);
    expect(await missingUtilities(utilitiesOf(allClasses(container)))).toEqual([]);
  }, 30_000);
});

import { fireEvent } from '@testing-library/react';
import sb8Perfect from './sb8-perfect.fixture.json';
import sb8Failed from './sb8-failed.fixture.json';

describe('SB8 scoring evidence', () => {
  it('renders all five numeric tiers and the scorer formula, without the legacy hard block', () => {
    const { getAllByText, getByRole, queryByText } = render(
      <ScoringDetail verdict={sb8Perfect} score={sb8Perfect.score} />
    );
    for (const name of [
      'Backend foundation',
      'Transactional correctness',
      '3D scene',
      'Interaction',
      'Excellence',
    ]) {
      expect(getAllByText(name).length).toBe(2);
    }
    expect(queryByText('Hard block')).toBeNull();
    expect(queryByText('Core build')).toBeNull();
    fireEvent.click(getByRole('button', { name: /Backend foundation/ }));
    expect(getAllByText('Boot state')).toHaveLength(1);
    fireEvent.click(getByRole('button', { name: /Excellence.*Clean console/ }));
    expect(getAllByText('Clean console')).toHaveLength(1);
  });

  it('shows the failed check evidence, excellence adjustment and actual critical multiplier', () => {
    const { getByText, getAllByText } = render(
      <ScoringDetail verdict={sb8Failed} score={sb8Failed.score} />
    );
    getByText('Swept collision');
    getByText('expected HTTP 409, got 200');
    getByText('× 0.6000');
    getByText('-0.1');
    // Actual Python evaluate output with one of 34 transactional checks failed.
    expect(getAllByText('59.6')).toHaveLength(1);
  });
});

import { projectBenchScore } from '../../benchScoreProjection';
import truncatedDesktopResult from './sb8-desktop-truncated.fixture.json';

it('renders the actual main-process projection after disk serialization as SB8', () => {
  const stored = JSON.parse(JSON.stringify(projectBenchScore(sb8Failed)));
  const { queryByText, getByText } = render(
    <ScoringDetail
      verdict={stored.verdict}
      score={sb8Failed.score}
      scorerVersion={stored.scorerVersion}
    />
  );
  expect(queryByText('Core build')).toBeNull();
  getByText('× 0.6000');
  getByText('Swept collision');
});

it('does not claim the legacy formula for the truncated result observed in the running app', () => {
  const stored = JSON.parse(JSON.stringify(truncatedDesktopResult));
  const { queryByText, getByText } = render(
    <ScoringDetail
      verdict={stored.verdict}
      score={stored.score}
      scorerVersion={stored.scorerVersion}
    />
  );
  expect(queryByText('Core build')).toBeNull();
  getByText(/missing its composition inputs/);
  getByText('Backend foundation');
});

import planningFailed from './sb8-planning-failed.fixture.json';

it('shows recorded route-planning weight and includes its failure in the excellence adjustment', () => {
  const { getByText, getAllByText } = render(
    <ScoringDetail verdict={planningFailed} score={planningFailed.score} />
  );
  expect(getAllByText('Route planning')).toHaveLength(2);
  getByText('25%');
  getByText('-5.0');
  getByText('70.0');
  expect(getAllByText('route absent').length).toBeGreaterThan(0);
  getByText(/A, B, C, D, F/);
});

it('keeps the historical perfect score and does not invent F for an older result', () => {
  const { getAllByText, queryByText } = render(
    <ScoringDetail verdict={sb8Perfect} score={sb8Perfect.score} />
  );
  expect(getAllByText('100.0').length).toBeGreaterThan(0);
  expect(queryByText('Route planning')).toBeNull();
});

it('does not apply the historical formula to F when the recorded metadata is missing', () => {
  const { weights: _weights, core_tiers: _core, ...missing } = planningFailed;
  const { getByText, getAllByText, queryByText } = render(
    <ScoringDetail verdict={missing} score={missing.score} />
  );
  getByText(/missing its composition inputs/);
  expect(queryByText('Weighted subtotal')).toBeNull();
  expect(getAllByText('route absent').length).toBeGreaterThan(0);
});

/** The payments family: SB7.1 scored S/Q/M as admission gates, SB7.2 weights them into the score. */
const paymentsVerdict = (sqm: { weight: number; admission_only?: boolean }) =>
  ({
    checks: [
      { check: 'a_files', tier: 'A', score: 1 },
      { check: 'x_concurrent', tier: 'X', score: 0.5 },
      { check: 's_tower_geometry', tier: 'S', score: 1 },
      { check: 'q_legible_presentation', tier: 'Q', score: 0.5 },
      { check: 'm_committed_event_replay', tier: 'M', score: 0 },
    ],
    tiers: {
      A: { mean: 1, checks: 1, weight: 0.5 },
      X: { mean: 0.5, checks: 1, weight: 0.5 - (sqm.admission_only ? 0 : 3 * sqm.weight) },
      S: { mean: 1, checks: 1, ...sqm },
      Q: { mean: 0.5, checks: 1, ...sqm },
      M: { mean: 0, checks: 1, ...sqm },
    },
    inner: 0.75,
    rawScore: 0.7,
    critical: { multiplier: 0.9, rows: [] },
    excellence: { fraction: 0.5, e_mean: 0.4, conditions: {} },
    admission: {
      visible: true,
      matching: true,
      good: false,
      excellence: { visual: false, backend: true },
      ceiling: 0.699,
      reasons: ['Readable presentation and field interaction: q_legible_presentation'],
    },
  }) as VerdictDetail;

it('shows SB7.2 visual tiers S/Q/M as weighted rows of the score, with their points', () => {
  const view = render(
    <ScoringDetail
      verdict={paymentsVerdict({ weight: 0.1 })}
      score={0.699}
      scorerVersion="sb-7.2"
    />
  );
  const table = view.getByRole('region', { name: 'Weighted tiers' });
  for (const [label, earned, weight, points] of [
    ['S 3D structure', '100.0%', '10%', '10.0'],
    ['Q Presentation', '50.0%', '10%', '5.0'],
    ['M Animation', '0.0%', '10%', '0.0'],
  ]) {
    const row = Array.from(table.querySelectorAll('tr')).find(
      (tr) => tr.querySelector('td')?.textContent === label
    );
    expect(row, label).toBeTruthy();
    expect(Array.from(row!.querySelectorAll('td')).map((td) => td.textContent)).toEqual([
      label,
      earned,
      weight,
      points,
    ]);
  }
  // 0.5·1 + 0.2·0.5 + 0.1·1 + 0.1·0.5 + 0.1·0 = 0.75
  view.getByText('75.0');
  expect(view.queryByText(/Admission gates/)).toBeNull();
  expect(view.queryAllByText(/admission gate$/)).toHaveLength(0);
  expect(view.getAllByText(/1 checks · weight 10%/, { selector: 'span' })).toHaveLength(3);
  // No earlier release's constants restated around SB7.2's recorded inputs.
  expect(view.queryByText(/0\.88 ×/)).toBeNull();
  view.getByText(/Recorded composition inputs: behavioral score 0\.7500/);
  view.getByText('Admission ceiling');
  // Ceiling and final score both 0.699: the admission ceiling still caps SB7.2.
  expect(view.getAllByText('0.699', { selector: 'dd' })).toHaveLength(2);
});

it('keeps SB7.1 S/Q/M as admission gates — no weight row, no 0% tier — and its own formula', () => {
  const view = render(
    <ScoringDetail
      verdict={paymentsVerdict({ weight: 0, admission_only: true })}
      score={0.699}
      scorerVersion="sb-7.1"
    />
  );
  const table = view.getByRole('region', { name: 'Weighted tiers' });
  expect(table.querySelector('table')?.textContent).not.toMatch(/3D structure/);
  view.getByText(
    'Admission gates, no weight — they set the ceiling and add no points: S 3D structure 100% · Q Presentation 50% · M Animation 0%.',
    { exact: false }
  );
  expect(view.getAllByText(/admission gate$/)).toHaveLength(3);
  view.getByText(/Earned credit: \(0\.88 × behavioral score 0\.7500/);
});

import forgeVerdict from './forge-alt.fixture.json';
describe('a Forge verdict reads its own tiers, bands and publishability (score_forge.py)', () => {
  const projected = projectBenchScore(forgeVerdict as never).verdict as unknown as VerdictDetail;

  it('groups checks under the Forge tier names, never the Gauntlet letters they share', async () => {
    const view = render(
      <ScoringDetail verdict={projected} score={0.799} scorerVersion="forge-1.0-rc" />
    );
    for (const name of [
      'Lint',
      'Platform currency',
      'Event pipeline',
      'Reconcile',
      'Storage',
      'Resolvers',
      'UI function',
      'Visual',
      'Rovo',
      'Excellence',
    ])
      expect(view.getAllByText(name).length).toBeGreaterThan(0);
    // B is Resolvers here, not Gauntlet's Behaviour; A is Rovo, not Structure.
    expect(view.queryByText('Behaviour')).toBeNull();
    expect(view.queryByText('Structure')).toBeNull();
    view.getByText(
      /Recorded composition inputs: inner score 0\.9833 .* before criticals 0\.9768 · critical multiplier 1\.0000/
    );
    assertStudioClean(view.container);
    expect(await missingUtilities(utilitiesOf(allClasses(view.container)))).toEqual([]);
  });

  it('a verdict with no failed band says no ceiling applied; an unpublishable one says why', () => {
    const passing = {
      ...projected,
      admission: { ceiling: 1, reasons: [], failedChecksByBand: [] },
      publishable: false,
      unpublishable_reasons: ['runtime shim (the in-repo shim, not the pinned Forge wrapper)'],
      runtime: 'shim',
    } as unknown as VerdictDetail;
    const view = render(
      <ScoringDetail verdict={passing} score={0.9925} scorerVersion="forge-1.0" />
    );
    view.getByText('Every admission band passed — no ceiling');
    expect(view.queryByTestId('forge-failed-band')).toBeNull();
    const facts = view.getByTestId('forge-verdict-facts');
    expect(facts).toHaveTextContent(
      'Not publishable: runtime shim (the in-repo shim, not the pinned Forge wrapper).'
    );
    expect(facts).toHaveTextContent("Scored on the shim runtime, not Atlassian's pinned wrapper.");
  });
});

import forge2Verdict from './forge2-pilot.fixture.json';
import { FORGE_ERA_TIER_ORDER, FORGE_TIERS } from './baselines';
describe('a forge-2.0 verdict reads its v1 tiers AND its v2 families R1–R9 (score_forge2.py)', () => {
  // The REAL GPT-6.1 Sol pilot verdict: v1 rows a quarter of the score, R1–R9 the other three quarters.
  const projected = projectBenchScore(forge2Verdict as never).verdict as unknown as VerdictDetail;

  it('groups every row under its own tier and composes all nineteen, so the table adds up to the score', async () => {
    const view = render(
      <ScoringDetail verdict={projected} score={forge2Verdict.score} scorerVersion="forge-2.0-rc" />
    );
    // No family is dropped for being new: each letter's group is named.
    for (const tier of FORGE_ERA_TIER_ORDER['forge-2.0'])
      expect(view.getAllByText(FORGE_TIERS[tier].name).length).toBeGreaterThan(0);
    const composition = view.getByRole('region', { name: 'Earned score composition' });
    const rows = within(composition).getAllByRole('row').slice(1);
    expect(rows.map((row) => row.firstElementChild?.textContent?.split(' ')[0])).toEqual(
      FORGE_ERA_TIER_ORDER['forge-2.0']
    );
    expect(within(composition).getByText('R5 Admin panel')).toBeInTheDocument();
    // The points column sums to the recorded earned score (each cell rounded to 0.1 point) — a 1.0-only
    // table would stop at the v1 quarter (21.9 points of 96.2).
    const points = rows.reduce((sum, row) => sum + Number(row.lastElementChild?.textContent), 0);
    expect(Math.abs(points - forge2Verdict.critical.pre_severity_score * 100)).toBeLessThan(
      0.05 * rows.length
    );
    view.getByText('Every admission band passed — no ceiling');
    assertStudioClean(view.container);
    expect(await missingUtilities(utilitiesOf(allClasses(view.container)))).toEqual([]);
  });
});

import forge2Haiku from './forge2-haiku-reliability.fixture.json';
import forge2Reference from './forge2-reference-reliability.fixture.json';
import forge2Sol from './forge2-sol-reliability.fixture.json';
import forge2Sonnet from './forge2-sonnet-reliability.fixture.json';
describe('a forge-2.0 verdict shows reliability as its own step, one line per group of tests that multiplies (score_forge2.py reliability())', () => {
  // score_forge2.py recompose() output under the group rule (forge2/final), each the scorer's whole object:
  // Sonnet 5.5 and GPT-6.1 Sol on the hardened task, and Haiku. The numbers each states, listed once.
  const CASES = {
    sonnet: { verdict: forge2Sonnet, inner: 0.9628, reliability: 0.7738, final: 0.745 },
    sol: { verdict: forge2Sol, inner: 0.9793, reliability: 0.8422, final: 0.8248 },
    haiku: { verdict: forge2Haiku, inner: 0.3378, reliability: 0.2954, final: 0.0998 },
  } as const;
  const show = (
    verdict: { score: number },
    scorerVersion = 'forge-2.0-rc',
    score = verdict.score
  ) =>
    render(
      <ScoringDetail
        verdict={projectBenchScore(verdict as never).verdict as unknown as VerdictDetail}
        score={score}
        scorerVersion={scorerVersion}
      />
    );
  type View = ReturnType<typeof render>;
  /** The steps as a person reads them: each label with the number under it. */
  const steps = (view: View) =>
    within(view.getByRole('region', { name: 'Score steps' }))
      .getAllByTestId('forge-step')
      .map((step) => ({
        label: step.querySelector('dt')?.textContent,
        shown: step.querySelector('dd')?.textContent ?? '',
        value: parseFloat(step.querySelector('dd')?.textContent ?? ''),
      }));
  /** The group lines as shown: group, worst test (and what sits under it), its score, the factor. */
  const lines = (view: View) =>
    within(view.getByRole('table', { name: 'Groups of tests that multiply the score' }))
      .getAllByTestId('reliability-line')
      .map((row) => {
        const [group, test, score, factor] = [...row.querySelectorAll('td')];
        return {
          tier: row.getAttribute('data-tier'),
          group: group.textContent,
          test: test.firstChild?.textContent,
          under: test.querySelector('[data-testid="reliability-folded"]')?.textContent ?? null,
          score: score.textContent,
          factor: parseFloat((factor.textContent ?? '').replace('× ', '')),
        };
      });
  const product = (factors: number[]) => factors.reduce((p, f) => p * f, 1);
  /** Each line is shown to four decimals, so n lines agree with the shown product to n half-units. */
  const rounding = (n: number) => 5e-5 * (n + 1);
  const words = (name: string) => {
    const t = name.replace(/_/g, ' ');
    return t.charAt(0).toUpperCase() + t.slice(1);
  };

  it.each(Object.entries(CASES))(
    '%s: the group lines multiply to the shown reliability and the steps to the shown final',
    (_name, { verdict, inner, reliability, final }) => {
      const view = show(verdict);
      const shown = steps(view);
      expect(shown.map((step) => step.label)).toEqual([
        'Tests earned',
        '× Critical multiplier',
        '× Reliability',
        '= Final score',
      ]);
      expect(shown.map((step) => step.value)).toEqual([inner, 1, reliability, final]);
      expect(
        Math.abs(shown[0].value * shown[1].value * shown[2].value - shown[3].value)
      ).toBeLessThan(rounding(3));
      // One line per group the scorer counted, in its order: the group, its worst test, that test's
      // score and the group's factor — and the group's other failed tests under it.
      const shownLines = lines(view);
      expect(shownLines).toEqual(
        verdict.reliability.defects.map((defect) => {
          const others =
            (verdict.reliability.folded as Record<string, string[]>)[defect.tier] ?? [];
          return {
            tier: defect.tier,
            group: `${defect.tier} ${FORGE_TIERS[defect.tier].name}`,
            test: words(defect.check),
            under: others.length
              ? `Already counted in this group: ${others.map(words).join(', ')}`
              : null,
            score: `${(defect.score * 100).toFixed(1)}%`,
            factor: defect.factor,
          };
        })
      );
      expect(
        Math.abs(product(shownLines.map((line) => line.factor)) - shown[2].value)
      ).toBeLessThan(rounding(shownLines.length));
      // The table's last row and the step state one reliability.
      expect(view.getByTestId('reliability-total')).toHaveTextContent(
        `Reliability× ${reliability.toFixed(4)}`
      );
      // The same final the admission block above states (three decimals there).
      const admission = view.getByRole('region', { name: 'Admission bands' });
      expect(within(admission).getByText('Final score').nextElementSibling).toHaveTextContent(
        final.toFixed(3)
      );
      expect(view.queryByTestId('reliability-floor')).toBeNull();
      expect(view.queryByText('at the floor')).toBeNull();
      expect(view.queryByTestId('reliability-critical')).toBeNull();
      // The rule in plain words, its two numbers read from the verdict.
      view.getByText(
        /Failed tests also multiply the score\. Each group of tests multiplies the score by its worst test: 0\.90 when that test fails completely, in proportion when it fails partly\. The group’s other failed tests are already counted by its worst one\. The excellence tier never multiplies, and together the groups never take the score below 0\.25 of what the tests earned\./
      );
      // No scorer jargon in the explanation (the check rows below quote the scorer's evidence verbatim,
      // by design, so only this section is held to it).
      expect(view.getByRole('region', { name: 'Score steps' }).textContent).not.toMatch(
        /ROOT_BLOCKS|vacuous|shortfall|priced_as_critical|folded|root/
      );
    }
  );

  it('Sonnet: the widget group is one line, its worst test, with the group’s other failed tests quietly under it', async () => {
    const view = show(forge2Sonnet);
    const widget = lines(view).find((line) => line.tier === 'U');
    expect(widget).toEqual({
      tier: 'U',
      group: 'U UI function',
      test: 'U widget live',
      under: 'Already counted in this group: U widget numbers, U widget chart, U ledger table',
      score: '0.0%',
      factor: 0.9,
    });
    // Each group once.
    const tiers = lines(view).map((line) => line.tier);
    expect(new Set(tiers).size).toBe(tiers.length);
    assertStudioClean(view.container);
    expect(await missingUtilities(utilitiesOf(allClasses(view.container)))).toEqual([]);
  });

  it('names the tests a critical defect priced instead, when there are any', () => {
    // SYNTHETIC: Sonnet's real block with one test moved to the critical list — none of the recomposed
    // verdicts fired a critical, so the line is proven on this patch only.
    const verdict = {
      ...forge2Sonnet,
      reliability: { ...forge2Sonnet.reliability, priced_as_critical: ['b_comment_adf_as_user'] },
    };
    const view = show(verdict);
    expect(view.getByTestId('reliability-critical')).toHaveTextContent(
      'Priced as a critical defect instead, so no group counts them: B comment adf as user.'
    );
  });

  it('at the floor: said on the step, on the total and in a sentence', () => {
    // SYNTHETIC: every group of Haiku's rows failing completely (18 × 0.90 = 0.150, below the 0.25 floor),
    // because no recomposed verdict reaches the floor under the group rule. The block and the earned
    // score are patched together so the steps still multiply.
    const tiers = Object.keys(forge2Haiku.tiers).filter((tier) => tier !== 'E');
    const defects = tiers.map((tier) => ({
      tier,
      check: forge2Haiku.checks.find((row) => row.tier === tier)!.check,
      score: 0,
      factor: 0.9,
    }));
    const earned = Math.round(forge2Haiku.inner * 0.25 * 1e4) / 1e4;
    const verdict = {
      ...forge2Haiku,
      score: earned,
      rawScore: earned,
      reliability: {
        ...forge2Haiku.reliability,
        multiplier: 0.25,
        floored: true,
        defects,
        folded: {},
      },
    };
    const view = show(verdict);
    const shown = steps(view);
    expect(shown[2].shown).toBe('0.2500at the floor');
    expect(
      Math.abs(shown[0].value * shown[1].value * shown[2].value - shown[3].value)
    ).toBeLessThan(rounding(3));
    // The lines multiply to LESS than the shown reliability: the floor is what holds it.
    expect(lines(view)).toHaveLength(18);
    expect(product(lines(view).map((line) => line.factor))).toBeLessThan(0.25);
    expect(view.getByTestId('reliability-total')).toHaveTextContent(
      'Reliability — at the floor× 0.2500'
    );
    expect(view.getByTestId('reliability-floor')).toHaveTextContent(
      'The groups above multiply to less than 0.25. Failed tests never take the score below 0.25 of what the tests earned, so reliability stays at 0.25.'
    );
  });

  it('the reference app: reliability 1.0000 and no line, said in words', () => {
    const view = show(forge2Reference);
    expect(steps(view).map((step) => [step.label, step.shown])).toEqual([
      ['Tests earned', '0.9907'],
      ['× Critical multiplier', '1.0000'],
      ['× Reliability', '1.0000'],
      ['= Final score', '0.9907'],
    ]);
    view.getByText('No group of tests multiplied the score');
    expect(
      view.queryByRole('table', { name: 'Groups of tests that multiply the score' })
    ).toBeNull();
    expect(view.queryAllByTestId('reliability-line')).toHaveLength(0);
    expect(view.queryByTestId('reliability-folded')).toBeNull();
    expect(view.queryByTestId('reliability-critical')).toBeNull();
  });

  it('names the result of the steps for what it is when an admission ceiling still applies to it', () => {
    // The same verdict shown under a final below its earned score — what a failed band leaves.
    const view = show(forge2Sonnet, 'forge-2.0', 0.499);
    expect(steps(view).map((step) => [step.label, step.shown])[3]).toEqual([
      '= Earned before the ceiling',
      CASES.sonnet.final.toFixed(4),
    ]);
  });

  it('a forge-2.0 verdict scored before the rule says so, and shows no step it never recorded', () => {
    const view = show(forge2Verdict);
    expect(view.queryByRole('region', { name: 'Score steps' })).toBeNull();
    expect(view.getByTestId('forge-verdict-facts')).toHaveTextContent(
      'Scored before failed tests multiplied the score: this result carries no reliability record, so its number leaves that step out. Re-score the saved build to see it under the current rule.'
    );
  });

  it('a forge-1.0 verdict shows none of it: no step, no line, no absence note', () => {
    const view = render(
      <ScoringDetail
        verdict={
          {
            ...projectBenchScore(forgeVerdict as never).verdict,
            // Even a block that strayed onto a 1.0 row is not 1.0's rule.
            reliability: forge2Sonnet.reliability,
          } as unknown as VerdictDetail
        }
        score={0.799}
        scorerVersion="forge-1.0"
      />
    );
    expect(view.queryByRole('region', { name: 'Score steps' })).toBeNull();
    expect(view.queryAllByTestId('reliability-line')).toHaveLength(0);
    expect(view.container.textContent).not.toMatch(/reliab|Failed tests also multiply/i);
  });
});
