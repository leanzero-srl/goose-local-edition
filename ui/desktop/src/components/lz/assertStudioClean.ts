import { expect } from 'vitest';

/**
 * The design bans, refused on rendered output (ui/desktop/DESIGN.md "Bans"): no left rail, no
 * faded opacity modifier or opacity utility, no alpha tint or color-mix in inline styles, no
 * native <select>. Every Studio primitive test runs its render through this.
 */
const CLASS_BANS: Array<[RegExp, string]> = [
  [/(^|\s)(?:[a-z-]+:)*border-l(?:-\S*)?(?=\s|$)/, 'a left rail (border-l*)'],
  [/\/(?:5|10|15|20|25|30)(?=\s|$)/, 'a faded opacity modifier (/10 /15 /20)'],
  [/(^|\s)(?:[a-z-]+:)*opacity-\d+/, 'an opacity utility'],
  [/(^|\s)(?:[a-z-]+:)*bg-opacity-/, 'bg-opacity'],
];

const STYLE_BANS: Array<[RegExp, string]> = [
  [/border-left/, 'a left rail (border-left)'],
  [/color-mix\(/, 'a color-mix wash'],
  [/rgba?\([^)]*,\s*0?\.\d+\)/, 'an alpha tint'],
  [/\bopacity\s*:/, 'an opacity'],
];

export function assertStudioClean(container: HTMLElement): void {
  expect(container.querySelector('select'), 'a native <select>').toBeNull();
  for (const el of Array.from(container.querySelectorAll<HTMLElement>('*'))) {
    const tag = el.tagName.toLowerCase();
    const cls = el.getAttribute('class') ?? '';
    for (const [re, why] of CLASS_BANS) {
      expect(cls, `<${tag} class="${cls}"> carries ${why}`).not.toMatch(re);
    }
    const style = el.getAttribute('style') ?? '';
    for (const [re, why] of STYLE_BANS) {
      expect(style, `<${tag} style="${style}"> carries ${why}`).not.toMatch(re);
    }
  }
}

/** Every distinct class name in the rendered tree, sorted. */
export function allClasses(container: HTMLElement): string[] {
  const set = new Set<string>();
  for (const el of Array.from(container.querySelectorAll<HTMLElement>('*'))) {
    el.classList.forEach((c) => set.add(c));
  }
  return [...set].sort();
}
