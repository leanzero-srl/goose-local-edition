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
 * `text-text-inverse/70`, `hover:bg-red-900/20`). Q-457 replaced every content, accent, badge,
 * hover and state use with a solid token and took this to ZERO. The one legitimate use is a SCRIM —
 * the translucent black a modal lays over the page behind it — so the allow-list takes only a
 * `bg-black/NN` on a full-viewport layer (`fixed inset-0` on the same line), keyed by file and
 * token, each with a one-word reason. A listed token anywhere else is still refused.
 */
const ALLOWED_SCRIMS: Record<string, string> = {
  'components/lz-dialogs/ConfirmCloseRunDialog.tsx bg-black/60': 'backdrop',
  'components/recipes/CreateEditRecipeModal.tsx bg-black/50': 'backdrop',
  'components/recipes/ImportRecipeForm.tsx bg-black/50': 'backdrop',
  'components/recipes/shared/CreateSubRecipeInline.tsx bg-black/50': 'backdrop',
  'components/recipes/shared/InstructionsEditor.tsx bg-black/50': 'backdrop',
  'components/recipes/shared/JsonSchemaEditor.tsx bg-black/50': 'backdrop',
  'components/recipes/shared/SubRecipeModal.tsx bg-black/50': 'backdrop',
  'components/schedule/ScheduleModal.tsx bg-black/50': 'backdrop',
  'components/sessions/SessionListView.tsx bg-black/50': 'backdrop',
  'components/settings/mode/ConfigureApproveMode.tsx bg-black/30': 'backdrop',
  'components/settings/models/bottom_bar/ModelsBottomBar.tsx bg-black/50': 'backdrop',
  'components/swarm/Clipped.tsx bg-black/60': 'backdrop',
  'components/swarm/SwarmRunPanel.tsx bg-black/60': 'backdrop',
  'components/ui/BaseModal.tsx bg-black/20': 'backdrop',
  'components/ui/dialog.tsx bg-black/50': 'backdrop',
  'components/ui/sheet.tsx bg-black/50': 'backdrop',
};

const SCRIM_TOKEN = /^bg-black\/\d{1,2}$/;
const FULL_VIEWPORT = /\bfixed\b.*\binset-0\b|\binset-0\b.*\bfixed\b/;
const ALPHA_WASH =
  /^(bg|text|border|ring|outline|fill|stroke|from|via|to|divide|decoration|shadow|placeholder|caret|accent)-([a-z0-9-]+|\[[^\]]+\]|\([^)]+\))\/(\d{1,2}|\[[^\]]+\]|\([^)]+\))$/;

function isListedScrim(file: string, token: string, text: string, allow: Record<string, string>) {
  return `${file} ${token}` in allow && SCRIM_TOKEN.test(token) && FULL_VIEWPORT.test(text);
}

function alphaWashes(root: string = SRC, allow: Record<string, string> = ALLOWED_SCRIMS): string[] {
  return tokens(root)
    .filter(({ token }) => ALPHA_WASH.test(utilityOf(token)))
    .filter(({ file, token, text }) => !isListedScrim(file, token, text, allow))
    .map(({ file, line, token }) => `${file}:${line} ${token}`);
}

describe('no faded content or control (Q-335, Q-457)', () => {
  it('no source file fades content with opacity-5..95 or an inline opacity below 1', () => {
    expect(fadedOpacities()).toEqual([]);
  });

  it('no colour carries an alpha modifier except a listed full-viewport scrim (Q-457)', () => {
    expect(alphaWashes()).toEqual([]);
  });

  it('every listed scrim is a black backdrop still drawn on a full-viewport layer', () => {
    const live = new Set(
      tokens(SRC)
        .filter(({ file, token, text }) => isListedScrim(file, token, text, ALLOWED_SCRIMS))
        .map(({ file, token }) => `${file} ${token}`)
    );
    for (const [key, reason] of Object.entries(ALLOWED_SCRIMS)) {
      expect(SCRIM_TOKEN.test(key.split(' ')[1] ?? ''), key).toBe(true);
      expect(reason, key).toMatch(/^[a-z]+$/);
      expect(live.has(key), `${key} is listed but no longer drawn — drop it`).toBe(true);
    }
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
          'export const G = () => <div className="fixed inset-0 z-50 bg-black/50" />;',
          "export const H = ({ on }: { on: boolean }) => <i className={on ? 'bg-red-500/10' : 'hover:bg-white/10'} />;",
          'export const I = () => <i className="text-[#fff]/60 border-(--x)/20 bg-lz-err-solid" />;',
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
      const washes = [
        'Fixture.tsx:7 bg-black/50',
        'Fixture.tsx:8 bg-black/50',
        'Fixture.tsx:9 bg-red-500/10',
        'Fixture.tsx:9 hover:bg-white/10',
        'Fixture.tsx:10 text-[#fff]/60',
        'Fixture.tsx:10 border-(--x)/20',
      ];
      expect(alphaWashes(dir, {})).toEqual(washes);
      // A listed scrim passes ONLY on its full-viewport line; a listed content wash never passes.
      const scrims = {
        'Fixture.tsx bg-black/50': 'backdrop',
        'Fixture.tsx bg-red-500/10': 'content',
      };
      expect(alphaWashes(dir, scrims)).toEqual(
        washes.filter((w) => w !== 'Fixture.tsx:8 bg-black/50')
      );
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});
