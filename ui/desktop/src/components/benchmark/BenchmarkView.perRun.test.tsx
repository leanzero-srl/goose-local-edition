import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { IntlTestWrapper } from '../../i18n/test-utils';
import solarVerdict from './sb72-solar-noserve.fixture.json';

/**
 * EVERY FINISHED RUN IS A FULL CARD. MEASURED 2026-10-03: a finished DeepSeek Pro run (sb-7.2, 0.699)
 * lost its breakdown and its Publish form the moment the next run (solar-mini4, 0.0083) finished,
 * because result.json held only the latest run. Each run now reads its OWN result (main's per-run
 * store, or the verdict in its own tree) and publishes it under its own key; a posted run says where
 * it is live instead of offering a second post.
 */

vi.mock('../swarm/SwarmRunPanel', async () => {
  const React = await import('react');
  const Stub = () => React.createElement('div');
  return { SwarmRunPanel: Stub, default: Stub };
});
vi.mock('../swarm/useSamplingDefaults', () => ({ useSaveSamplingDefaults: () => () => {} }));

import BenchmarkView from './BenchmarkView';

type ElectronMock = Record<string, unknown>;
const electron = () => (window as unknown as { electron: ElectronMock }).electron;

const row = (runId: string, model: string, score: number, startedAt: string) => ({
  label: `${model} · single agent`,
  score,
  tiers: { A: score },
  nodes: 1,
  provider: 'openrouter',
  mine: true,
  scorerVersion: 'sb-7.2',
  runId,
  modelId: model,
  workdir: `/runs/${runId}-r0`,
  runMeta: { startedAt, finishedAt: startedAt, engineEvents: 0, repairRounds: 0 },
  verdict: {
    checks: [{ check: 'serves_page', tier: 'A', score: 1, detail: `GET / -> 200 (${model})` }],
    tiers: { A: { mean: score, checks: 1, weight: 1 } },
  },
});
const DEEPSEEK = row(
  'cloud-07caff2d',
  '~deepseek/deepseek-pro-latest',
  0.699,
  '2026-10-02T20:00:00.000Z'
);
const SOLAR = row('cloud-25eb9626', 'upstage/solar-mini4', 0.0083, '2026-10-02T23:00:00.000Z');
const session = (r: typeof DEEPSEEK, extra: Record<string, unknown> = {}) => ({
  runId: r.runId,
  scorerVersion: 'sb-7.2',
  startedAt: r.runMeta.startedAt,
  endedAt: r.runMeta.startedAt,
  outcome: 'finished',
  score: r.score,
  tiers: r.tiers,
  nodes: 1,
  publishable: true,
  ...extra,
});

function mockElectron(sessions: unknown[]) {
  const e = electron();
  e.benchmarkRuntimeStatus = vi.fn(async () => ({ state: 'ready', downloadBytes: 0 }));
  e.benchmarkStatus = vi.fn(async () => ({ running: false }));
  // result.json is the LATEST run (solar) — DeepSeek's row must come from its own read.
  e.benchmarkRead = vi.fn(async () => SOLAR);
  e.benchmarkRunResult = vi.fn(async (key: string) =>
    key === DEEPSEEK.runId ? DEEPSEEK : key === SOLAR.runId ? SOLAR : null
  );
  e.benchmarkShots = vi.fn(async (workdir: string) => [
    { name: 'loaded', caption: `shot of ${workdir}`, b64: 'iVBORw0KGgo=' },
  ]);
  e.benchmarkMedia = vi.fn(async () => ({ videos: [] }));
  e.readSwarmRun = vi.fn(async () => null);
  e.fleetStatus = vi.fn(async () => ({}));
  e.benchmarkCatalog = vi.fn(async () => ({
    ok: true,
    benchmarks: [
      {
        scorerVersion: 'sb-7.2',
        title: 'SB7.2 payments',
        current: true,
        frozen: false,
        baselines: [],
      },
    ],
  }));
  e.benchmarkSessions = vi.fn(async () => ({ sessions }));
}

const mount = () =>
  render(
    <IntlTestWrapper>
      <BenchmarkView />
    </IntlTestWrapper>
  );

