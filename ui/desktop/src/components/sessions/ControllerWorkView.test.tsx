import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { IntlTestWrapper } from '../../i18n/test-utils';
import * as api from '../../acp/controller';
import type { CommandReceipt, WorkCommand } from '../../acp/controllerTypes';
import { fixtureEntry, fixtureHost, fixtureMessage, fixtureSnapshot } from './controllerFixtures';
import ControllerWorkView from './ControllerWorkView';

vi.mock('../../acp/controller', async (original) => ({
  ...(await original<typeof import('../../acp/controller')>()),
  controllerOpen: vi.fn(),
  controllerHistory: vi.fn(),
  controllerChanges: vi.fn(),
  controllerCommand: vi.fn(),
  controllerPendingCommands: vi.fn(),
  controllerReceipt: vi.fn(),
}));
vi.mock('../../acp/sessions', () => ({
  acpLoadSession: vi.fn(() => {
    throw new Error('Remote view must not activate a local session');
  }),
}));

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.controllerOpen).mockResolvedValue(fixtureSnapshot());
  vi.mocked(api.controllerHistory).mockResolvedValue({
    work: fixtureEntry.work,
    store_generation: 'generation-a',
    revision: 4,
    items: [fixtureMessage()],
    next_page_token: null,
    work_cursor: 'history-4',
  });
  vi.mocked(api.controllerChanges).mockResolvedValue({
    frames: [],
    resume_cursor: 'cursor-4',
    has_more: false,
  });
  vi.mocked(api.controllerPendingCommands).mockResolvedValue({ commands: [] });
});
describe('linked work user path', () => {
  it('opens actual owner history and targets queued send, steer and stop to the original run', async () => {
    vi.mocked(api.controllerCommand).mockImplementation(async (command) => ({
      command_id: command.command_id,
      work: command.work,
      authenticated_device_id: 'controller-a',
      revision: 5,
      state: 'delivered',
      run_id: 'run-a',
      message_id: null,
      input_id: null,
      checkpoint_id: null,
      reason: null,
      updated_at: '2026-10-26T10:00:00Z',
    }));
    render(
      <ControllerWorkView entry={fixtureEntry} host={fixtureHost} stale={false} onBack={vi.fn()} />,
      { wrapper: IntlTestWrapper }
    );
    expect(await screen.findByText('Saved answer')).toBeInTheDocument();
    expect(screen.getByText('Runs on Studio')).toBeInTheDocument();
    expect(api.controllerOpen).toHaveBeenCalledWith(fixtureEntry.work, 'generation-a');
    const input = screen.getByRole('textbox', { name: 'Message for this session on Studio' });
    fireEvent.change(input, { target: { value: 'Continue after this run' } });
    fireEvent.click(screen.getByRole('button', { name: 'Queue next message' }));
    await waitFor(() => expect(api.controllerCommand).toHaveBeenCalledTimes(1));
    expect(vi.mocked(api.controllerCommand).mock.calls[0][0]).toMatchObject({
      work: fixtureEntry.work,
      expected_store_generation: 'generation-a',
      expected_revision: 4,
      expected_run_id: 'run-a',
      action: 'send',
      payload: { content: [{ type: 'text', text: 'Continue after this run' }] },
    });
    await waitFor(() => expect(input).toHaveValue(''));
    fireEvent.change(input, { target: { value: 'Use the existing API' } });
    fireEvent.click(screen.getByRole('button', { name: 'Steer current run' }));
    await waitFor(() => expect(api.controllerCommand).toHaveBeenCalledTimes(2));
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Stop current run' })).toBeEnabled()
    );
    fireEvent.click(screen.getByRole('button', { name: 'Stop current run' }));
    await waitFor(() => expect(api.controllerCommand).toHaveBeenCalledTimes(3));
    expect(
      vi
        .mocked(api.controllerCommand)
        .mock.calls.map(([command]) => [command.action, command.expected_run_id])
    ).toEqual([
      ['send', 'run-a'],
      ['steer', 'run-a'],
      ['stop', 'run-a'],
    ]);
  });
  it('a lost ACK preserves the exact command UUID and payload for retry', async () => {
    vi.mocked(api.controllerCommand).mockRejectedValue(
      new Error('Connection ended before acknowledgement')
    );
    render(
      <ControllerWorkView entry={fixtureEntry} host={fixtureHost} stale={false} onBack={vi.fn()} />,
      { wrapper: IntlTestWrapper }
    );
    await screen.findByText('Saved answer');
    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'One queued message' } });
    fireEvent.click(screen.getByRole('button', { name: 'Queue next message' }));
    const retry = await screen.findByRole('button', { name: 'Retry same command' });
    await waitFor(() => expect(retry).toBeEnabled());
    fireEvent.click(retry);
    await waitFor(() => expect(api.controllerCommand).toHaveBeenCalledTimes(2));
    const [first, second] = vi.mocked(api.controllerCommand).mock.calls;
    expect(first[0].command_id).toBeTruthy();
    expect(second[0]).toEqual(first[0]);
  });
  it('offline owner metadata disables all write controls without claiming cached state is live', async () => {
    render(
      <ControllerWorkView
        entry={fixtureEntry}
        host={{ ...fixtureHost, availability: 'offline' }}
        stale={true}
        onBack={vi.fn()}
      />,
      { wrapper: IntlTestWrapper }
    );
    await screen.findByText('Saved answer');
    expect(screen.getByText(/Saved content may be out of date/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Stop current run' })).toBeDisabled();
    expect(screen.getByRole('textbox')).toBeDisabled();
    expect(api.controllerCommand).not.toHaveBeenCalled();
  });
  it('refreshes a definitively unaccepted steer and keeps the draft for a fresh user decision', async () => {
    vi.mocked(api.controllerCommand).mockRejectedValueOnce({
      data: { error: { status: 409, code: 'stale_revision', message: 'Owner refused' } },
    });
    render(
      <ControllerWorkView entry={fixtureEntry} host={fixtureHost} stale={false} onBack={vi.fn()} />,
      { wrapper: IntlTestWrapper }
    );
    await screen.findByText('Saved answer');
    const input = screen.getByRole('textbox');
    fireEvent.change(input, { target: { value: 'Keep this exact instruction' } });
    const refreshed = fixtureSnapshot();
    refreshed.entry = { ...refreshed.entry, revision: 8, current_run_id: 'run-b' };
    vi.mocked(api.controllerOpen).mockResolvedValue(refreshed);
    fireEvent.click(screen.getByRole('button', { name: 'Steer current run' }));
    await screen.findByText(/Not sent: the session changed/);
    await waitFor(() => expect(api.controllerOpen).toHaveBeenCalledTimes(2));
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Steer current run' })).toBeEnabled()
    );
    expect(input).toHaveValue('Keep this exact instruction');
    expect(screen.queryByRole('button', { name: 'Retry same command' })).not.toBeInTheDocument();
    expect(api.controllerCommand).toHaveBeenCalledTimes(1);
    vi.mocked(api.controllerCommand).mockImplementation(async (command) => ({
      command_id: command.command_id,
      work: command.work,
      authenticated_device_id: 'controller-a',
      revision: 9,
      state: 'delivered',
      run_id: 'run-b',
      message_id: null,
      input_id: null,
      checkpoint_id: null,
      reason: null,
      updated_at: '2026-10-26T10:00:00Z',
    }));
    fireEvent.click(screen.getByRole('button', { name: 'Steer current run' }));
    await waitFor(() => expect(api.controllerCommand).toHaveBeenCalledTimes(2));
    const [first, second] = vi.mocked(api.controllerCommand).mock.calls.map(([command]) => command);
    expect(second.command_id).not.toBe(first.command_id);
    expect(second.payload).toEqual(first.payload);
    expect(second).toMatchObject({ expected_revision: 8, expected_run_id: 'run-b' });
    await waitFor(() => expect(input).toHaveValue(''));
  });
  it.each([
    { status: 409, code: 'stale_run' },
    { status: 503, code: 'stale_revision' },
    { status: 404, code: 'command_not_found' },
  ])('retains immutable recovery for other owner errors: $status $code', async (error) => {
    vi.mocked(api.controllerCommand).mockRejectedValue({ data: { error } });
    render(
      <ControllerWorkView entry={fixtureEntry} host={fixtureHost} stale={false} onBack={vi.fn()} />,
      { wrapper: IntlTestWrapper }
    );
    await screen.findByText('Saved answer');
    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'Keep command' } });
    fireEvent.click(screen.getByRole('button', { name: 'Steer current run' }));
    const retry = await screen.findByRole('button', { name: 'Retry same command' });
    await waitFor(() => expect(retry).toBeEnabled());
    fireEvent.click(retry);
    await waitFor(() => expect(api.controllerCommand).toHaveBeenCalledTimes(2));
    expect(vi.mocked(api.controllerCommand).mock.calls[1][0]).toEqual(
      vi.mocked(api.controllerCommand).mock.calls[0][0]
    );
    expect(api.controllerOpen).toHaveBeenCalledTimes(1);
  });
  it('reconciles a command saved before disconnect without submitting it again', async () => {
    const command: WorkCommand = {
      command_id: 'saved-command',
      work: fixtureEntry.work,
      expected_store_generation: 'generation-a',
      expected_revision: 4,
      expected_run_id: 'run-a',
      action: 'send',
      payload: { content: [{ type: 'text', text: 'Already submitted' }] },
    };
    const receipt: CommandReceipt = {
      command_id: command.command_id,
      work: command.work,
      authenticated_device_id: 'controller-a',
      revision: 5,
      state: 'delivered',
      run_id: 'run-a',
      message_id: null,
      input_id: null,
      checkpoint_id: null,
      reason: null,
      updated_at: '2026-10-26T10:00:00Z',
    };
    vi.mocked(api.controllerPendingCommands).mockResolvedValue({
      commands: [{ command, receipt: null }],
    });
    vi.mocked(api.controllerReceipt).mockResolvedValue(receipt);
    render(
      <ControllerWorkView entry={fixtureEntry} host={fixtureHost} stale={false} onBack={vi.fn()} />,
      { wrapper: IntlTestWrapper }
    );
    expect(await screen.findByText('send · Delivered')).toBeInTheDocument();
    expect(api.controllerReceipt).toHaveBeenCalledWith('owner-a', 'saved-command');
    expect(api.controllerCommand).not.toHaveBeenCalled();
    expect(screen.queryByRole('button', { name: 'Retry same command' })).not.toBeInTheDocument();
    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'A new message' } });
    expect(screen.getByRole('button', { name: 'Queue next message' })).toBeEnabled();
  });
  it.each(['', 'Different unsent draft'])(
    'recovers the text of a reopened unaccepted command without replacing draft %j',
    async (draft) => {
      const command: WorkCommand = {
        command_id: 'restored-stale-command',
        work: fixtureEntry.work,
        expected_store_generation: 'generation-a',
        expected_revision: 2,
        expected_run_id: 'run-a',
        action: 'steer',
        payload: { content: [{ type: 'text', text: 'Recovered exact instruction' }] },
      };
      vi.mocked(api.controllerPendingCommands).mockResolvedValue({
        commands: [{ command, receipt: null }],
      });
      vi.mocked(api.controllerReceipt).mockRejectedValue(new Error('Receipt not found'));
      vi.mocked(api.controllerCommand).mockImplementation(async () => {
        vi.mocked(api.controllerPendingCommands).mockResolvedValue({ commands: [] });
        throw { data: { error: { status: 409, code: 'stale_revision' } } };
      });
      const view = render(
        <ControllerWorkView
          entry={fixtureEntry}
          host={fixtureHost}
          stale={false}
          onBack={vi.fn()}
        />,
        { wrapper: IntlTestWrapper }
      );
      const retry = await screen.findByRole('button', { name: 'Retry same command' });
      expect(screen.getByRole('textbox')).toHaveValue('');
      if (draft) fireEvent.change(screen.getByRole('textbox'), { target: { value: draft } });
      fireEvent.click(retry);
      await screen.findByText(/Not sent: the session changed/);
      await waitFor(() => expect(api.controllerOpen).toHaveBeenCalledTimes(2));
      expect(api.controllerCommand).toHaveBeenCalledTimes(1);
      expect(vi.mocked(api.controllerCommand).mock.calls[0][0]).toEqual(command);
      expect(screen.getByRole('textbox')).toHaveValue(draft || 'Recovered exact instruction');
      expect(
        screen.getByText('Recovered exact instruction', { selector: 'pre' })
      ).toBeInTheDocument();
      fireEvent.change(screen.getByRole('textbox'), { target: { value: '' } });
      expect(screen.getByRole('textbox')).toHaveValue('');
      const changed = { ...fixtureEntry, store_generation: 'generation-b' };
      view.rerender(
        <ControllerWorkView entry={changed} host={fixtureHost} stale={false} onBack={vi.fn()} />
      );
      expect(
        screen.queryByText('Recovered exact instruction', { selector: 'pre' })
      ).not.toBeInTheDocument();
      expect(screen.queryByText(/Not sent: the session changed/)).not.toBeInTheDocument();
      expect(screen.getByRole('textbox')).toHaveValue('');
    }
  );
  it('keeps non-composer content inspectable without flattening it into a new draft', async () => {
    const command: WorkCommand = {
      command_id: 'restored-multi-part',
      work: fixtureEntry.work,
      expected_store_generation: 'generation-a',
      expected_revision: 2,
      expected_run_id: 'run-a',
      action: 'send',
      payload: {
        content: [
          { type: 'text', text: 'First part' },
          { type: 'text', text: 'Second part' },
        ],
      },
    };
    vi.mocked(api.controllerPendingCommands).mockResolvedValue({
      commands: [{ command, receipt: null }],
    });
    vi.mocked(api.controllerReceipt).mockRejectedValue(new Error('Receipt not found'));
    vi.mocked(api.controllerCommand).mockImplementation(async () => {
      vi.mocked(api.controllerPendingCommands).mockResolvedValue({ commands: [] });
      throw { data: { error: { status: 409, code: 'stale_revision' } } };
    });
    render(
      <ControllerWorkView entry={fixtureEntry} host={fixtureHost} stale={false} onBack={vi.fn()} />,
      { wrapper: IntlTestWrapper }
    );
    fireEvent.click(await screen.findByRole('button', { name: 'Retry same command' }));
    await screen.findByText(/Not sent: the session changed/);
    expect(screen.getByRole('textbox')).toHaveValue('');
    expect(screen.getByText(/First part/, { selector: 'pre' })).toHaveTextContent('Second part');
    expect(screen.queryByText(/Your draft has been kept/)).not.toBeInTheDocument();
    expect(api.controllerCommand).toHaveBeenCalledTimes(1);
  });
  it('does not restore an old asynchronous rejection into another selected work', async () => {
    let rejectCommand!: (reason: unknown) => void;
    vi.mocked(api.controllerCommand).mockImplementation(
      () =>
        new Promise((_, reject) => {
          rejectCommand = reject;
        })
    );
    const view = render(
      <ControllerWorkView entry={fixtureEntry} host={fixtureHost} stale={false} onBack={vi.fn()} />,
      { wrapper: IntlTestWrapper }
    );
    await screen.findByText('Saved answer');
    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'Old session instruction' } });
    fireEvent.click(screen.getByRole('button', { name: 'Steer current run' }));
    await waitFor(() => expect(api.controllerCommand).toHaveBeenCalledTimes(1));
    const changed = { ...fixtureEntry, work: { ...fixtureEntry.work, work_id: 'other-work' } };
    vi.mocked(api.controllerOpen).mockResolvedValue({ ...fixtureSnapshot(), entry: changed });
    vi.mocked(api.controllerHistory).mockResolvedValue({
      work: changed.work,
      store_generation: 'generation-a',
      revision: 4,
      items: [],
      next_page_token: null,
      work_cursor: 'other-history',
    });
    view.rerender(
      <ControllerWorkView entry={changed} host={fixtureHost} stale={false} onBack={vi.fn()} />
    );
    await waitFor(() => expect(api.controllerOpen).toHaveBeenCalledTimes(2));
    rejectCommand({ data: { error: { status: 409, code: 'stale_revision' } } });
    await waitFor(() => expect(screen.getByRole('textbox')).toBeEnabled());
    expect(screen.getByRole('textbox')).toHaveValue('');
    expect(screen.queryByText('Old session instruction')).not.toBeInTheDocument();
    expect(screen.queryByText(/Not sent: the session changed/)).not.toBeInTheDocument();
    expect(api.controllerOpen).toHaveBeenCalledTimes(2);
  });
  it('keeps an unknown outcome visible and prevents another send or automatic retry', async () => {
    const command: WorkCommand = {
      command_id: 'uncertain-command',
      work: fixtureEntry.work,
      expected_store_generation: 'generation-a',
      expected_revision: 4,
      expected_run_id: 'run-a',
      action: 'send',
      payload: { content: [{ type: 'text', text: 'May have been delivered' }] },
    };
    vi.mocked(api.controllerPendingCommands).mockResolvedValue({
      commands: [
        {
          command,
          receipt: {
            command_id: command.command_id,
            work: command.work,
            authenticated_device_id: 'controller-a',
            revision: 5,
            state: 'outcome_unknown',
            run_id: 'run-a',
            message_id: null,
            input_id: null,
            checkpoint_id: null,
            reason: 'The owner restarted during dispatch.',
            updated_at: '2026-10-26T10:00:00Z',
          },
        },
      ],
    });
    render(
      <ControllerWorkView entry={fixtureEntry} host={fixtureHost} stale={false} onBack={vi.fn()} />,
      { wrapper: IntlTestWrapper }
    );
    expect(await screen.findByText(/The owner cannot confirm the outcome/)).toBeInTheDocument();
    expect(screen.getByText('The owner restarted during dispatch.')).toBeInTheDocument();
    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'Do it again' } });
    expect(screen.getByRole('button', { name: 'Queue next message' })).toBeDisabled();
    expect(screen.queryByRole('button', { name: 'Retry same command' })).not.toBeInTheDocument();
    expect(api.controllerCommand).not.toHaveBeenCalled();
    expect(api.controllerReceipt).not.toHaveBeenCalled();
  });
});
