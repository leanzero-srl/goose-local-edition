import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { IntlProvider } from 'react-intl';
import { createUserMessage, type Message } from '../../types/message';
import { mlxRemoteSingleStatus } from '../../acp/mlx-remote-single';
import { assertStudioClean } from '../lz/assertStudioClean';
import GooseMessage from '../GooseMessage';
import { splitLinkDrop } from './parseLinkDrop';
import { resetDropNamesForTests } from './dropNames';

const mockExtMethod = vi.fn();
vi.mock('../../acp/acpConnection', () => ({
  getAcpClient: async () => ({ extMethod: mockExtMethod }),
}));

const RETRY_LINE = '\n\nPlease retry if you think this is a transient or recoverable error.';
const PEER = 'worksmacstudio-lan-6a972f';
const MAC_NAME = "Work's Mac Studio";
/** recovery-kill-link, 23 s (after-error.png): the story, then the relay's error, glued. */
const KILL_LINK_ANSWER =
  'The light out Harrow Rock was supposed to go out with him.\n\nIn the morning he did what he’d always done. Wound. Clean';
const KILL_LINK_ERROR = `Ran into this error: Server error: linkRelayFailed: Link peer '${PEER}' lost this request in flight: the peer answers but no longer holds it (2 looks in a row) — it was dropped on the peer's side, as when its LeanZero Link restarts.${RETRY_LINE}`;
/** recovery-relaunch-peer, 22 s (s-024.png, Q-49). */
const RELAUNCH_ANSWER = 'He climbed the stairs one more time to listen, and';
const RELAUNCH_ERROR = KILL_LINK_ERROR;
/** The relay's other form: the request never reached the peer (inference.rs `unreachable_peer`). */
const UNREACHABLE_ERROR = `Ran into this error: Server error: linkRelayFailed: cannot reach Link peer '${PEER}': 2 looks in a row could not reach the peer through the mesh.${RETRY_LINE}`;

describe('splitLinkDrop — the two recorded drops, split off the partial answer', () => {
  it('kill-link: the answer is kept verbatim up to "Clean"; the error is the rest, the peer named', () => {
    const drop = splitLinkDrop(KILL_LINK_ANSWER + KILL_LINK_ERROR);
    expect(drop).toEqual({
      answer: KILL_LINK_ANSWER,
      raw: KILL_LINK_ERROR.trim(),
      peerId: PEER,
      inFlight: true,
      cause: null,
      macName: null,
    });
  });

  it('relaunch-peer: "…listen, andRan into this error…" splits at the wrap', () => {
    const drop = splitLinkDrop(RELAUNCH_ANSWER + RELAUNCH_ERROR);
    expect(drop?.answer).toBe(RELAUNCH_ANSWER);
    expect(drop?.raw.startsWith('Ran into this error: Server error: linkRelayFailed')).toBe(true);
  });

  it('the unreachable form, and the error alone as its own message', () => {
    expect(splitLinkDrop(UNREACHABLE_ERROR)).toMatchObject({
      answer: '',
      peerId: PEER,
      inFlight: false,
    });
  });

  it('any other trailing error, or an answer that merely mentions the relay, is not a Link drop', () => {
    expect(
      splitLinkDrop(`story${'Ran into this error: Server error: boom.'}${RETRY_LINE}`)
    ).toBeNull();
    expect(splitLinkDrop('the relay says linkRelayFailed sometimes')).toBeNull();
  });
});

function assistant(text: string | string[]): Message {
  const parts = Array.isArray(text) ? text : [text];
  return {
    id: 'a1',
    role: 'assistant',
    created: 2,
    content: parts.map((t) => ({ type: 'text' as const, text: t })),
    metadata: { userVisible: true, agentVisible: true },
  };
}

function show(message: Message, append = vi.fn(), trailing: Message[] = []) {
  const userTurn = createUserMessage('Write a 300-word story about a lighthouse keeper. No tools.');
  const utils = render(
    <IntlProvider locale="en" defaultLocale="en" messages={{}}>
      <MemoryRouter>
        <GooseMessage
          sessionId="s1"
          message={message}
          messages={[userTurn, message, ...trailing]}
          toolCallNotifications={new Map()}
          append={append}
          isStreaming={false}
        />
      </MemoryRouter>
    </IntlProvider>
  );
  return { ...utils, append };
}

