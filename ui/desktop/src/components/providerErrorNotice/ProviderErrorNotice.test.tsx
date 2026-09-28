import type { SessionNotification } from '@agentclientprotocol/sdk';
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { describe, expect, it, vi } from 'vitest';
import { IntlProvider } from 'react-intl';
import { createUserMessage, type Message, type ProviderErrorNotice } from '../../types/message';
import { createAcpSessionNotificationAdapter } from '../../acp/sessionNotificationAdapter';
import GooseMessage from '../GooseMessage';
import { TONE_FILL } from '../lz';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';

vi.mock('../../acp/acpConnection', () => ({
  getAcpClient: async () => ({ extMethod: vi.fn() }),
}));

const PERMANENT_CLOSER =
  'Sending the same request again will fail the same way until its cause is fixed.';
const TRANSIENT_CLOSER = 'Please retry if you think this is a transient or recoverable error.';

/** The critic's session on 3.0.68 (70-failed-session.png), as agents/agent.rs now writes it. */
const REFUSED: ProviderErrorNotice = {
  class: 'request',
  transient: false,
  said: "Only 'text' content type is supported.",
  detail:
    "Request failed: Resource not found (404) at http://127.0.0.1:8091/v1/chat/completions: Only 'text' content type is supported.",
};
const REFUSED_TEXT = `Ran into this error: ${REFUSED.detail}.\n\n${PERMANENT_CLOSER}`;

const BUSY: ProviderErrorNotice = {
  class: 'server',
  transient: true,
  said: 'Server is busy (max concurrent requests reached)',
  detail:
    'Server error: Server error (503 Service Unavailable) at http://127.0.0.1:8090/v1/chat/completions: Server is busy (max concurrent requests reached)',
};
const BUSY_TEXT = `Ran into this error: ${BUSY.detail}.\n\n${TRANSIENT_CLOSER}`;

function chunk(text: string, providerError?: unknown): SessionNotification {
  return {
    sessionId: 's1',
    update: {
      sessionUpdate: 'agent_message_chunk',
      content: { type: 'text', text },
      _meta: { goose: { messageId: 'err-1', created: 2, providerError } },
    } as SessionNotification['update'],
  };
}

function messageOf(n: SessionNotification): Message {
  const change = createAcpSessionNotificationAdapter()
    .apply(n)
    .find((c) => c.type === 'messages');
  if (!change || change.type !== 'messages') throw new Error('no messages change');
  return change.messages[0];
}

function show(message: Message, append = vi.fn()) {
  const userTurn = createUserMessage('Crop this image and tell me what the tool returned.');
  render(
    <IntlProvider locale="en" defaultLocale="en" messages={{}}>
      <MemoryRouter>
        <GooseMessage
          sessionId="s1"
          message={message}
          messages={[userTurn, message]}
          toolCallNotifications={new Map()}
          append={append}
          isStreaming={false}
        />
      </MemoryRouter>
    </IntlProvider>
  );
  return append;
}

describe('Q-302: a provider error is a notice in the error colour, in plain words', () => {
  it('the adapter keeps the notice goose put beside the chunk; a partial one is no notice', () => {
    expect(messageOf(chunk(REFUSED_TEXT, REFUSED)).metadata.providerError).toEqual(REFUSED);
    expect(messageOf(chunk('Hello')).metadata.providerError).toBeUndefined();
    expect(
      messageOf(chunk(REFUSED_TEXT, { class: 'request', said: 'x' })).metadata.providerError
    ).toBeUndefined();
  });

  it('a permanent refusal: the engine’s sentence, no retry advice, the endpoint only in Details', async () => {
    const append = show(messageOf(chunk(REFUSED_TEXT, REFUSED)));
    const notice = screen.getByTestId('provider-error-notice');
    expect(notice.getAttribute('role')).toBe('alert');
    expect(within(notice).getByTestId('provider-error-headline').textContent).toBe(
      'The model’s server refused this request'
    );
    expect(within(notice).getByTestId('provider-error-said').textContent).toBe(
      "Only 'text' content type is supported."
    );
    expect(within(notice).getByTestId('provider-error-advice').textContent).toBe(PERMANENT_CLOSER);
    expect(within(notice).queryByTestId('provider-error-retry')).toBeNull();
    // The raw wrapper, URL and closer are not on the page outside Details.
    expect(document.body.textContent).not.toContain('Ran into this error');
    expect(document.body.textContent).not.toContain('Please retry');
    const detail = screen.getByTestId('provider-error-detail');
    expect(detail.closest('[hidden]')).not.toBeNull();
    await userEvent.click(screen.getByText('Details'));
    expect(detail.closest('[hidden]')).toBeNull();
    expect(detail.textContent).toBe(REFUSED.detail);
    // Painted in the error colour: a solid fill, not grey text.
    const band = screen.getByTestId('provider-error-headline').parentElement!;
    for (const c of TONE_FILL.err.split(' ')) expect(band.className).toContain(c);
    assertStudioClean(notice);
    const classes = allClasses(notice).filter((c) => !c.startsWith('lucide'));
    expect(await missingUtilities(classes)).toEqual([]);
    expect(append).not.toHaveBeenCalled();
  }, 30_000);

  it('a transient failure advises sending again and offers Retry of the last turn', async () => {
    const append = show(messageOf(chunk(BUSY_TEXT, BUSY)));
    expect(screen.getByTestId('provider-error-headline').textContent).toBe(
      'The model’s server failed while answering'
    );
    expect(screen.getByTestId('provider-error-advice').textContent).toBe(
      'This can pass on its own — send it again in a moment.'
    );
    await userEvent.click(screen.getByTestId('provider-error-retry'));
    expect(append).toHaveBeenCalledWith('Crop this image and tell me what the tool returned.');
  });

  it('the model’s partial answer before the error renders as written, the notice below it', () => {
    const answer = 'The folder holds one file, panel.png. ';
    show(messageOf(chunk(answer + REFUSED_TEXT, REFUSED)));
    expect(document.body.textContent).toContain('The folder holds one file, panel.png.');
    expect(screen.getByTestId('provider-error-notice')).toBeTruthy();
    expect(document.body.textContent).not.toContain('Ran into this error');
  });

  it('a message with no notice is untouched text', () => {
    show(messageOf(chunk(REFUSED_TEXT)));
    expect(screen.queryByTestId('provider-error-notice')).toBeNull();
    expect(document.body.textContent).toContain('Ran into this error');
  });
});
