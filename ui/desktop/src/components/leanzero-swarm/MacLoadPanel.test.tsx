import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IntlTestWrapper } from '../../i18n/test-utils';

const mockGet = vi.fn();
const mockLog = vi.fn();
const mockCancel = vi.fn();
vi.mock('../../acp/leanzero-link', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../acp/leanzero-link')>()),
  leanzeroLinkJobsGet: (...a: unknown[]) => mockGet(...a),
  leanzeroLinkJobsLog: (...a: unknown[]) => mockLog(...a),
  leanzeroLinkJobsCancel: (...a: unknown[]) => mockCancel(...a),
}));
const mockRead = vi.fn();
const mockUpsert = vi.fn(async () => {});
vi.mock('../../acp/config', () => ({
  acpReadConfig: (...a: unknown[]) => mockRead(...a),
  acpUpsertConfig: (...a: unknown[]) => mockUpsert(...(a as [])),
}));

const macsMock = vi.hoisted(() => ({ macs: null as null | { nodeId: string; name: string }[] }));
vi.mock('./useMacs', async (importActual) => ({
  ...(await importActual<typeof import('./useMacs')>()),
  useMacsMaybe: () => (macsMock.macs ? { macs: macsMock.macs } : null),
}));

import { JobsSettings, MacLoadBars } from './MacLoadPanel';
import { BAR_FILL, JOBS_KEYS } from './macLoad';
import { brief, busyLoad } from './macLoad.fixtures';

const renderBars = (load = busyLoad()) =>
  render(
    <IntlTestWrapper>
      <MacLoadBars load={load} target="n-studio" macName="Work’s Mac Studio" />
    </IntlTestWrapper>
  );

beforeEach(() => {
  mockGet.mockReset();
  mockLog.mockReset();
  mockCancel.mockReset();
  mockRead.mockReset();
  mockUpsert.mockClear();
  macsMock.macs = null;
});
afterEach(cleanup);

