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
    const { getByText, getAllByText, queryByText } = render(
      <ScoringDetail verdict={verdict} score={0.8645} />
    );
    getByText(/Findings that held/);
    getByText('the served page renders NO data rows in a real browser');
    getByText('Round 0 · 2 findings');
    getByText('Round 2 · 0 findings');
    // Root-cause attribution names the root and the count it zeroed (true of this scorer lineage).
    getByText(/failed at the root and zeroed 2 downstream check/);
    // The chip counts checks below full marks — it never called them points.
    expect(getAllByText(/^\d+ checks? below full marks$/).length).toBeGreaterThan(0);
    expect(queryByText(/lost point/)).toBeNull();
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
describe('a Forge 1.0 verdict reads its own tiers, caps and publishability (score_forge.py)', () => {
  const projected = projectBenchScore(forgeVerdict as never).verdict as unknown as VerdictDetail;

  it('groups checks under the Forge tier names, never the Gauntlet letters they share', async () => {
    const view = render(
      <ScoringDetail verdict={projected} score={0.799} scorerVersion="forge-1.0-rc" />
    );
    for (const name of [
      'Lint and bundles',
      'Platform currency',
      'Event pipeline',
      'Reconcile',
      'Storage',
      'Resolvers and permissions',
      'UI function',
      'Visual',
      'Rovo',
      'Excellence',
    ])
      expect(view.getAllByText(name).length).toBeGreaterThan(0);
    // B is Resolvers and permissions here, not Gauntlet's Behaviour; A is Rovo, not Structure.
    expect(view.queryByText('Behaviour')).toBeNull();
    expect(view.queryByText('Structure')).toBeNull();
    // Frozen history: in Forge 1.0 "inner score" and "before criticals" are two numbers, so its line stays.
    view.getByText(
      /Recorded composition inputs: inner score 0\.9833 .* before criticals 0\.9768 · critical multiplier 1\.0000/
    );
    // Its weights print exactly, and a group counts tests: 0.08 × 0.88 is 7.04%, never "7%".
    expect(view.getAllByText(/^\d+ tests? · weight 8%$/).length).toBeGreaterThan(0);
    assertStudioClean(view.container);
    expect(await missingUtilities(utilitiesOf(allClasses(view.container)))).toEqual([]);
  });

  it('a verdict scored before the pull states the rule it was scored by: the lower of earned and cap', () => {
    // forge-alt is a Forge 1.0 verdict from before 2026-10-03: no `final_rule`, and its final sits ON its cap.
    const view = render(
      <ScoringDetail verdict={projected} score={0.799} scorerVersion="forge-1.0" />
    );
    const cap = view.getByRole('region', { name: 'Final score' });
    expect(
      [...cap.querySelectorAll('dl > div')].map((cell) => [
        cell.querySelector('dt')?.textContent,
        cell.querySelector('dd')?.textContent,
      ])
    ).toEqual([
      ['Earned score', '0.977'],
      ['Cap', '0.799'],
      ['Final score', '0.799'],
    ]);
    const band = view.getByTestId('forge-failed-band');
    expect(band).toHaveTextContent('Maximum 0.799');
    // Forge 1.0's caps keep the scorer's own label; the failed test is named as every test is.
    expect(band).toHaveTextContent('current platform, complete surfaces');
    expect(band).toHaveTextContent('Failed test:K widget edit bridge');
    expect(view.getByTestId('forge-cap-rule')).toHaveTextContent(
      /^The final score is the lower of the earned score and the lowest cap that applies\. Avoiding a cap adds no points\.$/
    );
    expect(cap.textContent).not.toMatch(/admission|ceiling|band|Here:/i);
  });

  it('a verdict with no failed cap says no cap applies; an unpublishable one says why', () => {
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
    view.getByText('No cap applies');
    // No cap is the word "none", never a cap of 1.000.
    expect(view.getByText('Cap').nextElementSibling).toHaveTextContent(/^none$/);
    expect(view.queryByTestId('forge-failed-band')).toBeNull();
    // What happened, in plain words and once — the next step belongs to the publish refusal, and a frozen
    // era has none.
    const facts = view.getByTestId('forge-verdict-facts');
    expect(facts).toHaveTextContent(
      "Cannot be published: it was scored on the shim runtime, not Atlassian's pinned Forge runtime."
    );
    expect(facts.textContent?.match(/shim runtime/g)).toHaveLength(1);
    expect(facts.textContent).not.toMatch(/Run the benchmark again|wrapper|unavailable row/);
  });
});

