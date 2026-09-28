import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
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
  type NeedsYouItemDto,
} from './sessionActivityStore';
import { getAnswerQueue, resetAnswerQueuesForTests } from './needsYouAnswerQueue';
import { foldKey } from './needsYouFold';
import { getPendingUserInput } from '../loops/pendingUserInput';

/**
 * Q-340 + Q-341, the owner's 3.0.69 screen: two stacked "Needs you" cards (1 of 2, 2 of 2) filled
 * the chat pane, their Submit/Dismiss row a sliver under the border, and nothing on them could be
 * picked while the prompt was being read.
 */
const LEAD: NeedsYouItemDto = {
  id: 'ny_lead',
  sessionId: 'jira',
  sessionName: 'Jira Migration Assessment',
  workingDir: '/w',
  question: 'An inactive project lead — which decision does the script give them?',
  why: 'The stated rules conflict on exactly that case.',
  recommendedAnswer: 'migrate — apply the lead override in every case',
  options: ["migrate + special 'lead-inactive' marker", 'skip unless they have an email'],
  createdAt: '2026-09-28T09:08:00Z',
  status: 'open',
};
const CSV: NeedsYouItemDto = {
  ...LEAD,
  id: 'ny_csv',
  question: 'Which delimiter should the review CSV use?',
  why: 'The owner opens it in Excel.',
  recommendedAnswer: 'A comma',
  options: ['A tab', 'A semicolon'],
  createdAt: '2026-09-28T09:09:00Z',
};

function renderTray(chatState: ChatState, sendAnswer = vi.fn()) {
  const view = render(
    <IntlProvider locale="en" messages={{}}>
      <NeedsYouTray sessionId="jira" chatState={chatState} sendAnswer={sendAnswer} />
    </IntlProvider>
  );
  const rerender = (next: ChatState) =>
    view.rerender(
      <IntlProvider locale="en" messages={{}}>
        <NeedsYouTray sessionId="jira" chatState={next} sendAnswer={sendAnswer} />
      </IntlProvider>
    );
  return { ...view, sendAnswer, rerenderWith: rerender };
}

const cards = () => screen.getAllByTestId('needs-you-card');
const card = (id: string) => cards().find((c) => c.getAttribute('data-item-id') === id)!;
const bodyOf = (el: HTMLElement) => {
  const band = within(el).getByTestId('needs-you-fold');
  return document.getElementById(band.getAttribute('aria-controls')!)!;
};

beforeEach(() => {
  window.localStorage.clear();
  acp.acpResolveNeedsYou.mockReset().mockImplementation(async (_s: string, id: string) => ({ id }));
  acp.acpSessionActivity.mockReset().mockResolvedValue({ running: [], needsYou: [], failed: [] });
});
afterEach(() => {
  resetSessionActivityForTests();
  resetAnswerQueuesForTests();
});

