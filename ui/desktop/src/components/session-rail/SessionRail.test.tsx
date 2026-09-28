import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import raw from '../../../../../crates/goose/src/session_loops/loops.fixture.json';
import { acpGetSessionListItem, type SessionListItem } from '../../acp/sessions';
import { AppEvents } from '../../constants/events';
import { IntlTestWrapper } from '../../i18n/test-utils';
import type { Message } from '../../types/message';
import { sessionChanges } from '../changes/fileDiff';
import { tickSlices } from '../loops/loopView';
import type { LoopRecord, LoopStatus, LoopStatusReason } from '../loops/model';
import { LOOP_MESSAGES, NOW_MS, loopRecord, markerId, waitingRecord } from '../loops/railFixtures';
import { onStartLoopRequest, type StartLoopRequest } from '../loops/startLoopRequest';
import type { ControlResult, SessionLoop } from '../loops/useSessionLoop';
import SessionRail from './SessionRail';

/** Every `HH:MM` of goosed's words as the chat's `en` clock writes it ("22:40" → "10:40 PM"). */
function inChatClock(text: string): string {
  return text.replace(/\b([01]\d|2[0-3]):([0-5]\d)\b/g, (_, h: string, m: string) => {
    const hour = Number(h);
    return `${hour % 12 === 0 ? 12 : hour % 12}:${m} ${hour < 12 ? 'AM' : 'PM'}`;
  });
}

vi.mock('../../acp/sessions', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../acp/sessions')>()),
  acpGetSessionListItem: vi.fn(),
}));

type Control = (action: string) => Promise<ControlResult>;

const runnerAbsent: Control = async () => ({
  kind: 'refused',
  refusal: { code: 'runner_absent', reason: 'The loop runner is not in this build' },
});

function renderRail({
  loop,
  messages = LOOP_MESSAGES,
  control = runnerAbsent,
  sessionId = 's1',
}: {
  loop: SessionLoop;
  messages?: Message[];
  control?: Control;
  sessionId?: string;
}) {
  return render(
    <SessionRail
      sessionId={sessionId}
      messages={messages}
      loop={loop}
      control={control}
      workingDir="/w"
      className="absolute right-4 top-2"
    />,
    { wrapper: IntlTestWrapper }
  );
}

const asLoop = (
  record: LoopRecord,
  status: LoopStatus = record.status,
  reason: LoopStatusReason | null | undefined = record.statusReason
): SessionLoop => ({ kind: 'loop', loop: record, status, reason });

beforeEach(() => {
  vi.mocked(acpGetSessionListItem).mockRejectedValue(new Error('no goosed in this test'));
  vi.spyOn(console, 'warn').mockImplementation(() => undefined);
  vi.useFakeTimers({ toFake: ['Date'] });
  vi.setSystemTime(NOW_MS);
  vi.spyOn(Date.prototype, 'getTimezoneOffset').mockReturnValue(0);
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  window.localStorage.clear();
});

