import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import MemoriesView, { type MemoryEntry } from './MemoriesView';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { TYPE } from '../lz';
import { assertStudioClean } from '../lz/assertStudioClean';

/**
 * The "black box memory" complaint (UX audit S3, 2026-09-23): the view must say where a memory came
 * from — only from what the store records — and let the person edit or delete it in place through
 * the app's own confirm, never a native dialog. A field with no data is not drawn.
 */

vi.mock('../Layout/useStartChatAbout', () => ({ useStartChatAbout: () => vi.fn() }));
vi.mock('../../utils/workingDir', () => ({ getInitialWorkingDir: () => '/proj/goose' }));
const sessionItem = vi.fn();
vi.mock('../../acp/sessions', () => ({
  acpGetSessionListItem: (id: string) => sessionItem(id),
}));

const imported: MemoryEntry = {
  id: 'global:voice-structure:0',
  category: 'voice-structure-is-the-tell',
  scope: 'global',
  tags: ['feedback', 'imported:claude-code'],
  content: 'Community replies read AI on STRUCTURE, not vocabulary.\nFix is subtraction.',
  updatedAt: Date.parse('2026-09-21T09:30:00Z'),
  filePath: '/home/me/.config/goose/memory/voice-structure-is-the-tell.txt',
};

const proposed: MemoryEntry = {
  id: 'local:lessons:0',
  category: 'lessons',
  scope: 'local',
  tags: ['feedback'],
  content: 'Webhook tests fail unless WEBHOOK_SECRET=dev is exported first.',
  updatedAt: 0,
  filePath: '/proj/goose/.goose/memory/lessons.txt',
  origin: {
    key: '20260921_30',
    proposedAt: 1790012779,
    why: 'User lost an hour debugging this.',
    polarity: 'positive',
  },
};

const mount = (list: MemoryEntry[]) => {
  const electron = (window as unknown as { electron: Record<string, unknown> }).electron;
  electron.listMemories = vi.fn(async () => list);
  electron.editMemory = vi.fn(async () => true);
  electron.deleteMemory = vi.fn(async () => true);
  return {
    electron,
    ...render(
      <IntlTestWrapper>
        <MemoriesView />
      </IntlTestWrapper>
    ),
  };
};

const detail = () => screen.getByTestId('memory-detail');
const provenanceRow = (key: string) => screen.queryByTestId(`memory-provenance-${key}`);