describe('MacLoadBars', () => {
  it('shows three meters with distinct solid fills, counts and words', () => {
    const { container } = renderBars();
    const meters = screen.getAllByRole('meter');
    expect(meters.map((m) => m.getAttribute('aria-label'))).toEqual([
      'Own use',
      'AI inference',
      'Jobs',
    ]);
    expect(meters.map((m) => m.getAttribute('aria-valuenow'))).toEqual(['62', '75', '50']);
    for (const id of ['own', 'inference', 'jobs'] as const) {
      const fill = screen.getByTestId(`mac-load-${id}-fill`);
      expect(fill.className).toContain(BAR_FILL[id]);
    }
    expect(screen.getByTestId('mac-load-own-fill').style.width).toBe('62%');
    expect(screen.getByTestId('mac-load-own-count').textContent).toBe('62%');
    expect(screen.getByTestId('mac-load-inference-count').textContent).toBe('3');
    expect(screen.getByTestId('mac-load-jobs-count').textContent).toBe('2');
    expect(screen.getByTestId('mac-load-own').textContent).toContain(
      'Idle 7m · cpu 35% · memory 62% used · gpu 48%'
    );
    expect(screen.getByTestId('mac-load-inference').textContent).toContain(
      '2 engine requests · 1 router slot'
    );
    expect(screen.getByTestId('mac-load-jobs').textContent).toContain('2 jobs running');
    // The house rules: no left rails, no rounded corners on the bar.
    expect(container.innerHTML).not.toMatch(/border-l-\d|rounded/);
  });

  it('lists every job, live and ended, with its origin and state', () => {
    renderBars();
    const list = screen.getByTestId('mac-load-job-list');
    expect(within(list).getAllByRole('listitem')).toHaveLength(3);
    expect(list.textContent).toContain('prompt from n-gabee');
    expect(screen.getByTestId('mac-job-j-done-state').textContent).toBe('done');
  });

  it('says so when the Mac reports no load', () => {
    render(
      <IntlTestWrapper>
        <MacLoadBars load={null} target="n-studio" macName="Studio" />
      </IntlTestWrapper>
    );
    expect(screen.getByTestId('mac-load-none')).toBeInTheDocument();
    expect(screen.queryAllByRole('meter')).toHaveLength(0);
  });

  it('opens the job dialog, tails the log, and cancels only after the confirm dialog', async () => {
    const user = userEvent.setup();
    const running = { ...busyLoad().jobs[0], log_bytes: 6 };
    mockGet.mockResolvedValue(running);
    mockLog.mockResolvedValue({ from: 0, next: 6, data: 'hello\n', done: false });
    mockCancel.mockResolvedValue({
      ...running,
      state: 'cancelled',
      exit_code: 143,
      reason: 'cancelled',
    });
    renderBars();
    await user.click(screen.getByTestId('mac-job-j-run'));
    const dialog = await screen.findByTestId('job-dialog');
    await waitFor(() => expect(screen.getByTestId('job-dialog-log').textContent).toBe('hello\n'));
    expect(mockGet).toHaveBeenCalledWith('n-studio', 'j-run');
    expect(mockLog).toHaveBeenCalledWith('n-studio', 'j-run', 0);
    expect(within(dialog).getByTestId('job-dialog-state').textContent).toBe('running');

    // Keep running: nothing is cancelled.
    await user.click(screen.getByTestId('job-dialog-cancel'));
    await user.click(await screen.findByRole('button', { name: 'Keep running' }));
    expect(mockCancel).not.toHaveBeenCalled();

    // Confirm: the cancel goes to the Mac that runs it, the dialog shows how it ended.
    await user.click(screen.getByTestId('job-dialog-cancel'));
    const confirmBox = await screen.findByText('Cancel job j-run?');
    expect(confirmBox).toBeInTheDocument();
    const buttons = screen.getAllByRole('button', { name: 'Cancel job' });
    await user.click(buttons[buttons.length - 1]);
    await waitFor(() => expect(mockCancel).toHaveBeenCalledWith('n-studio', 'j-run'));
    await waitFor(() =>
      expect(screen.getByTestId('job-dialog-state').textContent).toBe('cancelled')
    );
    expect(screen.getByTestId('job-dialog-exit').textContent).toBe('143');
    expect(screen.queryByTestId('job-dialog-cancel')).toBeNull();
  });

  it('names the Mac a job came from by its display name, the node id only when no Mac answers', async () => {
    const user = userEvent.setup();
    macsMock.macs = [{ nodeId: 'n-gabee', name: 'Gabee’s MacBook' }];
    mockGet.mockResolvedValue({ ...busyLoad().jobs[1], log_bytes: 0 });
    mockLog.mockResolvedValue({ from: 0, next: 0, data: '', done: true });
    renderBars();
    const list = screen.getByTestId('mac-load-job-list');
    expect(list.textContent).toContain('prompt from Gabee’s MacBook');
    expect(list.textContent).not.toContain('from n-gabee');
    // n-studio is on no Mac in the list: the id is all there is to show.
    expect(list.textContent).toContain('shell from n-studio');
    await user.click(screen.getByTestId('mac-job-j-wait'));
    await screen.findByTestId('job-dialog');
    await waitFor(() =>
      expect(screen.getByTestId('job-dialog-origin').textContent).toBe('Gabee’s MacBook')
    );
  });

  it('shows cancelling as Cancelling… and keeps reading the job until it ends', async () => {
    const user = userEvent.setup();
    const running = { ...busyLoad().jobs[0], log_bytes: 0 };
    // The log is already complete, but the group has not exited yet: the view must still be read.
    mockLog.mockResolvedValue({ from: 0, next: 0, data: '', done: true });
    mockGet.mockResolvedValueOnce(running);
    mockCancel.mockResolvedValue({ ...running, state: 'cancelling' });
    renderBars(busyLoad({ jobs: [brief('j-run', 'running'), brief('j-stop', 'cancelling')] }));
    expect(screen.getByTestId('mac-job-j-stop-state').textContent).toBe('Cancelling…');
    // A cancelling job still holds the Mac.
    expect(screen.getByTestId('mac-load-jobs-count').textContent).toBe('2');
    await user.click(screen.getByTestId('mac-job-j-run'));
    await screen.findByTestId('job-dialog');
    await waitFor(() => expect(screen.getByTestId('job-dialog-state').textContent).toBe('running'));
    await user.click(screen.getByTestId('job-dialog-cancel'));
    await screen.findByText('Cancel job j-run?');
    const buttons = screen.getAllByRole('button', { name: 'Cancel job' });
    mockGet.mockResolvedValue({ ...running, state: 'cancelled', exit_code: 143 });
    await user.click(buttons[buttons.length - 1]);
    await waitFor(() =>
      expect(screen.getByTestId('job-dialog-state').textContent).toBe('Cancelling…')
    );
    expect(screen.queryByTestId('job-dialog-cancel')).toBeNull();
    await waitFor(() =>
      expect(screen.getByTestId('job-dialog-state').textContent).toBe('cancelled')
    );
    expect(screen.getByTestId('job-dialog-exit').textContent).toBe('143');
  }, 10_000);

  it('shows the refusal reason when a job call fails', async () => {
    const user = userEvent.setup();
    mockGet.mockRejectedValue({
      message: 'invalid params',
      data: { code: 'not_found', reason: 'no such job' },
    });
    mockLog.mockResolvedValue({ from: 0, next: 0, data: '', done: true });
    renderBars();
    await user.click(screen.getByTestId('mac-job-j-done'));
    expect(await screen.findByTestId('job-dialog-error')).toHaveTextContent('no such job');
  });
});

