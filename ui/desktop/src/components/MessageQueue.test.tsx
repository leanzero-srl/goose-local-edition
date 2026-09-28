import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
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