import forge2Verdict from './forge2-pilot.fixture.json';
import { FORGE_ERA_TIER_ORDER, FORGE_TIERS } from './baselines';
describe('a forge-2.0 verdict reads its v1 tiers AND its v2 families R1–R9 (score_forge2.py)', () => {
  // The REAL GPT-6.1 Sol pilot verdict: v1 rows a quarter of the score, R1–R9 the other three quarters.
  const projected = projectBenchScore(forge2Verdict as never).verdict as unknown as VerdictDetail;

  it('lists every group under its own name and adds all nineteen up to what the tests earned', async () => {
    const view = render(
      <ScoringDetail verdict={projected} score={forge2Verdict.score} scorerVersion="forge-2.0-rc" />
    );
    // No family is dropped for being new: each letter's group is named.
    for (const tier of FORGE_ERA_TIER_ORDER['forge-2.0'])
      expect(view.getAllByText(FORGE_TIERS[tier].name).length).toBeGreaterThan(0);
    const earned = view.getByRole('region', { name: 'Tests earned' });
    expect(
      within(earned)
        .getAllByRole('columnheader')
        .map((th) => th.textContent)
    ).toEqual(['Group', 'Mean of its tests', 'Weight', 'Adds to the score']);
    const lines = within(earned).getAllByTestId('forge-group-line');
    expect(lines.map((row) => row.getAttribute('data-tier'))).toEqual(
      FORGE_ERA_TIER_ORDER['forge-2.0']
    );
    expect(within(earned).getByText('R5 Admin panel')).toBeInTheDocument();
    const cells = (row: Element) => [...row.querySelectorAll('td')].map((td) => td.textContent);
    // Each line is the verdict's own mean and weight, and what the two add — on the score's scale, to its
    // four decimals.
    const tiers = forge2Verdict.tiers as Record<string, { mean: number; weight: number }>;
    for (const row of lines) {
      const tier = row.getAttribute('data-tier')!;
      expect(cells(row)).toEqual([
        `${tier} ${FORGE_TIERS[tier].name}`,
        tiers[tier].mean.toFixed(4),
        `${Number((tiers[tier].weight * 100).toFixed(2))}%`,
        (tiers[tier].mean * tiers[tier].weight).toFixed(4),
      ]);
    }
    // The weights print at the precision that adds to 100%: 1.76% is never "2%" (whole percents add to 102%).
    expect(lines.map((row) => cells(row)[2])).toEqual([
      ...['1.76%', '2.2%', '3.52%', '3.08%', '1.76%', '2.64%', '3.52%', '1.76%', '1.76%', '3%'],
      ...['12%', '13%', '8%', '10%', '10%', '8%', '5%', '4%', '5%'],
    ]);
    const hundredths = lines.reduce(
      (sum, row) => sum + Math.round(parseFloat(cells(row)[2]) * 100),
      0
    );
    expect(hundredths).toBe(10000);
    // The closing line is the verdict's own "Tests earned", and the lines add up to it (each rounded to four
    // decimals) — a 1.0-only table would stop at the v1 quarter.
    expect(cells(within(earned).getByTestId('forge-tests-earned'))).toEqual([
      'Tests earned',
      '100%',
      forge2Verdict.critical.pre_severity_score.toFixed(4),
    ]);
    const added = lines.reduce((sum, row) => sum + Number(cells(row)[3]), 0);
    expect(Math.abs(added - forge2Verdict.critical.pre_severity_score)).toBeLessThan(
      5e-5 * (lines.length + 1)
    );
    // The E line is explained from the verdict's own two numbers, and the rounding is owned up to.
    expect(earned).toHaveTextContent(
      `E Excellence is the excellence gate ${forge2Verdict.excellence.fraction.toFixed(4)} (the average of its ${forge2Verdict.excellence.conditions.length} conditions) × the mean of its tests ${forge2Verdict.excellence.e_mean.toFixed(4)}. Each line is rounded to four decimals, so added up they can differ from the total in the last digit.`
    );
    // One name for one number: on a Forge 2.0 result "inner score" and "before criticals" ARE "Tests earned".
    expect(view.container.textContent).not.toMatch(/inner score|before criticals|Points of 100/);
    view.getByText('No cap applies');
    // A group's header counts tests and prints the same weight as the table.
    expect(view.getAllByText(/^\d+ tests · weight 1\.76%$/, { selector: 'span' })).toHaveLength(4);
    assertStudioClean(view.container);
    expect(await missingUtilities(utilitiesOf(allClasses(view.container)))).toEqual([]);
  });
});

