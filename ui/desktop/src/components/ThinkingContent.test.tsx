import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { IntlTestWrapper } from '../i18n/test-utils';
import ThinkingContent from './ThinkingContent';
import { getThinkingContent, type Message } from '../types/message';

// Q-100 (E2E #1/#2 turn 0): "Thinking" rows were grey (#878787) italic — the faded look the house
// bans — and some opened onto nothing.

function assistant(content: Message['content']): Message {
  return {
    role: 'assistant',
    created: 0,
    content,
    metadata: { userVisible: true, agentVisible: true },
  };
}

describe('Thinking rows (Q-100)', () => {
  it('a whitespace-only reasoning block gets no row', () => {
    expect(
      getThinkingContent(assistant([{ type: 'thinking', thinking: '\n\n', signature: '' }]))
    ).toBeNull();
    expect(
      getThinkingContent(
        assistant([
          { type: 'thinking', thinking: ' ', signature: '' },
          { type: 'thinking', thinking: '\n', signature: '' },
        ])
      )
    ).toBeNull();
  });

  it('a real reasoning block keeps its row and its words', () => {
    expect(
      getThinkingContent(
        assistant([
          { type: 'thinking', thinking: 'Empty work dir — creating the notes file', signature: '' },
        ])
      )
    ).toBe('Empty work dir — creating the notes file');
  });

  it('the row and its body are solid ink and upright, never grey italic', () => {
    render(<ThinkingContent content="Parse the raw notes first." isExpanded />, {
      wrapper: IntlTestWrapper,
    });
    const label = screen.getByText('Thinking');
    const trigger = label.closest('button') ?? label.parentElement!;
    expect(trigger.className).toContain('text-lz-ink-2');
    expect(trigger.className).not.toContain('text-text-secondary');
    expect(label.className).not.toContain('italic');
    const body = screen.getByText('Parse the raw notes first.');
    expect(body.closest('.italic')).toBeNull();
    expect(body.closest('.text-lz-ink-2')).not.toBeNull();
  });
});
