import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ScheduledJobDto } from '@aaif/goose-sdk';
import { IntlTestWrapper } from '../../../i18n/test-utils';
import { ScheduleModal } from '../ScheduleModal';

const schedules = vi.hoisted(() => ({
  acpListSchedules: vi.fn(),
  acpUpdateSchedule: vi.fn(),
}));

vi.mock('../../../acp/schedules', () => ({
  acpListSchedules: schedules.acpListSchedules,
  acpUpdateSchedule: schedules.acpUpdateSchedule,
  acpCreateSchedule: vi.fn(),
  acpDeleteSchedule: vi.fn(),
  acpPauseSchedule: vi.fn(),
  acpUnpauseSchedule: vi.fn(),
  acpKillRunningJob: vi.fn(),
  acpInspectRunningJob: vi.fn(),
}));

import SchedulesView from '../SchedulesView';

// Q-282: a schedule runs in the folder its owner chose. One saved before schedules recorded a folder
// has none: goose refuses to run it, so the Scheduler must say so and ask for one — never run it in
// goose's own process folder ($HOME since Q-257).
const legacyJob: ScheduledJobDto = {
  id: 'nightly-report',
  source: '/data/scheduled_recipes/nightly-report.yaml',
  cron: '0 0 2 * * *',
  currentlyRunning: false,
  paused: false,
};

const directoryChooser = vi.fn();

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.stubGlobal('ResizeObserver', ResizeObserverMock);
  Object.assign(window.electron, { directoryChooser });
  (window as unknown as { appConfig: { get: (k: string) => unknown } }).appConfig = {
    get: (key: string) => (key === 'GOOSE_WORKING_DIR' ? '/Users/me/window-project' : undefined),
  };
});

describe('the schedule folder (Q-282)', () => {
  it('a new schedule runs in this window’s folder unless another is chosen', () => {
    render(
      <ScheduleModal
        isOpen
        onClose={vi.fn()}
        onSubmit={vi.fn()}
        schedule={null}
        isLoadingExternally={false}
        apiErrorExternally={null}
        initialDeepLink={null}
      />,
      { wrapper: IntlTestWrapper }
    );

    expect(screen.getByTestId('schedule-folder')).toHaveTextContent('/Users/me/window-project');
  });

  it('editing a schedule with no folder refuses to save until a folder is chosen, then sends it', async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    render(
      <ScheduleModal
        isOpen
        onClose={vi.fn()}
        onSubmit={onSubmit}
        schedule={legacyJob}
        isLoadingExternally={false}
        apiErrorExternally={null}
        initialDeepLink={null}
      />,
      { wrapper: IntlTestWrapper }
    );

    expect(screen.getByTestId('schedule-no-folder')).toHaveTextContent(
      'This schedule does not run until you choose one'
    );
    await user.click(screen.getByRole('button', { name: 'Update Schedule' }));
    expect(onSubmit).not.toHaveBeenCalled();
    expect(screen.getByText('Choose the folder this schedule runs in.')).toBeInTheDocument();

    directoryChooser.mockResolvedValue({ canceled: false, filePaths: ['/Users/me/reports'] });
    await user.click(screen.getByRole('button', { name: 'Choose folder...' }));
    await waitFor(() =>
      expect(screen.getByTestId('schedule-folder')).toHaveTextContent('/Users/me/reports')
    );
    await user.click(screen.getByRole('button', { name: 'Update Schedule' }));
    expect(onSubmit).toHaveBeenCalledWith({ cron: '0 0 2 * * *', workingDir: '/Users/me/reports' });
  });

  it('the schedule list names a schedule with no folder and gives it the folder chosen', async () => {
    const user = userEvent.setup();
    schedules.acpListSchedules.mockResolvedValue([
      legacyJob,
      { ...legacyJob, id: 'weekly-digest', workingDir: '/Users/me/digest' },
    ]);
    schedules.acpUpdateSchedule.mockResolvedValue({ ...legacyJob, workingDir: '/Users/me/reports' });
    directoryChooser.mockResolvedValue({ canceled: false, filePaths: ['/Users/me/reports'] });

    render(
      <MemoryRouter>
        <SchedulesView />
      </MemoryRouter>,
      { wrapper: IntlTestWrapper }
    );

    await waitFor(() => expect(screen.getByTestId('schedule-no-folder-badge')).toBeInTheDocument());
    expect(screen.getAllByTestId('schedule-no-folder-badge')).toHaveLength(1);
    expect(screen.getByText('Runs in /Users/me/digest')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Choose folder' }));
    await waitFor(() =>
      expect(schedules.acpUpdateSchedule).toHaveBeenCalledWith(
        'nightly-report',
        '0 0 2 * * *',
        '/Users/me/reports'
      )
    );
  });
});
