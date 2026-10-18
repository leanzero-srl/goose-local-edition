import { render, cleanup, fireEvent, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, afterEach } from 'vitest';
import { IntlTestWrapper } from '../../i18n/test-utils';

/**
 * THE PUBLISH FORM TELLS THE TRUTH: the model id is engine truth (read-only — an editable field
 * publishes a lie), the title is the USER'S name for the run (required, never the generated
 * handle), and the outcome is a real state — the server's acceptance or its OWN error words —
 * never a gray status line.
 *
 * Measured on the live app over CDP (2026-08-30): Publish fired a real POST and the server's
 * "HTTP 400: checksSummary[49].tier must be one of A|B|C|D|J|V|P." landed in a neutral paragraph
 * that read like ordinary status; the header carried "publishing as mighty-crane-54f2".
 */

vi.mock('../swarm/SwarmRunPanel', async () => {
  const React = await import('react');
  const Stub = () => React.createElement('div', { 'data-testid': 'swarm-panel-stub' });
  return { SwarmRunPanel: Stub, default: Stub };
});

vi.mock('../swarm/useSamplingDefaults', () => ({
  useSaveSamplingDefaults: () => () => {},
}));

// goose's price list (canonical model info): Google's Gemini 4 Argon at $1.25 / $10 per 1M tokens.
const canonical = vi.fn(async (_provider: string, _model: string) => ({
  provider: 'google',
  model: 'gemini-4-argon',
  contextLimit: 1_000_000,
  reasoning: true,
  inputTokenCost: 1.25,
  outputTokenCost: 10,
  cacheReadTokenCost: null,
  currency: '$',
}));
vi.mock('../../utils/canonical', () => ({
  fetchCanonicalModelInfo: (provider: string, model: string) => canonical(provider, model),
}));

import BenchmarkView, { modelIdProblem, titleProblem } from './BenchmarkView';

type ElectronMock = Record<string, unknown>;
const electron = () => (window as unknown as { electron: ElectronMock }).electron;

const MINE = {
  name: 'Your fleet',
  score: 0.61,
  scorerVersion: 'sb-5.3',
  tiers: { A: 0.7, B: 0.5, C: 0.6, D: 0.6 },
  runMeta: {
    startedAt: '2026-08-28T09:00:00.000Z',
    finishedAt: '2026-08-28T12:00:00.000Z',
    engineEvents: 1200,
    repairRounds: 1,
  },
  workdir: '/tmp/bench',
};

const describedBy = (el: HTMLElement): string => {
  const id = el.getAttribute('aria-describedby');
  expect(id).toBeTruthy();
  return document.getElementById(id!)?.textContent ?? '';
};

function mockElectron(opts: { running: boolean; modelId?: string; frozen?: boolean }) {
  const e = electron();
  e.benchmarkStatus = vi.fn(async () =>
    opts.running
      ? {
          running: true,
          workdir: '/tmp/bench',
          startedAt: '2026-08-29T09:00:00.000Z',
          sampling: {},
        }
      : { running: false }
  );
  e.benchmarkRead = vi.fn(async () => ({ ...MINE, modelId: opts.modelId }));
  e.benchmarkShots = vi.fn(async () => []);
  e.readSwarmRun = vi.fn(async () => null);
  e.fleetStatus = vi.fn(async () => ({}));
  // The catalog/session contract the redesigned view renders from: one era, one finished
  // publishable session that IS the stored result — the publish form lives in its detail.
  e.benchmarkCatalog = vi.fn(async () => ({
    ok: true,
    fetchedAt: '2026-08-30T08:00:00.000Z',
    stale: false,
    benchmarks: [
      {
        scorerVersion: 'sb-5.3',
        title: 'VendorSync',
        current: !opts.frozen,
        frozen: opts.frozen === true,
        baselines: [{ label: 'Claude Opus 5', score: 0.9142, model: 'claude-opus-5' }],
      },
    ],
  }));
  e.benchmarkSessions = vi.fn(async () => ({
    sessions: [
      {
        runId: 'brun-mine',
        scorerVersion: 'sb-5.3',
        startedAt: '2026-08-28T09:00:00.000Z',
        endedAt: '2026-08-28T12:00:00.000Z',
        outcome: 'finished',
        score: 0.61,
        tiers: MINE.tiers,
        publishable: true,
      },
    ],
  }));
  e.benchmarkDeleteSession = vi.fn(async () => ({ ok: true }));
}

