import { render, screen, cleanup, fireEvent, waitFor, within } from '@testing-library/react';
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';
import { projectBenchScore } from '../../benchScoreProjection';
import { FORGE_ERA_TIER_ORDER } from './baselines';
import forgeVerdict from './forge-alt.fixture.json';
import forge2Verdict from './forge2-pilot.fixture.json';
import forge2Sonnet from './forge2-sonnet-reliability.fixture.json';

/**
 * THE FORGE FAMILY ON THE BENCHMARK VIEW. The app bundles forge-2.0; forge-1.0 is a frozen era whose
 * sessions stay readable. Two REAL verdicts: score_forge2.py's for the GPT-6.1 Sol pilot (2026-10-10, three
 * scoring seeds): forge-2.0-rc, 0.9618, no band, no critical — and score_forge.py's for the forge-1.0 alt
 * golden app (seed 0123456789abcdef, 2026-10-02): 0.799, capped by the "current platform, complete
 * surfaces" band on k_widget_edit_bridge. The switch scopes the whole view to Forge: its era picker, a
 * single-model run form with the kit's readiness and the bundled tier's run policy, its sessions, and a run
 * card that reads the verdict's own tier letters, admission band and screenshots — and refuses to publish
 * what the scorer itself marks not board-grade, or what the site has frozen.
 */

vi.mock('../swarm/SwarmRunPanel', async () => {
  const React = await import('react');
  const Stub = () => React.createElement('div', { 'data-testid': 'swarm-panel-stub' });
  return { SwarmRunPanel: Stub, default: Stub };
});
vi.mock('../swarm/useSamplingDefaults', () => ({ useSaveSamplingDefaults: () => () => {} }));
vi.mock('../../acp/modelFields', () => ({
  acpListModelFields: vi.fn(async () => ({
    source: 'provider_metadata',
    fields: [
      {
        id: 'effort',
        label: 'Effort',
        description: 'Sent as reasoning.effort.',
        kind: { type: 'select', options: ['high', 'medium', 'low'] },
      },
    ],
    values: { effort: 'low' },
  })),
  acpSaveModelFields: vi.fn(),
}));
vi.mock('../../acp/providers', () => ({
  acpListProviderDetails: vi.fn(async () => [
    {
      name: 'openrouter',
      is_configured: true,
      metadata: { display_name: 'OpenRouter', known_models: [] },
    },
  ]),
}));

import BenchmarkView from './BenchmarkView';

type ElectronMock = Record<string, unknown>;
const electron = () => (window as unknown as { electron: ElectronMock }).electron;

