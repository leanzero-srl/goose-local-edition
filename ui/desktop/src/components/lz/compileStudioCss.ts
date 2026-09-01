import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { __unstable__loadDesignSystem as loadDesignSystem } from '@tailwindcss/node';

/**
 * Compile main.css with the real Tailwind pipeline and answer, per class name, whether it
 * produces a rule. A class that compiles to nothing is a silent no-op in the app — the
 * `font-mono` case this system was born from — so the tests refuse it.
 */
export async function missingUtilities(classes: readonly string[]): Promise<string[]> {
  const base = resolve(__dirname, '../../styles');
  const css = readFileSync(resolve(base, 'main.css'), 'utf8');
  const design = await loadDesignSystem(css, { base });
  const out = design.candidatesToCss([...classes]);
  return classes.filter((_, i) => out[i] == null);
}