describe('queued jobs (waiting in the origin queue)', () => {
  const queued = brief('q-7', 'queued', {
    queue: {
      position: 2,
      strategy: 'local-first',
      refusals: [
        { node_id: 'n-gabee', code: 'memory_headroom', reason: 'needs 24 GB, 9 GB free' },
        { node_id: 'n-studio', code: 'heavy_slot_held', reason: 'a heavy job is running' },
      ],
    },
  });

  it('lists a waiting job with its place in line and each Mac’s reason, not as running', () => {
    macsMock.macs = [{ nodeId: 'n-gabee', name: 'Gabee’s MacBook' }];
    renderBars(busyLoad({ jobs: [brief('j-run', 'running'), queued] }));
    expect(screen.getByTestId('mac-job-q-7-state').textContent).toBe('Waiting, #2 in line');
    const reasons = screen.getByTestId('mac-job-q-7-refusals').textContent;
    expect(reasons).toContain('Gabee’s MacBook: needs 24 GB, 9 GB free');
    expect(reasons).toContain('n-studio: a heavy job is running');
    expect(screen.getByTestId('mac-load-jobs-count').textContent).toBe('1');
    expect(screen.getByTestId('mac-load-jobs').textContent).toContain(
      '1 job running · 1 job waiting for a Mac with room'
    );
  });

  it('cancels a waiting job from its row after a confirm', async () => {
    const user = userEvent.setup();
    mockCancel.mockResolvedValue({ ...queued, state: 'cancelled', log_bytes: 0 });
    renderBars(busyLoad({ jobs: [queued] }));
    await user.click(screen.getByTestId('mac-job-q-7-cancel'));
    await screen.findByText('Cancel job q-7?');
    expect(screen.getByText(/takes the job out of the queue/)).toBeInTheDocument();
    const buttons = screen.getAllByRole('button', { name: 'Cancel job' });
    await user.click(buttons[buttons.length - 1]);
    await waitFor(() => expect(mockCancel).toHaveBeenCalledWith('n-studio', 'q-7'));
  });

  it('the job dialog shows the place in line and reasons, then where it was placed', async () => {
    const user = userEvent.setup();
    macsMock.macs = [{ nodeId: 'n-gabee', name: 'Gabee’s MacBook' }];
    mockLog.mockResolvedValue({ from: 0, next: 0, data: '', done: false });
    mockGet.mockResolvedValueOnce({ ...queued, log_bytes: 0 });
    renderBars(busyLoad({ jobs: [queued] }));
    await user.click(screen.getByTestId('mac-job-q-7'));
    await screen.findByTestId('job-dialog');
    await waitFor(() =>
      expect(screen.getByTestId('job-dialog-state').textContent).toBe('Waiting, #2 in line')
    );
    expect(screen.getByTestId('job-dialog-refusals').textContent).toContain(
      'Gabee’s MacBook: needs 24 GB, 9 GB free'
    );
    // Cancellable while it waits.
    expect(screen.getByTestId('job-dialog-cancel')).toBeInTheDocument();
    // Placed: the view is the placed job's, with placed_on, and the queue section goes.
    mockGet.mockResolvedValue({
      ...brief('j-placed', 'running'),
      log_bytes: 0,
      placed_on: { node_id: 'n-gabee', job_id: 'j-placed' },
    });
    // The next 2 s poll reads it (testing-library's wait runs on this test's 10 s clock).
    await waitFor(() =>
      expect(screen.getByTestId('job-dialog-placed').textContent).toBe(
        'Gabee’s MacBook as j-placed'
      )
    );
    expect(screen.getByTestId('job-dialog-state').textContent).toBe('running');
    expect(screen.queryByTestId('job-dialog-queue')).toBeNull();
    // Still read through the queue id.
    expect(mockGet).toHaveBeenLastCalledWith('n-studio', 'q-7');
  }, 10_000);
});

