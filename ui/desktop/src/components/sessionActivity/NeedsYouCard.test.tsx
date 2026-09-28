import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { IntlProvider } from 'react-intl';

const acp = vi.hoisted(() => ({
  acpSessionActivity: vi.fn(),
  acpResolveNeedsYou: vi.fn(),
}));
vi.mock('../../acp/needsYou', () => acp);

import NeedsYouTray from './NeedsYouCard';
import { ChatState } from '../../types/chatState';
import {
  getSessionActivitySnapshot,
  resetSessionActivityForTests,
  seedSessionActivityForTests,
} from './sessionActivityStore';
import type { AcpElicitationRequest } from '../../acp/elicitationRequests';
import { resetAnswerQueuesForTests } from './needsYouAnswerQueue';

const ITEM = {
  id: 'ny_1',
  sessionId: 'sess-1',
  sessionName: 'Service setup',
  workingDir: '/proj/api',
  question: 'Which database should the service use?',
  why: 'The schema and the migration tool depend on it.',
  recommendedAnswer: 'PostgreSQL',
  options: ['SQLite', 'MySQL'],
  createdAt: '2026-09-26T10:00:00Z',
  status: 'open' as const,
};

function renderTray(
  chatState = ChatState.Idle,
  submitElicitationResponse?: () => Promise<boolean>
) {
  const sendAnswer = vi.fn();
  render(
    <IntlProvider locale="en" messages={{}}>
      <NeedsYouTray
        sessionId="sess-1"
        chatState={chatState}
        sendAnswer={sendAnswer}
        submitElicitationResponse={submitElicitationResponse}
      />
    </IntlProvider>
  );
  return sendAnswer;
}

