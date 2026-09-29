import { mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, relative, resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

/**
 * Q-335 — NO FADED CONTENT OR CONTROL. `opacity-50` on a disabled switch, `opacity-60` on a
 * dragged queue item, `hover:opacity-80` on a link: every one of these COMPILES, so the
 * dead-utility guard (classesCompile.test.ts) cannot see them, and assertStudioClean refuses them
 * only inside Studio renders. 53 survived host-wide. DESIGN.md ban 2: no `opacity-*` on content;
 * disabled is SOLID (the DISABLED / DISABLED_NOW tokens).
 *
 * Raw text on purpose: `classes += ' opacity-50'` or a class built in a helper is not a
 * className context, and a faded class there fades the screen all the same.
 *
 * `opacity-0` / `opacity-100` stay legal — they hide and show, they never wash.
 */

const SRC = resolve(__dirname, '../..');

/**
 * The ONLY faded classes allowed: a disabled state that still reads clearly. Each entry is
 * `<file relative to src> <exact token>` with its reason, and the token must carry a disabled
 * variant — the list cannot license a fade on content. Empty since Q-335 converted every one.
 */
const ALLOWED_DISABLED_FADES: Record<string, string> = {};

const DISABLED_VARIANT =
  /(^|:)(disabled|group-disabled(\/[\w-]+)?|peer-disabled(\/[\w-]+)?|aria-disabled|data-\[disabled\]|data-disabled):/;

/** The utility part of a class token: what follows its last top-level variant colon. */
function utilityOf(token: string): string {
  let depth = 0;
  let cut = 0;
  for (let i = 0; i < token.length; i++) {
    const c = token[i];
    if (c === '[' || c === '(') depth++;
    else if (c === ']' || c === ')') depth--;
    else if (c === ':' && depth === 0) cut = i + 1;
  }
  return token.slice(cut).replace(/^!|!$/g, '');
}

const OPACITY = /^-?opacity-(\d+|\[[^\]]+\]|\([^)]+\))$/;
const INLINE_FADE = /\bopacity\s*:\s*['"]?0?\.\d/;

function sourceFiles(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name !== 'node_modules' && entry.name !== '__tests__') sourceFiles(path, out);
    } else if (/\.(tsx?|jsx?)$/.test(entry.name) && !/\.(test|spec)\.[jt]sx?$/.test(entry.name)) {
      out.push(path);
    }
  }
  return out;
}

interface Token {
  file: string;
  line: number;
  text: string;
  token: string;
}

/** Every whitespace/quote-delimited token of every non-test source line under `root`. */
function tokens(root: string): Token[] {
  const out: Token[] = [];
  for (const path of sourceFiles(root)) {
    const file = relative(root, path);
    readFileSync(path, 'utf8')
      .split('\n')
      .forEach((text, i) => {
        for (const token of text.split(/[\s'"`{}]+/)) {
          if (token) out.push({ file, line: i + 1, text, token });
        }
      });
  }
  return out;
}

function fadedOpacities(
  root: string = SRC,
  allow: Record<string, string> = ALLOWED_DISABLED_FADES
): string[] {
  const found: string[] = [];
  const inlineSeen = new Set<string>();
  for (const { file, line, text, token } of tokens(root)) {
    const inline = INLINE_FADE.exec(text);
    if (inline && !inlineSeen.has(`${file}:${line}`)) {
      inlineSeen.add(`${file}:${line}`);
      found.push(`${file}:${line} inline ${inline[0]}`);
    }
    const m = OPACITY.exec(utilityOf(token));
    if (!m || m[1] === '0' || m[1] === '100') continue;
    if (`${file} ${token}` in allow && DISABLED_VARIANT.test(token)) continue;
    found.push(`${file}:${line} ${token}`);
  }
  return found;
}

/**
 * The same ban's other spelling: an alpha modifier on a colour (`bg-background-danger/10`,
 * `text-text-inverse/70`). Backdrop scrims (`bg-black/50` under a dialog) are legitimately
 * translucent and share the syntax, so this is a RATCHET, not a refusal: the count may only fall.
 * Measured 2026-09-29 after Q-335 converted the washes on the lines it touched.
 */
const ALPHA_WASH_BASELINE = 127;
const ALPHA_WASH =
  /^(bg|text|border|ring|outline|fill|stroke|from|via|to|divide|decoration|shadow|placeholder|caret|accent)-[a-z0-9-]+\/(\d{1,2}|\[[^\]]+\])$/;

function alphaWashes(root: string = SRC): string[] {
  return tokens(root)
    .filter(({ token }) => ALPHA_WASH.test(utilityOf(token)))
    .map(({ file, line, token }) => `${file}:${line} ${token}`);
}

describe('no faded content or control (Q-335)', () => {
  it('no source file fades content with opacity-5..95 or an inline opacity below 1', () => {
    expect(fadedOpacities()).toEqual([]);
  });

  it('alpha-modifier washes only shrink (scrims share the syntax)', () => {
    expect(alphaWashes().length).toBeLessThanOrEqual(ALPHA_WASH_BASELINE);
  });

  it('every allow-list entry is a disabled state, never content', () => {
    for (const key of Object.keys(ALLOWED_DISABLED_FADES)) {
      const token = key.split(' ')[1] ?? '';
      expect(DISABLED_VARIANT.test(token), key).toBe(true);
      expect(ALLOWED_DISABLED_FADES[key].length, key).toBeGreaterThan(20);
    }
  });

  it('refuses the Q-335 shapes and lets hide/show through', () => {
    const dir = mkdtempSync(join(tmpdir(), 'q335-faded-'));
    try {
      writeFileSync(
        join(dir, 'Fixture.tsx'),
        [
          'export const A = () => <i className="px-1 disabled:opacity-50" />;',
          "export const B = ({ on }: { on: boolean }) => <i className={`flex ${on ? 'opacity-60 scale-105' : ''}`} />;",
          "let classes = 'p-1'; classes += ' hover:opacity-80';",
          'export const C = () => <i className="opacity-0 group-hover:opacity-100 data-[state=open]:opacity-100" />;',
          'export const D = () => <i style={{ opacity: 0.9 }} className="text-[10px]" />;',
          'export const E = () => <i className="data-[disabled]:opacity-[0.4]!" />;',
          'export const F = () => <i className="bg-black/50 text-lz-ink-3" />;',
        ].join('\n')
      );
      const refused = [
        'Fixture.tsx:1 disabled:opacity-50',
        'Fixture.tsx:2 opacity-60',
        'Fixture.tsx:3 hover:opacity-80',
        'Fixture.tsx:5 inline opacity: 0.9',
        'Fixture.tsx:6 data-[disabled]:opacity-[0.4]!',
      ];
      expect(fadedOpacities(dir, {})).toEqual(refused);
      // The allow-list lets a listed DISABLED fade through, and never a listed content fade.
      const allow = {
        'Fixture.tsx disabled:opacity-50': 'a fixture: the disabled fade the list licenses',
        'Fixture.tsx opacity-60': 'a fixture: content cannot be licensed by the list',
      };
      expect(fadedOpacities(dir, allow)).toEqual(refused.slice(1));
      expect(alphaWashes(dir)).toEqual(['Fixture.tsx:7 bg-black/50']);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});