const projected = projectBenchScore(forge2Verdict as never);
const MINE = {
  label: 'openai/gpt-6.1-sol · single agent',
  score: forge2Verdict.score,
  tiers: projected.tiers,
  nodes: 1,
  provider: 'openrouter',
  mine: true,
  scorerVersion: projected.scorerVersion,
  runMeta: {
    startedAt: '2026-10-10T00:57:00.000Z',
    finishedAt: '2026-10-10T01:54:00.000Z',
    engineEvents: 0,
    repairRounds: 0,
  },
  workdir: '/tmp/forge2-run',
  modelId: 'openai/gpt-6.1-sol',
  runId: 'cloud-forge2-1',
  verdict: projected.verdict,
  budget: { max_calls: 300, calls_used: 176, stopped_by: 'model_finished' },
};
const SESSION = {
  runId: 'cloud-forge2-1',
  scorerVersion: 'forge-2.0-rc',
  startedAt: '2026-10-10T00:57:00.000Z',
  endedAt: '2026-10-10T01:54:00.000Z',
  outcome: 'finished',
  score: forge2Verdict.score,
  tiers: projected.tiers,
  nodes: 1,
  publishable: true,
};
// The forge-1.0 history: the alt golden app's real verdict under the frozen era's identity.
const projected10 = projectBenchScore(forgeVerdict as never);
const SESSION_10 = {
  runId: 'cloud-forge-1',
  scorerVersion: 'forge-1.0',
  startedAt: '2026-10-02T20:00:00.000Z',
  endedAt: '2026-10-02T21:10:00.000Z',
  outcome: 'finished',
  score: forgeVerdict.score,
  tiers: projected10.tiers,
  nodes: 1,
  publishable: true,
};
const ROW_10 = {
  label: 'openai/gpt-6-luna · single agent',
  score: forgeVerdict.score,
  tiers: projected10.tiers,
  nodes: 1,
  provider: 'openrouter',
  mine: true,
  scorerVersion: 'forge-1.0',
  runMeta: {
    startedAt: SESSION_10.startedAt,
    finishedAt: SESSION_10.endedAt,
    engineEvents: 0,
    repairRounds: 0,
  },
  workdir: '/tmp/forge-run',
  modelId: 'openai/gpt-6-luna',
  runId: 'cloud-forge-1',
  verdict: projected10.verdict,
  budget: { max_calls: 150, calls_used: 150, stopped_by: 'call_budget' },
};
const SB_SESSION = {
  runId: 'sb-run',
  scorerVersion: 'sb-7.2',
  startedAt: '2026-10-01T09:00:00.000Z',
  outcome: 'finished',
  score: 0.42,
  tiers: { A: 0.5 },
  nodes: 1,
  publishable: false,
};
const SB_CURRENT = {
  scorerVersion: 'sb-7.2',
  title: 'SB7.2 payments',
  family: 'sb',
  familyCurrent: true,
  current: true,
  frozen: false,
  baselines: [],
};
const FORGE_CURRENT = {
  scorerVersion: 'forge-2.0',
  title: 'Forge 2.0 — Scope Ledger 2',
  family: 'forge',
  familyCurrent: true,
  current: false,
  frozen: false,
  baselines: [],
};
const FORGE10_FROZEN = {
  scorerVersion: 'forge-1.0',
  title: 'Forge 1.0 — Scope Ledger',
  family: 'forge',
  familyCurrent: false,
  current: false,
  frozen: true,
  baselines: [{ label: 'GPT-6 Luna', score: 0.31, model: 'openai/gpt-6-luna' }],
};
const KIT_READY = {
  state: 'ready',
  missing: [],
  kitLockSha256: '663bf851ac7b8fadc8bd1bd385d3439f90579279e9e61f3751e895eb41c03640',
  callBudget: 300,
  reasoningEffort: 'medium',
};
const SHOTS = [
  { name: 'forge-widget-light', caption: 'Dashboard widget · light', b64: 'iVBORw0KGgo=' },
  { name: 'forge-sprint-dark', caption: 'Sprint action · dark', b64: 'iVBORw0KGgo=' },
];

function mockElectron(
  opts: { benchmarks?: unknown[]; kit?: unknown; sessions?: unknown[]; mine?: unknown } = {}
) {
  const e = electron();
  e.benchmarkRuntimeStatus = vi.fn(async () => ({ state: 'ready', downloadBytes: 0 }));
  e.benchmarkForgeKitStatus = vi.fn(async () => opts.kit ?? KIT_READY);
  e.benchmarkForgeKitPrepare = vi.fn(async () => KIT_READY);
  e.benchmarkStatus = vi.fn(async () => ({ running: false }));
  e.benchmarkRead = vi.fn(async () => ('mine' in opts ? opts.mine : MINE));
  // Each finished run's own stored row (main's per-run store): the forge-1.0 history included.
  e.benchmarkRunResult = vi.fn(async (key: string) =>
    key === SESSION_10.runId ? ROW_10 : key === SESSION.runId ? MINE : null
  );
  e.benchmarkShots = vi.fn(async () => SHOTS);
  e.readSwarmRun = vi.fn(async () => null);
  e.fleetStatus = vi.fn(async () => ({}));
  e.benchmarkCatalog = vi.fn(async () => ({
    ok: true,
    stale: false,
    benchmarks: opts.benchmarks ?? [SB_CURRENT, FORGE_CURRENT, FORGE10_FROZEN],
  }));
  e.benchmarkSessions = vi.fn(async () => ({
    sessions: opts.sessions ?? [SESSION, SESSION_10, SB_SESSION],
  }));
  e.benchmarkMedia = vi.fn(async () => ({ videos: [] }));
}