describe('SessionRail collapsed', () => {
  it('shows the loop pill first, then the Changes pill', () => {
    renderRail({ loop: asLoop(loopRecord()) });
    const pills = screen.getByTestId('session-rail-pills').querySelectorAll('button');
    expect(pills[0]).toHaveAttribute('data-testid', 'loop-rail-pill');
    expect(pills[1]).toHaveAttribute('data-testid', 'changes-rail-pill');
    expect(pills[0]).toHaveTextContent('Tick 5 running · 2m');
    expect(pills[0]).toHaveAttribute('data-tone', 'ok');
  });

  it('has no loop pill in a chat with no loop, and nothing at all with no loop and no change', () => {
    renderRail({ loop: { kind: 'none' } });
    expect(screen.queryByTestId('loop-rail-pill')).not.toBeInTheDocument();
    expect(screen.getByTestId('changes-rail-pill')).toBeInTheDocument();
  });

  it('renders nothing while the loop is read and no file changed', () => {
    const { container } = renderRail({ loop: { kind: 'loading' }, messages: [] });
    expect(container).toBeEmptyDOMElement();
  });

  it('labels each status as §4.7 does, in its own solid fill', () => {
    const cases: [SessionLoop, string, string][] = [
      [asLoop(waitingRecord()), 'Next tick 10:51 PM', 'accent'],
      [
        asLoop(loopRecord(), 'waiting_turn', { kind: 'reviewers', n: 4 }),
        "Next tick after goose's check of tick 4",
        'secondary',
      ],
      [
        asLoop(loopRecord(), 'waiting_you', { kind: 'no_delay', n: 4 }),
        'Loop waiting for you',
        'warn',
      ],
      [
        asLoop(loopRecord(), 'needs_you', {
          kind: 'asked',
          n: 4,
          itemId: 'ny',
          question: 'Comma?',
        }),
        'Loop needs you',
        'warn',
      ],
      [asLoop(loopRecord(), 'paused', { kind: 'by_you', afterTick: 4 }), 'Loop paused', 'stopped'],
      [asLoop(loopRecord(), 'elsewhere', null), 'Looping in another window', 'secondary'],
      [{ kind: 'unreadable', error: 'bad json' }, 'Loop unreadable', 'err'],
    ];
    for (const [loop, label, tone] of cases) {
      const { unmount } = renderRail({ loop });
      const pill = screen.getByTestId('loop-rail-pill');
      expect(pill, label).toHaveTextContent(label);
      expect(pill, label).toHaveAttribute('data-tone', tone);
      unmount();
    }
  });

  it('shows an ended loop until its Loop tab is opened once, then only Changes', () => {
    const ended = asLoop(
      loopRecord({ status: 'ended', statusReason: { kind: 'stopped_by_you', n: 4 } })
    );
    const { unmount } = renderRail({ loop: ended });
    expect(screen.getByTestId('loop-rail-pill')).toHaveTextContent('Loop ended');
    fireEvent.click(screen.getByTestId('loop-rail-pill'));
    expect(screen.getByTestId('loop-now')).toHaveTextContent('Stopped by you after tick 4');
    fireEvent.keyDown(screen.getByTestId('session-rail-panel'), { key: 'Escape' });
    expect(screen.queryByTestId('loop-rail-pill')).not.toBeInTheDocument();
    unmount();
    renderRail({ loop: ended });
    expect(screen.queryByTestId('loop-rail-pill')).not.toBeInTheDocument();
    expect(screen.getByTestId('changes-rail-pill')).toBeInTheDocument();
  });
});

describe('SessionRail open', () => {
  it('opens on the tab of the pill clicked, switches tabs, and Escape gives focus back', () => {
    renderRail({ loop: asLoop(loopRecord()) });
    fireEvent.click(screen.getByTestId('loop-rail-pill'));
    const panel = screen.getByTestId('session-rail-panel');
    expect(panel).toHaveAttribute('data-tab', 'loop');
    expect(screen.getByRole('tab', { name: 'Loop' })).toHaveAttribute('aria-selected', 'true');
    expect(screen.getByRole('tab', { name: 'Changes 2' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('tab', { name: 'Changes 2' }));
    expect(screen.getByTestId('changes-rail-panel')).toBeInTheDocument();
    fireEvent.keyDown(screen.getByTestId('changes-rail-panel'), { key: 'Escape' });
    expect(screen.queryByTestId('session-rail-panel')).not.toBeInTheDocument();
    expect(screen.getByTestId('loop-rail-pill')).toHaveFocus();
  });

  it('remembers open state and tab per session across a remount', () => {
    const { unmount } = renderRail({ loop: asLoop(loopRecord()) });
    fireEvent.click(screen.getByTestId('loop-rail-pill'));
    unmount();
    renderRail({ loop: asLoop(loopRecord()) });
    expect(screen.getByTestId('session-rail-panel')).toHaveAttribute('data-tab', 'loop');
    renderRail({ loop: asLoop(loopRecord()), sessionId: 'other' });
    expect(screen.getAllByTestId('loop-rail-pill')).toHaveLength(1);
  });

  it('works, forgetting only, when localStorage throws', () => {
    vi.spyOn(window.Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('denied');
    });
    vi.spyOn(window.Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('denied');
    });
    renderRail({ loop: asLoop(loopRecord()) });
    fireEvent.click(screen.getByTestId('loop-rail-pill'));
    expect(screen.getByTestId('session-rail-panel')).toBeInTheDocument();
  });

  it('is an overlay: its root is absolutely placed and the open panel never sets a width on the chat', () => {
    const { container } = renderRail({ loop: asLoop(loopRecord()) });
    const root = screen.getByTestId('changes-rail');
    expect(root.className).toContain('absolute');
    fireEvent.click(screen.getByTestId('loop-rail-pill'));
    expect(container.firstChild).toBe(root);
    expect(screen.getByTestId('session-rail-panel').className).toContain('w-[min(32rem');
  });
});

