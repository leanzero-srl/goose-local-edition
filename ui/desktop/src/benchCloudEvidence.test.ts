import { expect, it } from 'vitest';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import {
  cloudConsoleProvesExecution,
  cloudRunStarted,
  cloudUsageProvesExecution,
} from './benchCloudEvidence';
import { outcomeFromSlot } from './benchSessions';
it('recovers the two observed cloud failures without inventing a score', async () => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), 'cloud-evidence-'));
  try {
    expect(await cloudRunStarted(dir)).toBe(false);
    await fs.writeFile(
      path.join(dir, 'engine-console.log'),
      ' __( O)>  ● new session · google gemini-3.8-flash\n 20260920_1 · /run\n L L goose is ready\n ▸ shell\n command: write app.py\n'
    );
    expect(outcomeFromSlot(false, await cloudRunStarted(dir))).toBe('did_not_finish');
    await fs.unlink(path.join(dir, 'engine-console.log'));
    await fs.writeFile(
      path.join(dir, 'model-usage.json'),
      JSON.stringify({
        status: 'recorded',
        source: 'isolated Goose session counters',
        sessions: [{ accumulated_input_tokens: 9596417, accumulated_output_tokens: 75873 }],
      })
    );
    expect(outcomeFromSlot(false, await cloudRunStarted(dir))).toBe('did_not_finish');
    await fs.writeFile(path.join(dir, 'model-usage.json'), 'broken');
    await expect(cloudRunStarted(dir)).rejects.toThrow();
  } finally {
    await fs.rm(dir, { recursive: true, force: true });
  }
});
it('does not treat startup errors, empty files or unknown usage as execution', () => {
  expect(cloudConsoleProvesExecution('Error: model not found')).toBe(false);
  expect(cloudConsoleProvesExecution('')).toBe(false);
  expect(
    cloudUsageProvesExecution({
      status: 'recorded',
      source: 'isolated Goose session counters',
      sessions: [],
    })
  ).toBe(false);
  expect(cloudUsageProvesExecution({ sessions: [{ accumulated_input_tokens: 42 }] })).toBe(false);
});
