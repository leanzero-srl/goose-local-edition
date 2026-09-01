/**
 * LeanZero Studio — the class-name side of the token contract (ui/desktop/DESIGN.md).
 *
 * Every colour, type step, radius and motion value the Studio primitives use is a Tailwind
 * utility over a `--*-lz-*` token in src/styles/main.css. Remake surfaces compose THESE
 * constants; nobody hand-writes a hex, an opacity or a `border-l`.
 *
 * Every string here is a literal so Tailwind's source scan sees it — a template-built class
 * name would never be generated.
 *
 * MEASURED HAZARD: tailwind-merge classifies `text-lz-display` and `text-lz-ink` as the same
 * "text colour" group and DELETES the earlier one (`twMerge('text-lz-display text-lz-ink')`
 * returns `text-lz-ink`). Studio classes therefore go through `cx()` below — a plain join —
 * never through `cn()`/twMerge.
 */

export type Tone = 'ok' | 'warn' | 'err' | 'stopped' | 'accent' | 'secondary';
export type NodeIndex = 1 | 2 | 3 | 4 | 5 | 6;
export type ColorRegister = 'fill' | 'text' | 'dot';

export const TONES: readonly Tone[] = ['ok', 'warn', 'err', 'stopped', 'accent', 'secondary'];
export const NODE_INDEXES: readonly NodeIndex[] = [1, 2, 3, 4, 5, 6];

/** Plain class join — no merging, no dedupe (see the tailwind-merge hazard above). */
export function cx(...parts: Array<string | false | null | undefined | 0>): string {
  let out = '';
  for (const p of parts) if (p) out += (out ? ' ' : '') + p;
  return out;
}

/** A solid FILL that carries its ink — chips, badges, the selected state. */
export const TONE_FILL: Record<Tone, string> = {
  ok: 'bg-lz-ok-solid text-white',
  warn: 'bg-lz-warn-solid text-white',
  err: 'bg-lz-err-solid text-white',
  stopped: 'bg-lz-stopped-solid text-white',
  accent: 'bg-lz-accent text-lz-accent-ink',
  secondary: 'bg-lz-secondary text-lz-secondary-ink',
};

/** The hue as FOREGROUND — text and icons on a surface. */
export const TONE_TEXT: Record<Tone, string> = {
  ok: 'text-lz-ok',
  warn: 'text-lz-warn',
  err: 'text-lz-err',
  stopped: 'text-lz-stopped',
  accent: 'text-lz-accent',
  secondary: 'text-lz-secondary',
};

/** The hue as a MARK with no text on it — dots, bars, progress. */
export const TONE_DOT: Record<Tone, string> = {
  ok: 'bg-lz-ok',
  warn: 'bg-lz-warn',
  err: 'bg-lz-err',
  stopped: 'bg-lz-stopped',
  accent: 'bg-lz-accent',
  secondary: 'bg-lz-secondary',
};

/** Node identity — the 6-hue ramp with the ink each hue was measured to carry. IDENTITY ONLY. */
export const NODE_FILL: Record<NodeIndex, string> = {
  1: 'bg-lz-node-1 text-lz-node-1-ink',
  2: 'bg-lz-node-2 text-lz-node-2-ink',
  3: 'bg-lz-node-3 text-lz-node-3-ink',
  4: 'bg-lz-node-4 text-lz-node-4-ink',
  5: 'bg-lz-node-5 text-lz-node-5-ink',
  6: 'bg-lz-node-6 text-lz-node-6-ink',
};

export const NODE_TEXT: Record<NodeIndex, string> = {
  1: 'text-lz-node-1',
  2: 'text-lz-node-2',
  3: 'text-lz-node-3',
  4: 'text-lz-node-4',
  5: 'text-lz-node-5',
  6: 'text-lz-node-6',
};

export const NODE_DOT: Record<NodeIndex, string> = {
  1: 'bg-lz-node-1',
  2: 'bg-lz-node-2',
  3: 'bg-lz-node-3',
  4: 'bg-lz-node-4',
  5: 'bg-lz-node-5',
  6: 'bg-lz-node-6',
};

export function toneClasses(tone: Tone, register: ColorRegister = 'fill'): string {
  return register === 'fill'
    ? TONE_FILL[tone]
    : register === 'text'
      ? TONE_TEXT[tone]
      : TONE_DOT[tone];
}

export function nodeClasses(node: NodeIndex, register: ColorRegister = 'fill'): string {
  return register === 'fill'
    ? NODE_FILL[node]
    : register === 'text'
      ? NODE_TEXT[node]
      : NODE_DOT[node];
}

/** The type scale, each step with the ink it is set in. `zone` is the ONLY uppercase register. */
export const TYPE = {
  display: 'text-lz-display text-lz-ink',
  h1: 'text-lz-h1 text-lz-ink',
  h2: 'text-lz-h2 text-lz-ink',
  body: 'text-lz-body text-lz-ink',
  bodyMuted: 'text-lz-body text-lz-ink-2',
  meta: 'text-lz-meta text-lz-ink-3',
  zone: 'text-lz-zone uppercase text-lz-ink-2',
  mono: 'font-mono text-lz-mono text-lz-ink',
} as const;

/** Surfaces. Hover is a SOLID step to surface-2; selection is the accent fill or a 2px inset ring. */
export const SURFACE = {
  page: 'bg-lz-bg text-lz-ink',
  card: 'bg-lz-surface border border-lz-border rounded-lz-card',
  /** The one elevation — overlays only (popover, menu, dialog). */
  overlay:
    'bg-lz-surface border border-lz-border rounded-lz-card shadow-lz-overlay dark:shadow-lz-overlay-dark',
  inset: 'bg-lz-surface-2',
  hover: 'hover:bg-lz-surface-2',
  selected: 'bg-lz-accent text-lz-accent-ink',
  selectedRing: 'ring-2 ring-inset ring-lz-accent',
  hairline: 'border-lz-border',
  outline: 'border border-lz-border-strong',
} as const;

export const RADIUS = {
  control: 'rounded-lz-control',
  card: 'rounded-lz-card',
  pill: 'rounded-lz-pill',
} as const;

export const ROW = { default: 'h-lz-row', dense: 'h-lz-row-dense' } as const;

export const SPACE = {
  page: 'p-lz-page',
  pageX: 'px-lz-page',
  section: 'gap-lz-section',
  card: 'p-lz-card',
} as const;

/** The accent ring the app already owns (`--color-ring`), on :focus-visible only. */
export const FOCUS =
  'outline-none focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-ring';

/** 120ms ease-out, colours only. */
export const MOTION = 'transition-colors duration-120 ease-lz';

/**
 * Weights. MEASURED: `font-medium`, `font-semibold` and `font-bold` compile to NOTHING in this
 * app (the MCP theme registration sets their tokens to `initial`), so the Studio carries its own.
 * The type-scale steps already embed their weight; these are for emphasis inside a step.
 */
export const WEIGHT = { medium: 'font-lz-medium', semibold: 'font-lz-semibold' } as const;

/** Tabular figures — every number a person compares down a column. */
export const TNUM = 'tnum';

/** Disabled is a SOLID neutral (surface-2 fill, ink-3 text), never an opacity. */
export const DISABLED =
  'disabled:pointer-events-none disabled:border-lz-border disabled:bg-lz-surface-2 disabled:text-lz-ink-3';
