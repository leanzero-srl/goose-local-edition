import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';
import type { Mac } from './macs';

const acp = vi.hoisted(() => ({
  submit: vi.fn(),
  place: vi.fn(),
  get: vi.fn(),
  log: vi.fn(),
}));
vi.mock('../../acp/leanzero-link', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../acp/leanzero-link')>()),
  leanzeroLinkJobsSubmit: (...a: unknown[]) => acp.submit(...a),
  leanzeroLinkJobsPlace: (...a: unknown[]) => acp.place(...a),
  leanzeroLinkJobsGet: (...a: unknown[]) => acp.get(...a),
  leanzeroLinkJobsLog: (...a: unknown[]) => acp.log(...a),
}));

import { RunJobDialog } from './RunJobDialog';

const ALLOWS = { manage_models: true, answer_chat: true, run_split: false, run_jobs: true };
const SELF: Mac = {
  key: 'self',
  isSelf: true,
  nodeId: 'mbp-local-faf207',
  name: 'Mihai Macbook',
  hostname: 'mihai-macbook',
  meshIp: '100.64.0.1',
  online: true,
  sessionsActive: 0,
  allows: { ...ALLOWS, run_jobs: false },
  load: null,
  pollError: null,
};
const STUDIO: Mac = {
  ...SELF,
  key: 'studio-7f3a',
  isSelf: false,
  nodeId: 'studio-7f3a',
  name: 'Work’s Mac Studio',
  allows: ALLOWS,
};
const GABEE: Mac = { ...STUDIO, key: 'gabee-1', nodeId: 'gabee-1', name: 'Gabee’s MacBook' };
const OFFLINE: Mac = { ...STUDIO, key: 'old-1', nodeId: 'old-1', name: 'Old Mini', online: false };

function renderDialog(macs: Mac[] = [SELF, STUDIO, GABEE, OFFLINE]) {
  const onClose = vi.fn();
  render(
    <IntlTestWrapper>
      <RunJobDialog macs={macs} onClose={onClose} />
    </IntlTestWrapper>
  );
  return onClose;
}