describe('GooseMessage — a turn the serving Mac dropped (Q-49)', () => {
  beforeEach(() => resetDropNamesForTests());
  afterEach(async () => {
    mockExtMethod.mockResolvedValue({ status: { state: 'off' } });
    await act(async () => {
      await mlxRemoteSingleStatus();
    });
  });

  it('renders the partial answer as written, then the notice naming the Mac — the jargon only behind Details', async () => {
    mockExtMethod.mockResolvedValue({
      status: {
        state: 'ready',
        peer: PEER,
        peerHostname: 'WorksMacStudio.lan',
        peerComputerName: "Work's Mac Studio",
      },
    });
    await act(async () => {
      await mlxRemoteSingleStatus();
    });
    // The agent loop appends the error as a second text part: concatenated, it glued ("CleanRan").
    const { append } = show(assistant([KILL_LINK_ANSWER, KILL_LINK_ERROR]));
    expect(screen.getByText(/Wound\. Clean$/)).toBeInTheDocument();
    const notice = screen.getByTestId('link-drop-notice');
    expect(notice.textContent).toContain(
      "Work's Mac Studio stopped answering mid-reply — the answer above stops there."
    );
    // The error's words live only behind Details — never in the answer.
    expect(screen.getAllByText(/Ran into this error/).map((el) => el.dataset.testid)).toEqual([
      'link-drop-raw',
    ]);
    expect(screen.getByTestId('link-drop-raw')).not.toBeVisible();
    await userEvent.click(screen.getByRole('button', { name: /Details/ }));
    expect(screen.getByTestId('link-drop-raw')).toBeVisible();
    expect(screen.getByTestId('link-drop-raw').textContent).toBe(KILL_LINK_ERROR.trim());
    await userEvent.click(screen.getByTestId('link-drop-retry'));
    expect(append).toHaveBeenCalledWith(
      'Write a 300-word story about a lighthouse keeper. No tools.'
    );
    // The notice itself (GooseMessage's own timestamp carries a hover fade that predates it).
    assertStudioClean(notice);
  });

  it('a node id the route does not name is "The linked Mac" — never the id', () => {
    show(assistant(RELAUNCH_ANSWER + RELAUNCH_ERROR));
    const headline = screen.getByTestId('link-drop-headline');
    expect(headline.textContent).toBe(
      'The linked Mac stopped answering mid-reply — the answer above stops there.'
    );
  });

  it('the error as its own message: no answer above, so the notice says the turn got none', () => {
    show(assistant(UNREACHABLE_ERROR));
    expect(screen.getByTestId('link-drop-notice').textContent).toContain(
      'The linked Mac stopped answering — this turn got no answer.'
    );
    expect(screen.getByTestId('link-drop-retry')).toBeInTheDocument();
  });

  it('an older message (not the chat’s latest) offers no Retry', () => {
    show(assistant(RELAUNCH_ANSWER + RELAUNCH_ERROR), vi.fn(), [createUserMessage('another turn')]);
    expect(screen.getByTestId('link-drop-notice')).toBeInTheDocument();
    expect(screen.queryByTestId('link-drop-retry')).toBeNull();
  });

  it('Q-54: a Mac that restarted goose mid-answer is named so — from the drop’s own words', () => {
    const restart = `Ran into this error: Server error: linkRelayFailed: Link peer '${PEER}' lost this request in flight: Work's Mac Studio is restarting goose.${RETRY_LINE}`;
    expect(splitLinkDrop(restart)).toMatchObject({ cause: 'restart', macName: MAC_NAME });
    show(assistant(RELAUNCH_ANSWER + restart));
    expect(screen.getByTestId('link-drop-headline').textContent).toBe(
      "Work's Mac Studio restarted goose mid-answer — the answer above stops there."
    );
  });

  it('Q-62: the name is stored with the drop — a later route change does not rename it', async () => {
    mockExtMethod.mockResolvedValue({
      status: { state: 'ready', peer: PEER, peerComputerName: MAC_NAME },
    });
    await act(async () => {
      await mlxRemoteSingleStatus();
    });
    const first = show(assistant(RELAUNCH_ANSWER + RELAUNCH_ERROR));
    expect(screen.getByTestId('link-drop-headline').textContent).toContain(MAC_NAME);
    first.unmount();
    // The route moves to another Mac (or ends): the old notice keeps its Mac.
    mockExtMethod.mockResolvedValue({ status: { state: 'off' } });
    await act(async () => {
      await mlxRemoteSingleStatus();
    });
    show(assistant(RELAUNCH_ANSWER + RELAUNCH_ERROR));
    expect(screen.getByTestId('link-drop-headline').textContent).toBe(
      "Work's Mac Studio stopped answering mid-reply — the answer above stops there."
    );
  });
});