describe('the Loop tab', () => {
  const open = (loop: SessionLoop, control?: Control) => {
    renderRail({ loop, control });
    fireEvent.click(screen.getByTestId('loop-rail-pill'));
  };

  it('heads the loop with its goal, status, cadence line, check and state file', () => {
    open(asLoop(loopRecord()));
    expect(screen.getByTestId('loop-goal')).toHaveTextContent(
      'Make scripts/generate_users.js produce every problem class in notes/kickoff.md'
    );
    expect(screen.getByTestId('loop-status-chip')).toHaveTextContent('Running');
    expect(screen.getByTestId('loop-line-two')).toHaveTextContent(
      'every 10 min · tick 5 · since 10:00 PM · 22m 54s · 40K tokens'
    );
    expect(screen.getByTestId('loop-check-line')).toHaveTextContent(
      'Check: node scripts/validate_users.js'
    );
    expect(screen.getByTestId('loop-state-file')).toHaveTextContent(
      'State file: .goose/loops/users-csv/NOW.md'
    );
  });

  it('says a control goosed refused in goosed words, never a success', async () => {
    const control = vi.fn(runnerAbsent);
    open(asLoop(loopRecord()), control);
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Pause' }));
    });
    expect(control).toHaveBeenCalledWith('pause');
    expect(screen.getByTestId('loop-said')).toHaveTextContent(
      'goose refused: The loop runner is not in this build'
    );
    expect(screen.getByTestId('loop-status-chip')).toHaveTextContent('Running');
  });

  it('disables Run a tick now while a tick runs, and asks before Stop loop', async () => {
    const control = vi.fn(runnerAbsent);
    open(asLoop(loopRecord()), control);
    expect(screen.getByRole('button', { name: 'Run a tick now' })).toBeDisabled();
    fireEvent.click(
      within(screen.getByTestId('loop-controls')).getByRole('button', { name: 'Stop loop' })
    );
    const dialog = screen.getByTestId('loop-stop-dialog');
    expect(dialog).toHaveTextContent('Tick 5 stops now and keeps what it did.');
    expect(control).not.toHaveBeenCalled();
    await act(async () => {
      fireEvent.click(within(dialog).getByRole('button', { name: 'Stop loop' }));
    });
    expect(control).toHaveBeenCalledWith('stop');
  });

  it('says so when no Start dialog is in the build, and hands the request to one that is', () => {
    open(asLoop(waitingRecord()));
    fireEvent.click(screen.getByRole('button', { name: 'Edit' }));
    expect(screen.getByTestId('loop-said')).toHaveTextContent(
      'The loop dialog is not in this build yet.'
    );
    const got: StartLoopRequest[] = [];
    const off = onStartLoopRequest((r) => got.push(r) > 0);
    fireEvent.click(screen.getByRole('button', { name: 'Edit' }));
    off();
    expect(got).toHaveLength(1);
    expect(got[0]).toMatchObject({ sessionId: 's1', mode: 'edit' });
    expect(got[0].from?.id).toBe('lp_0a1b2c3d');
  });

  it('shows no loop as the empty tab, with its way in', () => {
    renderRail({ loop: { kind: 'none' } });
    fireEvent.click(screen.getByTestId('changes-rail-pill'));
    fireEvent.click(screen.getByRole('tab', { name: 'Loop' }));
    expect(screen.getByTestId('loop-panel-empty')).toHaveTextContent('No loop in this chat.');
    expect(screen.getByTestId('loop-panel-empty')).toHaveTextContent('with /loop <goal>');
    expect(screen.getByRole('button', { name: 'Start a loop' })).toBeInTheDocument();
  });

  it('names an unreadable record and keeps only Stop loop', () => {
    open({ kind: 'unreadable', error: 'expected value at line 1 column 1' });
    expect(screen.getByTestId('loop-now')).toHaveTextContent(
      'The loop record could not be read: expected value at line 1 column 1'
    );
    expect(screen.getAllByRole('button').map((b) => b.textContent)).not.toContain('Pause');
    expect(screen.getByRole('button', { name: 'Stop loop' })).toBeInTheDocument();
  });
});

