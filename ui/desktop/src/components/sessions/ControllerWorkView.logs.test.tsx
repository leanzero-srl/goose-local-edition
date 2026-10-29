import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { IntlTestWrapper } from '../../i18n/test-utils';
import * as api from '../../acp/controller';
import type { CatalogEntry, HistoryPage, WorkKind } from '../../acp/controllerTypes';
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
  controllerHistory: vi.fn(),
  controllerHistoryLatest: vi.fn(),
  controllerChanges: vi.fn(),
  controllerPendingCommands: vi.fn(),
}));

let releaseChanges: (batch: api.WorkChanges) => void;
const externalEntry = (kind: WorkKind = 'shell_job'): CatalogEntry => ({
  ...fixtureEntry,
  work: { ...fixtureEntry.work, kind },
  capabilities: ['stop'],
});
const page = (
  entry: CatalogEntry,
  text: string,
  start: number,
  end: number,
  next: string | null = null,
  revision = 4
): HistoryPage => ({
  work: entry.work,
  store_generation: entry.store_generation,
  revision,
  items: text ? [fixtureMessage(`log_${start}_${end}`, revision, text)] : [],
  next_page_token: next,
  work_cursor: `history-${revision}`,
});

function renderLog(entry = externalEntry()) {
  vi.mocked(api.controllerOpen).mockResolvedValue({ ...fixtureSnapshot(), entry });
  return render(
    <ControllerWorkView entry={entry} host={fixtureHost} stale={false} onBack={vi.fn()} />,
    { wrapper: IntlTestWrapper }
  );
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(api.controllerPendingCommands).mockResolvedValue({ commands: [] });
  vi.mocked(api.controllerChanges).mockImplementation(
    () =>
      new Promise((resolve) => {
        releaseChanges = resolve;
      })
  );
});

