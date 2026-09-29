import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ScheduledJobDto } from '@aaif/goose-sdk';
import { IntlTestWrapper } from '../../../i18n/test-utils';

const schedules = vi.hoisted(() => ({
  acpListSchedules: vi.fn(),
  acpDeleteSchedule: vi.fn(),
}));

vi.mock('../../../acp/schedules', () => ({
  acpListSchedules: schedules.acpListSchedules,
  acpDeleteSchedule: schedules.acpDeleteSchedule,
  acpUpdateSchedule: vi.fn(),
  acpCreateSchedule: vi.fn(),
  acpPauseSchedule: vi.fn(),
  acpUnpauseSchedule: vi.fn(),
  acpKillRunningJob: vi.fn(),
  acpInspectRunningJob: vi.fn(),
}));

import SchedulesView from '../SchedulesView';

const job: ScheduledJobDto = {
  id: 'nightly-report',
  source: '/data/scheduled_recipes/nightly-report.yaml',
  cron: '0 0 2 * * *',
  currentlyRunning: false,
  paused: false,
  workingDir: '/Users/me/project',
};

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}

describe('removing a schedule asks in the app, never through window.confirm', () => {
  let nativeConfirm: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    vi.clearAllMocks();
    vi.stubGlobal('ResizeObserver', ResizeObserverMock);
    nativeConfirm = vi.spyOn(window, 'confirm').mockReturnValue(true);
    schedules.acpListSchedules.mockResolvedValue([job]);
    schedules.acpDeleteSchedule.mockResolvedValue(undefined);
  });

  afterEach(() => {
    nativeConfirm.mockRestore();
  });

  it('cancel keeps the schedule; Remove deletes it', async () => {
    const user = userEvent.setup();
    render(
      <MemoryRouter>
        <IntlTestWrapper>
          <SchedulesView />
        </IntlTestWrapper>
      </MemoryRouter>
    );

    await user.click(await screen.findByRole('button', { name: 'Remove' }));
    let dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText('Remove schedule?')).toBeInTheDocument();
    expect(
      within(dialog).getByText('Remove schedule "nightly-report"? The recipe will be kept.')
    ).toBeInTheDocument();

    await user.click(within(dialog).getByRole('button', { name: 'Cancel' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).not.toBeInTheDocument());
    expect(schedules.acpDeleteSchedule).not.toHaveBeenCalled();

    await user.click(screen.getByRole('button', { name: 'Remove' }));
    dialog = await screen.findByRole('dialog');
    await user.click(within(dialog).getByRole('button', { name: 'Remove' }));
    await waitFor(() => expect(schedules.acpDeleteSchedule).toHaveBeenCalledWith('nightly-report'));
    expect(nativeConfirm).not.toHaveBeenCalled();
  });
});