describe('the tick ledger', () => {
  const openWaiting = () => {
    renderRail({ loop: asLoop(waitingRecord()) });
    fireEvent.click(screen.getByTestId('loop-rail-pill'));
  };
  const row = (n: number) =>
    screen.getAllByTestId('loop-tick-row').find((r) => r.getAttribute('data-tick') === String(n))!;

  it('lists ended ticks newest first; the tick in flight is the NOW block, not a row', () => {
    renderRail({ loop: asLoop(loopRecord()) });
    fireEvent.click(screen.getByTestId('loop-rail-pill'));
    expect(screen.getAllByTestId('loop-tick-row').map((r) => r.getAttribute('data-tick'))).toEqual([
      '4',
      '3',
      '2',
      '1',
    ]);
    expect(screen.getByTestId('loop-now')).toHaveTextContent('Tick 5 · started 10:41 PM · 2m');
  });

  it("lists each tick's files through the same sessionChanges over that tick's messages", () => {
    openWaiting();
    const slices = tickSlices(LOOP_MESSAGES, waitingRecord());
    fireEvent.click(within(row(1)).getAllByRole('button')[0]);
    const files = within(row(1)).getAllByTestId('changes-rail-file');
    const expected = sessionChanges(slices[0].messages!);
    expect(files.map((f) => f.querySelector('button')?.getAttribute('title'))).toEqual(
      expected.files.map((f) => f.path)
    );
    expect(within(row(1)).getByTestId('loop-tick-files')).toHaveTextContent(
      `Written or edited by goose+${expected.added}−${expected.removed}`
    );
    expect(within(row(1)).getByTestId('loop-tick-check')).toHaveTextContent(
      'Check node scripts/validate_users.js exited 1 — "missing class: duplicate emails"'
    );
    expect(within(row(1)).getByText('Next: add case-only duplicate emails')).toBeInTheDocument();
    expect(within(row(1)).getByText('on 27B · both Macs · 16K tokens')).toBeInTheDocument();
  });

  it('collapses a quiet tick to one line with the exact words', () => {
    openWaiting();
    expect(within(row(4)).getByTestId('loop-tick-quiet')).toHaveTextContent(
      '10:31 PM · no write or edit outside the state file · check the svc- account format against kickoff.md'
    );
    expect(within(row(3)).queryByTestId('loop-tick-quiet')).not.toBeInTheDocument();
  });

  it('says a failed tick with its error and its chip', () => {
    openWaiting();
    expect(within(row(2)).getByTestId('loop-tick-chip')).toHaveTextContent('Failed');
    expect(within(row(2)).getByTestId('loop-tick-chip')).toHaveAttribute('data-tone', 'err');
    expect(row(2)).toHaveTextContent('Failed: Provider error: stream ended early');
  });

  it('names the asked, yielded, stopped-by-you, no-report and removed-by-edit rows', () => {
    const record = waitingRecord();
    const ticks = record.ticks!;
    const answered: Message = {
      role: 'user',
      created: 2,
      content: [{ type: 'text', text: 'Semicolon, please' }],
      metadata: { userVisible: true, agentVisible: true },
    };
    const messages = [...LOOP_MESSAGES];
    messages.splice(
      messages.findIndex((m) => m.id === markerId(3)),
      0,
      answered
    );
    const edited = messages.filter((m) => m.id !== markerId(4));
    const special = {
      ...record,
      ticks: [
        { ...ticks[0], outcome: { kind: 'no_report' as const }, report: null },
        {
          ...ticks[1],
          outcome: { kind: 'asked' as const, itemId: 'ny', question: 'Comma or semicolon?' },
        },
        {
          ...ticks[2],
          outcome: { kind: 'yielded' as const, toSession: 's2', toChat: 'Kickoff notes' },
        },
        { ...ticks[3], outcome: { kind: 'stopped_by_you' as const }, report: null },
        ticks[4],
      ],
    };
    renderRail({ loop: asLoop(special), messages: edited });
    fireEvent.click(screen.getByTestId('loop-rail-pill'));
    for (const n of [1, 2, 3, 4]) fireEvent.click(within(row(n)).getAllByRole('button')[0]);
    expect(within(row(1)).getByTestId('loop-tick-no-report')).toHaveTextContent(
      'Ended without a loop report'
    );
    expect(within(row(1)).getByTestId('loop-tick-last-words')).toHaveTextContent(
      'Its last words: "Read kickoff.md; listed 6 problem classes."'
    );
    expect(within(row(2)).getByTestId('loop-tick-asked')).toHaveTextContent(
      'Asked you: "Comma or semicolon?"'
    );
    expect(within(row(2)).getByTestId('loop-tick-answered')).toHaveTextContent(
      'You answered: "Semicolon, please"'
    );
    expect(within(row(3)).getByTestId('loop-tick-yielded')).toHaveTextContent(
      'Yielded to your turn in "Kickoff notes"'
    );
    expect(within(row(3)).getByTestId('loop-tick-chip')).toHaveAttribute('data-tone', 'secondary');
    expect(within(row(4)).getByTestId('loop-tick-stopped')).toHaveTextContent(
      'You stopped this tick.'
    );
    expect(within(row(4)).getByTestId('loop-tick-removed')).toHaveTextContent(
      "This tick's messages were removed by an edit"
    );
  });

  it('marks the tick the loop ended on as Goal met, and the tick it paused on as Stalled', () => {
    const met = waitingRecord({
      status: 'ended',
      statusReason: { kind: 'goal_met', n: 5, check: 'node scripts/validate_users.js' },
    });
    renderRail({ loop: asLoop(met) });
    fireEvent.click(screen.getByTestId('loop-rail-pill'));
    expect(within(row(5)).getByTestId('loop-tick-chip')).toHaveTextContent('Goal met');
    expect(within(row(3)).getByTestId('loop-tick-chip')).toHaveTextContent('Progress');
  });
});

