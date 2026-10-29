import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { IntlTestWrapper } from '../../i18n/test-utils';
import * as api from '../../acp/controller';
import type { HistoryPage } from '../../acp/controllerTypes';
import {
  fixtureEntry,
  fixtureFrame,
  fixtureHost,
  fixtureMessage,
  fixtureSnapshot,
} from './controllerFixtures';
import ControllerWorkView from './ControllerWorkView';

vi.mock('../../acp/controller', async (original) => ({
  ...(await original<typeof import('../../acp/controller')>()),
  controllerOpen: vi.fn(),
  controllerHistoryLatest: vi.fn(),
  controllerHistory: vi.fn(),
  controllerChanges: vi.fn(),
  controllerCommand: vi.fn(),
  controllerPendingCommands: vi.fn(),
}));

let releaseChanges: (batch: api.WorkChanges) => void;
let resize: () => void;
const page = (ids: string[], token: string | null = null): HistoryPage => ({
  work: fixtureEntry.work,
  store_generation: 'generation-a',
  revision: 4,
  items: ids.map((id) => fixtureMessage(id, 3, id)),
  next_page_token: token,
  work_cursor: 'cursor-4',
});
const rowHeight = (row: Element) => 100 + (row.textContent?.length ?? 0);
const viewport = () => screen.getByRole('region', { name: 'Session transcript' });
const readEarlier = () => {
  const element = viewport();
  element.scrollTop = 25;
  fireEvent.scroll(element);
  return element;
};
const renderWork = () =>
  render(
    <ControllerWorkView entry={fixtureEntry} host={fixtureHost} stale={false} onBack={vi.fn()} />,
    { wrapper: IntlTestWrapper }
  );

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(api.controllerOpen).mockResolvedValue({
    ...fixtureSnapshot(),
    live_messages: [fixtureMessage('live', 4, 'Started')],
  });
  vi.mocked(api.controllerHistoryLatest).mockResolvedValue(page(['recent-a', 'recent-b'], 'older'));
  vi.mocked(api.controllerPendingCommands).mockResolvedValue({ commands: [] });
  vi.mocked(api.controllerChanges).mockImplementation(
    () =>
      new Promise((resolve) => {
        releaseChanges = resolve;
      })
  );
  const tops = new WeakMap<HTMLElement, number>();
  vi.spyOn(HTMLElement.prototype, 'scrollHeight', 'get').mockImplementation(function (
    this: HTMLElement
  ) {
    return [...this.querySelectorAll('article')].reduce(
      (height, row) => height + rowHeight(row),
      0
    );
  });
  vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(160);
  vi.spyOn(HTMLElement.prototype, 'scrollTop', 'get').mockImplementation(function (
    this: HTMLElement
  ) {
    return tops.get(this) ?? 0;
  });
  vi.spyOn(HTMLElement.prototype, 'scrollTop', 'set').mockImplementation(function (
    this: HTMLElement,
    value
  ) {
    tops.set(this, Math.max(0, Math.min(value, this.scrollHeight - this.clientHeight)));
  });
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
    this: HTMLElement
  ) {
    const region = this.closest('[role="region"]');
    const rows = [...(region?.querySelectorAll('article') ?? [])];
    const index = rows.indexOf(this);
    const height = index >= 0 ? rowHeight(this) : 160;
    const top =
      index >= 0
        ? rows.slice(0, index).reduce((total, row) => total + rowHeight(row), 0) - region!.scrollTop
        : 0;
    return {
      top,
      bottom: top + height,
      height,
      left: 0,
      right: 500,
      width: 500,
      x: 0,
      y: top,
      toJSON: () => ({}),
    };
  });
  vi.stubGlobal(
    'ResizeObserver',
    class {
      constructor(callback: () => void) {
        resize = callback;
      }
      observe() {}
      disconnect() {}
    }
  );
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

const growLive = async () => {
  await waitFor(() => expect(api.controllerChanges).toHaveBeenCalledTimes(1));
  await act(async () =>
    releaseChanges({
      frames: [
        fixtureFrame('partial_replacement', 5, {
          message_id: 'live',
          message_revision: 5,
          offset_bytes: 7,
          content: fixtureMessage('live', 5, ' streaming the same message').message,
        }),
      ],
      resume_cursor: 'cursor-5',
      has_more: false,
    })
  );
};

