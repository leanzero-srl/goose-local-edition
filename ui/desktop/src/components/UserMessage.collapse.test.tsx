import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { IntlTestWrapper } from '../i18n/test-utils';
import { createUserMessage } from '../types/message';
import UserMessage from './UserMessage';

/**
 * UX audit C4: the seeded ask-AI brief rendered as a full-height dark wall. A user message taller
 * than half the window renders compactly — its heading line and the start of its body, clipped —
 * with "Show the full brief" (the conversation's opening message) or "Show the full message".
 * "Tall" is the message's own RENDERED height against the window, never a character count.
 */

const BRIEF = [
  'I want to work on one of my goose memories — category "lms-ps-is-fleet-ground-truth", global scope, tags: reference. It currently says:',
  '',
  'Fleet diagnosis: `lms ps` is truth for BUSY-vs-IDLE.',
].join('\n');

let renderedHeight = 0;
const scrollHeight = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'scrollHeight');

beforeEach(() => {
  Object.defineProperty(HTMLElement.prototype, 'scrollHeight', {
    configurable: true,
    get: () => renderedHeight,
  });
  vi.stubGlobal('innerHeight', 1000);
});

afterEach(() => {
  if (scrollHeight) Object.defineProperty(HTMLElement.prototype, 'scrollHeight', scrollHeight);
  vi.unstubAllGlobals();
});

const mount = (opensConversation: boolean) =>
  render(
    <IntlTestWrapper>
      <UserMessage message={createUserMessage(BRIEF)} opensConversation={opensConversation} />
    </IntlTestWrapper>
  );

describe('UserMessage — a wall renders compactly', () => {
  it('a message taller than half the window collapses to a fifth of it, with Show the full brief', async () => {
    renderedHeight = 1400;
    const user = userEvent.setup();
    mount(true);
    const body = screen.getByTestId('user-message-body');
    expect(body.getAttribute('data-collapsed')).toBe('true');
    expect(body.style.maxHeight).toBe('200px');
    expect(body.className).toContain('overflow-hidden');
    // The heading line (the item) is the top of what stays visible.
    expect(body.textContent).toContain('lms-ps-is-fleet-ground-truth');

    const toggle = screen.getByTestId('user-message-toggle');
    expect(toggle.textContent).toBe('Show the full brief');
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    await user.click(toggle);
    expect(body.getAttribute('data-collapsed')).toBeNull();
    expect(body.style.maxHeight).toBe('');
    expect(toggle.textContent).toBe('Show less');
  });

  it('a later wall says Show the full message', () => {
    renderedHeight = 900;
    mount(false);
    expect(screen.getByTestId('user-message-toggle').textContent).toBe('Show the full message');
  });

  it('a seeded brief at half the window (the 3.0.10 ask-AI case: 480px of 1000) collapses', () => {
    renderedHeight = 480;
    mount(true);
    expect(screen.getByTestId('user-message-body').getAttribute('data-collapsed')).toBe('true');
  });

  it('a message that fits renders whole, with no toggle', () => {
    renderedHeight = 300; // under a third of the 1000px window
    mount(true);
    expect(screen.getByTestId('user-message-body').getAttribute('data-collapsed')).toBeNull();
    expect(screen.queryByTestId('user-message-toggle')).toBeNull();
  });
});
