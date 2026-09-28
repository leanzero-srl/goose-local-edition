import { resolvedPaint, studioToken } from './resolvedPaint';
import { TONE_TEXT, TONES } from './tokens';

/** Any element that names a tone (ok, warn, err, stopped, accent, secondary) as its text colour. */
export const TONE_TEXT_SELECTOR = TONES.map((tone) => `.${TONE_TEXT[tone]}`).join(', ');

/**
 * Every element under `root` that NAMES a tone as its text colour but does not PAINT it — another
 * colour utility on the same element wins in the compiled CSS (Q-247: `cx(TYPE.meta,
 * TONE_TEXT.err)` painted the meta grey across the app) — or that names two tones at once. The
 * stylesheet order decides, never the class attribute's, so this reads the real compiled CSS
 * through resolvedPaint. src/test/setup.ts runs it after every test, over everything rendered.
 */
export async function tonesThatDoNotPaint(root: Element): Promise<string[]> {
  const wrong: string[] = [];
  for (const el of Array.from(root.querySelectorAll<HTMLElement>(TONE_TEXT_SELECTOR))) {
    const tones = TONES.filter((tone) => el.classList.contains(TONE_TEXT[tone]));
    const where =
      el.getAttribute('data-testid') ??
      el.closest('[data-testid]')?.getAttribute('data-testid') ??
      el.tagName.toLowerCase();
    const said = `${where} "${(el.textContent ?? '').trim().slice(0, 60)}" class="${el.getAttribute('class')}"`;
    if (tones.length > 1) {
      wrong.push(`${said} names two tones (${tones.join(' + ')})`);
      continue;
    }
    const want = studioToken(`--color-lz-${tones[0]}`, 'light');
    const got = (await resolvedPaint(el, 'light')).text;
    if (got !== want) wrong.push(`${said} paints ${got}, not its ${tones[0]} (${want})`);
  }
  return wrong;
}
