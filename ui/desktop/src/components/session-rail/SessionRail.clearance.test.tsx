import { fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { KICKOFF_EDIT, diffResponse, editRequest, message } from '../changes/fixtures';
import type { Message } from '../../types/message';
import SessionRail from './SessionRail';

/**
 * Q-315: at 460 px "4 files +659 −116" sat on the first message bubble — the rail's pills float in
 * the conversation's corner. The rail now says how tall its pills are, and the conversation starts
 * below them (BaseChat's `session-rail-clearance`); open, or with no pills, it asks for nothing.
 */
const ownerEdit: Message[] = [
  message('assistant', [editRequest('c1', '/w/notes/kickoff.md')]),
  message('user', [
    diffResponse('c1', {
      path: '/w/notes/kickoff.md',
      unified: KICKOFF_EDIT,
      added: 4,
      removed: 1,
    }),
  ]),
];

function renderRail(messages: Message[], onPillsHeight: (px: number) => void) {
  return render(
    <SessionRail
      sessionId="s-clearance"
      messages={messages}
      loop={{ kind: 'none' }}
      control={async () => ({ kind: 'failed', error: 'unused' })}
      onPillsHeight={onPillsHeight}
    />,
    { wrapper: IntlTestWrapper }
  );
}

describe('the rail’s pills never sit on the conversation (Q-315)', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    window.localStorage.clear();
  });

  it('collapsed: the pills’ measured height; opened: 0; closed again: the height again', () => {
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
      this: HTMLElement
    ) {
      const height = this.dataset.testid === 'session-rail-pills' ? 32 : 0;
      return {
        x: 0,
        y: 0,
        top: 0,
        left: 0,
        right: 0,
        bottom: height,
        width: 0,
        height,
      } as ReturnType<Element['getBoundingClientRect']>;
    });
    const onPillsHeight = vi.fn();
    renderRail(ownerEdit, onPillsHeight);
    expect(onPillsHeight).toHaveBeenLastCalledWith(32);
    fireEvent.click(screen.getByTestId('changes-rail-pill'));
    expect(onPillsHeight).toHaveBeenLastCalledWith(0);
    fireEvent.click(screen.getByTestId('session-rail-close'));
    expect(onPillsHeight).toHaveBeenLastCalledWith(32);
  });

  it('no file changed and no loop: no pills, and 0 is all it ever says', () => {
    const onPillsHeight = vi.fn();
    renderRail([message('user', [{ type: 'text', text: 'hi' }])], onPillsHeight);
    expect(onPillsHeight).toHaveBeenCalled();
    expect(onPillsHeight.mock.calls.every(([px]) => px === 0)).toBe(true);
  });
});
