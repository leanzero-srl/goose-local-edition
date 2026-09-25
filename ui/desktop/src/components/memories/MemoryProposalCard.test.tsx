import { describe, expect, it, vi, beforeEach } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { ChatState } from '../../types/chatState';
import type { MemoryProposalDto } from '../../acp/proposals';

const list = vi.fn();
const answer = vi.fn();
vi.mock('../../acp/proposals', () => ({
  acpListMemoryProposals: (...a: unknown[]) => list(...a),
  acpAnswerMemoryProposal: (...a: unknown[]) => answer(...a),
}));

import MemoryProposalCards, { POLARITY_FILL } from './MemoryProposalCard';

const proposal = (over: Partial<MemoryProposalDto> = {}): MemoryProposalDto => ({
  id: 'p-1-1',
  key: 'sess-1',
  kind: 'memory',
  polarity: 'negative',
  text: 'The retry lives in src/http/retry.js, not in the call sites.',
  why: 'the user corrected the same mistake three times',
  category: 'lessons',
  tags: ['feedback'],
  isGlobal: false,
  sources: [],
  createdAt: 1,
  state: 'open',
  ...over,
});

const mount = () =>
  render(
    <IntlTestWrapper>
      <MemoryProposalCards sessionId="sess-1" chatState={ChatState.Idle} />
    </IntlTestWrapper>
  );

beforeEach(() => {
  list.mockReset();
  answer.mockReset();
  answer.mockResolvedValue({ proposal: null, outcome: 'added' });
});

describe('MemoryProposalCards — J8, the card under the last message', () => {
  it('renders the verdict as a SOLID chip in the memories hue, the text, the why, and Save / No / Edit', async () => {
    list.mockResolvedValue([proposal()]);
    mount();
    const card = await screen.findByTestId('memory-proposal-card');
    expect(card.textContent).toContain('Save this as a memory?');
    const chip = screen.getByTestId('memory-proposal-polarity');
    expect(chip.textContent).toBe('Negative');
    expect(chip.style.backgroundColor).toBe('rgb(180, 83, 9)');
    expect(POLARITY_FILL.negative).toBe('#b45309');
    expect(POLARITY_FILL.positive).toBe('#0f766e');
    expect(card.textContent).toContain('Why: the user corrected the same mistake three times');
    expect(screen.getByTestId('memory-proposal-save')).toBeTruthy();
    expect(screen.getByTestId('memory-proposal-no')).toBeTruthy();
    expect(screen.getByTestId('memory-proposal-edit')).toBeTruthy();
    // No rail, no tint: the card is the plain surface card and the chip is a solid fill.
    expect(card.className).not.toMatch(/border-l-/);
  });

  it('Save answers with save and the card leaves; No answers with decline and writes nothing else', async () => {
    list.mockResolvedValue([proposal(), proposal({ id: 'p-1-2', text: 'second' })]);
    mount();
    const saves = await screen.findAllByTestId('memory-proposal-save');
    fireEvent.click(saves[0]);
    await waitFor(() =>
      expect(answer).toHaveBeenCalledWith(
        'sess-1',
        expect.objectContaining({ id: 'p-1-1', key: 'sess-1' }),
        'save',
        undefined
      )
    );
    await waitFor(() => expect(screen.getAllByTestId('memory-proposal-card')).toHaveLength(1));
    fireEvent.click(screen.getByTestId('memory-proposal-no'));
    await waitFor(() =>
      expect(answer).toHaveBeenLastCalledWith(
        'sess-1',
        expect.objectContaining({ id: 'p-1-2' }),
        'decline',
        undefined
      )
    );
    await waitFor(() => expect(screen.queryByTestId('memory-proposal-card')).toBeNull());
  });

  it('Edit opens the text; Save then sends the edited text', async () => {
    list.mockResolvedValue([proposal()]);
    mount();
    fireEvent.click(await screen.findByTestId('memory-proposal-edit'));
    const box = screen.getByTestId('memory-proposal-text') as HTMLTextAreaElement;
    fireEvent.change(box, { target: { value: 'shorter' } });
    fireEvent.click(screen.getByTestId('memory-proposal-save'));
    await waitFor(() =>
      expect(answer).toHaveBeenCalledWith('sess-1', expect.anything(), 'save', 'shorter')
    );
  });

  it('an expired proposal reads "expired — not saved" and offers no Save', async () => {
    list.mockResolvedValue([proposal({ state: 'expired' })]);
    mount();
    const card = await screen.findByTestId('memory-proposal-card');
    expect(card.textContent).toContain('Expired — not saved');
    expect(screen.queryByTestId('memory-proposal-save')).toBeNull();
  });

  it('nothing renders when there is nothing open, and a failed read leaves the transcript alone', async () => {
    list.mockRejectedValue(new Error('acp down'));
    const { container } = mount();
    await waitFor(() => expect(list).toHaveBeenCalled());
    expect(container.querySelector('[data-testid="memory-proposals"]')).toBeNull();
  });
});

describe('MemoryProposalCards — where a card came from (Q-82)', () => {
  /** E2E #2: run #1's lookup at 22:04, filed under the project, listed at the bottom of run #2. */
  const atlassian = proposal({
    id: 'p-1790104-1',
    key: 'wd-fa718b6d3b269f16',
    kind: 'knowledge',
    polarity: undefined,
    text: 'Atlassian Data Center End of Life timeline (official): EOL = 28 Mar 2029',
    why: 'grounded by a lookup this turn',
    category: 'atlassian-migration',
    sources: ['https://www.atlassian.com/licensing/data-center-end-of-life'],
    createdAt: Date.UTC(2026, 8, 25, 19, 4) / 1000,
  });

  it('a card filed for the whole project says so, and when — never passes as this chat’s own', async () => {
    list.mockResolvedValue([atlassian]);
    mount();
    const origin = await screen.findByTestId('memory-proposal-origin');
    expect(origin.textContent).toMatch(/^From a chat in this project · Sep 25, /);
  });

  it('a card this chat asked for carries no origin line', async () => {
    list.mockResolvedValue([{ ...atlassian, key: 'sess-1' }]);
    mount();
    await screen.findByTestId('memory-proposal-card');
    expect(screen.queryByTestId('memory-proposal-origin')).toBeNull();
  });
});
