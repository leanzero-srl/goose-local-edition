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

function mockElectron(opts: { running: boolean; modelId?: string }) {
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
}

describe('modelIdProblem / titleProblem — the two rules the hints and the Publish button read from', () => {
  it('refuses an absent or junk engine model id with the reason', () => {
    expect(modelIdProblem(undefined)).toMatch(/no model id from the engine/);
    expect(modelIdProblem('')).toMatch(/no model id from the engine/);
    expect(modelIdProblem('qwen3')).toMatch(/5-character model id — too short/);
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
    expect(
      (electron().benchmarkPublish as ReturnType<typeof vi.fn>).mock.calls[0][0]
    ).toEqual({ title: 'sb-7 first try' }); // no model in the payload — engine truth lives in main
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

  it('locks the tier and node-count toggles during a run with the reason on each', async () => {
    mockElectron({ running: true, modelId: 'qwen3.6-27b-mtp' });
    const { findByRole, getByRole } = render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );
    const two = await findByRole('button', { name: '2' });
    await waitFor(() => expect(two).toBeDisabled());
    expect(two.getAttribute('title')).toMatch(/Locked while a run is live/);
    expect(describedBy(two)).toMatch(/locked while the run is live/);

    const tier = getByRole('button', { name: 'sb-5.3' });
    expect(tier).toBeDisabled();
    expect(tier.getAttribute('title')).toMatch(/Locked while a run is live/);
    expect(describedBy(tier)).toMatch(/locked while the run is live/);
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
