import { describe, expect, it } from 'vitest';
import { foldEvents } from './useSwarmRun';

// THE FIFTH LANE PATH. A comment in useSwarmRun says these fields are "set on all four of these paths".
// There are five, and the BUILD worker lane -- the one that wins in laneSources and therefore feeds both
// the fleet strip and the inspector -- was never counted. Every field below was undefined for the whole
// of BUILD, which is where a run spends its hours.
//
// This test exists so "which paths carry the digest fields" stops being something anyone counts by hand.
describe('a BUILD worker lane carries the whole digest', () => {
  const digest = {
    thinking_chars: 4096,
    last_thinking: 'the rolling window',
    full_thinking: 'the durable think.log, much longer than the window',
    full_transcript: 'the durable task.log',
    transcript_bytes: 200000,
    judging: true,
    queued_chunks: 3,
    phase: 'processing',
    last_text: 'answer text',
    recent: ['ran a command'],
    reasoning: 'short digest reasoning',
    full_reasoning: 'the 24k clip',
    calls: [],
    tool_calls: 2,
    errors: 0,
  };

  const events = [
    { event: 'run_started', ts: '2026-08-29T10:00:00Z' },
    { event: 'task_dispatched', ts: '2026-08-29T10:00:01Z', task_id: 'ledgerd-core', model: 'mihai-qwen' },
  ];

  it('does not drop the thinking, transcript or supervision fields', () => {
    const folded = foldEvents(events as never, { 'ledgerd-core': digest } as never);
    const lane = folded.lanes.find((l) => l.taskId === 'ledgerd-core');
    expect(lane, 'the build lane must exist').toBeTruthy();
    const blob = JSON.stringify(lane);

    // Each of these was undefined on a build lane and each broke something specific:
    //   thinkingChars   gates the thinking line
    //   fullThinking    is what the inspector renders instead of the rolling window
    //   fullTranscript  is what stops OUTPUT falling back to the 24k clip
    //   judging         is the only way to say "frozen because the supervisor is reading"
    for (const marker of [
      'the durable think.log',
      'the durable task.log',
      '4096',
      'processing',
    ]) {
      expect(blob).toContain(marker);
    }
  });
});
