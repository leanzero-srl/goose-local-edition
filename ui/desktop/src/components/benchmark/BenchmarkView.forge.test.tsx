import { render, screen, cleanup, fireEvent, waitFor, within } from '@testing-library/react';
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';
import { projectBenchScore } from '../../benchScoreProjection';
import forgeVerdict from './forge-alt.fixture.json';

/**
 * THE FORGE FAMILY ON THE BENCHMARK VIEW. The fixture is a REAL score_forge.py verdict (the alt golden
 * app, seed 0123456789abcdef, 2026-10-02): forge-1.0-rc, 0.799 — capped by the "current platform,
 * complete surfaces" band on k_widget_edit_bridge. The switch scopes the whole view to Forge: its era
 * picker, a single-model run form with the kit's readiness and the tier's run policy, its sessions, and a
 * run card that reads the verdict's own tier letters, admission band and screenshots — and refuses to
 * publish what the scorer itself marks not board-grade.
 */

vi.mock('../swarm/SwarmRunPanel', async () => {
  const React = await import('react');
  const Stub = () => React.createElement('div', { 'data-testid': 'swarm-panel-stub' });
  return { SwarmRunPanel: Stub, default: Stub };
});
vi.mock('../swarm/useSamplingDefaults', () => ({ useSaveSamplingDefaults: () => () => {} }));
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

