import fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';

/** Keep the previous published result, screenshots and session index coherent on a failed retry. */
export async function benchmarkResultTransaction<T>(
  targets: string[],
  commit: () => Promise<T>
): Promise<T> {
  const backup = await fs.mkdtemp(path.join(os.tmpdir(), 'benchmark-result-'));
  const saved: Array<{ target: string; backup: string; existed: boolean }> = [];
  {
    for (const [index, target] of targets.entries()) {
      const copy = path.join(backup, String(index));
      let existed = true;
      try {
        await fs.lstat(target);
      } catch (error) {
        if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error;
        existed = false;
      }
      if (existed) await fs.cp(target, copy, { recursive: true, dereference: false });
      saved.push({ target, backup: copy, existed });
    }
    try {
      const result = await commit();
      await fs
        .rm(backup, { recursive: true, force: true })
        .catch((error) => console.warn('Benchmark result backup retained:', backup, error));
      return result;
    } catch (error) {
      const failures: string[] = [];
      for (const entry of saved) {
        try {
          await fs.rm(entry.target, { recursive: true, force: true });
          if (entry.existed)
            await fs.cp(entry.backup, entry.target, { recursive: true, dereference: false });
        } catch (restoreError) {
          failures.push(`${entry.target}: ${String(restoreError)}`);
        }
      }
      if (failures.length)
        throw new Error(
          `Result could not be restored; backups retained at ${backup}: ${failures.join('; ')}. Original error: ${String(error)}`
        );
      await fs
        .rm(backup, { recursive: true, force: true })
        .catch((cleanupError) =>
          console.warn('Restored benchmark backup retained:', backup, cleanupError)
        );
      throw error;
    }
  }
}
