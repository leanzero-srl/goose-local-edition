import { afterEach, describe, expect, it } from 'vitest';
import React from 'react';
import { act, render } from '@testing-library/react';
import { useSmoothText } from './SwarmRunPanel';
import { pageVisibility } from './formationVisualState';

/**
 * THE HIDDEN-WINDOW FREEZE (measured over CDP on the live r0 benchmark, 2026-08-30). The fleet
 * cell's typewriter advanced `shown` only inside requestAnimationFrame, and a hidden/occluded
 * window suspends rAF entirely: probed live, `document.visibilityState === 'hidden'`, zero rAF
 * callbacks in 1.5s, the committed React prop moving from a 1,253-char paragraph to
 * "💭 Hmm wait, …" while the painted DOM held 507 stale chars ending mid-word ("… + feed") across
 * every sample. tick_ui.mjs reported it as "digest ADVANCED and cell text did NOT" on two
 * consecutive ticks. The digest, the hook and the memo were all fresh — the animation state was
 * the one stale link. A hidden page must deliver the target directly, like reduced motion does.
 */

let visibility: DocumentVisibilityState = 'visible';
Object.defineProperty(document, 'visibilityState', {
  configurable: true,
  get: () => visibility,
});
const setVisibility = (v: DocumentVisibilityState) => {
  visibility = v;
  act(() => {
    document.dispatchEvent(new Event('visibilitychange'));
  });
};

const Probe: React.FC<{ text: string }> = ({ text }) => (
  <div data-testid="probe">{useSmoothText(text)}</div>
);

afterEach(() => {
  visibility = 'visible';
});

describe('a hidden window snaps the live text instead of freezing the typewriter', () => {
  it('pageVisibility reads hidden as not visible and everything else as visible', () => {
    expect(pageVisibility({ visibilityState: 'hidden' })).toBe(false);
    expect(pageVisibility({ visibilityState: 'visible' })).toBe(true);
    expect(pageVisibility(undefined)).toBe(true);
  });

  it('paints the full target immediately while hidden, with no typewriter lag', () => {
    setVisibility('hidden');
    const { getByTestId } = render(<Probe text="first streamed thought" />);
    expect(getByTestId('probe').textContent).toBe('first streamed thought');
  });

  it('follows a target update while hidden — the measured regression shape', () => {
    setVisibility('hidden');
    const { getByTestId, rerender } = render(
      <Probe text="I'll go with 3 for notifierd to be safe on the ratio claim" />
    );
    rerender(<Probe text="💭 Hmm wait, but there's risk of over-listing" />);
    expect(getByTestId('probe').textContent).toBe(
      "💭 Hmm wait, but there's risk of over-listing"
    );
  });

  it('keeps the last delivered text when the window becomes visible again', () => {
    setVisibility('hidden');
    const { getByTestId } = render(<Probe text="carried across the visibility change" />);
    setVisibility('visible');
    expect(getByTestId('probe').textContent).toBe('carried across the visibility change');
  });
});
