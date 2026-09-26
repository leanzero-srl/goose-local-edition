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

function renderTray(chatState = ChatState.Idle, submitElicitationResponse?: () => Promise<boolean>) {
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
  afterEach(() => resetSessionActivityForTests());

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

  it('one click on the recommended answer closes the item, THEN sends the answer to the model', async () => {
    const sendAnswer = renderTray();
    fireEvent.click(screen.getByTestId('needs-you-recommended'));
    await waitFor(() => expect(sendAnswer).toHaveBeenCalledTimes(1));
    expect(acp.acpResolveNeedsYou).toHaveBeenCalledWith('sess-1', 'ny_1', 'answer', 'PostgreSQL');
    expect(sendAnswer).toHaveBeenCalledWith(
      'Answer to your question "Which database should the service use?": PostgreSQL'
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

    const input = await screen.findByTestId('needs-you-input');
    await waitFor(() => expect((input as HTMLTextAreaElement).disabled).toBe(false));
    fireEvent.change(input, { target: { value: 'Use DuckDB' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    await waitFor(() => expect(sendAnswer).toHaveBeenCalledTimes(2));
    expect(sendAnswer.mock.calls[1][0]).toContain(': Use DuckDB');
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

  it('while goose is working the answers wait, and the card says so', () => {
    renderTray(ChatState.Streaming);
    expect((screen.getByTestId('needs-you-recommended') as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTestId('needs-you-dismiss') as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText(/goose is working/)).toBeTruthy();
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
    seedSessionActivityForTests({ needsYou: [{ ...ITEM, sessionId: 'other' }], elicitations: [elicitation] });
    renderTray(ChatState.Idle, vi.fn().mockResolvedValue(true));
    expect(screen.queryByTestId('needs-you-card')).toBeNull();
    const pinned = screen.getByTestId('needs-you-elicitation');
    expect(within(pinned).getByText('Choose a project')).toBeTruthy();
  });
});