beforeEach(() => {
  acp.place.mockImplementation(async (args: { strategy?: string }) => ({
    placements: [
      { nodeId: 'gabee-1', userActive: false, score: 0.9 },
      { nodeId: 'studio-7f3a', userActive: true, score: 0.4 },
    ],
    strategy: args.strategy ?? 'local-first',
    refusals: [],
  }));
  acp.get.mockResolvedValue({
    job_id: 'j-new',
    origin_node: 'mbp-local-faf207',
    kind: 'shell',
    started_at: '2026-10-25T10:00:00Z',
    state: 'running',
    log_bytes: 0,
  });
  acp.log.mockResolvedValue({ from: 0, next: 0, data: '', done: false });
});
afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('RunJobDialog', () => {
  it('offers Best Mac plus each online Mac by name, and the ranking', async () => {
    const user = userEvent.setup();
    renderDialog();
    expect(await screen.findByTestId('run-job-dialog')).toBeInTheDocument();
    await waitFor(() => expect(acp.place).toHaveBeenCalledWith({ heavy: false }));
    expect(await screen.findByTestId('run-job-ranking')).toHaveTextContent(
      'Gabee’s MacBook · Work’s Mac Studio (in use)'
    );
    await user.click(screen.getByRole('combobox', { name: 'Run on' }));
    const options = screen.getAllByRole('option').map((o) => o.textContent);
    expect(options[0]).toContain('Best Mac');
    expect(options.slice(1)).toEqual([
      'Mihai Macbookjobs are off on this Mac',
      'Work’s Mac Studio',
      'Gabee’s MacBook',
    ]);
    // Never a native control.
    expect(document.querySelector('select')).toBeNull();
  });

  it('shows the Best Mac label in the field, never the raw value (D12)', async () => {
    const user = userEvent.setup();
    renderDialog();
    const field = screen.getByRole('combobox', { name: 'Run on' });
    expect(field).toHaveValue('Best Mac');
    expect(field).not.toHaveValue('__best__');
    await user.click(field);
    await user.click(screen.getByRole('option', { name: 'Work’s Mac Studio' }));
    expect(field).toHaveValue('Work’s Mac Studio');
  });

  it('lists a Mac whose jobs switch is off, disabled with the reason (D11)', async () => {
    const user = userEvent.setup();
    renderDialog();
    const field = screen.getByRole('combobox', { name: 'Run on' });
    await user.click(field);
    const off = screen.getByRole('option', { name: /Mihai Macbook/ });
    expect(off).toHaveAttribute('aria-disabled', 'true');
    expect(off).toHaveTextContent('jobs are off on this Mac');
    expect(screen.getByRole('option', { name: 'Work’s Mac Studio' })).not.toHaveAttribute(
      'aria-disabled'
    );
    // The offline Mac is still not listed.
    expect(screen.queryByRole('option', { name: /Old Mini/ })).toBeNull();
    await user.click(off);
    expect(field).toHaveValue('Best Mac');
    expect(screen.getByTestId('run-job-strategy-hint')).toBeInTheDocument();
  });

  it('submits to the best Mac with no target, then opens the job', async () => {
    const user = userEvent.setup();
    acp.submit.mockResolvedValue({ nodeId: 'gabee-1', jobId: 'j-new' });
    renderDialog();
    await user.type(screen.getByTestId('run-job-command'), 'make test');
    await user.click(screen.getByRole('switch', { name: 'Heavy job' }));
    await user.type(screen.getByTestId('run-job-mem'), '24');
    await user.type(screen.getByTestId('run-job-timeout'), '600');
    await user.click(screen.getByTestId('run-job-submit'));
    await waitFor(() =>
      expect(acp.submit).toHaveBeenCalledWith(
        { kind: 'shell', command: 'make test', heavy: true, need_mem_gb: 24, timeout_s: 600 },
        undefined,
        'local-first'
      )
    );
    expect(await screen.findByTestId('job-dialog')).toHaveTextContent('Job j-new');
    await waitFor(() => expect(acp.get).toHaveBeenCalledWith('gabee-1', 'j-new'));
    expect(screen.queryByTestId('run-job-dialog')).toBeNull();
  });

  it('submits a prompt to the Mac picked by name', async () => {
    const user = userEvent.setup();
    acp.submit.mockResolvedValue({ nodeId: 'studio-7f3a', jobId: 'j-new' });
    renderDialog();
    await user.click(screen.getByRole('combobox', { name: 'Run on' }));
    await user.click(screen.getByRole('option', { name: 'Work’s Mac Studio' }));
    await user.click(screen.getByTestId('run-job-kind-prompt'));
    await user.type(screen.getByTestId('run-job-command'), 'summarise the logs');
    await user.click(screen.getByTestId('run-job-submit'));
    await waitFor(() =>
      expect(acp.submit).toHaveBeenCalledWith(
        { kind: 'prompt', command: 'summarise the logs', heavy: false },
        'studio-7f3a'
      )
    );
  });

  it('shows the refusal reason inline and keeps the form', async () => {
    const user = userEvent.setup();
    acp.submit.mockRejectedValue({
      message: 'Invalid params',
      data: { code: 'heavy_busy', reason: 'heavy slot held' },
    });
    renderDialog();
    await user.type(screen.getByTestId('run-job-command'), 'make test');
    await user.click(screen.getByTestId('run-job-submit'));
    expect(await screen.findByTestId('run-job-error')).toHaveTextContent(
      'Not sent: heavy slot held'
    );
    expect(screen.getByTestId('run-job-dialog')).toBeInTheDocument();
    expect(screen.queryByTestId('job-dialog')).toBeNull();
  });

  it('does not send an empty command or a bad number', async () => {
    const user = userEvent.setup();
    renderDialog();
    expect(screen.getByTestId('run-job-submit')).toBeDisabled();
    await user.type(screen.getByTestId('run-job-command'), 'ls');
    expect(screen.getByTestId('run-job-submit')).toBeEnabled();
    await user.type(screen.getByTestId('run-job-timeout'), 'soon');
    expect(screen.getByTestId('run-job-submit')).toBeDisabled();
  });

  it('says so when no Mac takes jobs', async () => {
    acp.place.mockResolvedValue({ placements: [], strategy: 'local-first', refusals: [] });
    renderDialog([SELF]);
    expect(await screen.findByTestId('run-job-none')).toHaveTextContent(
      'None of your Macs takes jobs now'
    );
  });

  it('defaults the placement to the setting and overrides it for this job', async () => {
    const user = userEvent.setup();
    acp.place.mockImplementation(async (args: { strategy?: string }) => ({
      placements: [{ nodeId: 'gabee-1', userActive: false, score: 0.9 }],
      strategy: args.strategy ?? 'selfish',
      refusals: [],
    }));
    acp.submit.mockResolvedValue({ nodeId: 'gabee-1', jobId: 'j-new', queued: false });
    renderDialog();
    // The setting (selfish here) is what jobsPlace answered with no strategy given.
    await waitFor(() =>
      expect(screen.getByTestId('run-job-strategy-selfish')).toHaveAttribute('aria-checked', 'true')
    );
    expect(screen.getByTestId('run-job-strategy-hint').textContent).toContain(
      'Keeps this Mac free'
    );
    await user.click(screen.getByTestId('run-job-strategy-local-first'));
    await waitFor(() =>
      expect(acp.place).toHaveBeenLastCalledWith({ heavy: false, strategy: 'local-first' })
    );
    await user.type(screen.getByTestId('run-job-command'), 'ls');
    await user.click(screen.getByTestId('run-job-submit'));
    await waitFor(() =>
      expect(acp.submit).toHaveBeenCalledWith(
        { kind: 'shell', command: 'ls', heavy: false },
        undefined,
        'local-first'
      )
    );
  });

  it('hides the placement choice when a Mac is picked by name', async () => {
    const user = userEvent.setup();
    renderDialog();
    expect(await screen.findByTestId('run-job-strategy-selfish')).toBeInTheDocument();
    await user.click(screen.getByRole('combobox', { name: 'Run on' }));
    await user.click(screen.getByRole('option', { name: 'Work’s Mac Studio' }));
    expect(screen.queryByTestId('run-job-strategy-selfish')).toBeNull();
    expect(screen.queryByTestId('run-job-strategy-hint')).toBeNull();
  });

  it('shows each Mac’s reason when none has room, then opens the queued job', async () => {
    const user = userEvent.setup();
    acp.place.mockResolvedValue({
      placements: [],
      strategy: 'local-first',
      refusals: [
        { nodeId: 'gabee-1', code: 'memory_headroom', reason: 'needs 24 GB, 9 GB free' },
        { nodeId: 'studio-7f3a', code: 'heavy_slot_held', reason: 'a heavy job is running' },
      ],
    });
    acp.submit.mockResolvedValue({
      nodeId: 'mbp-local-faf207',
      jobId: 'q-1',
      queued: true,
      queuePosition: 2,
    });
    acp.get.mockResolvedValue({
      job_id: 'q-1',
      origin_node: 'mbp-local-faf207',
      kind: 'shell',
      started_at: '2026-10-25T10:00:00Z',
      state: 'queued',
      log_bytes: 0,
      queue: {
        position: 2,
        strategy: 'local-first',
        refusals: [
          { node_id: 'gabee-1', code: 'memory_headroom', reason: 'needs 24 GB, 9 GB free' },
        ],
      },
    });
    renderDialog();
    const refusals = await screen.findByTestId('run-job-refusals');
    expect(refusals.textContent).toContain('waits in line on this Mac');
    expect(refusals.textContent).toContain('Gabee’s MacBook: needs 24 GB, 9 GB free');
    expect(refusals.textContent).toContain('Work’s Mac Studio: a heavy job is running');
    await user.type(screen.getByTestId('run-job-command'), 'make test');
    await user.click(screen.getByTestId('run-job-submit'));
    expect(await screen.findByTestId('job-dialog')).toHaveTextContent('Job q-1');
    expect(screen.getByTestId('job-dialog-state').textContent).toBe('Waiting, #2 in line');
    await waitFor(() => expect(acp.get).toHaveBeenCalledWith('mbp-local-faf207', 'q-1'));
    expect(screen.getByTestId('job-dialog-cancel')).toBeInTheDocument();
  });
});