describe('JobsSettings', () => {
  it('shows the placement strategy, My Mac first by default, and saves a change at once', async () => {
    const user = userEvent.setup();
    mockRead.mockResolvedValue(null);
    render(
      <IntlTestWrapper>
        <JobsSettings />
      </IntlTestWrapper>
    );
    const local = await screen.findByTestId('jobs-strategy-local-first');
    expect(local).toHaveTextContent('My Mac first');
    expect(local).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByTestId('jobs-strategy-selfish')).toHaveTextContent('Other Macs first');
    expect(screen.getByTestId('jobs-settings-strategy-hint').textContent).toContain(
      'Runs on this Mac when it has room'
    );
    await user.click(screen.getByTestId('jobs-strategy-selfish'));
    await waitFor(() => expect(mockUpsert).toHaveBeenCalledWith(JOBS_KEYS.strategy, 'selfish'));
    expect(screen.getByTestId('jobs-settings-strategy-hint').textContent).toContain(
      'Keeps this Mac free'
    );
  });

  it('reads a saved strategy back', async () => {
    mockRead.mockImplementation(async (key: string) =>
      key === JOBS_KEYS.strategy ? 'selfish' : null
    );
    render(
      <IntlTestWrapper>
        <JobsSettings />
      </IntlTestWrapper>
    );
    expect(await screen.findByTestId('jobs-strategy-selfish')).toHaveAttribute(
      'aria-checked',
      'true'
    );
  });

  it('reads every jobs key with defaults and writes each one back', async () => {
    const user = userEvent.setup();
    mockRead.mockImplementation(async (key: string) =>
      key === JOBS_KEYS.policy ? 'refuse' : key === JOBS_KEYS.idleS ? '600' : null
    );
    render(
      <IntlTestWrapper>
        <JobsSettings />
      </IntlTestWrapper>
    );
    const sw = await screen.findByRole('switch', { name: 'Run jobs other Macs send' });
    expect(mockRead.mock.calls.map((c) => c[0]).sort()).toEqual(Object.values(JOBS_KEYS).sort());
    expect(sw).toHaveAttribute('aria-checked', 'false');
    const idle = screen.getByTestId('jobs-settings-idle') as HTMLInputElement;
    const minFree = screen.getByTestId('jobs-settings-minfree') as HTMLInputElement;
    expect(idle.value).toBe('600');
    expect(minFree.value).toBe('20');

    await user.click(sw);
    await waitFor(() => expect(mockUpsert).toHaveBeenCalledWith(JOBS_KEYS.allow, true));
    await user.click(screen.getByTestId('jobs-policy-last_resort'));
    await waitFor(() => expect(mockUpsert).toHaveBeenCalledWith(JOBS_KEYS.policy, 'last_resort'));
    await user.clear(idle);
    await user.type(idle, '120');
    await user.tab();
    await waitFor(() => expect(mockUpsert).toHaveBeenCalledWith(JOBS_KEYS.idleS, 120));
    await user.clear(minFree);
    await user.type(minFree, '25');
    await user.tab();
    await waitFor(() => expect(mockUpsert).toHaveBeenCalledWith(JOBS_KEYS.minFree, 25));
    expect(document.querySelector('select')).toBeNull();
  });

  it('refuses an out-of-range number without writing it', async () => {
    const user = userEvent.setup();
    mockRead.mockResolvedValue(null);
    render(
      <IntlTestWrapper>
        <JobsSettings />
      </IntlTestWrapper>
    );
    const minFree = (await screen.findByTestId('jobs-settings-minfree')) as HTMLInputElement;
    await user.clear(minFree);
    await user.type(minFree, '95');
    await user.tab();
    expect(mockUpsert).not.toHaveBeenCalled();
  });
});
