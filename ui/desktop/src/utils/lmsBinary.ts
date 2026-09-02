import path from 'node:path';

/**
 * Where LM Studio's `lms` CLI is, resolved ONCE per app lifetime.
 *
 * U-H2 (gate 8, 2026-09-02): the panel's corroboration hook polls `fleet-status` every 1.5s on every
 * install, and the handler shelled `lms ps --json` on each tick with `'lms'` as the fallback binary —
 * on a Mac without LM Studio that was a failing execFile (ENOENT) every 1.5s for as long as a panel was
 * open. The lookup is the one the handler always had (`~/.lmstudio/bin/lms`, then PATH); what changes
 * is that an ABSENT binary is remembered, answered with the honest empty map, and logged once — no
 * spawn, no new timer. An `lms` installed after launch is seen on the next app start.
 */
export interface LmsLookup {
  home: string;
  pathEnv: string | undefined;
  platform: NodeJS.Platform;
  exists: (candidate: string) => boolean;
}

export function findLmsBinary(l: LmsLookup): string | null {
  const p = l.platform === 'win32' ? path.win32 : path.posix;
  const bundled = p.join(l.home, '.lmstudio', 'bin', 'lms');
  if (l.exists(bundled)) return bundled;
  const names = l.platform === 'win32' ? ['lms.exe', 'lms.cmd', 'lms'] : ['lms'];
  for (const dir of (l.pathEnv ?? '').split(p.delimiter)) {
    if (!dir) continue;
    for (const name of names) {
      const candidate = p.join(dir, name);
      if (l.exists(candidate)) return candidate;
    }
  }
  return null;
}

let resolution: { bin: string | null } | null = null;

/** The memoised answer; `find` runs on the first call only, and `onAbsent` fires once, then never. */
export function resolveLmsOnce(find: () => string | null, onAbsent: () => void): string | null {
  if (!resolution) {
    resolution = { bin: find() };
    if (!resolution.bin) onAbsent();
  }
  return resolution.bin;
}

export function resetLmsResolutionForTests(): void {
  resolution = null;
}