describe('Q-340: needs-you folds — each card to one line, the stack to one bar', () => {
  it('two questions: one bar says how many and the first question; folding it hides both cards, and it sticks per chat', () => {
    seedSessionActivityForTests({ needsYou: [LEAD, CSV] });
    const view = renderTray(ChatState.Idle);
    const bar = screen.getByTestId('needs-you-stack');
    expect(bar.tagName).toBe('BUTTON');
    expect(bar.textContent).toContain('Needs you · 2 questions');
    expect(screen.getByTestId('needs-you-stack-first').textContent).toBe(`— ${LEAD.question}`);
    expect(bar.getAttribute('aria-expanded')).toBe('true');
    const list = document.getElementById(bar.getAttribute('aria-controls')!)!;
    expect(list).toBe(screen.getByTestId('needs-you-list'));
    expect(list.hidden).toBe(false);

    fireEvent.click(bar);
    expect(bar.getAttribute('aria-expanded')).toBe('false');
    expect(list.hidden).toBe(true);
    expect(JSON.parse(window.localStorage.getItem(foldKey('jira'))!)).toMatchObject({
      stackFolded: true,
    });

    // Leaving the chat and coming back: still one bar.
    view.unmount();
    renderTray(ChatState.Idle);
    expect(screen.getByTestId('needs-you-stack').getAttribute('aria-expanded')).toBe('false');
    expect(screen.getByTestId('needs-you-list').hidden).toBe(true);
    // Another chat keeps its own.
    expect(window.localStorage.getItem(foldKey('other'))).toBeNull();
  });

  it('a card folds to one line: the hand, Needs you, the question cut to the line, "1 of 2" — and a typed answer survives the fold', () => {
    seedSessionActivityForTests({ needsYou: [LEAD, CSV] });
    renderTray(ChatState.Idle);
    const lead = card(LEAD.id);
    const band = within(lead).getByTestId('needs-you-fold');
    expect(band.tagName).toBe('BUTTON');
    expect(band.getAttribute('aria-expanded')).toBe('true');
    fireEvent.change(within(lead).getByTestId('needs-you-input'), {
      target: { value: 'migrate, but flag them' },
    });

    fireEvent.click(band);
    expect(lead.getAttribute('data-folded')).toBe('true');
    expect(band.getAttribute('aria-expanded')).toBe('false');
    expect(bodyOf(lead).hidden).toBe(true);
    const summary = within(lead).getByTestId('needs-you-fold-summary');
    expect(summary.textContent).toBe(LEAD.question);
    expect(summary.className).toContain('truncate');
    expect(band.textContent).toContain('Needs you');
    expect(band.textContent).toContain('1 of 2');
    // The other card is untouched (it arrived as a line — see the next test).
    expect(card(CSV.id).getAttribute('data-folded')).toBe('true');

    fireEvent.click(band);
    expect(bodyOf(lead).hidden).toBe(false);
    expect((within(lead).getByTestId('needs-you-input') as HTMLTextAreaElement).value).toBe(
      'migrate, but flag them'
    );
  });

  it('a new question opens the folded stack and itself, and the count moves', async () => {
    seedSessionActivityForTests({ needsYou: [LEAD, CSV] });
    renderTray(ChatState.Idle);
    fireEvent.click(within(card(LEAD.id)).getByTestId('needs-you-fold'));
    fireEvent.click(screen.getByTestId('needs-you-stack'));
    expect(screen.getByTestId('needs-you-list').hidden).toBe(true);

    const third = { ...CSV, id: 'ny_third', question: 'Keep the archived projects?' };
    act(() => seedSessionActivityForTests({ needsYou: [LEAD, CSV, third] }));
    const bar = screen.getByTestId('needs-you-stack');
    expect(bar.textContent).toContain('Needs you · 3 questions');
    expect(bar.getAttribute('aria-expanded')).toBe('true');
    expect(screen.getByTestId('needs-you-list').hidden).toBe(false);
    expect(card('ny_third').getAttribute('data-folded')).toBe('false');
    // What the person folded by hand stays folded.
    expect(card(LEAD.id).getAttribute('data-folded')).toBe('true');
    await waitFor(() =>
      expect(JSON.parse(window.localStorage.getItem(foldKey('jira'))!).seen).toContain('ny_third')
    );
  });

  it('the cause of the clipped action row: in the height cap only the question part of an open card scrolls — band and answer footer never shrink', () => {
    seedSessionActivityForTests({ needsYou: [LEAD, CSV] });
    renderTray(ChatState.Idle);
    const classesOf = (el: HTMLElement) => el.className.split(/\s+/);
    const list = classesOf(screen.getByTestId('needs-you-list'));
    expect(list).toEqual(expect.arrayContaining(['max-h-[45vh]', 'overflow-y-auto', 'flex-col']));
    // Folded: one line that never shrinks.
    expect(classesOf(card(CSV.id))).toContain('shrink-0');
    // Open: the card may shrink, its question part scrolls, band and footer hold.
    const open = card(LEAD.id);
    expect(classesOf(open)).toEqual(expect.arrayContaining(['flex', 'min-h-0', 'flex-col']));
    expect(classesOf(within(open).getByTestId('needs-you-fold'))).toContain('shrink-0');
    expect(classesOf(within(open).getByTestId('needs-you-card-scroll'))).toEqual(
      expect.arrayContaining(['min-h-0', 'overflow-y-auto'])
    );
    const footer = within(open).getByTestId('needs-you-card-footer');
    expect(classesOf(footer)).toContain('shrink-0');
    expect(within(footer).getByTestId('needs-you-answer')).toBeTruthy();
    expect(within(footer).getByTestId('needs-you-dismiss')).toBeTruthy();
    expect(within(footer).getByTestId('needs-you-input')).toBeTruthy();
  });

  it('one question has no stack bar — its own band folds it', () => {
    seedSessionActivityForTests({ needsYou: [LEAD] });
    renderTray(ChatState.Idle);
    expect(screen.queryByTestId('needs-you-stack')).toBeNull();
    expect(within(card(LEAD.id)).getByTestId('needs-you-fold').textContent).not.toContain('of');
  });

  it('questions arriving together: the stack opens, the first card opens, the rest arrive as their one line', () => {
    seedSessionActivityForTests({ needsYou: [LEAD, CSV] });
    renderTray(ChatState.Idle);
    expect(screen.getByTestId('needs-you-stack').getAttribute('aria-expanded')).toBe('true');
    expect(card(LEAD.id).getAttribute('data-folded')).toBe('false');
    expect(card(CSV.id).getAttribute('data-folded')).toBe('true');
    expect(within(card(CSV.id)).getByTestId('needs-you-fold-summary').textContent).toBe(
      CSV.question
    );
    // Opening it is one click on its line.
    fireEvent.click(within(card(CSV.id)).getByTestId('needs-you-fold'));
    expect(card(CSV.id).getAttribute('data-folded')).toBe('false');
  });

  it('an unreadable stored fold reads as new questions: the stack open, the first card open', () => {
    window.localStorage.setItem(foldKey('jira'), '{not json');
    seedSessionActivityForTests({ needsYou: [LEAD, CSV] });
    renderTray(ChatState.Idle);
    expect(screen.getByTestId('needs-you-stack').getAttribute('aria-expanded')).toBe('true');
    expect(card(LEAD.id).getAttribute('data-folded')).toBe('false');
  });
});