const projected = projectBenchScore(forgeVerdict as never);
const MINE = {
  label: 'openai/gpt-6-luna · single agent',
  score: forgeVerdict.score,
  tiers: projected.tiers,
  nodes: 1,
  provider: 'openrouter',
  mine: true,
  scorerVersion: projected.scorerVersion,
  runMeta: {
    startedAt: '2026-10-02T20:00:00.000Z',
    finishedAt: '2026-10-02T21:10:00.000Z',
    engineEvents: 0,
    repairRounds: 0,
  },
  workdir: '/tmp/forge-run',
  modelId: 'openai/gpt-6-luna',
  runId: 'cloud-forge-1',
  verdict: projected.verdict,
  budget: { max_calls: 150, calls_used: 150, stopped_by: 'call_budget' },
};
const SESSION = {
  runId: 'cloud-forge-1',
  scorerVersion: 'forge-1.0-rc',
  startedAt: '2026-10-02T20:00:00.000Z',
  endedAt: '2026-10-02T21:10:00.000Z',
  outcome: 'finished',
  score: forgeVerdict.score,
  tiers: projected.tiers,
  nodes: 1,
  publishable: true,
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
  scorerVersion: 'forge-1.0',
  title: 'Forge 1.0 — Scope Ledger',
  family: 'forge',
  familyCurrent: true,
  current: false,
  frozen: false,
  baselines: [{ label: 'GPT-6 Luna', score: 0.31, model: 'openai/gpt-6-luna' }],
};
const KIT_READY = {
  state: 'ready',
  missing: [],
  kitLockSha256: '0a0b8c760fb127e629cb57d4db2c2437308ad963ceb787a5bf749c28574aea58',
  callBudget: 150,
  walletDefaultUsd: '50',
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
  e.benchmarkShots = vi.fn(async () => SHOTS);
  e.readSwarmRun = vi.fn(async () => null);
  e.fleetStatus = vi.fn(async () => ({}));
  e.benchmarkCatalog = vi.fn(async () => ({
    ok: true,
    stale: false,
    benchmarks: opts.benchmarks ?? [SB_CURRENT, FORGE_CURRENT],
  }));
  e.benchmarkSessions = vi.fn(async () => ({ sessions: opts.sessions ?? [SESSION, SB_SESSION] }));
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
      /^Forge 1\.0 · Scope Ledger runs one model in goose/
    );
    expect(await screen.findByTestId('forge-kit-ready')).toHaveTextContent('Forge kit ready');
    expect(screen.getByTestId('forge-run-policy')).toHaveTextContent(
      'One model · 150-call budget · stops at $50 unless you set a limit below · reasoning effort medium'
    );
    // No Swarm entrant for Forge; the single-model provider, model and spend limit are the form.
    expect(screen.queryByRole('button', { name: 'Swarm' })).toBeNull();
    expect(screen.getByRole('textbox', { name: 'Model ID' })).toBeInTheDocument();
    expect(screen.getByLabelText('Stop a run at (US dollars, OpenRouter)')).toBeInTheDocument();
    // The era picker lists only Forge's eras — Gauntlet's are another family.
    const chooser = screen.getByRole('combobox', { name: 'Benchmark' });
    await waitFor(() => expect(chooser.textContent).toContain('Forge 1.0 · Scope Ledger'));
    fireEvent.click(chooser);
    expect(screen.getAllByRole('option').map((o) => o.textContent)).toEqual([
      'Forge 1.0 · Scope Ledger',
      'Forge 1.0 rc (history)',
    ]);
    fireEvent.keyDown(chooser, { key: 'Escape' });
    assertStudioClean(container);
  });

  it('renders a forge verdict: tier letters L…E, the band that capped it, screenshots, and no publish', async () => {
    mockElectron();
    mount();
    expect(await screen.findByRole('heading', { name: 'Forge 1.0 rc' })).toBeInTheDocument();
    // The rc of the current era is that benchmark before its freeze — never "an earlier benchmark".
    expect(screen.getByText('uncalibrated')).toBeInTheDocument();
    expect(screen.getByTestId('era-note').textContent).toMatch(
      /Scored by Forge 1\.0 rc, the uncalibrated scorer of Forge 1\.0 · Scope Ledger/
    );
    for (const tier of ['L', 'K', 'T', 'R', 'S', 'B', 'U', 'V', 'A', 'E'])
      expect(screen.getByTestId(`tier-cell-${tier}`)).toBeInTheDocument();
    expect(screen.getByTestId('tier-cell-K')).toHaveTextContent('Platform currency');
    expect(screen.getByTestId('tier-cell-K')).toHaveTextContent('K 83%');
    // The band the verdict recorded, with the check that held it — read, not restated.
    const band = await screen.findByTestId('forge-failed-band');
    expect(band).toHaveTextContent('Capped at 0.799');
    expect(band).toHaveTextContent('current platform, complete surfaces');
    expect(within(band).getByText('k_widget_edit_bridge')).toBeInTheDocument();
    expect(screen.getByTestId('forge-verdict-facts')).toHaveTextContent(
      'forge-1.0-rc is uncalibrated (rc thresholds)'
    );
    // Screenshots from forge-shots/, under the forge captions.
    expect(screen.getByText('Dashboard widget · light')).toBeInTheDocument();
    expect(screen.getByText('Sprint action · dark')).toBeInTheDocument();
    // The call budget that ended it.
    expect(screen.getByText('Stopped at the 150-call budget')).toBeInTheDocument();
    // Publishing refuses what the scorer marked not board-grade, in words, before any POST.
    expect(screen.getByTestId('forge-publish-refusal')).toHaveTextContent(
      'Scored by forge-1.0-rc: the Forge thresholds are not frozen yet'
    );
    expect(screen.getByRole('button', { name: 'Publish' })).toBeDisabled();
    // Every class the Forge card, the switch and the kit panel emit compiles (a dead utility is
    // invisible in the running app). lucide's identifiers are names, not utilities.
    const classes = allClasses(document.body).filter((c) => !c.startsWith('lucide'));
    expect(await missingUtilities(classes)).toEqual([]);
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
    window.location.hash = '#/benchmark?era=forge-1.0&run=cloud-forge-retry';
    mockElectron({
      mine: null,
      sessions: [
        {
          runId: 'cloud-forge-retry',
          scorerVersion: 'forge-1.0',
          startedAt: '2026-10-03T08:00:00.000Z',
          outcome: 'did_not_finish',
          publishable: false,
          retryScoring: { ready: true },
          scoringError: 'REFUSED: forge_probe.mjs exited 1 without observations',
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

  it('launches Forge as one model on forge-1.0, never as a swarm', async () => {
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
    fireEvent.click(screen.getByRole('button', { name: 'Run benchmark' }));
    await waitFor(() =>
      expect(cloud).toHaveBeenCalledWith('openrouter', 'openai/gpt-6-luna', 'forge-1.0')
    );
    expect(swarm).not.toHaveBeenCalled();
  });
});
