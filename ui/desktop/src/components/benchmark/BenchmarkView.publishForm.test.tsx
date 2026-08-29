import { render, cleanup, fireEvent, waitFor } from '@testing-library/react';
import { describe, it, expect, vi, afterEach } from 'vitest';
import { IntlTestWrapper } from '../../i18n/test-utils';

/**
 * THE PUBLISH FORM IS A FORM: visible labels, and an invalid field that says WHY.
 *
 * Measured on the live app over CDP (2026-08-29): the Model input carried aria-invalid="true" with no
 * message linked to it, and both it and the Title input were named only by their placeholders — the
 * "Model *" label above the field was not associated with it. The node-count toggles were disabled
 * during a run with no reason on them.
 */

vi.mock('../swarm/SwarmRunPanel', async () => {
  const React = await import('react');
  const Stub = () => React.createElement('div', { 'data-testid': 'swarm-panel-stub' });
  return { SwarmRunPanel: Stub, default: Stub };
});

vi.mock('../swarm/useSamplingDefaults', () => ({
  useSaveSamplingDefaults: () => () => {},
}));

import BenchmarkView, { modelFieldProblem } from './BenchmarkView';

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

function mockElectron(opts: { running: boolean; modelId: string }) {
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
  e.benchmarkIdentity = vi.fn(async () => ({ handle: 'mihai' }));
  e.benchmarkShots = vi.fn(async () => []);
  e.readSwarmRun = vi.fn(async () => null);
  e.fleetStatus = vi.fn(async () => ({}));
}

describe('modelFieldProblem — the one rule behind aria-invalid, the hint and the Publish tooltip', () => {
  it('names the true reason', () => {
    expect(modelFieldProblem('')).toMatch(/^Required/);
    expect(modelFieldProblem('   ')).toMatch(/^Required/);
    expect(modelFieldProblem('qwen3')).toBe('Too short — 5 of at least 8 characters.');
    expect(modelFieldProblem('x'.repeat(121))).toBe('Too long — 121 of at most 120 characters.');
    expect(modelFieldProblem('qwen3.6-27b-mtp')).toBeNull();
  });
});

describe('the publish form', () => {
  afterEach(() => cleanup());

  it('labels Model and Title visibly, and links the invalid Model field to its reason', async () => {
    mockElectron({ running: false, modelId: 'qwen3' });
    const { findByLabelText, getByLabelText, getByRole } = render(
      <IntlTestWrapper>
        <BenchmarkView />
      </IntlTestWrapper>
    );
    const model = (await findByLabelText(/^Model/)) as HTMLInputElement;
    expect(model.tagName).toBe('INPUT');
    await waitFor(() => expect(model.value).toBe('qwen3'));
    expect(model.getAttribute('aria-invalid')).toBe('true');
    expect(describedBy(model)).toContain('Too short — 5 of at least 8 characters.');
    expect(getByRole('button', { name: /Publish/ }).getAttribute('title')).toContain(
      'Too short — 5 of at least 8 characters.'
    );

    const title = getByLabelText(/^Title/) as HTMLInputElement;
    expect(title.tagName).toBe('INPUT');

    fireEvent.change(model, { target: { value: '' } });
    expect(describedBy(model)).toContain('Required');

    fireEvent.change(model, { target: { value: 'qwen3.6-27b-mtp' } });
    expect(model.getAttribute('aria-invalid')).toBe('false');
    expect(describedBy(model)).not.toContain('Too short');
    expect(describedBy(model)).not.toContain('Required');
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