const mount = () =>
  render(
    <IntlTestWrapper>
      <BenchmarkView />
    </IntlTestWrapper>
  );

describe('Benchmark view — the Gauntlet | Forge switch', () => {
  beforeEach(() => {
    window.location.hash = '#/benchmark?family=forge';
  });
  afterEach(() => {
    cleanup();
    window.location.hash = '';
  });

  it('scopes the whole view to Forge: era picker, single-model form, kit readiness, run policy', async () => {
    mockElectron();
    const { container } = mount();
    const toggle = screen.getByRole('radiogroup', { name: 'Benchmark type' });
    const forge = within(toggle).getByRole('radio', { name: /Forge/ });
    expect(forge).toHaveAttribute('aria-checked', 'true');
    expect(forge.className).toContain('bg-lz-family-forge');
    expect(within(toggle).getByRole('radio', { name: /Gauntlet/ })).toHaveAttribute(
      'aria-checked',
      'false'
    );
    expect(screen.getByTestId('family-intro').textContent).toMatch(
      /^Forge 2\.0 · Scope Ledger 2 runs one model in goose: it ships v2 of a working Atlassian Forge app/
    );
    expect(await screen.findByTestId('forge-kit-ready')).toHaveTextContent('Forge kit ready');
    expect(screen.getByText('forge-2.0 runtime')).toBeInTheDocument();
    expect(screen.getByTestId('forge-run-policy')).toHaveTextContent(
      'One model · 300-call budget · reasoning effort medium'
    );
    // No Swarm entrant for Forge; the single-model provider, model and spend limit are the form.
    expect(screen.queryByRole('button', { name: 'Swarm' })).toBeNull();
    expect(screen.getByRole('textbox', { name: 'Model ID' })).toBeInTheDocument();
    expect(screen.getByLabelText('Stop a run at (US dollars, OpenRouter)')).toBeInTheDocument();
    // The era picker lists only Forge's eras — Gauntlet's are another family — the bundled one
    // runnable, forge-1.0 kept as frozen history.
    const chooser = screen.getByRole('combobox', { name: 'Benchmark' });
    await waitFor(() => expect(chooser.textContent).toContain('Forge 2.0 · Scope Ledger 2'));
    fireEvent.click(chooser);
    expect(screen.getAllByRole('option').map((o) => o.textContent)).toEqual([
      'Forge 2.0 · Scope Ledger 2',
      'Forge 2.0 rc (history)',
      'Forge 1.0 · Scope Ledger (frozen)',
    ]);
    fireEvent.keyDown(chooser, { key: 'Escape' });
    assertStudioClean(container);
  });

  it('renders a forge-2.0 verdict: the v1 tiers and R1–R9, no band, screenshots, and no publish', async () => {
    mockElectron();
    mount();
    expect(await screen.findByRole('heading', { name: 'Forge 2.0 rc' })).toBeInTheDocument();
    // The rc of the current era is that benchmark before its freeze — never "an earlier benchmark".
    expect(screen.getByText('uncalibrated')).toBeInTheDocument();
    expect(screen.getByTestId('era-note').textContent).toMatch(
      /Scored by Forge 2\.0 rc, the uncalibrated scorer of Forge 2\.0 · Scope Ledger 2/
    );
    // Every one of the era's nineteen letters, each named, with the weight THIS verdict recorded.
    for (const tier of FORGE_ERA_TIER_ORDER['forge-2.0'])
      expect(screen.getByTestId(`tier-cell-${tier}`)).toBeInTheDocument();
    expect(screen.getByTestId('tier-cell-R5')).toHaveTextContent('Admin panel');
    expect(screen.getByTestId('tier-cell-R5')).toHaveTextContent('R5 93%');
    expect(screen.getByTestId('tier-cell-R5')).toHaveTextContent('weight 10%');
    expect(screen.getByTestId('tier-cell-R')).toHaveTextContent('Reconcile');
    expect(screen.getByTestId('tier-cell-R')).toHaveTextContent('R 75%');
    // No band capped it — said, not left blank.
    expect(await screen.findByText('Every admission band passed — no ceiling')).toBeInTheDocument();
    expect(screen.queryByTestId('forge-failed-band')).toBeNull();
    expect(screen.getByTestId('forge-verdict-facts')).toHaveTextContent(
      'forge-2.0-rc is uncalibrated (rc thresholds)'
    );
    // Screenshots from forge-shots/, under the forge captions.
    expect(screen.getByText('Dashboard widget · light')).toBeInTheDocument();
    expect(screen.getByText('Sprint action · dark')).toBeInTheDocument();
    // How the run ended against the tier's own budget.
    expect(
      screen.getByText('Finished on its own after 176 of 300 model calls')
    ).toBeInTheDocument();
    // Publishing refuses what the scorer marked not board-grade, in its era's words, before any POST.
    expect(screen.getByTestId('forge-publish-refusal')).toHaveTextContent(
      'Scored by forge-2.0-rc: the Forge thresholds are not frozen yet, so the result is not board-grade. Forge results publish once the forge-2.0 freeze pins them.'
    );
    expect(screen.getByRole('button', { name: 'Publish' })).toBeDisabled();
    // Every class the Forge card, the switch and the kit panel emit compiles (a dead utility is
    // invisible in the running app). lucide's identifiers are names, not utilities.
    const classes = allClasses(document.body).filter((c) => !c.startsWith('lucide'));
    expect(await missingUtilities(classes)).toEqual([]);
  });

  it('explains a forge-2.0 score under the reliability rule on the run card: tests × criticals × reliability = final, one line per group', async () => {
    // Sonnet 5.5 on the hardened task, recomposed under the group rule (the scorer's whole verdict).
    const ruled = projectBenchScore(forge2Sonnet as never);
    const mine = { ...MINE, score: forge2Sonnet.score, tiers: ruled.tiers, verdict: ruled.verdict };
    mockElectron({
      mine,
      sessions: [
        { ...SESSION, score: forge2Sonnet.score, tiers: ruled.tiers },
        SESSION_10,
        SB_SESSION,
      ],
    });
    (electron().benchmarkRunResult as ReturnType<typeof vi.fn>).mockImplementation(
      async (key: string) => (key === SESSION.runId ? mine : null)
    );
    mount();
    const stepsRegion = await screen.findByRole('region', { name: 'Score steps' });
    const shown = within(stepsRegion)
      .getAllByTestId('forge-step')
      .map((step) => step.querySelector('dd')?.textContent);
    expect(shown).toEqual([
      forge2Sonnet.critical.pre_severity_score.toFixed(4),
      forge2Sonnet.critical.multiplier.toFixed(4),
      forge2Sonnet.reliability.multiplier.toFixed(4),
      forge2Sonnet.score.toFixed(4),
    ]);
    const lines = within(stepsRegion).getAllByTestId('reliability-line');
    expect(lines.map((row) => row.getAttribute('data-tier'))).toEqual(
      forge2Sonnet.reliability.defects.map((defect) => defect.tier)
    );
    // The headline number is the same final.
    expect(
      screen.getByText(`${(forge2Sonnet.score * 100).toFixed(1)}%`, { selector: 'div' })
    ).toBeInTheDocument();
    // An rc result is still refused at publish in its scorer's words.
    expect(screen.getByTestId('forge-publish-refusal')).toHaveTextContent(
      /^Scored by forge-2\.0-rc: the Forge thresholds are not frozen yet/
    );
    const classes = allClasses(document.body).filter((c) => !c.startsWith('lucide'));
    expect(await missingUtilities(classes)).toEqual([]);
  });

  it('keeps a forge-1.0 run readable as frozen history: its own ten tiers and band, publish closed', async () => {
    window.location.hash = '#/benchmark?era=forge-1.0&run=cloud-forge-1';
    mockElectron();
    mount();
    expect(
      await screen.findByRole('heading', { name: 'Forge 1.0 · Scope Ledger' })
    ).toBeInTheDocument();
    expect(
      screen.getByText('Frozen on the site — sessions stay viewable; submissions are closed.')
    ).toBeInTheDocument();
    expect(screen.getByTestId('era-note').textContent).toMatch(
      /This run is from an earlier benchmark \(Forge 1\.0\)\. The current benchmark, Forge 2\.0 · Scope Ledger 2/
    );
    // Its own era's letters under their own names — no 2.0 family appears on a 1.0 run.
    for (const tier of FORGE_ERA_TIER_ORDER['forge-1.0'])
      expect(screen.getByTestId(`tier-cell-${tier}`)).toBeInTheDocument();
    expect(screen.getByTestId('tier-cell-K')).toHaveTextContent('Platform currency');
    expect(screen.getByTestId('tier-cell-K')).toHaveTextContent('K 83%');
    expect(screen.queryByTestId('tier-cell-R1')).toBeNull();
    // The band its own verdict recorded, with the check that held it — read, not restated.
    const band = await screen.findByTestId('forge-failed-band');
    expect(band).toHaveTextContent('Capped at 0.799');
    expect(band).toHaveTextContent('current platform, complete surfaces');
    expect(within(band).getByText('k_widget_edit_bridge')).toBeInTheDocument();
    expect(screen.getByText('Stopped at the 150-call budget')).toBeInTheDocument();
    // The site froze the era: publishing is closed, said in words.
    expect(screen.getByText('Benchmark frozen — submissions closed.')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Publish' })).toBeDisabled();
  });

  it('switches families through the URL, by click and by arrow key, and Gauntlet reads as before', async () => {
    mockElectron();
    mount();
    const toggle = screen.getByRole('radiogroup', { name: 'Benchmark type' });
    fireEvent.click(within(toggle).getByRole('radio', { name: /Gauntlet/ }));
    expect(window.location.hash).toBe('#/benchmark?family=sb');
    await waitFor(() =>
      expect(within(toggle).getByRole('radio', { name: /Gauntlet/ })).toHaveAttribute(
        'aria-checked',
        'true'
      )
    );
    expect(screen.getByTestId('family-intro').textContent).toMatch(
      /^Gauntlet 7\.2 · payments runs with Swarm or a single model/
    );
    expect(screen.queryByTestId('forge-kit-ready')).toBeNull();
    expect(screen.getByRole('button', { name: 'Swarm' })).toBeInTheDocument();
    expect(
      await screen.findByRole('heading', { name: 'Gauntlet 7.2 · payments' })
    ).toBeInTheDocument();
    fireEvent.keyDown(within(toggle).getByRole('radio', { name: /Gauntlet/ }), {
      key: 'ArrowRight',
    });
    expect(window.location.hash).toBe('#/benchmark?family=forge');
  });

  it('refuses to launch Forge, in words, while the site lists no current Forge era or the kit is unprepared', async () => {
    mockElectron({
      benchmarks: [SB_CURRENT],
      kit: { state: 'missing', missing: ['app-modules', 'wrapper/wrapper.js'], callBudget: 150 },
    });
    mount();
    expect(
      await screen.findByText(/leanzero\.net lists no current Forge benchmark yet/)
    ).toBeInTheDocument();
    expect(
      await screen.findByText('Not prepared yet: Forge app packages, Atlassian runtime wrapper.')
    ).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Prepare Forge kit' })).toBeEnabled();
    expect(screen.getByRole('button', { name: 'Run benchmark' })).toBeDisabled();
  });

  it('offers Retry scoring for a Forge build whose scoring did not finish — the saved build, no model run', async () => {
    window.location.hash = '#/benchmark?era=forge-2.0&run=cloud-forge-retry';
    mockElectron({
      mine: null,
      sessions: [
        {
          runId: 'cloud-forge-retry',
          scorerVersion: 'forge-2.0',
          startedAt: '2026-10-10T08:00:00.000Z',
          outcome: 'did_not_finish',
          publishable: false,
          retryScoring: { ready: true },
          scoringError: 'REFUSED: forge2_probe.mjs exited 1 without observations',
        },
      ],
    });
    const retry = vi.fn(() => new Promise(() => {}));
    const cloud = vi.fn();
    electron().benchmarkRetryScoring = retry;
    electron().benchmarkRunCloud = cloud;
    mount();
    expect(await screen.findByText('Model build completed. Scoring did not finish.')).toBeVisible();
    await screen.findByTestId('forge-kit-ready');
    const button = screen.getByRole('button', { name: 'Retry scoring' });
    await waitFor(() => expect(button).toBeEnabled());
    fireEvent.click(button);
    expect(retry).toHaveBeenCalledWith('cloud-forge-retry');
    expect(cloud).not.toHaveBeenCalled();
  });

  it('a finished Forge run whose clip could not be verified offers Retry scoring, not a clip-less post', async () => {
    window.location.hash = '#/benchmark?era=forge-2.0-rc&run=cloud-forge2-1';
    mockElectron({ sessions: [{ ...SESSION, retryScoring: { ready: true } }] });
    const retry = vi.fn(() => new Promise(() => {}));
    electron().benchmarkRetryScoring = retry;
    mount();
    const block = await screen.findByTestId('clip-retry');
    expect(block).toHaveTextContent('No graded browser clip could be verified');
    const button = within(block).getByRole('button', { name: 'Retry scoring' });
    await waitFor(() => expect(button).toBeEnabled());
    fireEvent.click(button);
    expect(retry).toHaveBeenCalledWith('cloud-forge2-1');
  });

  it('launches Forge as one model on forge-2.0, never as a swarm', async () => {
    mockElectron({ sessions: [], mine: null });
    const cloud = vi.fn(async () => null);
    const swarm = vi.fn(async () => null);
    electron().benchmarkRunCloud = cloud;
    electron().benchmarkRun = swarm;
    mount();
    await screen.findByTestId('forge-kit-ready');
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Model provider' })).toBeEnabled()
    );
    fireEvent.keyDown(screen.getByRole('button', { name: 'Model provider' }), { key: 'ArrowDown' });
    fireEvent.click(await screen.findByRole('menuitem', { name: 'OpenRouter' }));
    expect(screen.getByRole('button', { name: 'Run benchmark' })).toBeDisabled();
    fireEvent.change(screen.getByRole('textbox', { name: 'Model ID' }), {
      target: { value: 'openai/gpt-6-luna' },
    });
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Run benchmark' })).toBeEnabled()
    );
    // The model's saved effort rides to main (which records it as pinned and never sends it);
    // the form shows the tier's pin on the row.
    const effortRow = await screen.findByTestId('model-field-row-effort');
    expect(effortRow.textContent).toContain('Pinned for this run: Forge runs every model');
    fireEvent.click(screen.getByRole('button', { name: 'Run benchmark' }));
    await waitFor(() =>
      expect(cloud).toHaveBeenCalledWith('openrouter', 'openai/gpt-6-luna', 'forge-2.0', {
        effort: 'low',
      })
    );
    expect(swarm).not.toHaveBeenCalled();
  });
});
