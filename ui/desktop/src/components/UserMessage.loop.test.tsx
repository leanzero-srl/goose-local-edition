import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { acpGetSessionListItem, type SessionListItem } from '../acp/sessions';
import { AppEvents } from '../constants/events';
import { IntlTestWrapper } from '../i18n/test-utils';
import { createUserMessage, type Message } from '../types/message';
import { loopRecord, markerId } from './loops/railFixtures';
import {
  LoopSessionContext,
  onStartLoopRequest,
  type LoopSessionValue,
  type StartLoopRequest,
} from './loops/startLoopRequest';
import UserMessage from './UserMessage';

vi.mock('../acp/sessions', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../acp/sessions')>()),
  acpGetSessionListItem: vi.fn(),
}));

beforeEach(() => {
  vi.mocked(acpGetSessionListItem).mockRejectedValue(new Error('no goosed in this test'));
  vi.spyOn(console, 'warn').mockImplementation(() => undefined);
  vi.spyOn(Date.prototype, 'getTimezoneOffset').mockReturnValue(0);
  window.electron = { ...(window.electron ?? {}), logInfo: vi.fn() } as typeof window.electron;
});
afterEach(() => vi.restoreAllMocks());

function loopSessionOf(sessionId: string, loop: LoopSessionValue['loop']): LoopSessionValue {
  return {
    sessionId,
    loop,
    state: loop ? { kind: 'loop', loop, status: loop.status } : { kind: 'none' },
    reload: () => undefined,
  };
}

function mount(message: Message, session: LoopSessionValue | null) {
  return render(
    <IntlTestWrapper>
      <LoopSessionContext.Provider value={session}>
        <UserMessage message={message} />
      </LoopSessionContext.Provider>
    </IntlTestWrapper>
  );
}

const PROMPT = 'Loop tick 3 — "Make it pass" · every 10 min\nFinish by calling loop_report.';

function tickMessage(withMetadata: boolean): Message {
  const id = markerId(3);
  return {
    id,
    role: 'user',
    created: Date.parse('2026-09-27T22:21:00Z') / 1000,
    content: [{ type: 'text', text: PROMPT }],
    metadata: {
      userVisible: true,
      agentVisible: true,
      ...(withMetadata ? { loopTick: { loopId: 'lp_0a1b2c3d', n: 3, messageId: id } } : {}),
    },
  };
}

describe('UserMessage and loops', () => {
  it('renders a tick prompt as the divider, not a bubble, with the exact prompt one click away', () => {
    mount(tickMessage(true), loopSessionOf('s1', loopRecord()));
    const marker = screen.getByTestId('loop-tick-marker');
    expect(marker).toHaveTextContent('Loop tick 3 · 22:21 · every 10 min');
    expect(marker.id).toBe(`loop-tick-${markerId(3)}`);
    expect(screen.queryByTestId('user-message-body')).not.toBeInTheDocument();
    expect(screen.queryByText('Edit')).not.toBeInTheDocument();
    expect(screen.queryByTestId('user-message-loop-this')).not.toBeInTheDocument();
    expect(screen.getByTestId('loop-tick-copy-prompt')).toHaveTextContent('Copy prompt');
    fireEvent.click(screen.getByTestId('loop-tick-marker-toggle'));
    expect(screen.getByTestId('loop-tick-prompt').textContent).toBe(PROMPT);
  });

  it('reads a replayed tick id as a marker even without metadata, and without the loop says no cadence', () => {
    mount(tickMessage(false), null);
    expect(screen.getByTestId('loop-tick-marker')).toHaveTextContent('Loop tick 3 · 22:21');
    expect(screen.getByTestId('loop-tick-marker')).not.toHaveTextContent('every');
  });

  it('says why a yielded tick stopped', () => {
    const record = loopRecord();
    record.ticks = record.ticks!.map((t) =>
      t.n === 3
        ? { ...t, outcome: { kind: 'yielded', toSession: 's2', toChat: 'Kickoff notes' } }
        : t
    );
    mount(tickMessage(true), loopSessionOf('s1', record));
    expect(screen.getByTestId('loop-tick-marker-yielded')).toHaveTextContent(
      'Stopped at 22:27 for your message in "Kickoff notes" — the loop continues after your turn.'
    );
  });

  it('says the tick after a yield started after your turn, not by the cadence (Q-279)', () => {
    const record = loopRecord();
    record.ticks = record.ticks!.map((t) =>
      t.n === 3 ? { ...t, origin: 'after_your_turn' as const } : t
    );
    mount(tickMessage(true), loopSessionOf('s1', record));
    expect(screen.getByTestId('loop-tick-marker')).toHaveTextContent(
      'Loop tick 3 · 22:21 · after your turn'
    );
    expect(screen.getByTestId('loop-tick-marker')).not.toHaveTextContent('every 10 min');
  });

  it('names no cause when the record no longer holds the tick, rather than guess the cadence', () => {
    const record = loopRecord();
    record.ticks = record.ticks!.filter((t) => t.n !== 3);
    mount(tickMessage(true), loopSessionOf('s1', record));
    expect(screen.getByTestId('loop-tick-marker')).toHaveTextContent('Loop tick 3 · 22:21');
    expect(screen.getByTestId('loop-tick-marker')).not.toHaveTextContent('every');
  });

  it('names the chat a tick yielded to as it is called now, and follows a rename (Q-279)', async () => {
    vi.mocked(acpGetSessionListItem).mockResolvedValue({
      id: 's2',
      name: 'Simple pong reply',
    } as SessionListItem);
    const record = loopRecord();
    record.ticks = record.ticks!.map((t) =>
      t.n === 3 ? { ...t, outcome: { kind: 'yielded', toSession: 's2', toChat: 'New Chat' } } : t
    );
    mount(tickMessage(true), loopSessionOf('s1', record));
    expect(await screen.findByText(/"Simple pong reply"/)).toBeInTheDocument();
    expect(acpGetSessionListItem).toHaveBeenCalledWith('s2');
    expect(screen.getByTestId('loop-tick-marker-yielded')).not.toHaveTextContent('New Chat');
    act(() => {
      window.dispatchEvent(
        new CustomEvent(AppEvents.SESSION_RENAMED, {
          detail: { sessionId: 's2', newName: 'Pong, renamed' },
        })
      );
    });
    expect(screen.getByTestId('loop-tick-marker-yielded')).toHaveTextContent(
      'Stopped at 22:27 for your message in "Pong, renamed" — the loop continues after your turn.'
    );
  });

  it("offers Loop this on a person's message and hands its words to the Start dialog", () => {
    const got: StartLoopRequest[] = [];
    const off = onStartLoopRequest((r) => got.push(r) > 0);
    mount(createUserMessage('Make every test pass'), loopSessionOf('s1', null));
    fireEvent.click(screen.getByTestId('user-message-loop-this'));
    off();
    expect(got).toEqual([{ sessionId: 's1', mode: 'start', goal: 'Make every test pass' }]);
    expect(screen.queryByTestId('user-message-loop-this-refused')).not.toBeInTheDocument();
  });

  it('says so when no Start dialog took Loop this, and has no Loop this outside a chat', () => {
    const { unmount } = mount(createUserMessage('Make every test pass'), loopSessionOf('s1', null));
    fireEvent.click(screen.getByTestId('user-message-loop-this'));
    expect(screen.getByTestId('user-message-loop-this-refused')).toHaveTextContent(
      'The loop dialog is not in this build yet.'
    );
    unmount();
    mount(createUserMessage('Make every test pass'), null);
    expect(screen.queryByTestId('user-message-loop-this')).not.toBeInTheDocument();
  });
});