describe('NeedsYouTray — pinned above the composer until answered or dismissed', () => {
  beforeEach(() => {
    acp.acpResolveNeedsYou.mockReset().mockResolvedValue(ITEM);
    acp.acpSessionActivity.mockReset().mockResolvedValue({ running: [], needsYou: [], failed: [] });
    seedSessionActivityForTests({ needsYou: [ITEM] });
  });
  afterEach(() => {
    resetSessionActivityForTests();
    resetAnswerQueuesForTests();
  });

  it('says what is needed and why, offers the recommended answer, the options and a free field', () => {
    renderTray();
    const card = screen.getByTestId('needs-you-card');
    expect(within(card).getByText('Needs you')).toBeTruthy();
    expect(screen.getByTestId('needs-you-question').textContent).toBe(ITEM.question);
    expect(screen.getByTestId('needs-you-why').textContent).toBe(ITEM.why);
    expect(screen.getByTestId('needs-you-recommended').textContent).toContain('PostgreSQL');
    expect(screen.getAllByTestId('needs-you-option').map((o) => o.textContent)).toEqual([
      'SQLite',
      'MySQL',
    ]);
    expect(screen.getByTestId('needs-you-input')).toBeTruthy();
    // Solid, never a rail or a wash: a full amber border and a solid amber band.
    expect(card.className).toContain('border-2');
    expect(card.className).toContain('border-lz-warn-solid');
    expect(card.className).not.toMatch(/border-l-/);
  });

  it('one click on the recommended answer closes the item, THEN sends the answer to the model, marked as that item’s answer (Q-344)', async () => {
    const sendAnswer = renderTray();
    fireEvent.click(screen.getByTestId('needs-you-recommended'));
    await waitFor(() => expect(sendAnswer).toHaveBeenCalledTimes(1));
    expect(acp.acpResolveNeedsYou).toHaveBeenCalledWith('sess-1', 'ny_1', 'answer', 'PostgreSQL');
    expect(sendAnswer).toHaveBeenCalledWith(
      'Answer to your question "Which database should the service use?": PostgreSQL',
      ['ny_1']
    );
    expect(getSessionActivitySnapshot().needsYou).toEqual([]);
  });

  it('an option chip answers in one click; a typed answer goes on Enter', async () => {
    // The engine keeps listing the item, so the card is back after the first answer.
    acp.acpSessionActivity.mockResolvedValue({ running: [], needsYou: [ITEM], failed: [] });
    const sendAnswer = renderTray();
    fireEvent.click(screen.getAllByTestId('needs-you-option')[0]);
    await waitFor(() => expect(sendAnswer).toHaveBeenCalledTimes(1));
    expect(sendAnswer.mock.calls[0][0]).toContain(': SQLite');
    expect(sendAnswer.mock.calls[0][1]).toEqual(['ny_1']);

    const input = await screen.findByTestId('needs-you-input');
    await waitFor(() => expect((input as HTMLTextAreaElement).disabled).toBe(false));
    fireEvent.change(input, { target: { value: 'Use DuckDB' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    await waitFor(() => expect(sendAnswer).toHaveBeenCalledTimes(2));
    expect(sendAnswer.mock.calls[1][0]).toContain(': Use DuckDB');
    expect(sendAnswer.mock.calls[1][1]).toEqual(['ny_1']);
  });

  it('dismiss closes the item and sends nothing', async () => {
    const sendAnswer = renderTray();
    fireEvent.click(screen.getByTestId('needs-you-dismiss'));
    await waitFor(() =>
      expect(acp.acpResolveNeedsYou).toHaveBeenCalledWith('sess-1', 'ny_1', 'dismiss', undefined)
    );
    expect(sendAnswer).not.toHaveBeenCalled();
    await waitFor(() => expect(screen.queryByTestId('needs-you-card')).toBeNull());
  });

  it('a failed save keeps the card and the typed answer, and sends nothing', async () => {
    acp.acpResolveNeedsYou.mockRejectedValue(new Error('engine gone'));
    const sendAnswer = renderTray();
    const input = screen.getByTestId('needs-you-input') as HTMLTextAreaElement;
    fireEvent.change(input, { target: { value: 'Use DuckDB' } });
    fireEvent.click(screen.getByTestId('needs-you-answer'));
    await screen.findByRole('alert');
    expect(sendAnswer).not.toHaveBeenCalled();
    expect(screen.getByTestId('needs-you-card')).toBeTruthy();
    expect(input.value).toBe('Use DuckDB');
  });

  it('while goose is working an answer is taken and QUEUED (Q-341): nothing resolves or sends yet, and the card says so', () => {
    const sendAnswer = renderTray(ChatState.Streaming);
    const recommended = screen.getByTestId('needs-you-recommended') as HTMLButtonElement;
    expect(recommended.disabled).toBe(false);
    expect((screen.getByTestId('needs-you-dismiss') as HTMLButtonElement).disabled).toBe(false);
    expect(screen.getByTestId('needs-you-busy').textContent).toBe(
      'goose is working — an answer you give now waits and is sent when this turn ends.'
    );
    fireEvent.click(recommended);
    expect(acp.acpResolveNeedsYou).not.toHaveBeenCalled();
    expect(sendAnswer).not.toHaveBeenCalled();
    expect(screen.getByTestId('needs-you-queued').textContent).toContain(
      'Queued · answers when this turn ends'
    );
  });

  it('shows nothing for another session, and a live MCP elicitation surfaces pinned too', () => {
    const elicitation = {
      id: 'acp_elicitation_1',
      sessionId: 'sess-1',
      request: {
        mode: 'form',
        sessionId: 'sess-1',
        message: 'Choose a project',
        requestedSchema: { type: 'object', properties: {} },
      },
    } as unknown as AcpElicitationRequest;
    seedSessionActivityForTests({
      needsYou: [{ ...ITEM, sessionId: 'other' }],
      elicitations: [elicitation],
    });
    renderTray(ChatState.Idle, vi.fn().mockResolvedValue(true));
    expect(screen.queryByTestId('needs-you-card')).toBeNull();
    const pinned = screen.getByTestId('needs-you-elicitation');
    expect(within(pinned).getByText('Choose a project')).toBeTruthy();
  });
});
