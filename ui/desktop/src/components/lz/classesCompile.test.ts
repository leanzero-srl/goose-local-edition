import { mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { __unstable__loadDesignSystem as loadDesignSystem } from '@tailwindcss/node';
import { beforeAll, describe, expect, it } from 'vitest';
import { utilityCss } from './compileStudioCss';
import { type ClassUse, sourceClassUses } from './sourceClasses';
import { FOCUS } from './tokens';

/**
 * Q-325 — EVERY CLASS THE SOURCE NAMES MUST EXIST. Tailwind drops a candidate it cannot resolve
 * without a word, so a dead utility is invisible everywhere but the screen: the session title's
 * `focus-visible:ring-border-active` (no `--color-border-active` token) left Tab with no ring, and
 * the sweep that found it counted over 100 more on 664 sites — `font-medium` on 155, a whole older palette
 * (`text-textStandard`, `bg-background-default`), shadcn names this app never registered.
 *
 * Static on purpose: the source is read once, not rendered, so nothing here depends on which
 * tests happen to mount which branch — and a class in a branch no test renders is caught too.
 */

const stylesDir = resolve(__dirname, '../../styles');
const srcDir = resolve(__dirname, '../..');

/** Class-named bindings that are not class lists, with the reason. */
const NOT_CLASS_LISTS: Record<string, string> = {
  'components/codeThemes.ts': 'a Prism token-type colour map whose `className` key is a token kind',
};

/** Tailwind's own markers (`group`, `peer`, their named forms) and the typography plugin's opt-out. */
const MARKER = /^(group|peer)(\/[\w-]+)?$|^not-prose$/;

type DesignSystem = Awaited<ReturnType<typeof loadDesignSystem>>;

/**
 * Whether a token NAMES a Tailwind utility: it parses as one, carries a variant or an important
 * mark, or starts with a utility's root (`resize-vertical` → `resize`). A token that does none of
 * these is the app's own hook (`goose-message-content`, `parameter-input`) and is not judged here.
 */
function namesAUtility(design: DesignSystem, token: string): boolean {
  if (design.parseCandidate(token).length > 0) return true;
  if (/[:!]/.test(token) || token.startsWith('-') || token.startsWith('[')) return true;
  const parts = token.split('-');
  for (let i = 1; i <= parts.length; i++) {
    const root = parts.slice(0, i).join('-');
    if (design.utilities.has(root, 'functional') || design.utilities.has(root, 'static')) {
      return true;
    }
  }
  return false;
}

function cssFiles(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) cssFiles(path, out);
    else if (entry.name.endsWith('.css')) out.push(path);
  }
  return out;
}

