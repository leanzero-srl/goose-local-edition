import fs from 'node:fs/promises';
import path from 'node:path';

export function cloudUsageProvesExecution(value: unknown): boolean {
  if (!value || typeof value !== 'object') return false;
  const usage = value as { status?: unknown; source?: unknown; sessions?: unknown };
  if (
    usage.status !== 'recorded' ||
    usage.source !== 'isolated Goose session counters' ||
    !Array.isArray(usage.sessions)
  )
    return false;
  return usage.sessions.some(
    (session) =>
      session &&
      typeof session === 'object' &&
      ['accumulated_input_tokens', 'accumulated_output_tokens'].some(
        (key) =>
          typeof session[key] === 'number' && Number.isFinite(session[key]) && session[key] > 0
      )
  );
}

/** The runner's durable console header proves Goose started, even when interrupted before usage flush. */
export function cloudConsoleProvesExecution(text: string): boolean {
  return /new session\s*·\s*[A-Za-z0-9_-]+\s+\S+/.test(text) && /goose is ready/.test(text);
}

export async function cloudRunStarted(workdir: string): Promise<boolean> {
  try {
    const file = await fs.open(path.join(workdir, 'engine-console.log'), 'r');
    try {
      const prefix = Buffer.alloc(64 * 1024);
      const { bytesRead } = await file.read(prefix, 0, prefix.length, 0);
      if (cloudConsoleProvesExecution(prefix.subarray(0, bytesRead).toString('utf8'))) return true;
    } finally {
      await file.close();
    }
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error;
  }
  try {
    return cloudUsageProvesExecution(
      JSON.parse(await fs.readFile(path.join(workdir, 'model-usage.json'), 'utf8'))
    );
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') return false;
    throw error;
  }
}
