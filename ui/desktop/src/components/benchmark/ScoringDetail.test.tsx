import { render } from '@testing-library/react';
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
    expect(chips.some((c) => tone(c) === 'ok' && c.classList.contains('bg-lz-ok-solid'))).toBe(true);
    expect(chips.some((c) => tone(c) === 'warn' && c.classList.contains('bg-lz-warn-solid'))).toBe(
      true
    );
    // The held findings sit under a solid err header, and the composition earned-fill is the accent.
    expect(container.querySelector('[data-testid="findings-held"] .bg-lz-err-solid')).not.toBeNull();
    expect(container.querySelectorAll('svg rect.fill-lz-accent').length).toBeGreaterThan(0);
    expect(container.innerHTML).not.toMatch(/color-node-|color-block|#[0-9a-f]{6}/i);
    assertStudioClean(container);
    expect(await missingUtilities(utilitiesOf(allClasses(container)))).toEqual([]);
  }, 30_000);
});
