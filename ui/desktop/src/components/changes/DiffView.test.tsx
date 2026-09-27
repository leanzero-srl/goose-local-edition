import { fireEvent, render, screen, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { DiffView } from './DiffView';
import { parseUnifiedDiff, type FileDiff } from './fileDiff';
import { KICKOFF_EDIT, bigCreate, unified } from './fixtures';

function diffOf(
  text: string,
  added: number,
  removed: number,
  before: FileDiff['before'] = 'file'
): FileDiff {
  return { path: '/w/f', before, added, removed, hunks: parseUnifiedDiff(text) };
}

function renderDiff(diff: FileDiff) {
  return render(<DiffView diff={diff} />, { wrapper: IntlTestWrapper });
}

describe('DiffView', () => {
  it('draws the owner edit: the removed line red with its old number, the added lines green with new numbers', () => {
    renderDiff(diffOf(KICKOFF_EDIT, 4, 1));
    const removed = screen.getAllByTestId('diff-line-del');
    const added = screen.getAllByTestId('diff-line-add');
    expect(removed).toHaveLength(1);
    expect(added).toHaveLength(4);
    expect(removed[0]).toHaveTextContent('3−Agenda: TBD');
    expect(within(removed[0]).getByText('Agenda: TBD')).toHaveClass('text-lz-err');
    expect(within(removed[0]).getByText('−')).toHaveClass('bg-lz-err-solid');
    expect(added.map((row) => row.textContent)).toEqual([
      '3+Agenda:',
      '4+- scope',
      '5+- dates',
      '6+- owners',
    ]);
    expect(within(added[0]).getByText('+')).toHaveClass('bg-lz-ok-solid');
    expect(screen.getByTestId('diff-hunk')).toHaveTextContent('Line 1');
    expect(screen.queryByTestId('diff-show-all')).not.toBeInTheDocument();
  });

  it('says a created file is new and shows every line as added', () => {
    renderDiff(diffOf(bigCreate('/w/n.md', 2), 2, 0, 'none'));
    expect(screen.getByTestId('diff-new-file')).toHaveTextContent('New file');
    expect(screen.getAllByTestId('diff-line-add')).toHaveLength(2);
    expect(screen.queryByTestId('diff-line-del')).not.toBeInTheDocument();
  });

  it('says when the file held no text before instead of calling it new', () => {
    renderDiff(diffOf(bigCreate('/w/b.bin', 1), 1, 0, 'unreadable'));
    expect(screen.getByTestId('diff-unreadable-before')).toBeInTheDocument();
    expect(screen.queryByTestId('diff-new-file')).not.toBeInTheDocument();
  });

  it('says so when a write changed no lines', () => {
    renderDiff(diffOf(unified('/w/f', ''), 0, 0));
    expect(screen.getByTestId('diff-empty')).toBeInTheDocument();
  });

  it('folds a diff taller than half the window behind Show all, then shows every line', () => {
    const budget = Math.floor((window.innerHeight * 0.5) / 18);
    const count = budget * 3;
    renderDiff(diffOf(bigCreate('/w/huge.txt', count), count, 0, 'none'));
    expect(screen.getAllByTestId('diff-line-add')).toHaveLength(budget - 1);
    const showAll = screen.getByTestId('diff-show-all');
    expect(showAll).toHaveTextContent(`Show all ${count} lines`);
    fireEvent.click(showAll);
    expect(screen.getAllByTestId('diff-line-add')).toHaveLength(count);
    expect(screen.queryByTestId('diff-show-all')).not.toBeInTheDocument();
  });
});