describe('modelIdProblem / titleProblem — the two rules the hints and the Publish button read from', () => {
  it('refuses an absent or junk engine model id with the reason', () => {
    expect(modelIdProblem(undefined)).toMatch(/no model id from the engine/);
    expect(modelIdProblem('')).toMatch(/no model id from the engine/);
    expect(modelIdProblem('o3')).toBeNull();
    expect(modelIdProblem('gpt-4o')).toBeNull();
    expect(modelIdProblem('x'.repeat(121))).toMatch(/120 characters/);
    expect(modelIdProblem('qwen3.6-27b-mtp')).toBeNull();
  });
  it('requires a user-chosen title', () => {
    expect(titleProblem('')).toMatch(/^Title is required/);
    expect(titleProblem('   ')).toMatch(/^Title is required/);
    expect(titleProblem('My M4 fleet first run')).toBeNull();
  });
});

describe('the publish form', () => {
  afterEach(() => cleanup());

  it('renders the model id READ-ONLY from the run (labeled, copyable, never editable)', async () => {
    mockElectron({ running: false, modelId: 'qwen3.6-27b-mtp' });
    const { findByLabelText } = render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );
    const model = (await findByLabelText(/^Model/)) as HTMLInputElement;
    await waitFor(() => expect(model.value).toBe('qwen3.6-27b-mtp'));
    expect(model.readOnly).toBe(true);
    expect(model.disabled).toBe(false); // disabled would kill selection-for-copy
    expect(describedBy(model)).toContain('Engine truth');
  });

  it('shows the header without the generated-handle chip', async () => {
    mockElectron({ running: false, modelId: 'qwen3.6-27b-mtp' });
    const { findByLabelText, queryByText } = render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );
    await findByLabelText(/^Model/);
    expect(queryByText(/publishing as/i)).toBeNull();
  });

  it('disables Publish while the title is empty, with the reason on the button', async () => {
    mockElectron({ running: false, modelId: 'qwen3.6-27b-mtp' });
    const { findByRole, getByLabelText } = render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );
    const publishBtn = await findByRole('button', { name: /Publish/ });
    await waitFor(() => expect(publishBtn).toBeDisabled());
    expect(publishBtn.getAttribute('title')).toContain('Title is required');

    const title = getByLabelText(/^Title/) as HTMLInputElement;
    expect(describedBy(title)).toContain('Title is required');
    fireEvent.change(title, { target: { value: 'My M4 fleet first run' } });
    expect(publishBtn).not.toBeDisabled();
    expect(publishBtn.getAttribute('title')).toContain('Publish this result');
  });

  it('disables Publish with the reason when the run recorded no model id', async () => {
    mockElectron({ running: false, modelId: undefined });
    const { findByRole } = render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );
    const publishBtn = await findByRole('button', { name: /Publish/ });
    await waitFor(() =>
      expect(publishBtn.getAttribute('title')).toContain('no model id from the engine')
    );
    expect(publishBtn).toBeDisabled();
  });

  it("renders the ERROR state with the server's own words, aria-live", async () => {
    mockElectron({ running: false, modelId: 'qwen3.6-27b-mtp' });
    const serverWords = 'HTTP 400: checksSummary[49].tier must be one of A|B|C|D|J|V|P.';
    electron().benchmarkPublish = vi.fn(async () => ({ ok: false, error: serverWords }));
    const { findByRole, getByLabelText, findByText, getByRole } = render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );
    const publishBtn = await findByRole('button', { name: /Publish/ });
    fireEvent.change(getByLabelText(/^Title/), { target: { value: 'sb-7 first try' } });
    await waitFor(() => expect(publishBtn).not.toBeDisabled());
    fireEvent.click(publishBtn);
    const errorLine = await findByText(new RegExp('checksSummary\\[49\\]'));
    expect(errorLine.textContent).toContain(serverWords);
    expect(getByRole('status').getAttribute('aria-live')).toBe('polite');
    // No model in the payload — engine truth lives in main; the run is named by its own key.
    const sent = (electron().benchmarkPublish as ReturnType<typeof vi.fn>).mock.calls[0][0];
    expect(sent).toEqual({ title: 'sb-7 first try', runKey: expect.any(String) });
    expect(sent).not.toHaveProperty('model');
  });

  it("surfaces main's frozen refusal verbatim — the {ok:false, status:'error', message} shape", async () => {
    mockElectron({ running: false, modelId: 'qwen3.6-27b-mtp' });
    const refusal = 'benchmark sb-5.3 is frozen — submissions closed by the site';
    electron().benchmarkPublish = vi.fn(async () => ({
      ok: false,
      status: 'error',
      message: refusal,
    }));
    const { findByRole, getByLabelText, findByText } = render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );
    const publishBtn = await findByRole('button', { name: /Publish/ });
    fireEvent.change(getByLabelText(/^Title/), { target: { value: 'stale cache try' } });
    await waitFor(() => expect(publishBtn).not.toBeDisabled());
    fireEvent.click(publishBtn);
    const errorLine = await findByText(/Publish failed:/);
    expect(errorLine.textContent).toContain(refusal);
  });

  it('renders the ACCEPTED state with what went live: title, score, url', async () => {
    mockElectron({ running: false, modelId: 'qwen3.6-27b-mtp' });
    electron().benchmarkPublish = vi.fn(async () => ({
      ok: true,
      status: 'live',
      url: '/agentic-benchmarks/run/brun-1234',
    }));
    const { findByRole, getByLabelText, findByText } = render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );
    const publishBtn = await findByRole('button', { name: /Publish/ });
    fireEvent.change(getByLabelText(/^Title/), { target: { value: 'My M4 fleet first run' } });
    await waitFor(() => expect(publishBtn).not.toBeDisabled());
    fireEvent.click(publishBtn);
    const accepted = await findByText(/Live on leanzero\.net/);
    expect(accepted.textContent).toContain('My M4 fleet first run');
    expect(accepted.textContent).toContain('61.0%');
    expect(accepted.textContent).toContain('/agentic-benchmarks/run/brun-1234');
  });

  it('locks the node-count toggles and the benchmark dropdown during a run, with the reason', async () => {
    mockElectron({ running: true, modelId: 'qwen3.6-27b-mtp' });
    const { findByRole, getByRole, queryByRole } = render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );
    await findByRole('button', { name: '2' });
    // Re-query inside waitFor: the controls move from the idle section into the live-run block
    // when the re-attach lands, so the first-found node is replaced, not mutated.
    await waitFor(() => expect(getByRole('button', { name: '2' })).toBeDisabled());
    const two = getByRole('button', { name: '2' });
    expect(two.getAttribute('title')).toMatch(/Locked while a run is live/);
    expect(describedBy(two)).toMatch(/locked while the run is live/);

    // The benchmark is a DROPDOWN (Mihai 2026-09-22: "the type of SB should be a drop down"), not
    // a row of tier buttons; it is locked with the rest of the setup while a run is live.
    expect(queryByRole('button', { name: 'sb-5.3' })).toBeNull();
    expect(queryByRole('button', { name: /sb-6/ })).toBeNull();
    expect(getByRole('combobox', { name: 'Benchmark' })).toBeDisabled();
  });

  it('disables Publish on a FROZEN era with submissions-closed as the reason', async () => {
    mockElectron({ running: false, modelId: 'qwen3.6-27b-mtp', frozen: true });
    const { findByRole, getByLabelText } = render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );
    const publishBtn = await findByRole('button', { name: /Publish/ });
    fireEvent.change(getByLabelText(/^Title/), { target: { value: 'frozen era try' } });
    await waitFor(() =>
      expect(publishBtn.getAttribute('title')).toContain('Benchmark frozen — submissions closed')
    );
    expect(publishBtn).toBeDisabled();
  });

  it('describes the node buttons by what they do when nothing locks them', async () => {
    mockElectron({ running: false, modelId: 'qwen3.6-27b-mtp' });
    const { findByRole } = render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );
    const one = await findByRole('button', { name: '1' });
    expect(one).not.toBeDisabled();
    expect(one.getAttribute('title')).toBe('Run on 1 node');
    expect(one.getAttribute('aria-describedby')).toBeNull();
  });
});

