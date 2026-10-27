/**
 * A call into goose that does not answer in time is a FAILED call, said in words — never a wait
 * nobody ends. The owner's Engine panel sat on "Checking" for hours (3.0.132) because one status
 * read never resolved and every later tick waited behind it.
 *
 * The time limit does not cancel the call (an ACP request cannot be withdrawn); it only stops
 * waiting for it. Its late answer, if one ever comes, is dropped.
 */
export class TimeLimitError extends Error {
  readonly limitMs: number;

  constructor(message: string, limitMs: number) {
    super(message);
    this.name = 'TimeLimitError';
    this.limitMs = limitMs;
  }
}

export function isTimeLimitError(error: unknown): error is TimeLimitError {
  return error instanceof TimeLimitError;
}

/** `promise`, or a `TimeLimitError` carrying `message` once `limitMs` pass without an answer. */
export async function withTimeLimit<T>(
  promise: Promise<T>,
  limitMs: number,
  message: string
): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | null = null;
  const limit = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new TimeLimitError(message, limitMs)), limitMs);
  });
  try {
    return await Promise.race([promise, limit]);
  } finally {
    if (timer !== null) clearTimeout(timer);
  }
}

/** "12 s" — the limit as the error words say it. */
export function limitWords(limitMs: number): string {
  return `${Math.round(limitMs / 1000)} s`;
}
