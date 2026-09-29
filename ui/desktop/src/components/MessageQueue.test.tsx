import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { IntlTestWrapper } from '../i18n/test-utils';
import { MessageQueue, type QueuedMessage } from './MessageQueue';

const queued: QueuedMessage[] = [
  { id: 'm1', content: 'now the project plan', timestamp: Date.now(), images: [] },
];

function renderQueue(afterCompaction: boolean) {
  return render(
    <MessageQueue
      queuedMessages={queued}
      onRemoveMessage={vi.fn()}
      onClearQueue={vi.fn()}
      onStopAndSend={vi.fn()}
      afterCompaction={afterCompaction}
    />,
    { wrapper: IntlTestWrapper }
  );
}

describe('Q-357: a message typed while the chat compacts waits for the compacted conversation', () => {
  it('says so, and offers no way to send it now', () => {
    renderQueue(true);
    expect(screen.getByTestId('queue-after-compaction').textContent).toBe(
      'Sends right after compacting'
    );
    expect(screen.queryByTitle('Stop current processing and send this message now')).toBeNull();
    expect(screen.queryByTitle('Send this message now')).toBeNull();
  });

  it('outside a compaction the queue is as before', () => {
    renderQueue(false);
    expect(screen.queryByTestId('queue-after-compaction')).toBeNull();
    expect(screen.getByTitle('Stop current processing and send this message now')).toBeTruthy();
  });
});

/**
 * Q-476 — a dragged queue row stays inside the list. The 3.0.76 walk (q335-queue-drag-*.png) showed
 * the dragged row rotated 2° and scaled 105%, spilling past both edges of the queue panel, and the
 * drop target scaled 102% so its ring overran the list. The row now says "dragged" / "drop here"
 * with a solid INSET ring alone — no transform, so it can never leave its box.
 */
describe('Q-476: dragging a queued message never rotates, scales or moves a row', () => {
  const TRANSFORM = /(^|\s)-?(rotate|scale|translate|skew)-/;

  it('the dragged row and the drop target keep their box: an inset ring, no transform', () => {
    const two: QueuedMessage[] = [
      { id: 'a', content: 'queued alpha', timestamp: Date.now(), images: [] },
      { id: 'b', content: 'queued beta', timestamp: Date.now(), images: [] },
    ];
    render(
      <MessageQueue
        queuedMessages={two}
        onRemoveMessage={vi.fn()}
        onClearQueue={vi.fn()}
        onStopAndSend={vi.fn()}
        onReorderMessages={vi.fn()}
      />,
      { wrapper: IntlTestWrapper }
    );
    const bubbles = screen.getAllByTestId('queue-bubble');
    const rows = bubbles.map((b) => b.parentElement as HTMLElement);
    const dataTransfer = { effectAllowed: '', dropEffect: '', setData: vi.fn() };

    fireEvent.mouseEnter(rows[0]);
    expect(bubbles[0].className, 'hovered row').not.toMatch(TRANSFORM);

    fireEvent.dragStart(rows[1], { dataTransfer });
    fireEvent.dragOver(rows[0], { dataTransfer });

    const [target, dragged] = screen.getAllByTestId('queue-bubble');
    expect(dragged.className).toMatch(/ring-inset/);
    expect(dragged.className).toMatch(/ring-lz-accent/);
    expect(dragged.className, 'dragged row').not.toMatch(TRANSFORM);
    expect(target.className).toMatch(/ring-inset/);
    expect(target.className, 'drop target').not.toMatch(TRANSFORM);
    for (const b of screen.getAllByTestId('queue-bubble')) {
      expect(b.className).not.toMatch(/backdrop-blur/);
    }
  });
});
