import { describe, expect, it } from 'vitest';
import { inspectorOutputText, inspectorThinkingText } from './SwarmRunPanel';
import real from './__fixtures__/realLaneDigest.json';
import realOutput from './__fixtures__/realOutputDigest.json';

// THE ARCHIVED-TREE REPLAY, APPLIED TO THE UI.
//
// The other inspector tests use synthetic strings. These use REAL engine output captured from run
// swarm-3node-r0, where a digest's rolling view is a few hundred to a couple of thousand characters and
// the durable log beside it is several times that — the exact proportions that made a pane look truncated
// and, when the two were joined, made it render everything twice.
//
// BOTH channels are covered because the bug shipped in both: THINKING (`last_thinking` vs `<task>.think.log`)
// and OUTPUT (`last_text` vs `<task>.log`). The rules below are written once and applied to each channel,
// because a per-channel copy is how the two views drifted apart in the first place.

/// ONE DEFINITION OF "RENDERED EXACTLY ONCE". The rolling view is a suffix of the durable stream, so its
/// opening line appears once in a correct render and twice the moment the two are concatenated. Trim before
/// slicing: a raw view often starts mid-whitespace, and a head carrying that whitespace stops matching the
/// appended copy, which would make a joined render look clean.
const occurrencesOfRollingHead = (rendered: string, rollingView: string): number => {
  const head = rollingView.trim().slice(0, 120);
  expect(head.length).toBeGreaterThan(50);
  return rendered.split(head).length - 1;
};

/// ONE DEFINITION OF "THIS FIXTURE STILL DESCRIBES THE BUG IT WAS CAPTURED FOR". If someone regenerates a
/// fixture from a call whose rolling view happens to cover the whole durable stream, every assertion above
/// it keeps passing while proving nothing.
const expectRollingViewIsStrictSuffix = (rollingView: string, durable: string): void => {
  expect(rollingView.trim().length).toBeGreaterThan(50);
  expect(rollingView.length).toBeLessThan(durable.length);
  expect(durable.trimEnd().endsWith(rollingView.trim())).toBe(true);
};

describe('the inspector THINKING pane against real engine output', () => {
  const lane = {
    fullThinking: real.full_thinking,
    fullReasoning: real.full_reasoning ?? undefined,
    reasoning: real.reasoning ?? undefined,
    lastThinking: real.last_thinking ?? undefined,
    lastText: real.last_text ?? undefined,
    recent: real.recent ?? [],
  };

  it('shows the durable log, not the rolling window', () => {
    const out = inspectorThinkingText(lane);
    expect(out.length).toBeGreaterThan((real.last_thinking ?? '').length);
    expect(out).toBe(real.full_thinking.trim());
  });

  it('does not render the reasoning twice', () => {
    expect(occurrencesOfRollingHead(inspectorThinkingText(lane), real.last_thinking ?? '')).toBe(1);
  });

  it('falls back correctly on a lane that produced no answer-channel text', () => {
    // This lane wrote no <task>.log at all -- a pure-reasoning coverage call. OUTPUT must not invent
    // content, and must not throw.
    const out = inspectorOutputText({
      recent: real.recent ?? [],
      lastText: real.last_text ?? undefined,
    });
    expect(typeof out).toBe('string');
    expect(out).not.toContain('undefined');
  });

  it('the fixture still describes the bug it was captured for', () => {
    expectRollingViewIsStrictSuffix(real.last_thinking ?? '', real.full_thinking);
  });
});

// THE OTHER HALF OF THE SAME BUG. `main.ts` had been supplying `full_transcript` on every digest and the
// pane read `last_text`, so `approval-workflow` ended mid-sentence at "currency". Two lanes of one run are
// replayed: the lane Mihai reported it on, and `open`, whose digest also carries a tool-call summary — the
// third channel, which is prepended rather than chosen between.
describe.each([
  { name: 'slice-approval-workflow', fixture: realOutput.approval_workflow },
  { name: 'open', fixture: realOutput.open },
])('the inspector OUTPUT pane against real engine output ($name)', ({ fixture }) => {
  const lane = {
    fullTranscript: fixture.full_transcript,
    lastText: fixture.last_text,
    recent: fixture.recent as string[],
  };

  it('shows the durable transcript, not the rolling view', () => {
    const out = inspectorOutputText(lane);
    expect(out).toContain(fixture.full_transcript.trim());
    expect(out.length).toBeGreaterThan(fixture.last_text.length);
  });

  it('does not render the answer twice', () => {
    expect(occurrencesOfRollingHead(inspectorOutputText(lane), fixture.last_text)).toBe(1);
  });

  it('renders the beginning of the answer, which the rolling view had scrolled away', () => {
    // The truncation Mihai saw: the rolling view starts partway through, so its first line is NOT the
    // transcript's first line. A pane reading the durable log opens on the real beginning.
    const opening = fixture.full_transcript.trim().slice(0, 80);
    expect(fixture.last_text).not.toContain(opening);
    expect(inspectorOutputText(lane)).toContain(opening);
  });

  it('the fixture still describes the bug it was captured for', () => {
    expectRollingViewIsStrictSuffix(fixture.last_text, fixture.full_transcript);
  });
});

describe('the inspector OUTPUT pane keeps the tool-call channel separate', () => {
  const fixture = realOutput.open;
  const lane = {
    fullTranscript: fixture.full_transcript,
    lastText: fixture.last_text,
    recent: fixture.recent as string[],
  };

  it('prepends what the model DID to what it SAID', () => {
    expect(fixture.recent.length).toBeGreaterThan(0);
    const out = inspectorOutputText(lane);
    for (const summary of fixture.recent) {
      expect(out.indexOf(summary)).toBeLessThan(out.indexOf(fixture.full_transcript.trim()));
    }
  });

  it('drops the rolling view entirely rather than appending it after the summaries', () => {
    const out = inspectorOutputText(lane);
    const withoutDurable = out.replace(fixture.full_transcript.trim(), '');
    expect(withoutDurable).not.toContain(fixture.last_text.trim().slice(0, 120));
  });
});