describe('Q-341: answering while the turn runs — queued, then sent when it ends', () => {
  it('a pick during the turn is queued: the card, its folded line and the bar say so; the tick door waits on it', () => {
    seedSessionActivityForTests({ needsYou: [LEAD, CSV] });
    const { sendAnswer } = renderTray(ChatState.Streaming);
    fireEvent.click(within(card(LEAD.id)).getByTestId('needs-you-recommended'));

    expect(acp.acpResolveNeedsYou).not.toHaveBeenCalled();
    expect(sendAnswer).not.toHaveBeenCalled();
    const queued = within(card(LEAD.id)).getByTestId('needs-you-queued');
    expect(queued.textContent).toContain('Queued · answers when this turn ends');
    expect(within(queued).getByTestId('needs-you-queued-answer').textContent).toBe(
      LEAD.recommendedAnswer
    );
    expect(queued.className).toContain('bg-lz-secondary');
    expect(within(card(LEAD.id)).getByTestId('needs-you-fold-queued').textContent).toBe('Queued');
    expect(screen.getByTestId('needs-you-stack-queued').textContent).toBe('1 queued');
    expect(getPendingUserInput('jira')).toBe(1);
    // The question stays open on every surface until the answer is really sent.
    expect(getSessionActivitySnapshot().needsYou.map((i) => i.id)).toEqual([LEAD.id, CSV.id]);
  });

  it('Cancel takes it back into the text box; a second pick replaces the first', () => {
    seedSessionActivityForTests({ needsYou: [LEAD] });
    renderTray(ChatState.Streaming);
    const lead = card(LEAD.id);
    fireEvent.click(within(lead).getAllByTestId('needs-you-option')[0]);
    fireEvent.click(within(lead).getAllByTestId('needs-you-option')[1]);
    expect(getAnswerQueue('jira').queued.map((q) => q.answer)).toEqual([LEAD.options[1]]);

    fireEvent.click(within(lead).getByTestId('needs-you-queued-cancel'));
    expect(getAnswerQueue('jira').queued).toEqual([]);
    expect(getPendingUserInput('jira')).toBe(0);
    expect(within(lead).queryByTestId('needs-you-queued')).toBeNull();
    expect((within(lead).getByTestId('needs-you-input') as HTMLTextAreaElement).value).toBe(
      LEAD.options[1]
    );
  });

  it('the turn ends: every queued answer is resolved first, then ONE message carries them all', async () => {
    seedSessionActivityForTests({ needsYou: [LEAD, CSV] });
    const { sendAnswer, rerenderWith } = renderTray(ChatState.Streaming);
    fireEvent.click(within(card(CSV.id)).getAllByTestId('needs-you-option')[0]);
    const input = within(card(LEAD.id)).getByTestId('needs-you-input');
    fireEvent.change(input, { target: { value: 'migrate, flag them' } });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(getAnswerQueue('jira').queued.map((q) => q.itemId)).toEqual([CSV.id, LEAD.id]);

    rerenderWith(ChatState.Idle);
    await waitFor(() => expect(sendAnswer).toHaveBeenCalledTimes(1));
    expect(acp.acpResolveNeedsYou.mock.calls).toEqual([
      ['jira', CSV.id, 'answer', 'A tab'],
      ['jira', LEAD.id, 'answer', 'migrate, flag them'],
    ]);
    // Both resolves ran before the one message.
    const sentAt = sendAnswer.mock.invocationCallOrder[0];
    for (const order of acp.acpResolveNeedsYou.mock.invocationCallOrder) {
      expect(order).toBeLessThan(sentAt);
    }
    expect(sendAnswer).toHaveBeenCalledWith(
      `Answer to your question "${CSV.question}": A tab\n\n` +
        `Answer to your question "${LEAD.question}": migrate, flag them`
    );
    expect(getAnswerQueue('jira')).toEqual({ queued: [], sending: [], unsent: [] });
    expect(getPendingUserInput('jira')).toBe(0);
  });

  it('dismiss never waits for the turn: it closes at once and drops a queued answer', async () => {
    seedSessionActivityForTests({ needsYou: [LEAD] });
    const { sendAnswer } = renderTray(ChatState.Streaming);
    fireEvent.click(within(card(LEAD.id)).getByTestId('needs-you-recommended'));
    fireEvent.click(within(card(LEAD.id)).getByTestId('needs-you-dismiss'));
    await waitFor(() =>
      expect(acp.acpResolveNeedsYou).toHaveBeenCalledWith('jira', LEAD.id, 'dismiss', undefined)
    );
    await waitFor(() => expect(getAnswerQueue('jira').queued).toEqual([]));
    expect(sendAnswer).not.toHaveBeenCalled();
  });

  it('a question closed while its answer waited says so at once, and nothing is sent for it', async () => {
    seedSessionActivityForTests({ needsYou: [LEAD] });
    const { sendAnswer, rerenderWith } = renderTray(ChatState.Streaming);
    fireEvent.click(within(card(LEAD.id)).getByTestId('needs-you-recommended'));
    act(() => seedSessionActivityForTests({ needsYou: [] }));
    const notice = screen.getByTestId('needs-you-unsent');
    expect(notice.textContent).toContain(
      'Not sent — this question was closed before your queued answer could go.'
    );
    expect(notice.textContent).toContain(`Your answer: ${LEAD.recommendedAnswer}`);
    expect(notice.className).toContain('bg-lz-err-solid');

    rerenderWith(ChatState.Idle);
    await waitFor(() => expect(getAnswerQueue('jira').queued).toEqual([]));
    expect(acp.acpResolveNeedsYou).not.toHaveBeenCalled();
    expect(sendAnswer).not.toHaveBeenCalled();
    expect(screen.getByTestId('needs-you-unsent')).toBeTruthy();
    fireEvent.click(screen.getByTestId('needs-you-unsent-close'));
    expect(screen.queryByTestId('needs-you-tray')).toBeNull();
  });

  it('a save that fails when the turn ends is shown with the answer, and nothing is sent', async () => {
    acp.acpResolveNeedsYou.mockRejectedValue(new Error('engine gone'));
    seedSessionActivityForTests({ needsYou: [LEAD] });
    const { sendAnswer, rerenderWith } = renderTray(ChatState.Streaming);
    fireEvent.click(within(card(LEAD.id)).getByTestId('needs-you-recommended'));
    rerenderWith(ChatState.Idle);
    const notice = await screen.findByTestId('needs-you-unsent');
    expect(notice.textContent).toContain(
      'Not sent — your queued answer could not be saved: engine gone'
    );
    expect(sendAnswer).not.toHaveBeenCalled();
    // The card is still there to answer again.
    expect(card(LEAD.id)).toBeTruthy();
  });

  it('while a stop is being settled the queued answer keeps waiting', () => {
    seedSessionActivityForTests({ needsYou: [LEAD] });
    const sendAnswer = vi.fn();
    const view = render(
      <IntlProvider locale="en" messages={{}}>
        <NeedsYouTray sessionId="jira" chatState={ChatState.Streaming} sendAnswer={sendAnswer} />
      </IntlProvider>
    );
    fireEvent.click(within(card(LEAD.id)).getByTestId('needs-you-recommended'));
    view.rerender(
      <IntlProvider locale="en" messages={{}}>
        <NeedsYouTray
          sessionId="jira"
          chatState={ChatState.Idle}
          sendBlocked
          sendAnswer={sendAnswer}
        />
      </IntlProvider>
    );
    expect(acp.acpResolveNeedsYou).not.toHaveBeenCalled();
    expect(getAnswerQueue('jira').queued).toHaveLength(1);
  });
});