/** Every class selector the app's own stylesheets define (`.no-drag`, `.page-transition`, …). */
function cssDefinedClasses(): Set<string> {
  const defined = new Set<string>();
  for (const path of cssFiles(srcDir)) {
    const css = readFileSync(path, 'utf8').replace(/\/\*[\s\S]*?\*\//g, '');
    for (const m of css.matchAll(/\.(-?[_a-zA-Z][\w-]*)(?=[^{};]*\{)/g)) defined.add(m[1]);
  }
  return defined;
}

async function deadClasses(uses: ClassUse[]): Promise<string[]> {
  const css = readFileSync(resolve(stylesDir, 'main.css'), 'utf8');
  const design = await loadDesignSystem(css, { base: stylesDir });
  const defined = cssDefinedClasses();
  const tokens = [...new Set(uses.map((u) => u.token))];
  const out = design.candidatesToCss(tokens);
  const dead = new Set(
    tokens.filter(
      (t, i) => out[i] == null && !MARKER.test(t) && !defined.has(t) && namesAUtility(design, t)
    )
  );
  return uses
    .filter((u) => dead.has(u.token) && !(u.file in NOT_CLASS_LISTS))
    .map((u) => `${u.file}:${u.line} ${u.token}`);
}

/**
 * Class strings that clear the outline (`outline-none`) and then draw one under a variant
 * (`focus-visible:outline-2`) without restoring its style under that variant. Tailwind's
 * `outline-none` sets `--tw-outline-style: none` and `outline-2` draws `outline-style:
 * var(--tw-outline-style)`, so such a ring computes to `none 0px` — measured in Chromium on the
 * FOCUS token itself, which every Studio primitive and ui/button carried.
 */
function outlinesThatNeverDraw(uses: ClassUse[]): string[] {
  const byLiteral = new Map<number, ClassUse[]>();
  for (const use of uses) {
    const group = byLiteral.get(use.literal);
    if (group) group.push(use);
    else byLiteral.set(use.literal, [use]);
  }
  const wrong: string[] = [];
  for (const group of byLiteral.values()) {
    const tokens = group.map((u) => u.token);
    if (!tokens.includes('outline-none')) continue;
    for (const token of tokens) {
      const width = /^(.+:)outline-(\d+|\[[^\]]+\])$/.exec(token);
      if (!width) continue;
      const styled =
        tokens.includes(`${width[1]}outline-solid`) ||
        tokens.includes(`${width[1]}outline-dashed`) ||
        tokens.includes(`${width[1]}outline-dotted`) ||
        tokens.includes(`${width[1]}outline-double`);
      if (!styled)
        wrong.push(`${group[0].file}:${group[0].line} ${token} without ${width[1]}outline-solid`);
    }
  }
  return wrong;
}

/** One scan of ui/desktop/src (~5 s over every source file), shared by every test that reads it. */
let scanned: ClassUse[] | null = null;
beforeAll(() => {
  scanned = sourceClassUses();
}, 60_000);
function allUses(): ClassUse[] {
  if (!scanned) throw new Error('the source scan did not run');
  return scanned;
}

describe('the focus ring draws (Q-325)', () => {
  it('FOCUS restores a solid outline under :focus-visible', async () => {
    const css = await utilityCss(FOCUS.split(' '));
    expect(css.every((rule) => rule != null)).toBe(true);
    expect(css.join('\n')).toMatch(/:focus-visible\s*\{[^}]*outline-style:\s*solid/);
  });

  it('no class string clears the outline and draws one it never styles', () => {
    expect(outlinesThatNeverDraw(allUses())).toEqual([]);
  });
});

describe('every class ui/desktop/src names compiles (Q-325)', () => {
  it('reads the whole source, not a sample', () => {
    const uses = allUses();
    const files = new Set(uses.map((u) => u.file));
    expect(files.size).toBeGreaterThan(300);
    const header = uses.filter((u) => u.file === join('components', 'SessionActionsHeader.tsx'));
    expect(header.map((u) => u.token)).toContain('pointer-events-auto');
  });

  it('no className, class helper or class-named binding names a utility that produces no CSS', async () => {
    expect(await deadClasses(allUses())).toEqual([]);
  }, 60_000);

  it('refuses the Q-325 shape: a ring on a token that does not exist, in every class context', async () => {
    const dir = mkdtempSync(join(tmpdir(), 'q325-classes-'));
    try {
      writeFileSync(
        join(dir, 'Fixture.tsx'),
        [
          "const cx = (...c: unknown[]) => c.join(' ');",
          "const TITLE_CLASSES = 'rounded-sm focus-visible:ring-border-active';",
          'export const A = () => <button className="px-1 focus:ring-border-active" />;',
          "export const B = ({ on }: { on: boolean }) => <a className={cx('p-1', on && 'text-textStandard')} />;",
          "export const C = ({ on }: { on: boolean }) => <i className={`flex ${on ? 'font-regular' : ''}`} />;",
          "export const D = ({ kind }: { kind: string }) => <b className={kind === 'bg-nope-x' ? 'block' : 'no-drag'} />;",
          'export const E = () => <i className="outline-none focus-visible:outline-2 focus-visible:outline-ring" />;',
          'export const F = () => <i className="outline-none focus-visible:outline-solid focus-visible:outline-2" />;',
        ].join('\n')
      );
      const uses = sourceClassUses(dir);
      expect(outlinesThatNeverDraw(uses)).toEqual([
        'Fixture.tsx:7 focus-visible:outline-2 without focus-visible:outline-solid',
      ]);
      const dead = await deadClasses(uses);
      expect(dead).toEqual([
        'Fixture.tsx:2 focus-visible:ring-border-active',
        'Fixture.tsx:3 focus:ring-border-active',
        'Fixture.tsx:4 text-textStandard',
        'Fixture.tsx:5 font-regular',
      ]);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }, 60_000);
});
