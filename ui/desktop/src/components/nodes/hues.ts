import type { NodeRole } from './model';

/**
 * ONE palette for nodes and strategies, in three families that never share a hue
 * (DESIGN-NODES-AND-STRATEGIES.md §4.6):
 *
 * - KIND is identity, never a colour: solid ink with an icon (MLX, Cloud, Endpoint). The where chip
 *   and the legacy "Follows this Mac" chip are ink OUTLINES (a full 2px border, no fill, no tint).
 * - STATE is one colour per meaning: green can take work now · amber loading · slate not loaded ·
 *   orange needs a step · red can't run (held by a build, key missing, last call failed too).
 * - ROLE is its own six hues: Chat, Planning, Build, Testing, Frontend, Backend.
 *
 * Every fill carries the ink that passes 4.5:1 on it, and the fill/ink pair is the same in both
 * themes (the ratio does not depend on the page behind the chip) — except the kind chip, whose ink
 * fill would vanish into the dark page, so it inverts. `hues.test.ts` holds every pair of fills that
 * can sit on one card or one strategy row at CIEDE2000 ≥ 15, and every ink at ≥ 4.5.
 *
 * Re-picks the check forced, from the design's first hexes (the design names this outcome):
 * - needs a step #EA580C → #F97316: #EA580C sat 14.4 from the can't-run red.
 * - Planning #9333EA → #A21CAF: #9333EA sat 10.0 from Build's indigo.
 * - Testing #0284C7 → #0EA5E9 with dark ink: neither white (4.10) nor dark ink (4.33) reached 4.5.
 *
 * Every class string is a literal so Tailwind's source scan generates it.
 */

export interface InkedFill {
  fill: string;
  ink: string;
}

export interface Hue {
  light: InkedFill;
  dark: InkedFill;
  /** The fill and its ink as Tailwind classes, both themes. */
  className: string;
}

const INK = '#111827';
const WHITE = '#FFFFFF';

const same = (fill: string, ink: string, className: string): Hue => ({
  light: { fill, ink },
  dark: { fill, ink },
  className,
});

export const KIND_HUE: Hue = {
  light: { fill: INK, ink: WHITE },
  dark: { fill: '#F3F4F6', ink: INK },
  className: 'bg-[#111827] text-[#FFFFFF] dark:bg-[#F3F4F6] dark:text-[#111827]',
};

export type StateHue = 'green' | 'amber' | 'slate' | 'orange' | 'red';

export const STATE_HUES: Record<StateHue, Hue> = {
  green: same('#15803D', WHITE, 'bg-[#15803D] text-[#FFFFFF]'),
  amber: same('#F59E0B', INK, 'bg-[#F59E0B] text-[#111827]'),
  slate: same('#475569', WHITE, 'bg-[#475569] text-[#FFFFFF]'),
  orange: same('#F97316', INK, 'bg-[#F97316] text-[#111827]'),
  red: same('#DC2626', WHITE, 'bg-[#DC2626] text-[#FFFFFF]'),
};

export const ROLE_HUES: Record<NodeRole, Hue> = {
  chat: same('#DB2777', WHITE, 'bg-[#DB2777] text-[#FFFFFF]'),
  planning: same('#A21CAF', WHITE, 'bg-[#A21CAF] text-[#FFFFFF]'),
  build: same('#4F46E5', WHITE, 'bg-[#4F46E5] text-[#FFFFFF]'),
  testing: same('#0EA5E9', INK, 'bg-[#0EA5E9] text-[#111827]'),
  frontend: same('#0D9488', INK, 'bg-[#0D9488] text-[#111827]'),
  backend: same('#92400E', WHITE, 'bg-[#92400E] text-[#FFFFFF]'),
};

/** Outline registers: a full 2px border and the page's own ink — never a fill or a tint. */
export const OUTLINE = {
  /** The where chip and "Follows this Mac". */
  ink: 'border-2 border-current bg-transparent text-lz-ink',
  /** "Another way is running": slate's own hue as the border, the page's ink as the text. */
  slate: 'border-2 border-[#475569] bg-transparent text-lz-ink dark:border-[#94A3B8]',
} as const;

/** Every fill that can sit beside another on one node card or one strategy row, by owner. */
export function paletteFills(theme: 'light' | 'dark'): { owner: string; fill: InkedFill }[] {
  return [
    { owner: 'kind', fill: KIND_HUE[theme] },
    ...Object.entries(STATE_HUES).map(([k, h]) => ({ owner: `state.${k}`, fill: h[theme] })),
    ...Object.entries(ROLE_HUES).map(([k, h]) => ({ owner: `role.${k}`, fill: h[theme] })),
  ];
}
