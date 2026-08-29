import { describe, expect, it } from 'vitest';
import { inspectorThinkingText } from './SwarmRunPanel';

// This pane has been wrong twice and each time it looked like the OTHER failure, so both are pinned here.
describe('the node inspector shows the whole reasoning, exactly once', () => {
  it('never concatenates the durable log with the rolling window', () => {
    // The window is a SUFFIX of the log by construction: same stream, two views.
    const log = 'Let me analyze my slice: ledgerd-core.\n\nMy responsibilities:\n1. CLI parsing\n';
    const window = 'me analyze my slice: ledgerd-core.\n\nMy responsibilities:\n1. CLI parsing\n';
    const out = inspectorThinkingText({ fullThinking: log, lastThinking: window });
    expect(out).toBe(log.trim());
    expect(out.split('My responsibilities').length - 1).toBe(1);
  });

  it('falls back to the rolling window only when no durable log exists', () => {
    expect(inspectorThinkingText({ lastThinking: 'only the window' })).toBe('only the window');
    expect(inspectorThinkingText({})).toBe('');
  });

  it('prefers the durable log over every shorter view of the same stream', () => {
    const out = inspectorThinkingText({
      fullThinking: 'the whole 55,000 character transcript',
      fullReasoning: 'a shorter answer-channel view',
      reasoning: 'shorter still',
      lastThinking: 'the 2,400-char tail',
    });
    expect(out).toBe('the whole 55,000 character transcript');
  });
});
