import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { ForgeBuildSeries } from './ForgeBuildSeries';
import { seriesFixture, completedSeriesFixture, sessionsFixture } from './series.fixture';
import type { BenchSeriesSnapshot } from '../../benchSeries';
import type { BenchSession } from './bridge';

vi.mock('../../utils/canonical', () => ({ fetchCanonicalModelInfo: vi.fn(async () => null) }));

function mount(series: BenchSeriesSnapshot, sessions = sessionsFixture(series)) {
  Object.assign(window.electron, { benchmarkSeries: vi.fn(async () => series) });
  const open = vi.fn();
  const refresh = vi.fn(async () => {});
  render(
    <IntlTestWrapper>
      <ForgeBuildSeries
        series={series}
        sessions={sessions}
        disabled={series.running}
        onOpenBuild={open}
        onPublished={refresh}
      />
    </IntlTestWrapper>
  );
  return { open, refresh };
}

describe('Forge two-build presentation and publication', () => {
  it('shows separate actual build states, with no score or median for queued work', () => {
    const series = seriesFixture();
    mount(series);
    expect(screen.getAllByText('Build 1 of 2 is running')).toHaveLength(2);
    expect(screen.getByTestId('forge-series-build-2')).toHaveTextContent('Waiting to start');
    expect(screen.getByTestId('forge-series-summary')).toHaveTextContent('No scored build');
    expect(screen.getByRole('button', { name: 'Publish 2 builds' })).toBeDisabled();
    expect(screen.queryByText('0.0000')).toBeNull();
  });
  it('keeps one genuine score provisional after a second-build failure', () => {
    const base = completedSeriesFixture();
    const failed: BenchSeriesSnapshot = {
      ...base,
      stage: 'failed',
      builds: [
        base.builds[0],
        { ...base.builds[1], outcome: 'failed', score: undefined, error: 'Native grading failed' },
      ],
      summary: { count: 1, provisional: true, reason: 'Only the first build was graded.' },
    };
    mount(failed);
    expect(screen.getByTestId('forge-series-build-1')).toHaveTextContent('0.6000');
    expect(screen.getByTestId('forge-series-build-2')).toHaveTextContent('Native grading failed');
    expect(screen.getByTestId('forge-series-summary')).toHaveTextContent(
      'One scored build is provisional'
    );
    expect(screen.queryByText(/Median /)).toBeNull();
  });
  it('shows canonical median/range and opens each actual individual evidence card', () => {
    const series = completedSeriesFixture();
    const { open } = mount(series);
    expect(screen.getByTestId('forge-series-summary')).toHaveTextContent(
      'Local median 0.7000 · range 0.6000–0.8000'
    );
    fireEvent.click(screen.getByRole('button', { name: 'Open build 1 evidence' }));
    fireEvent.click(screen.getByRole('button', { name: 'Open build 2 evidence' }));
    expect(open.mock.calls).toEqual(series.builds.map((build) => [build.runId]));
  });
  it('retains a first accepted link and retries only the unaccepted second build', async () => {
    const series = completedSeriesFixture();
    let sessions: BenchSession[] = sessionsFixture(series);
    // Main publishes the pair in ONE call (benchSeriesPublish): build 1 lands, build 2 is refused, both reported.
    const post = vi.fn(async ({ runKey }: { runKey: string }) => {
      if (runKey === series.builds[0].runId) {
        sessions = sessions.map((session) =>
          session.runId === runKey
            ? {
                ...session,
                publishable: false,
                published: {
                  url: '/first',
                  title: 'Fixture pair',
                  score: 0.6,
                  publishedAt: '2026-10-12T11:00:00Z',
                  source: 'app',
                },
              }
            : session
        );
        return {
          ok: false,
          partial: true,
          error: 'Build 2 was refused: Server refused second build',
          builds: [
            { index: 1, ok: true, url: '/first' },
            { index: 2, ok: false, error: 'Server refused second build' },
          ],
        };
      }
      return {
        ok: true,
        pair: true,
        url: '/first',
        builds: [
          { index: 1, ok: true, alreadyLive: true, url: '/first' },
          { index: 2, ok: true, url: '/second' },
        ],
      };
    });
    Object.assign(window.electron, {
      benchmarkSessions: vi.fn(async () => ({ sessions })),
      benchmarkPublish: post,
    });
    mount(series);
    fireEvent.change(screen.getByRole('textbox', { name: 'Publication title' }), {
      target: { value: 'Fixture pair' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Publish 2 builds' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Server refused second build');
    expect(
      within(screen.getByTestId('forge-series-build-1')).getByRole('link', {
        name: 'Live on leanzero.net',
      })
    ).toHaveAttribute('href', 'https://leanzero.net/first');
    fireEvent.click(screen.getByRole('button', { name: 'Publish remaining build' }));
    await waitFor(() => expect(post).toHaveBeenCalledTimes(2));
    expect(post.mock.calls.map(([args]) => args.runKey)).toEqual([
      series.builds[0].runId,
      series.builds[1].runId,
    ]);
    await waitFor(() =>
      expect(screen.getAllByRole('link', { name: 'Live on leanzero.net' })).toHaveLength(2)
    );
  });
  it('refuses a disappearing child index instead of declaring a partial publication complete', async () => {
    const series = completedSeriesFixture();
    const post = vi.fn();
    Object.assign(window.electron, {
      benchmarkSessions: vi.fn(async () => ({ sessions: [] })),
      benchmarkPublish: post,
    });
    mount(series);
    fireEvent.change(screen.getByRole('textbox', { name: 'Publication title' }), {
      target: { value: 'Fixture' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Publish 2 builds' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Publishing waits for two complete');
    expect(post).not.toHaveBeenCalled();
  });
  it('reserves publication synchronously before a delayed history read so rapid clicks cannot duplicate either child', async () => {
    const series = completedSeriesFixture();
    let release: ((value: { sessions: BenchSession[] }) => void) | undefined;
    const pending = new Promise<{ sessions: BenchSession[] }>((resolve) => {
      release = resolve;
    });
    const history = vi.fn(() => pending);
    const post = vi.fn(async ({ runKey }: { runKey: string }) => ({
      ok: true,
      pair: true,
      url: `/${runKey}`,
      builds: series.builds.map((build) => ({
        index: build.index,
        ok: true,
        url: `/${build.runId}`,
      })),
    }));
    Object.assign(window.electron, { benchmarkSessions: history, benchmarkPublish: post });
    mount(series);
    fireEvent.change(screen.getByRole('textbox', { name: 'Publication title' }), {
      target: { value: 'Fixture pair' },
    });
    const button = screen.getByRole('button', { name: 'Publish 2 builds' });
    act(() => {
      button.click();
      button.click();
    });
    expect(history).toHaveBeenCalledTimes(1);
    expect(screen.getByRole('button', { name: 'Checking the two builds…' })).toBeDisabled();
    await act(async () => release?.({ sessions: sessionsFixture(series) }));
    // One publish action for the pair: main posts both builds (benchSeriesPublish), never the renderer one by one.
    await waitFor(() => expect(post).toHaveBeenCalledTimes(1));
    expect(post.mock.calls[0][0].runKey).toBe(series.builds[0].runId);
    await waitFor(() =>
      expect(screen.getAllByRole('link', { name: 'Live on leanzero.net' })).toHaveLength(2)
    );
  });
  it('refuses a formerly comparable pair when the fresh canonical series no longer has two comparable scores', async () => {
    const series = completedSeriesFixture();
    const post = vi.fn();
    Object.assign(window.electron, {
      benchmarkSessions: vi.fn(async () => ({ sessions: sessionsFixture(series) })),
      benchmarkPublish: post,
    });
    mount(series);
    Object.assign(window.electron, {
      benchmarkSeries: vi.fn(async () => ({
        ...series,
        summary: { count: 0, provisional: true, reason: 'Comparison inputs changed' },
      })),
    });
    fireEvent.change(screen.getByRole('textbox', { name: 'Publication title' }), {
      target: { value: 'Fixture pair' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Publish 2 builds' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Publishing waits for two complete');
    expect(post).not.toHaveBeenCalled();
  });
  it('prefills a readable title and, with no provider bill, sends the declared prices with both builds', async () => {
    const series = completedSeriesFixture();
    const usage = {
      sessions: [
        {
          accumulated_input_tokens: 1_000_000,
          accumulated_output_tokens: 10_000,
          accumulated_cache_read_tokens: 0,
        },
      ],
    };
    // The site's own answers per build, as main reports them: build 1 carries a receipt, build 2 joins it.
    const post = vi.fn(async () => ({
      ok: true,
      pair: true,
      url: '/x',
      builds: [
        { index: 1, ok: true, url: '/x', coupled: false, comparisonReceipt: true },
        { index: 2, ok: true, url: '/y', coupled: true, comparisonReceipt: true },
      ],
    }));
    Object.assign(window.electron, {
      benchmarkSessions: vi.fn(async () => ({ sessions: sessionsFixture(series) })),
      benchmarkPublish: post,
      benchmarkRunResult: vi.fn(async () => ({
        billedCost: { status: 'unavailable', provider: 'google' },
        modelUsage: usage,
      })),
      benchmarkPublishSuggestion: vi.fn(async () => ({
        title: 'Gemini 4 Argon, single model via Google',
        local: false,
      })),
    });
    mount(series);
    const title = screen.getByRole('textbox', { name: 'Publication title' }) as HTMLInputElement;
    await waitFor(() => expect(title.value).toBe('Gemini 4 Argon, single model via Google'));
    fireEvent.change(await screen.findByTestId('forge-series-price-input'), {
      target: { value: '1,25' },
    });
    fireEvent.change(screen.getByTestId('forge-series-price-output'), { target: { value: '10' } });
    expect(screen.getByTestId('forge-series-price-hint')).toHaveTextContent(
      'Published as an estimate: $2.70 for 2,000,000 prompt'
    );
    fireEvent.click(screen.getByRole('button', { name: 'Publish 2 builds' }));
    await waitFor(() => expect(post).toHaveBeenCalledTimes(1));
    for (const [args] of post.mock.calls as unknown as Array<[Record<string, unknown>]>) {
      expect(args.title).toBe('Gemini 4 Argon, single model via Google');
      expect(args.declaredPrices).toEqual({ inputUsdPerMTok: 1.25, outputUsdPerMTok: 10 });
    }
    const notes = await screen.findByTestId('forge-series-notes');
    await waitFor(() =>
      expect(notes).toHaveTextContent('Build 2: Coupled with this setup’s other build')
    );
    expect(notes).toHaveTextContent('Build 1: Carries its comparison receipt');
  });
  it('a half-filled price blocks publishing both builds', async () => {
    const series = completedSeriesFixture();
    const usage = {
      sessions: [
        {
          accumulated_input_tokens: 10,
          accumulated_output_tokens: 1,
          accumulated_cache_read_tokens: 0,
        },
      ],
    };
    Object.assign(window.electron, {
      benchmarkSessions: vi.fn(async () => ({ sessions: sessionsFixture(series) })),
      benchmarkPublish: vi.fn(),
      benchmarkRunResult: vi.fn(async () => ({
        billedCost: { status: 'unavailable' },
        modelUsage: usage,
      })),
      benchmarkPublishSuggestion: vi.fn(async () => null),
    });
    mount(series);
    fireEvent.change(screen.getByRole('textbox', { name: 'Publication title' }), {
      target: { value: 'T' },
    });
    fireEvent.change(await screen.findByTestId('forge-series-price-input'), {
      target: { value: '1' },
    });
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Publish 2 builds' })).toBeDisabled()
    );
  });
});