describe('selected external log history', () => {
  it.each(['shell_job', 'agent_work', 'benchmark'] as const)(
    'shows only the latest bounded %s page until older output is requested',
    async (kind) => {
      const entry = externalEntry(kind);
      vi.mocked(api.controllerHistory).mockResolvedValue(
        page(entry, 'Latest output', 100, 200, 'older-100')
      );
      renderLog(entry);
      expect(await screen.findByText('Latest output')).toBeInTheDocument();
      expect(api.controllerHistory).toHaveBeenCalledExactlyOnceWith(
        entry.work,
        'generation-a',
        null
      );
      expect(screen.getByRole('button', { name: 'Load older output' })).toBeEnabled();
      expect(screen.queryByText('No log output yet.')).not.toBeInTheDocument();
      expect(screen.queryByText(/Automatic log refresh is paused/)).not.toBeInTheDocument();

      const scroll = screen.getByRole('region', { name: 'Session transcript' });
      Object.defineProperty(scroll, 'scrollHeight', {
        configurable: true,
        get: () => (screen.queryByText('Earlier output') ? 900 : 500),
      });
      scroll.scrollTop = 180;

      const older = page(entry, 'Earlier output', 0, 100);
      (older.items[0].message as { created: number }).created = 20;
      vi.mocked(api.controllerHistory).mockResolvedValue(older);
      fireEvent.click(screen.getByRole('button', { name: 'Load older output' }));
      expect(await screen.findByText('Earlier output')).toBeInTheDocument();
      expect(api.controllerHistory).toHaveBeenLastCalledWith(
        entry.work,
        'generation-a',
        'older-100'
      );
      expect(screen.getAllByRole('article').map((item) => item.textContent)).toEqual([
        expect.stringContaining('Earlier output'),
        expect.stringContaining('Latest output'),
      ]);
      expect(screen.getByText(/Automatic log refresh is paused/)).toBeInTheDocument();
      expect(scroll.scrollTop).toBe(580);
      expect(screen.queryByRole('button', { name: 'Load older output' })).not.toBeInTheDocument();
    }
  );

  it('refreshes a growing log from its selected work event without a catalog or phase change', async () => {
    const entry = externalEntry();
    vi.mocked(api.controllerHistory).mockResolvedValueOnce(
      page(entry, 'Initial tail', 100, 200, 'older-100')
    );
    renderLog(entry);
    await screen.findByText('Initial tail');
    await waitFor(() => expect(api.controllerChanges).toHaveBeenCalledTimes(1));
    vi.mocked(api.controllerOpen).mockResolvedValue({
      ...fixtureSnapshot(),
      entry: { ...entry, revision: 5 },
      work_cursor: 'cursor-5',
      history_watermark: { ...fixtureSnapshot().history_watermark, revision: 5 },
    });
    vi.mocked(api.controllerHistory).mockResolvedValue(
      page(entry, 'New live output', 120, 220, 'older-120', 5)
    );
    await act(async () => {
      releaseChanges({
        frames: [
          {
            ...fixtureFrame('log_updated', 5, { entry: { ...entry, revision: 5 } }),
            work: entry.work,
          },
        ],
        resume_cursor: 'cursor-5',
        has_more: false,
      });
    });
    expect(await screen.findByText('New live output')).toBeInTheDocument();
    expect(screen.queryByText('Initial tail')).not.toBeInTheDocument();
    expect(api.controllerHistory).toHaveBeenCalledTimes(2);
    expect(api.controllerHistory).toHaveBeenLastCalledWith(entry.work, 'generation-a', null);
  });

  it('preserves expanded history across growth replay and resumes the latest tail on Refresh', async () => {
    const entry = externalEntry();
    vi.mocked(api.controllerHistory).mockResolvedValueOnce(
      page(entry, 'Tail being read', 100, 200, 'older-100')
    );
    renderLog(entry);
    await screen.findByText('Tail being read');
    await waitFor(() => expect(api.controllerChanges).toHaveBeenCalledTimes(1));
    vi.mocked(api.controllerHistory).mockResolvedValueOnce(
      page(entry, 'Older output being read', 0, 100)
    );
    fireEvent.click(screen.getByRole('button', { name: 'Load older output' }));
    await screen.findByText('Older output being read');
    vi.mocked(api.controllerOpen).mockResolvedValue({
      ...fixtureSnapshot(),
      entry: { ...entry, revision: 5 },
      work_cursor: 'cursor-5',
      history_watermark: { ...fixtureSnapshot().history_watermark, revision: 5 },
    });
    await act(async () => {
      releaseChanges({
        frames: [
          {
            ...fixtureFrame('log_updated', 5, { entry: { ...entry, revision: 5 } }),
            work: entry.work,
          },
        ],
        resume_cursor: 'cursor-5',
        has_more: false,
      });
    });
    await waitFor(() => expect(api.controllerOpen).toHaveBeenCalledTimes(2));
    expect(api.controllerHistory).toHaveBeenCalledTimes(2);
    expect(screen.getByText('Older output being read')).toBeInTheDocument();
    expect(screen.getByText('Tail being read')).toBeInTheDocument();
    expect(screen.getByText(/Automatic log refresh is paused/)).toBeInTheDocument();

    vi.mocked(api.controllerHistory).mockResolvedValueOnce(
      page(entry, 'Current tail after refresh', 120, 220, 'older-120', 5)
    );
    fireEvent.click(screen.getByRole('button', { name: 'Refresh log' }));
    expect(await screen.findByText('Current tail after refresh')).toBeInTheDocument();
    expect(screen.queryByText('Older output being read')).not.toBeInTheDocument();
    expect(screen.queryByText(/Automatic log refresh is paused/)).not.toBeInTheDocument();
    expect(api.controllerHistory).toHaveBeenLastCalledWith(entry.work, 'generation-a', null);
  });

  it('jumping from paused older output fetches the current owner tail', async () => {
    const entry = externalEntry();
    vi.mocked(api.controllerHistory).mockResolvedValueOnce(
      page(entry, 'Previous tail', 100, 200, 'older')
    );
    renderLog(entry);
    await screen.findByText('Previous tail');
    const element = screen.getByRole('region', { name: 'Session transcript' });
    Object.defineProperty(element, 'scrollHeight', { configurable: true, get: () => 900 });
    Object.defineProperty(element, 'clientHeight', { configurable: true, get: () => 100 });
    element.scrollTop = 25;
    fireEvent.scroll(element);
    vi.mocked(api.controllerHistory).mockResolvedValueOnce(page(entry, 'Earlier output', 0, 100));
    fireEvent.click(screen.getByRole('button', { name: 'Load older output' }));
    await screen.findByText('Earlier output');
    vi.mocked(api.controllerHistory).mockResolvedValueOnce(
      page(entry, 'Current owner tail', 200, 300)
    );
    fireEvent.click(screen.getByRole('button', { name: 'Jump to latest' }));
    expect(await screen.findByText('Current owner tail')).toBeInTheDocument();
    expect(api.controllerHistory).toHaveBeenCalledTimes(3);
    expect(api.controllerHistory).toHaveBeenLastCalledWith(entry.work, 'generation-a', null);
    expect(screen.queryByText('Earlier output')).not.toBeInTheDocument();
    expect(screen.queryByText(/Automatic log refresh is paused/)).not.toBeInTheDocument();
    expect(element.scrollTop).toBe(900);
  });

  it('shows empty output only after a successful empty read', async () => {
    vi.mocked(api.controllerHistory).mockRejectedValueOnce(new Error('Log file cannot be read'));
    renderLog();
    expect(await screen.findByRole('alert')).toHaveTextContent('Log file cannot be read');
    expect(screen.queryByText('No log output yet.')).not.toBeInTheDocument();
    vi.mocked(api.controllerHistory).mockResolvedValueOnce(page(externalEntry(), '', 0, 0));
    fireEvent.click(screen.getByRole('button', { name: 'Refresh log' }));
    expect(await screen.findByText('No log output yet.')).toBeInTheDocument();
    expect(screen.queryAllByRole('article')).toHaveLength(0);
  });

  it('does not replace older output when a tail refresh was already in flight', async () => {
    const entry = externalEntry();
    let finishTail!: (value: HistoryPage) => void;
    vi.mocked(api.controllerHistory).mockResolvedValueOnce(
      page(entry, 'Original tail', 100, 200, 'older-100')
    );
    renderLog(entry);
    await screen.findByText('Original tail');
    await waitFor(() => expect(api.controllerChanges).toHaveBeenCalledTimes(1));
    vi.mocked(api.controllerHistory).mockImplementation((_work, _generation, token) =>
      token === null
        ? new Promise((resolve) => {
            finishTail = resolve;
          })
        : Promise.resolve(page(entry, 'Protected older output', 0, 100))
    );
    await act(async () => {
      releaseChanges({
        frames: [
          {
            ...fixtureFrame('log_updated', 5, { entry: { ...entry, revision: 5 } }),
            work: entry.work,
          },
        ],
        resume_cursor: 'cursor-5',
        has_more: false,
      });
    });
    await waitFor(() => expect(api.controllerHistory).toHaveBeenCalledTimes(2));
    fireEvent.click(screen.getByRole('button', { name: 'Load older output' }));
    await screen.findByText('Protected older output');
    await act(async () => {
      finishTail(page(entry, 'New tail arriving late', 120, 220, 'older-120', 5));
    });
    expect(screen.getByText('Protected older output')).toBeInTheDocument();
    expect(screen.getByText('Original tail')).toBeInTheDocument();
    expect(screen.queryByText('New tail arriving late')).not.toBeInTheDocument();
    expect(screen.getByText(/Automatic log refresh is paused/)).toBeInTheDocument();
  });

  it('loads one latest chat page, then older messages only on request', async () => {
    vi.mocked(api.controllerHistoryLatest).mockResolvedValue({
      ...page(fixtureEntry, '', 0, 0, 'chat-older'),
      items: [fixtureMessage('chat-2', 4, 'Latest chat message')],
    });
    vi.mocked(api.controllerHistory).mockResolvedValue({
      ...page(fixtureEntry, '', 0, 0),
      items: [fixtureMessage('chat-1', 3, 'First chat message')],
    });
    renderLog(fixtureEntry);
    expect(await screen.findByText('Latest chat message')).toBeInTheDocument();
    expect(screen.queryByText('First chat message')).not.toBeInTheDocument();
    expect(api.controllerHistoryLatest).toHaveBeenCalledExactlyOnceWith(
      fixtureEntry.work,
      'generation-a',
      'cursor-4'
    );
    expect(api.controllerHistory).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Load older messages' }));
    expect(await screen.findByText('First chat message')).toBeInTheDocument();
    expect(api.controllerHistory).toHaveBeenCalledExactlyOnceWith(
      fixtureEntry.work,
      'generation-a',
      'chat-older'
    );
    expect(screen.queryByRole('button', { name: 'Refresh log' })).not.toBeInTheDocument();
    expect(screen.queryByText(/Automatic log refresh is paused/)).not.toBeInTheDocument();
  });
});