describe('every finished run keeps its own card and can be published', () => {
  afterEach(() => {
    cleanup();
    window.location.hash = '';
  });

  it('an OLDER finished run shows its own breakdown, shots and Publish form, and posts under its own key', async () => {
    mockElectron([session(SOLAR), session(DEEPSEEK)]);
    window.location.hash = `#/benchmark?era=sb-7.2&run=${DEEPSEEK.runId}`;
    const publish = vi.fn(async () => ({ ok: true, url: '/runs/abc' }));
    electron().benchmarkPublish = publish;
    mount();
    // Its own check evidence and its own tree's screenshots — never the latest run's.
    expect(
      await screen.findByText(/GET \/ -> 200 \(~deepseek\/deepseek-pro-latest\)/)
    ).toBeVisible();
    expect(screen.getByText(`shot of ${DEEPSEEK.workdir}`)).toBeVisible();
    expect(screen.queryByText(/kept with the latest stored result only/)).toBeNull();
    expect(screen.getByDisplayValue('~deepseek/deepseek-pro-latest')).toBeInTheDocument();
    fireEvent.change(screen.getByPlaceholderText('e.g. My M4 fleet first run'), {
      target: { value: 'DeepSeek Pro' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Publish' }));
    await waitFor(() =>
      expect(publish).toHaveBeenCalledWith({ title: 'DeepSeek Pro', runKey: DEEPSEEK.runId })
    );
    expect(await screen.findByText(/Live on leanzero.net — “DeepSeek Pro” · 69.9%/)).toBeVisible();
  });

  it('a run already posted says where it is live and offers no second post', async () => {
    mockElectron([
      session(SOLAR),
      session(DEEPSEEK, {
        publishable: false,
        published: {
          url: '/runs/abc',
          title: 'DeepSeek Pro',
          score: 0.699,
          publishedAt: 'x',
          source: 'app',
        },
      }),
    ]);
    window.location.hash = `#/benchmark?era=sb-7.2&run=${DEEPSEEK.runId}`;
    mount();
    expect(await screen.findByTestId('published-live')).toHaveTextContent(
      'Live on leanzero.net — “DeepSeek Pro” · 69.9% · leanzero.net/runs/abc'
    );
    expect(screen.queryByRole('button', { name: 'Publish' })).toBeNull();
  });

  it('a run the site board named (posted before the index) shows its board URL', async () => {
    mockElectron([
      session(SOLAR),
      session(DEEPSEEK, {
        publishable: false,
        published: {
          url: 'https://leanzero.net/agentic-benchmarks/run/brun-2cbdb9f5-48c9-4845-ac7a-9d025c1cc0d0',
          title: 'GPT-6 Luna, single model via OpenRouter',
          score: 0.699,
          publishedAt: null,
          source: 'board',
        },
      }),
    ]);
    window.location.hash = `#/benchmark?era=sb-7.2&run=${DEEPSEEK.runId}`;
    mount();
    expect(await screen.findByTestId('published-live')).toHaveTextContent(
      'Live on leanzero.net — “GPT-6 Luna, single model via OpenRouter” · 69.9% · leanzero.net/agentic-benchmarks/run/brun-2cbdb9f5-48c9-4845-ac7a-9d025c1cc0d0'
    );
    expect(screen.queryByRole('button', { name: 'Publish' })).toBeNull();
  });

  it('a run whose app never served a page states that on its card instead of a missing clip', async () => {
    // The REAL solar-mini4 rows (sb72-solar-noserve.fixture.json).
    const solar = {
      ...SOLAR,
      verdict: {
        checks: solarVerdict.checks,
        tiers: { A: { mean: 0.0083, checks: 1, weight: 1 } },
      },
    };
    mockElectron([session(SOLAR)]);
    electron().benchmarkRunResult = vi.fn(async () => solar);
    electron().benchmarkMedia = vi.fn(async () => ({
      videos: [],
      error: 'ENOENT: no such file or directory, open .../bench-media/media-manifest.json',
    }));
    window.location.hash = `#/benchmark?era=sb-7.2&run=${SOLAR.runId}`;
    mount();
    expect(await screen.findByTestId('recording-absent')).toHaveTextContent(
      'No recording: the app never served a page (server_runs: process survives 5s without binding; serves_page: GET / -> None).'
    );
    expect(screen.queryByText(/ENOENT/)).toBeNull();
  });

  it('switching runs swaps the card to the newly selected run', async () => {
    mockElectron([session(SOLAR), session(DEEPSEEK)]);
    window.location.hash = `#/benchmark?era=sb-7.2&run=${SOLAR.runId}`;
    mount();
    expect(await screen.findByDisplayValue('upstage/solar-mini4')).toBeInTheDocument();
    window.location.hash = `#/benchmark?era=sb-7.2&run=${DEEPSEEK.runId}`;
    window.dispatchEvent(new Event('hashchange'));
    expect(await screen.findByDisplayValue('~deepseek/deepseek-pro-latest')).toBeInTheDocument();
    expect(screen.queryByDisplayValue('upstage/solar-mini4')).toBeNull();
  });
});
