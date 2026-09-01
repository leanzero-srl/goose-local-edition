import { describe, expect, it } from 'vitest';
import { missingUtilities } from './compileStudioCss';
import {
  DISABLED,
  FOCUS,
  MOTION,
  NODE_DOT,
  NODE_FILL,
  NODE_INDEXES,
  NODE_TEXT,
  RADIUS,
  ROW,
  SPACE,
  SURFACE,
  TNUM,
  TONE_DOT,
  TONE_FILL,
  TONE_TEXT,
  TONES,
  TYPE,
  WEIGHT,
  cx,
  nodeClasses,
  toneClasses,
} from './tokens';

const EVERY_CONSTANT: Record<string, string> = {
  ...Object.fromEntries(Object.entries(TONE_FILL).map(([k, v]) => [`TONE_FILL.${k}`, v])),
  ...Object.fromEntries(Object.entries(TONE_TEXT).map(([k, v]) => [`TONE_TEXT.${k}`, v])),
  ...Object.fromEntries(Object.entries(TONE_DOT).map(([k, v]) => [`TONE_DOT.${k}`, v])),
  ...Object.fromEntries(Object.entries(NODE_FILL).map(([k, v]) => [`NODE_FILL.${k}`, v])),
  ...Object.fromEntries(Object.entries(NODE_TEXT).map(([k, v]) => [`NODE_TEXT.${k}`, v])),
  ...Object.fromEntries(Object.entries(NODE_DOT).map(([k, v]) => [`NODE_DOT.${k}`, v])),
  ...Object.fromEntries(Object.entries(TYPE).map(([k, v]) => [`TYPE.${k}`, v])),
  ...Object.fromEntries(Object.entries(SURFACE).map(([k, v]) => [`SURFACE.${k}`, v])),
  ...Object.fromEntries(Object.entries(RADIUS).map(([k, v]) => [`RADIUS.${k}`, v])),
  ...Object.fromEntries(Object.entries(ROW).map(([k, v]) => [`ROW.${k}`, v])),
  ...Object.fromEntries(Object.entries(SPACE).map(([k, v]) => [`SPACE.${k}`, v])),
  ...Object.fromEntries(Object.entries(WEIGHT).map(([k, v]) => [`WEIGHT.${k}`, v])),
  FOCUS,
  MOTION,
  TNUM,
  DISABLED,
};

const everyClass = [...new Set(Object.values(EVERY_CONSTANT).flatMap((s) => s.split(/\s+/)))];

describe('LeanZero Studio tokens.ts — the class-name contract', () => {
  it('cx joins without merging (tailwind-merge would delete text-lz-display before text-lz-ink)', () => {
    expect(cx('text-lz-display', 'text-lz-ink')).toBe('text-lz-display text-lz-ink');
    expect(cx('a', false, null, undefined, 0, '', 'b')).toBe('a b');
  });

  it('every class in every constant compiles to a real rule against main.css', async () => {
    expect(await missingUtilities(everyClass)).toEqual([]);
  }, 30_000);

  it('no constant carries a left rail, an opacity, a tint modifier or a bare colour', () => {
    for (const [name, value] of Object.entries(EVERY_CONSTANT)) {
      expect(value, name).not.toMatch(/(^|\s)(?:[a-z-]+:)*border-l(?:-\S*)?(?=\s|$)/);
      expect(value, name).not.toMatch(/\/(?:5|10|15|20|25|30)(?=\s|$)/);
      expect(value, name).not.toMatch(/opacity/);
      expect(value, name).not.toMatch(/#[0-9a-f]{3,8}|\[#|rgba?\(/);
    }
  });

  it('colour constants route ONLY through lz tokens (plus white ink on the status fills)', () => {
    for (const tone of TONES) {
      expect(TONE_FILL[tone]).toMatch(/^bg-lz-\S+ text-(lz-\S+|white)$/);
      expect(TONE_TEXT[tone]).toMatch(/^text-lz-[a-z]+$/);
      expect(TONE_DOT[tone]).toMatch(/^bg-lz-[a-z]+$/);
      expect(toneClasses(tone)).toBe(TONE_FILL[tone]);
      expect(toneClasses(tone, 'text')).toBe(TONE_TEXT[tone]);
      expect(toneClasses(tone, 'dot')).toBe(TONE_DOT[tone]);
    }
    for (const n of NODE_INDEXES) {
      expect(NODE_FILL[n]).toBe(`bg-lz-node-${n} text-lz-node-${n}-ink`);
      expect(NODE_TEXT[n]).toBe(`text-lz-node-${n}`);
      expect(NODE_DOT[n]).toBe(`bg-lz-node-${n}`);
      expect(nodeClasses(n)).toBe(NODE_FILL[n]);
    }
    // The status fills carry the darker -solid step; the text register carries the vivid step.
    for (const tone of ['ok', 'warn', 'err', 'stopped'] as const) {
      expect(TONE_FILL[tone]).toContain(`bg-lz-${tone}-solid`);
      expect(TONE_TEXT[tone]).toBe(`text-lz-${tone}`);
    }
  });

  it('the zone register is the ONLY uppercase step, and every step names its ink', () => {
    for (const [step, cls] of Object.entries(TYPE)) {
      expect(cls, step).toMatch(/text-lz-ink(-\d)?/);
      if (step === 'zone') expect(cls).toContain('uppercase');
      else expect(cls).not.toMatch(/uppercase|tracking-/);
    }
    expect(TYPE.mono).toContain('font-mono');
  });

  it('focus is the app accent ring, motion is 120ms ease-out, disabled is a solid neutral', () => {
    expect(FOCUS).toContain('focus-visible:outline-ring');
    expect(MOTION).toBe('transition-colors duration-120 ease-lz');
    expect(DISABLED).toContain('disabled:bg-lz-surface-2');
    expect(DISABLED).toContain('disabled:text-lz-ink-3');
    expect(DISABLED).not.toMatch(/opacity/);
    expect(SURFACE.selected).toBe('bg-lz-accent text-lz-accent-ink');
    expect(SURFACE.selectedRing).toBe('ring-2 ring-inset ring-lz-accent');
    expect(SURFACE.hover).toBe('hover:bg-lz-surface-2');
    expect(SURFACE.card).not.toMatch(/shadow/);
    expect(SURFACE.overlay).toContain('shadow-lz-overlay');
  });
});
