/**
 * Q-383: every wait in a test runs on the running test's own timeout — never on a clock of its own.
 *
 * A wait with its own clock (testing-library's 1 s default, a hand-picked 500 ms, 4 s or 5 s
 * deadline) fails first whenever the machine is slower than whoever picked the number: full runs
 * beside a cargo build failed one such wait per run while every file passed alone. A wait that polls
 * a condition until the test's own timeout fails only when the thing never happens.
 *
 * setup.ts starts the clock before every test from the test's own timeout (the suite's, or the one
 * the test passes), so a test that asks for 30 s gets 30 s in every wait inside it too.
 */
import { configure } from '@testing-library/react';

// setTimeout's ceiling: a longer delay fires at once (Node clamps it to 1 ms). A test whose timeout
// is disabled (0) waits on the longest delay a timer can hold, not on no delay.
const LONGEST_TIMER_MS = 2 ** 31 - 1;

let current: number | null = null;

export function startTestClock(testTimeoutMs: number): void {
  current = testTimeoutMs > 0 ? testTimeoutMs : LONGEST_TIMER_MS;
  configure({ asyncUtilTimeout: current });
}

export function testClock(): number {
  if (current === null) {
    throw new Error('testClock() read outside a running test — setup.ts starts it in beforeEach');
  }
  return current;
}

/**
 * Polls `check` until it holds, for as long as the running test may take. For real processes and
 * real files, where no DOM mutation announces the change and testing-library's waitFor does not
 * apply. `what` names the condition in the failure.
 */
export async function pollUntil(
  check: () => boolean | Promise<boolean>,
  what: string,
  intervalMs = 20
): Promise<void> {
  const deadline = Date.now() + testClock();
  while (!(await check())) {
    if (Date.now() >= deadline) throw new Error(`timed out waiting for ${what}`);
    await new Promise((resolve) => setTimeout(resolve, intervalMs));
  }
}