import forge2Haiku from './forge2-haiku-reliability.fixture.json';
import forge2Reference from './forge2-reference-reliability.fixture.json';
import forge2Sol from './forge2-sol-reliability.fixture.json';
import forge2Sonnet from './forge2-sonnet-reliability.fixture.json';
describe('a forge-2.0 verdict explains its score top-down: tests earned, × critical defects, × reliability, final score (score_forge2.py)', () => {
  // score_forge2.py recompose() output under the group rule (forge2/final), each the scorer's whole object:
  // Sonnet 5.5 and GPT-6.1 Sol on the hardened task, and Haiku. The numbers each states, listed once.
  const CASES = {
    sonnet: { verdict: forge2Sonnet, inner: 0.9628, reliability: 0.7738, final: 0.745 },
    sol: { verdict: forge2Sol, inner: 0.9793, reliability: 0.8422, final: 0.8248 },
    haiku: { verdict: forge2Haiku, inner: 0.3378, reliability: 0.2954, final: 0.0998 },
  } as const;
  const show = (
    verdict: { score: number; [key: string]: unknown },
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
  /** Every section of the explanation, in the order the card draws them. */
  const SECTIONS = [
    'Score steps',
    'Tests earned',
    'Critical defects',
    'Reliability',
    'Final score',
  ];
  /** A section's own words: its text without the cells and lines that quote a test's name (a test is
   *  named after what it tests — "R3 no duplicate rows" — so those are not the explanation's vocabulary). */
  const prose = (view: View, name: string) => {
    const copy = view.getByRole('region', { name }).cloneNode(true) as HTMLElement;
    copy
      .querySelectorAll(
        'td, [data-testid="reliability-folded"], [data-testid="reliability-critical"]'
      )
      .forEach((quoted) => quoted.remove());
    return copy.textContent ?? '';
  };
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
      // ONE vocabulary, in this order, on every Forge surface.
      expect(shown.map((step) => step.label)).toEqual([
        'Tests earned',
        '× Critical defects',
        '× Reliability',
        '= Final score',
      ]);
      // …and the sections under the line explain the steps in that same order.
      const regions = view
        .getAllByRole('region')
        .map((region) => region.getAttribute('aria-label'))
        .filter((label) => SECTIONS.includes(label ?? ''));
      expect(regions).toEqual(SECTIONS);
      expect(
        within(view.getByRole('region', { name: 'Critical defects' })).getByText(
          'No critical defect was observed'
        )
      ).toBeInTheDocument();
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
            // A score is four decimals everywhere on a Forge 2.0 screen, as leanzero.net prints it.
            score: defect.score.toFixed(4),
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
      // The Final score section below states the same final, in the same four decimals, and no cap.
      const last = view.getByRole('region', { name: 'Final score' });
      expect(
        [...last.querySelectorAll('dl > div')].map((cell) => [
          cell.querySelector('dt')?.textContent,
          cell.querySelector('dd')?.textContent,
        ])
      ).toEqual([
        ['Earned score', final.toFixed(4)],
        ['Cap', 'none'],
        ['Final score', final.toFixed(4)],
      ]);
      // No three-decimal or percent copy of a score in the steps, the reliability lines or the final.
      for (const name of ['Score steps', 'Reliability', 'Final score'])
        expect(view.getByRole('region', { name }).textContent).not.toMatch(/\d%|\b\d\.\d{3}\b/);
      expect(view.queryByTestId('reliability-floor')).toBeNull();
      expect(view.queryByText('at the floor')).toBeNull();
      expect(view.queryByTestId('reliability-critical')).toBeNull();
      // The rule in the task text's one sentence, its numbers read from the verdict; the floor is the
      // groups' — a critical defect multiplies on top of it.
      expect(view.getByTestId('reliability-rule')).toHaveTextContent(
        'Each group of tests multiplies the score by 1 − 0.10 × the shortfall of its worst test: by 0.90 when that test fails completely, by 1 when every test in the group passes. The group’s other failed tests are already counted by its worst one. E Excellence never multiplies. Together the groups never take the score below 0.25 of what the tests earned; a critical defect still multiplies on top.'
      );
      // No scorer-internal word in the explanation (the test rows below quote the scorer's evidence
      // verbatim, by design, so only these sections are held to it).
      for (const name of SECTIONS)
        expect(prose(view, name)).not.toMatch(
          /ROOT_BLOCKS|vacuous|priced_as_critical|folded|\broot|\brc\b|\brows?\b|tier letter|admission|ceiling|\bband|\bchecks?\b|inner score|before criticals|suppressed/i
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
      score: '0.0000',
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
    // The floor is on the groups' product only: "failed tests never take the score below" overstated it.
    expect(view.getByTestId('reliability-floor')).toHaveTextContent(
      'The groups above multiply to less than 0.25. Together they never take the score below 0.25 of what the tests earned, so reliability stays at 0.25. A critical defect still multiplies on top.'
    );
  });

  it('the reference app: reliability 1.0000 and no line, said in words', () => {
    const view = show(forge2Reference);
    expect(steps(view).map((step) => [step.label, step.shown])).toEqual([
      ['Tests earned', '0.9907'],
      ['× Critical defects', '1.0000'],
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

  it('names the result of the steps for what it is when a cap still applies to it', () => {
    // The same verdict shown under a final below its earned score — what a cap leaves.
    const view = show(forge2Sonnet, 'forge-2.0', 0.499);
    expect(steps(view).map((step) => [step.label, step.shown])[3]).toEqual([
      '= Earned score, before the cap',
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

  /** The Final score section as read: its three numbers, and its rule. */
  const finalScore = (view: View) => {
    const section = within(view.container).getByRole('region', { name: 'Final score' });
    return {
      section,
      numbers: [...section.querySelectorAll('dl > div')].map((cell) => [
        cell.querySelector('dt')?.textContent,
        cell.querySelector('dd')?.textContent,
      ]),
      rule: within(view.container).getByTestId('forge-cap-rule').textContent,
    };
  };
  /** The facts banner of ONE render (a test that renders twice reads each by its own container). */
  const facts = (view: View) => within(view.container).getByTestId('forge-verdict-facts');
  /** score_forge2.py capped_final, written out — the test's own arithmetic, to hold the screen to it. */
  const cappedFinal = (earned: number, cap: number) => Math.min(earned, cap - 0.05 * (1 - earned));
  const RULE =
    'Under a cap the final score is min(earned, cap − 0.05 × (1 − earned)): below the cap, and closer to it the more the run earned. When more than one cap applies, the lowest counts. With no cap the final score is the earned score. Avoiding a cap adds no points.';
  /** A real verdict with its build failing to bundle: the 0.499 cap, and the final the scorer's rule gives. */
  const underCap = (verdict: typeof forge2Sol | typeof forge2Haiku) => ({
    ...verdict,
    score: Number(cappedFinal(verdict.rawScore, 0.499).toFixed(4)),
    admission: {
      ...verdict.admission,
      ceiling: 0.499,
      reasons: ['deployable: l_deployable, l_bundles_load (maximum 0.499)'],
      failedChecksByBand: [
        { ceiling: 0.499, band: 'deployable', checks: ['l_deployable', 'l_bundles_load'] },
      ],
    },
  });

  it('under a cap: the rule shown is the one the final was computed by, and the three numbers on screen agree with it', () => {
    // SYNTHETIC cap on Sol's real verdict (no recomposed verdict is capped): earned 0.8248 under 0.499.
    const verdict = underCap(forge2Sol);
    const final = cappedFinal(forge2Sol.rawScore, 0.499);
    expect(final).toBeCloseTo(0.49024, 10);
    const view = show(verdict, 'forge-2.0');
    const shown = finalScore(view);
    // The final is neither the earned score nor the cap — which is why "the lower of earned and the
    // ceiling" was false here.
    expect(shown.numbers).toEqual([
      ['Earned score', '0.8248'],
      ['Cap', '0.499'],
      ['Final score', '0.4902'],
    ]);
    // The rule, its pull read from the verdict's own `final_rule`, then the worked numbers.
    expect(shown.rule).toBe(`${RULE} Here: min(0.8248, 0.499 − 0.05 × (1 − 0.8248)) = 0.4902.`);
    const [earned, cap, shownFinal] = shown.numbers.map(([, value]) => Number(value));
    expect(cappedFinal(earned, cap).toFixed(4)).toBe(shownFinal.toFixed(4));
    // The steps end on the earned score, named as that; the cap section takes it from there.
    expect(steps(view).map((step) => [step.label, step.shown])[3]).toEqual([
      '= Earned score, before the cap',
      '0.8248',
    ]);
    // The cap in the task text's own words, with the tests that set it — no scorer label, no raw id.
    const band = view.getByTestId('forge-failed-band');
    expect(band).toHaveTextContent('Maximum 0.499');
    expect(band).toHaveTextContent(
      'Lint errors, or manifest functions that do not bundle and load.'
    );
    expect(band).toHaveTextContent('Failed tests:L deployableL bundles load');
    expect(band.textContent).not.toMatch(/deployable:|l_deployable|Capped at/);
    expect(view.queryByText('No cap applies')).toBeNull();
    expect(shown.section.textContent).not.toMatch(
      /admission|ceiling|\bband|earned credit|\bchecks?\b/i
    );
  });

  it('under a cap that does not bind: the final is the earned score, and the worked numbers say so', () => {
    // Haiku's real 0.0998 under the same cap: 0.499 − 0.05 × 0.9002 is far above it.
    const view = show(underCap(forge2Haiku), 'forge-2.0');
    const shown = finalScore(view);
    expect(shown.numbers).toEqual([
      ['Earned score', '0.0998'],
      ['Cap', '0.499'],
      ['Final score', '0.0998'],
    ]);
    expect(shown.rule).toBe(`${RULE} Here: min(0.0998, 0.499 − 0.05 × (1 − 0.0998)) = 0.0998.`);
    expect(steps(view)[3].label).toBe('= Final score');
  });

  it('prints no worked numbers the rule does not reproduce, and quotes a rule it cannot read', () => {
    // A final that is not what the rule gives (here: left on the cap) gets the rule and no "Here:".
    const off = show({ ...underCap(forge2Sol), score: 0.499 }, 'forge-2.0');
    expect(finalScore(off).rule).toBe(RULE);
    // The pull is read from the verdict, never typed: another rule's number is the one shown…
    const other = underCap(forge2Sol);
    const pulled = show(
      {
        ...other,
        score: Number(Math.min(0.8248, 0.499 - 0.1 * (1 - 0.8248)).toFixed(4)),
        admission: {
          ...other.admission,
          final_rule: 'final = min(earned, ceiling - 0.1 * (1 - earned))',
        },
      },
      'forge-2.0'
    );
    expect(finalScore(pulled).rule).toContain('min(earned, cap − 0.1 × (1 − earned))');
    expect(finalScore(pulled).rule).toContain(
      'Here: min(0.8248, 0.499 − 0.1 × (1 − 0.8248)) = 0.4815.'
    );
    // …and a rule in a form this view does not know is quoted as recorded, with no arithmetic of ours.
    const unknown = show(
      {
        ...other,
        admission: { ...other.admission, final_rule: 'final = earned * ceiling' },
      },
      'forge-2.0'
    );
    expect(finalScore(unknown).rule).toBe(
      'The scorer recorded its rule for the final score as: final = earned * ceiling. Avoiding a cap adds no points.'
    );
  });

  it('says nothing of roots on a Forge 2.0 result: every test counts at its own score and each group multiplies', () => {
    // Sonnet's real verdict carries root_causes (u_widget_numbers 0.875 over six tests scoring 0–0.97).
    expect(Object.keys(forge2Sonnet.root_causes)).toEqual(['u_widget_numbers']);
    const view = show(forge2Sonnet, 'forge-2.0');
    expect(view.queryByText('Root-cause attribution')).toBeNull();
    expect(view.queryByText('Tests that can share a cause')).toBeNull();
    expect(view.container.textContent).not.toMatch(
      /failed at the root|zeroed|one defect, not|downstream/
    );
    // What IS true of those tests is on the screen: four groups each multiply for them.
    expect(
      lines(view).filter((line) => ['T', 'B', 'U', 'A'].includes(line.tier ?? ''))
    ).toHaveLength(4);
  });

  it('a Forge 1.0 result lists tests that can share a cause without claiming one failed or zeroed another', () => {
    const view = render(
      <ScoringDetail
        verdict={
          {
            ...projectBenchScore(forgeVerdict as never).verdict,
            // The Forge scorers list a root with its dependents whenever each scored below 1, and repeat a
            // name reached by two routes.
            root_causes: {
              u_widget_numbers: ['u_widget_chart', 'a_action_result', 'u_widget_chart'],
              b_hidden_count: ['a_action_permissions'],
            },
          } as unknown as VerdictDetail
        }
        score={0.799}
        scorerVersion="forge-1.0"
      />
    );
    view.getByText('Tests that can share a cause');
    expect(view.queryByText('Root-cause attribution')).toBeNull();
    const text = view.container.textContent ?? '';
    expect(text).toContain(
      'U widget numbers scored below full marks, and so did 2 tests that the same fault can fail: U widget chart, A action result. Each keeps its own score.'
    );
    expect(text).toContain(
      'B hidden count scored below full marks, and so did 1 test that the same fault can fail: A action permissions. Each keeps its own score.'
    );
    expect(text).not.toMatch(/failed at the root|zeroed|one defect, not|downstream/);
  });

  it('states each critical defect in the task text’s words, and a test that cost nothing more as that', () => {
    // SYNTHETIC rows in score_forge2.py criticals()' own shape (no recomposed verdict fired a critical):
    // one charged test, one of the same kind, one the scorer counts under another, one note unknown here.
    const why = 'a duplicate side effect (>= 2 comments for one click, or duplicate ledger rows)';
    const rows = [
      { check: 't_no_double_count', class: 'duplicate', score: 0, why, factor: 0.6 },
      {
        check: 'r6_replay_once',
        class: 'duplicate',
        score: 0,
        why,
        factor: 1,
        suppressed: 'class:duplicate priced by t_no_double_count',
      },
      {
        check: 'b_comment_exactly_once',
        class: 'duplicate',
        score: 0,
        why,
        factor: 1,
        suppressed: 'root:t_no_double_count',
      },
      { check: 'r1_v1_rows_intact', class: 'migration', score: 0, why: 'x', factor: 0.6 },
      {
        check: 'r5_admin_only',
        class: 'future',
        score: 0,
        why: 'a new kind',
        factor: 1,
        suppressed: 'held',
      },
    ];
    const view = show(
      { ...forge2Sonnet, critical: { ...forge2Sonnet.critical, multiplier: 0.36, rows } },
      'forge-2.0'
    );
    const section = view.getByRole('region', { name: 'Critical defects' });
    expect(section).toHaveTextContent(
      'Each kind of critical defect that is observed multiplies the whole score by 0.60, at most once however many tests observe it.'
    );
    expect(
      within(section)
        .getAllByTestId('critical-line')
        .map((line) => line.textContent)
    ).toEqual([
      // The replayed CI event is in the words: the scorer's own string for this kind leaves it out.
      'T no double count × 0.60: a duplicate side effect: two or more comments for one click, duplicate ledger rows, or a replayed CI event applied twice.',
      'R6 replay once: no additional penalty. T no double count already multiplied the score for this kind of defect, and each kind multiplies it once.',
      'B comment exactly once: no additional penalty. The scorer counts it as the same defect as T no double count, which already multiplied the score.',
      'R1 v1 rows intact × 0.60: v1 rows lost or corrupted by the migration.',
      'R5 admin only: no additional penalty (the scorer’s note: held).'.replace('’', "'"),
    ]);
    expect(within(section).queryByText('No critical defect was observed')).toBeNull();
    // The scorer's raw record never reaches the person.
    expect(section.textContent).not.toMatch(
      /class:|priced by|root:|suppressed|factor|>= 2|t_no_double/
    );
  });

  it('a group’s header counts tests below full marks, never "lost points"', () => {
    const view = show(forge2Sonnet, 'forge-2.0');
    // Sonnet's U group: four of its tests scored below 1 (u_widget_live 0, three partly).
    const header = view.getByRole('button', { name: /UI function/ });
    expect(header).toHaveTextContent('4 tests below full marks');
    expect(header).toHaveTextContent('tests · weight 3.52%');
    // The group's mean and each test's score are four decimals, the digits the reliability line names:
    // "U widget live 0.0000" there, "0.0000" on its own row here — never "88" beside "0.8837".
    const chip = (element: Element) =>
      element.querySelector('[data-testid="score-chip"]')?.textContent;
    expect(chip(header)).toBe(forge2Sonnet.tiers.U.mean.toFixed(4));
    fireEvent.click(header);
    const rows = [...(header.parentElement?.querySelectorAll('[data-testid="score-chip"]') ?? [])]
      .slice(1)
      .map((element) => element.textContent);
    expect(rows).toContain('0.0000');
    expect(rows.every((text) => /^[01]\.\d{4}$/.test(text ?? ''))).toBe(true);
    // One test is "1 test".
    expect(view.getByRole('button', { name: /R5 Admin panel|Admin panel/ })).toHaveTextContent(
      '1 test below full marks'
    );
    expect(view.container.textContent).not.toMatch(/lost point/);
  });

  it('says in plain words why a result is on hold, cannot be published, or is a measurement', () => {
    const held = show(
      { ...forge2Sonnet, status: 'held', publishable: false, harness_missing: ['a', 'b', 'c'] },
      'forge-2.0'
    );
    expect(facts(held)).toHaveTextContent(
      'On hold: the built app uses 3 Forge or Jira calls that the benchmark’s simulated platform does not support yet. The score is not zeroed and cannot be published as it is.'.replace(
        '’',
        "'"
      )
    );
    // The hold IS the reason: no second line saying the scorer gave none.
    expect(facts(held).textContent).not.toMatch(/Cannot be published|no reason/);
    const unrun = show(
      { ...forge2Sonnet, publishable: false, unpublishable_reasons: ['2 unavailable row(s)'] },
      'forge-2.0'
    );
    expect(facts(unrun)).toHaveTextContent(
      "Cannot be published: the scorer could not run 2 of its tests, which is a scoring problem and not the model's."
    );
    // Scored before the thresholds were final: said without "rc" or "uncalibrated".
    const early = show(forge2Sonnet, 'forge-2.0-rc');
    expect(facts(early)).toHaveTextContent(
      "Scored before Forge 2.0's thresholds were final: a measurement, not a board result."
    );
    for (const view of [held, unrun, early])
      expect(facts(view).textContent).not.toMatch(
        /\brc\b|uncalibrated|emulator|unavailable row|Held for rescore/i
      );
  });
});