describe('latest controller transcript position', () => {
  it('opens at the bottom and follows same-ID text growth while still there', async () => {
    renderWork();
    await screen.findByText('Started');
    const element = viewport();
    const before = element.scrollTop;
    expect(before).toBe(element.scrollHeight - element.clientHeight);
    expect(api.controllerHistoryLatest).toHaveBeenCalledExactlyOnceWith(
      fixtureEntry.work,
      'generation-a',
      'cursor-4'
    );
    expect(api.controllerHistory).not.toHaveBeenCalled();
    await growLive();
    expect(element.scrollTop).toBeGreaterThan(before);
    expect(element.scrollTop).toBe(element.scrollHeight - element.clientHeight);
    expect(screen.queryByRole('button', { name: 'Jump to latest' })).not.toBeInTheDocument();
  });
  it('keeps the reading anchor during streaming and resumes following only after explicit jump', async () => {
    renderWork();
    await screen.findByText('Started');
    const element = readEarlier();
    await growLive();
    expect(element.scrollTop).toBe(25);
    expect(element).toHaveAttribute('aria-live', 'off');
    fireEvent.click(screen.getByRole('button', { name: 'Jump to latest' }));
    expect(element.scrollTop).toBe(element.scrollHeight - element.clientHeight);
    const before = element.scrollTop;
    screen.getByText('Started streaming the same message').textContent +=
      ' expanded image or details';
    act(() => resize());
    expect(element.scrollTop).toBeGreaterThan(before);
    expect(element.scrollTop).toBe(element.scrollHeight - element.clientHeight);
  });
  it('preserves the visible row when two older tied pages are prepended', async () => {
    vi.mocked(api.controllerHistory)
      .mockResolvedValueOnce(page(['middle-a', 'middle-b'], 'oldest'))
      .mockResolvedValueOnce(page(['oldest-a', 'oldest-b']));
    renderWork();
    await screen.findByText('Started');
    readEarlier();
    const anchor = screen.getByText('recent-a').closest('article')!;
    const initialTop = anchor.getBoundingClientRect().top;
    fireEvent.click(screen.getByRole('button', { name: 'Load older messages' }));
    await screen.findByText('middle-a');
    expect(anchor.getBoundingClientRect().top).toBe(initialTop);
    fireEvent.click(screen.getByRole('button', { name: 'Load older messages' }));
    await screen.findByText('oldest-a');
    expect(anchor.getBoundingClientRect().top).toBe(initialTop);
    expect(screen.getAllByRole('article').map((row) => row.dataset.controllerMessageId)).toEqual([
      'oldest-a',
      'oldest-b',
      'middle-a',
      'middle-b',
      'recent-a',
      'recent-b',
      'live',
    ]);
    expect(api.controllerHistoryLatest).toHaveBeenCalledTimes(1);
    expect(api.controllerHistory).toHaveBeenCalledTimes(2);
  });
  it.each(['declined', 'stale'])(
    'does not jump when send is %s; same-work refresh keeps older rows',
    async (failure) => {
      vi.mocked(api.controllerHistory).mockResolvedValueOnce(page(['older-read']));
      vi.mocked(api.controllerCommand).mockRejectedValue(
        failure === 'stale'
          ? { data: { error: { status: 409, code: 'stale_revision' } } }
          : new Error('Sharing approval declined')
      );
      renderWork();
      await screen.findByText('Started');
      fireEvent.click(screen.getByRole('button', { name: 'Load older messages' }));
      await screen.findByText('older-read');
      const element = readEarlier();
      fireEvent.change(screen.getByRole('textbox'), { target: { value: 'Keep draft' } });
      fireEvent.click(screen.getByRole('button', { name: 'Queue next message' }));
      if (failure === 'stale') {
        await screen.findByText(/Not sent: the session changed/);
        await waitFor(() => expect(api.controllerOpen).toHaveBeenCalledTimes(2));
        await waitFor(() => expect(screen.getByRole('textbox')).toBeEnabled());
      } else await screen.findByRole('alert');
      expect(element.scrollTop).toBe(25);
      expect(screen.getByText('older-read')).toBeInTheDocument();
      expect(screen.getByRole('textbox')).toHaveValue('Keep draft');
      expect(api.controllerHistoryLatest).toHaveBeenCalledTimes(1);
      expect(api.controllerCommand).toHaveBeenCalledTimes(1);
    }
  );
  it('refreshes an expired history cursor explicitly and drops old pinned rows', async () => {
    vi.mocked(api.controllerHistory).mockRejectedValue({
      data: { error: { status: 410, code: 'snapshot_required' } },
    });
    renderWork();
    await screen.findByText('Started');
    const element = readEarlier();
    vi.mocked(api.controllerHistoryLatest).mockResolvedValue(page(['new-latest']));
    fireEvent.click(screen.getByRole('button', { name: 'Load older messages' }));
    expect(await screen.findByText('new-latest')).toBeInTheDocument();
    expect(screen.getByText(/Saved history is no longer valid/)).toBeInTheDocument();
    expect(screen.queryByText('recent-a')).not.toBeInTheDocument();
    expect(element.scrollTop).toBe(element.scrollHeight - element.clientHeight);
    expect(api.controllerHistoryLatest).toHaveBeenCalledTimes(2);
  });
  it('retains older paging that finishes while the second replay batch is in flight', async () => {
    vi.mocked(api.controllerHistory)
      .mockResolvedValueOnce(page(['middle'], 'oldest'))
      .mockResolvedValueOnce(page(['oldest']));
    renderWork();
    await screen.findByText('Started');
    await waitFor(() => expect(api.controllerChanges).toHaveBeenCalledTimes(1));
    await act(async () =>
      releaseChanges({
        frames: [
          fixtureFrame('message_upsert', 5, {
            item: fixtureMessage('streamed', 5, 'Streamed'),
            previous_message_id: null,
          }),
        ],
        resume_cursor: 'cursor-5',
        has_more: true,
      })
    );
    await waitFor(() => expect(api.controllerChanges).toHaveBeenCalledTimes(2));
    fireEvent.click(screen.getByRole('button', { name: 'Load older messages' }));
    await screen.findByText('middle');
    await act(async () =>
      releaseChanges({
        frames: [fixtureFrame('run_state', 6, { entry: { ...fixtureEntry, revision: 6 } })],
        resume_cursor: 'cursor-6',
        has_more: false,
      })
    );
    expect(screen.getByText('middle')).toBeInTheDocument();
    expect(screen.getByText('Streamed')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Load older messages' }));
    await screen.findByText('oldest');
    expect(api.controllerHistory).toHaveBeenLastCalledWith(
      fixtureEntry.work,
      'generation-a',
      'oldest'
    );
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });
  it('stops replay when even a fresh snapshot cursor is rejected instead of resetting forever', async () => {
    vi.mocked(api.controllerChanges).mockRejectedValue({
      data: {
        error: {
          status: 410,
          code: 'snapshot_required',
          message: 'Fresh snapshot rejected',
        },
      },
    });
    renderWork();
    expect(await screen.findByRole('alert')).toHaveTextContent('Fresh snapshot rejected');
    expect(api.controllerHistoryLatest).toHaveBeenCalledTimes(2);
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 1100));
    });
    expect(api.controllerChanges).toHaveBeenCalledTimes(2);
    expect(api.controllerOpen).toHaveBeenCalledTimes(2);
  });
  it('refuses a latest page from another watermark without falling back to oldest history', async () => {
    vi.mocked(api.controllerHistoryLatest).mockResolvedValue({ ...page(['wrong']), revision: 99 });
    renderWork();
    expect(await screen.findByRole('alert')).toHaveTextContent('History did not match');
    expect(screen.queryByText('wrong')).not.toBeInTheDocument();
    expect(api.controllerHistory).not.toHaveBeenCalled();
  });
  it('retains the older control for an empty visible page without draining hidden rows', async () => {
    vi.mocked(api.controllerOpen).mockResolvedValue(fixtureSnapshot());
    vi.mocked(api.controllerHistoryLatest).mockResolvedValue(page([], 'older-hidden'));
    renderWork();
    await screen.findByRole('button', { name: 'Load older messages' });
    expect(api.controllerHistoryLatest).toHaveBeenCalledTimes(1);
    expect(api.controllerHistory).not.toHaveBeenCalled();
    expect(screen.queryByText('No visible messages yet.')).not.toBeInTheDocument();
  });
});