describe('the price a publish carries (Mihai 2026-10-18)', () => {
  afterEach(() => cleanup());
  const usage = {
    status: 'recorded',
    sessions: [
      {
        accumulated_input_tokens: 3_000_000,
        accumulated_output_tokens: 40_000,
        accumulated_cache_read_tokens: 2_000_000,
      },
    ],
  };
  const openRouterBill = {
    provider: 'openrouter',
    source: 'https://openrouter.ai/api/v1/generation (usage per generation id)',
    status: 'complete',
    billed_usd: 0.2959,
    request_count: 263,
    missing: [],
    incomplete_reasons: [],
  };
  const renderWith = (row: Record<string, unknown>, setup?: () => void) => {
    canonical.mockClear();
    mockElectron({ running: false, modelId: 'gemini-4-argon' });
    electron().benchmarkRead = vi.fn(async () => ({ ...MINE, modelId: 'gemini-4-argon', ...row }));
    electron().benchmarkPublish = vi.fn(async () => ({
      ok: true,
      url: '/agentic-benchmarks/run/brun-1',
    }));
    setup?.();
    return render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );
  };

  it('an OpenRouter-billed run offers no price fields: its bill is the price', async () => {
    const { findByTestId, queryByTestId, findByRole, getByLabelText } = renderWith({
      provider: 'openrouter',
      billedCost: openRouterBill,
      modelUsage: usage,
    });
    expect((await findByTestId('publish-price-billed')).textContent).toContain('$0.2959');
    expect(queryByTestId('publish-price')).toBeNull();
    const publishBtn = await findByRole('button', { name: /Publish/ });
    fireEvent.change(getByLabelText(/^Title/), { target: { value: 'Billed run' } });
    await waitFor(() => expect(publishBtn).not.toBeDisabled());
    fireEvent.click(publishBtn);
    await waitFor(() => expect(electron().benchmarkPublish).toHaveBeenCalled());
    const sent = (electron().benchmarkPublish as ReturnType<typeof vi.fn>).mock.calls[0][0];
    expect(sent).not.toHaveProperty('declaredPrices');
  });

  it("a run with no bill is prefilled from goose's price list and publishes the declared prices", async () => {
    const { findByTestId, findByRole, getByLabelText } = renderWith({
      provider: 'google',
      billedCost: { status: 'unavailable', provider: 'google', reason: 'no per-call bill' },
      modelUsage: usage,
    });
    await waitFor(async () =>
      expect(((await findByTestId('publish-price-input')) as HTMLInputElement).value).toBe('1.25')
    );
    expect(canonical).toHaveBeenCalledWith('google', 'gemini-4-argon');
    expect(((await findByTestId('publish-price-output')) as HTMLInputElement).value).toBe('10');
    // The run read 2M cached tokens and goose's list has no cache-read price: charging them at the full input
    // price would overstate the bill, so Publish waits for the cached price (app review 2026-10-18).
    const publishBtn = await findByRole('button', { name: /Publish/ });
    fireEvent.change(getByLabelText(/^Title/), { target: { value: 'Argon build' } });
    await waitFor(() => expect(publishBtn.getAttribute('title')).toMatch(/cached input price/));
    expect(publishBtn).toBeDisabled();
    fireEvent.change(await findByTestId('publish-price-cached'), { target: { value: '0,125' } });
    // 1M uncached x $1.25 + 2M cached x $0.125 + 40k x $10 = $1.90
    await waitFor(async () =>
      expect((await findByTestId('publish-price-hint')).textContent).toContain(
        'Published as an estimate: $1.90 for 3,000,000 prompt (2,000,000 cached) and 40,000 completion tokens'
      )
    );
    await waitFor(() => expect(publishBtn).not.toBeDisabled());
    fireEvent.click(publishBtn);
    await waitFor(() => expect(electron().benchmarkPublish).toHaveBeenCalled());
    const sent = (electron().benchmarkPublish as ReturnType<typeof vi.fn>).mock.calls[0][0];
    expect(sent.declaredPrices).toEqual({
      inputUsdPerMTok: 1.25,
      outputUsdPerMTok: 10,
      cachedInputUsdPerMTok: 0.125,
    });
  });

  it('a run that used this Mac is never prefilled from a cloud price list, whatever provider served it', async () => {
    const { findByTestId } = renderWith({
      provider: 'openai',
      localEngine: true,
      billedCost: { status: 'unavailable', provider: 'openai' },
      modelUsage: usage,
    });
    const input = (await findByTestId('publish-price-input')) as HTMLInputElement;
    await new Promise((r) => setTimeout(r, 50));
    expect(input.value).toBe('');
    expect(canonical).not.toHaveBeenCalled();
  });

  it('prefills a readable title from main, and the success card says what the site did', async () => {
    const { findByRole, findByLabelText, findByText } = renderWith(
      { provider: 'openrouter', billedCost: openRouterBill, modelUsage: usage },
      () => {
        electron().benchmarkPublishSuggestion = vi.fn(async () => ({
          title: 'DeepSeek V4.1 Flash 1017, single model via OpenRouter',
          local: false,
        }));
        electron().benchmarkPublish = vi.fn(async () => ({
          ok: true,
          url: '/agentic-benchmarks/run/brun-1',
          baseline: true,
          priceWarning: "Published without the price: leanzero.net did not accept this run's bill (x).",
        }));
      }
    );
    const title = (await findByLabelText(/^Title/)) as HTMLInputElement;
    await waitFor(() => expect(title.value).toBe('DeepSeek V4.1 Flash 1017, single model via OpenRouter'));
    const publishBtn = await findByRole('button', { name: /Publish/ });
    await waitFor(() => expect(publishBtn).not.toBeDisabled());
    fireEvent.click(publishBtn);
    expect(await findByText('Listed as a LeanZero cloud baseline.')).toBeInTheDocument();
    expect(await findByText(/Published without the price/)).toBeInTheDocument();
  });

  it('a half-filled price blocks Publish with the reason; clearing it publishes without a price', async () => {
    const { findByTestId, findByRole, getByLabelText } = renderWith({
      provider: 'google',
      modelUsage: usage,
    });
    const input = (await findByTestId('publish-price-input')) as HTMLInputElement;
    await waitFor(() => expect(input.value).toBe('1.25'));
    fireEvent.change(await findByTestId('publish-price-output'), { target: { value: '' } });
    fireEvent.change(getByLabelText(/^Title/), { target: { value: 'Half priced' } });
    const publishBtn = await findByRole('button', { name: /Publish/ });
    await waitFor(() =>
      expect(publishBtn.getAttribute('title')).toMatch(/both the input and the output price/)
    );
    expect(publishBtn).toBeDisabled();
    fireEvent.change(input, { target: { value: '' } });
    await waitFor(() => expect(publishBtn).not.toBeDisabled());
    fireEvent.click(publishBtn);
    await waitFor(() => expect(electron().benchmarkPublish).toHaveBeenCalled());
    const sent = (electron().benchmarkPublish as ReturnType<typeof vi.fn>).mock.calls[0][0];
    expect(sent).not.toHaveProperty('declaredPrices');
  });
});
