import { describe, expect, it } from 'vitest';
import { digestStreamFields } from './useSwarmRun';
import type { FormingCall, TurnLane } from './useSwarmRun';

/**
 * II-11b UI half: the forming state flows through digestStreamFields — the ONE shared join every
 * lane path uses — and is never carried from a previous poll. The sidecar's absence IS the engine
 * saying nothing is forming (the file is removed on completion and at scope exit), so a carried
 * row would outlive its own call and render a dead amber "forming…" forever.
 */
const FORMING: FormingCall[] = [{ id: 'call-7', name: 'developer__write_file', since_ms: 1_000 }];

describe('forming flows through the shared digest join and never survives its sidecar', () => {
  it('passes forming through when the digest carries it', () => {
    const out = digestStreamFields('lane-a', { forming: FORMING });
    expect(out.forming).toEqual(FORMING);
  });

  it('does NOT carry a previous poll\'s forming when the sidecar is gone', () => {
    const prev: Partial<TurnLane> = { forming: FORMING, toolCalls: 3 };
    const out = digestStreamFields('lane-a', { tool_calls: 4 }, prev);
    expect(out.forming).toBeUndefined();
    // the carry itself still works for fields that ARE carried
    expect(out.toolCalls).toBe(4);
  });

  it('an absent digest yields no forming row', () => {
    expect(digestStreamFields('lane-a', undefined, { forming: FORMING }).forming).toBeUndefined();
  });
});
