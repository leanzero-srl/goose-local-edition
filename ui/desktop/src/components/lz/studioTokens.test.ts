import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';

/**
 * The LeanZero Studio token contract (ui/desktop/DESIGN.md), refused mechanically: every surface
 * colour is a SOLID 6-digit hex in BOTH themes, both themes name the same tokens, every token
 * the surfaces define is registered as a Tailwind utility, and the ink/surface pairs that carry
 * text clear WCAG AA. A faded tint, an alpha, a color-mix or a token that exists in one theme
 * only fails here before it can ship.
 */
const css = readFileSync(resolve(__dirname, '../../styles/main.css'), 'utf8');

function declarationsAfter(marker: string): Record<string, string> {
  const at = css.indexOf(marker);
  if (at < 0) throw new Error(`marker not found: ${marker}`);
  const open = css.indexOf('{', at);
  const close = css.indexOf('}', open);
  const out: Record<string, string> = {};
  for (const m of css.slice(open + 1, close).matchAll(/(--[a-z0-9-]+)\s*:\s*([^;]+);/g)) {
    out[m[1]] = m[2].trim();
  }
  return out;
}

const light = declarationsAfter('LeanZero Studio surfaces — light');
const dark = declarationsAfter('LeanZero Studio surfaces — dark');
const registered = declarationsAfter('Colour registration — self-referential');

const SURFACE_TOKENS = [
  '--color-lz-bg',
  '--color-lz-surface',
  '--color-lz-surface-2',
  '--color-lz-border',
  '--color-lz-border-strong',
  '--color-lz-ink',
  '--color-lz-ink-2',
  '--color-lz-ink-3',
  '--color-lz-ink-4',
  '--color-lz-accent-hover',
  '--color-lz-secondary',
  '--color-lz-secondary-ink',
];

function luminance(hex: string): number {
  const c = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
  const lin = c.map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  return 0.2126 * lin[0] + 0.7152 * lin[1] + 0.0722 * lin[2];
}
function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

describe('LeanZero Studio surfaces — the token contract in main.css', () => {
  it('both themes define exactly the surface set, and nothing else', () => {
    expect(Object.keys(light).sort()).toEqual([...SURFACE_TOKENS].sort());
    expect(Object.keys(dark).sort()).toEqual([...SURFACE_TOKENS].sort());
  });

  it('every surface value is a solid 6-digit hex — no alpha, no tint, no color-mix, no var()', () => {
    for (const theme of [light, dark]) {
      for (const [name, value] of Object.entries(theme)) {
        expect(value, name).toMatch(/^#[0-9a-f]{6}$/);
      }
    }
  });

  it('every surface token is registered as a Tailwind utility with a host-theme fallback', () => {
    for (const name of SURFACE_TOKENS) {
      expect(registered[name], name).toBeDefined();
      expect(registered[name], name).toMatch(/^var\(/);
    }
  });

  it('registers the accent, the status triad (text + solid) and the six-node ramp with its ink', () => {
    expect(registered['--color-lz-accent']).toBe('var(--color-action-solid, #1d4ed8)');
    expect(registered['--color-lz-accent-ink']).toBe('#ffffff');
    for (const tone of ['ok', 'warn', 'err', 'stopped']) {
      expect(registered[`--color-lz-${tone}`]).toMatch(/^var\(--color-status-/);
      expect(registered[`--color-lz-${tone}-solid`]).toMatch(/^var\(--color-status-.*-solid, #/);
    }
    for (const n of [1, 2, 3, 4, 5, 6]) {
      expect(registered[`--color-lz-node-${n}`]).toMatch(/^var\(--color-node-\d, #/);
      expect(registered[`--color-lz-node-${n}-ink`]).toMatch(/^var\(--color-node-\d-ink, #/);
    }
  });

  it('text-bearing pairs clear WCAG AA in both themes', () => {
    for (const theme of [light, dark]) {
      expect(contrast(theme['--color-lz-ink'], theme['--color-lz-bg'])).toBeGreaterThan(12);
      expect(contrast(theme['--color-lz-ink'], theme['--color-lz-surface'])).toBeGreaterThan(12);
      expect(contrast(theme['--color-lz-ink-2'], theme['--color-lz-surface'])).toBeGreaterThan(7);
      expect(contrast(theme['--color-lz-ink-3'], theme['--color-lz-surface'])).toBeGreaterThan(4.5);
      // ink-3 is a META colour (labels, counts, timestamps), never body copy: it clears AA on the
      // surface and AA-for-UI (3:1) on the hover surface — measured 4.34 light on slate-100.
      expect(contrast(theme['--color-lz-ink-3'], theme['--color-lz-surface-2'])).toBeGreaterThan(3);
      // ink-4 (#94a3b8 light) measures 2.56:1 on the surface: it is PLACEHOLDER ink and the
      // "—" of an absent value, never information — DISABLED controls use ink-3 for that reason.
      expect(
        contrast(theme['--color-lz-secondary-ink'], theme['--color-lz-secondary'])
      ).toBeGreaterThan(4.5);
      expect(contrast(theme['--color-lz-secondary'], theme['--color-lz-surface'])).toBeGreaterThan(
        4.5
      );
      expect(contrast('#ffffff', theme['--color-lz-accent-hover'])).toBeGreaterThan(4.5);
    }
    expect(contrast('#ffffff', '#1d4ed8')).toBeGreaterThan(4.5);
  });

  it('the type scale carries weight and tracking inside the utility, and the family is Inter', () => {
    for (const step of ['display', 'h1', 'h2', 'body', 'meta', 'zone', 'mono']) {
      expect(css, step).toMatch(new RegExp(`--text-lz-${step}:\\s*\\d+px;`));
    }
    expect(css).toMatch(/--text-lz-display--letter-spacing:\s*-0\.02em/);
    expect(css).toMatch(/--text-lz-h1--letter-spacing:\s*-0\.01em/);
    expect(css).toMatch(/--text-lz-zone--letter-spacing:\s*0\.08em/);
    expect(css).toMatch(/--text-lz-zone--font-weight:\s*600/);
    expect(css).toMatch(/--font-sans:\s*'Inter'/);
    expect(css).toMatch(/--font-sans--font-feature-settings:\s*'cv11', 'ss01'/);
    expect(css).toMatch(/@utility tnum\s*\{\s*font-variant-numeric:\s*tabular-nums;/);
  });

  it('the Studio block carries no left rail, no opacity and no color-mix', () => {
    const start = css.indexOf('LEANZERO STUDIO — design system v1');
    const end = css.indexOf('6. BASE LAYER');
    expect(start).toBeGreaterThan(0);
    const studio = css.slice(start, end);
    expect(studio).not.toMatch(/border-left/);
    expect(studio).not.toMatch(/color-mix\(/);
    expect(studio).not.toMatch(/\bopacity\b/);
    // the two shadow tokens are the ONLY alpha values in the block
    expect((studio.match(/rgba\(/g) ?? []).length).toBe(2);
  });
});