interface SentenceCase {
  name: string;
  record: LoopRecord;
  status: LoopStatus;
  reason?: LoopStatusReason;
  now: string;
  utcOffsetMinutes: number;
  expect: { text: string };
}

describe('a chat a tick yielded to is named as it is called now (Q-279)', () => {
  it('in the tick row and in the NOW line, following a rename', async () => {
    vi.mocked(acpGetSessionListItem).mockResolvedValue({
      id: 's2',
      name: 'Simple pong reply',
    } as SessionListItem);
    const record = waitingRecord();
    const ticks = record.ticks!;
    const yielded = {
      ...record,
      status: 'waiting_turn' as const,
      statusReason: { kind: 'user_turn' as const, sessionId: 's2', chat: 'New Chat' },
      ticks: [
        ...ticks.slice(0, 4),
        {
          ...ticks[4],
          report: null,
          outcome: { kind: 'yielded' as const, toSession: 's2', toChat: 'New Chat' },
        },
      ],
    };
    renderRail({ loop: asLoop(yielded) });
    fireEvent.click(screen.getByTestId('loop-rail-pill'));
    const row5 = screen
      .getAllByTestId('loop-tick-row')
      .find((r) => r.getAttribute('data-tick') === '5')!;
    expect(
      await within(row5).findByText('Yielded to your turn in "Simple pong reply"')
    ).toBeTruthy();
    expect(screen.getByTestId('loop-now')).toHaveTextContent(
      'Tick 6 is due — it starts when your turn in "Simple pong reply" ends'
    );
    act(() => {
      window.dispatchEvent(
        new CustomEvent(AppEvents.SESSION_RENAMED, {
          detail: { sessionId: 's2', newName: 'Pong, renamed' },
        })
      );
    });
    expect(within(row5).getByText('Yielded to your turn in "Pong, renamed"')).toBeTruthy();
    expect(screen.getByTestId('loop-now')).toHaveTextContent('your turn in "Pong, renamed" ends');
    expect(screen.queryByText(/New Chat/)).not.toBeInTheDocument();
  });
});

describe('the NOW block, one case per status and reason of the fixture (§8.4)', () => {
  const cases = (raw as unknown as { sentences: SentenceCase[] }).sentences;

  it('says each state exactly as the fixture expects, with its own actions', () => {
    const actions: Partial<Record<string, string[]>> = {
      checking: ['Stop check'],
      waiting_you: ['Run next tick', 'Pause'],
      ended: ['Start a new loop'],
      elsewhere: ['Pause', 'Stop loop'],
    };
    for (const c of cases) {
      vi.setSystemTime(Date.parse(c.now));
      vi.mocked(Date.prototype.getTimezoneOffset).mockReturnValue(-c.utcOffsetMinutes);
      window.localStorage.clear();
      const { unmount } = renderRail({
        loop: asLoop(c.record, c.status, c.reason),
        messages: [],
      });
      fireEvent.click(screen.getByTestId('loop-rail-pill'));
      const now = screen.getByTestId('loop-now');
      // The fixture pins goosed's HH:MM; the rail says each time in the chat's clock (Q-316).
      expect(now.textContent, c.name).toBe(inChatClock(c.expect.text));
      const block = now.parentElement!;
      for (const label of actions[c.status] ?? []) {
        expect(
          within(block).getByRole('button', { name: label }),
          `${c.name}: ${label}`
        ).toBeInTheDocument();
      }
      if (c.status === 'paused' && c.reason?.kind === 'closed') {
        expect(
          within(block).getByRole('button', { name: 'Resume — run one tick now' })
        ).toBeInTheDocument();
      }
      unmount();
    }
  });
});