describe('MemoriesView — provenance, inline edit and delete', () => {
  beforeEach(() => {
    sessionItem.mockReset();
    sessionItem.mockResolvedValue({ id: '20260921_30', name: 'Webhook secret configuration' });
  });

  it('opens on the first memory with its full text and where it came from — an imported one names its source and file', async () => {
    const { container } = mount([imported, proposed]);
    await screen.findByTestId('memory-detail');
    expect(
      within(detail()).getByRole('heading', { name: 'Voice Structure Is The Tell' })
    ).toBeTruthy();
    expect(screen.getByTestId('memory-body').textContent).toContain('Fix is subtraction.');
    expect(provenanceRow('scope')?.textContent).toContain('Global — recalled in every project');
    expect(provenanceRow('source-imported:claude-code')?.textContent).toContain(
      'Imported from Claude Code'
    );
    expect(provenanceRow('tags')?.textContent).toContain('feedback');
    expect(provenanceRow('tags')?.textContent).not.toContain('imported:claude-code');
    expect(provenanceRow('changed')).not.toBeNull();
    expect(provenanceRow('file')?.textContent).toContain('voice-structure-is-the-tell.txt');
    // No proposal on disk for it: no session, no reason, no proposed date is claimed.
    expect(provenanceRow('session')).toBeNull();
    expect(provenanceRow('why')).toBeNull();
    expect(provenanceRow('proposed')).toBeNull();
    assertStudioClean(container);
  });

  it('a memory saved from an agent proposal names the session (by title), the date asked and the reason; a zero mtime draws no date', async () => {
    sessionItem.mockResolvedValue({ id: '20260921_30', name: 'Webhook secret configuration' });
    mount([imported, proposed]);
    fireEvent.click(await screen.findByText('Lessons', { selector: 'button *' }));
    expect(provenanceRow('scope')?.textContent).toContain('This project — goose');
    expect(provenanceRow('source-proposal')?.textContent).toContain('Saved from an agent proposal');
    expect(provenanceRow('why')?.textContent).toContain('User lost an hour debugging this.');
    expect(provenanceRow('proposed')).not.toBeNull();
    expect(provenanceRow('changed')).toBeNull();
    const session = await screen.findByTestId('memory-origin-session');
    await waitFor(() => expect(session.textContent).toContain('Webhook secret configuration'));
    expect(sessionItem).toHaveBeenCalledWith('20260921_30');
    fireEvent.click(session);
    expect(window.location.hash).toBe('#/pair?resumeSessionId=20260921_30');
  });

  it('a proposal whose session is gone says so, with the id', async () => {
    sessionItem.mockRejectedValue(new Error('not found'));
    mount([proposed]);
    await screen.findByTestId('memory-detail');
    await waitFor(() =>
      expect(provenanceRow('session')?.textContent).toContain('no longer in the session list')
    );
    expect(provenanceRow('session')?.textContent).toContain('20260921_30');
  });

  it('Edit → Save writes through edit-memory with the old and new text', async () => {
    const { electron } = mount([imported]);
    await screen.findByTestId('memory-detail');
    fireEvent.click(within(detail()).getByRole('button', { name: 'Edit' }));
    const box = screen.getByRole('textbox', { name: 'Memory text' });
    fireEvent.change(box, { target: { value: 'Structure is the tell.' } });
    fireEvent.click(within(detail()).getByRole('button', { name: 'Save' }));
    await waitFor(() =>
      expect(electron.editMemory).toHaveBeenCalledWith({
        scope: 'global',
        category: 'voice-structure-is-the-tell',
        oldContent: imported.content,
        newContent: 'Structure is the tell.',
        workingDir: '/proj/goose',
      })
    );
  });

  it('Delete asks through the app dialog (never window.confirm) and deletes only on confirm', async () => {
    const confirmSpy = vi.spyOn(window, 'confirm');
    const { electron } = mount([imported]);
    await screen.findByTestId('memory-detail');
    fireEvent.click(within(detail()).getByRole('button', { name: 'Delete' }));
    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText('Delete "Voice Structure Is The Tell"?')).toBeTruthy();
    expect(electron.deleteMemory).not.toHaveBeenCalled();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Delete' }));
    await waitFor(() =>
      expect(electron.deleteMemory).toHaveBeenCalledWith({
        scope: 'global',
        category: 'voice-structure-is-the-tell',
        content: imported.content,
        workingDir: '/proj/goose',
      })
    );
    expect(confirmSpy).not.toHaveBeenCalled();
  });

  it('the list rows carry a kind word, not an unlabelled coloured square, and the title is full ink', async () => {
    mount([imported, proposed]);
    const title = await screen.findByRole('heading', { level: 1, name: 'Memories' });
    for (const c of TYPE.display.split(' ')) expect(title.className).toContain(c);
    const rows = await screen.findAllByTestId('memory-row');
    expect(rows[0].querySelector('[data-testid="library-row-label"]')?.textContent).toBe(
      'feedback'
    );
    expect(rows[0].querySelector('.w-2.h-2')).toBeNull();
  });

  it('search filters by name, text and tag', async () => {
    mount([imported, proposed]);
    await screen.findAllByTestId('memory-row');
    const search = screen.getByRole('textbox', { name: 'Search memories by name, text or tag' });
    fireEvent.change(search, { target: { value: 'webhook' } });
    expect(screen.getAllByTestId('memory-row')).toHaveLength(1);
    expect(within(detail()).getByRole('heading', { name: 'Lessons' })).toBeTruthy();
    fireEvent.change(search, { target: { value: 'claude-code' } });
    expect(screen.getAllByTestId('memory-row')).toHaveLength(1);
    expect(screen.getByTestId('memory-row').textContent).toContain('Voice Structure');
  });
});
