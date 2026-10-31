// pw.mjs — where the harness gets playwright-core, on any Mac and as any user (laptop and Gabee run as
// mihaiperdum, the Studios as workhorse). Every driver used to import it from a hard-coded
// /Users/mihaiperdum/Projects/goose/ui/node_modules path, so nothing ran on the Studios.
// Order: PLAYWRIGHT_CORE (a path to the package dir or its index.mjs), the checkout this file lives in
// (harness/ is local-edition/mlx/quality/harness, so the repo root is four levels up), $HOME/Projects/goose,
// then a plain module lookup from here. The first that exists wins; none = a loud error naming every candidate.
import { existsSync } from 'node:fs';
import { homedir } from 'node:os';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { createRequire } from 'node:module';

const HERE = fileURLToPath(new URL('.', import.meta.url));
const entry = (dir) => (dir.endsWith('.mjs') ? dir : `${dir.replace(/\/$/, '')}/index.mjs`);

export function playwrightCandidates(env = process.env, home = env.HOME || homedir(), here = HERE) {
  const c = [];
  if (env.PLAYWRIGHT_CORE) c.push(entry(env.PLAYWRIGHT_CORE));
  c.push(entry(`${here.replace(/\/$/, '')}/../../../../ui/node_modules/playwright-core`));
  c.push(entry(`${home}/Projects/goose/ui/node_modules/playwright-core`));
  return c;
}

export function resolvePlaywright({ env = process.env, home, here, exists = existsSync } = {}) {
  const tried = playwrightCandidates(env, home ?? (env.HOME || homedir()), here ?? HERE);
  const hit = tried.find((p) => exists(p));
  if (hit) return hit;
  try { return createRequire(import.meta.url).resolve('playwright-core'); } catch {}
  throw new Error(`playwright-core not found; tried ${tried.join(', ')} and a module lookup (set PLAYWRIGHT_CORE)`);
}

const mod = await import(pathToFileURL(resolvePlaywright()).href);
export const chromium = mod.chromium ?? mod.default?.chromium;
