import { describe, expect, it } from 'vitest';
import { toolOutcome } from './toolOutcome';

describe('tool outcome reflects the actual response', () => {
  it('never invents completion when a stream ends without a response', () => {
    expect(toolOutcome(undefined, false, false)).toBe('pending');
    expect(toolOutcome(undefined, true, false)).toBe('loading');
    expect(toolOutcome(undefined, true, true)).toBe('pending');
  });
  it('retains MCP failures inside successful transport envelopes', () => {
    expect(
      toolOutcome({ status: 'success', value: { isError: true, content: [] } }, false, false)
    ).toBe('error');
    expect(toolOutcome({ status: 'error', error: 'offline' }, false, false)).toBe('error');
    expect(toolOutcome({ status: 'success', value: { content: [] } }, false, false)).toBe(
      'success'
    );
    expect(toolOutcome({ unexpected: true }, false, false)).toBe('pending');
  });
});
