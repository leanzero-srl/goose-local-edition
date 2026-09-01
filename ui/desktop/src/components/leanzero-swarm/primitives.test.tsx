import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { AZURE, Chip, GREEN, RED, SLATE } from './primitives';

// The token doctrine (main.css `.local-edition`): ONE accent, the status triad for state, and
// nothing else coloured. These pin the two chip registers and that a publisher takes no hue.
describe('leanzero primitives — chip registers', () => {
  it('a quiet chip is an outline in the secondary text colour with NO fill', () => {
    const { getByText } = render(<Chip quiet>lmstudio-community</Chip>);
    const chip = getByText('lmstudio-community');
    expect(chip.style.backgroundColor).toBe('');
    expect(chip.className).toContain('border-border-primary');
    expect(chip.className).toContain('text-text-secondary');
    // Metadata is never shouted: no uppercase, no letter-spacing.
    expect(chip.className).not.toMatch(/uppercase|tracking-/);
  });

  it('a filled chip carries its semantic colour and ink', () => {
    const { getByText } = render(
      <Chip color={GREEN} ink="#ffffff">
        mounted
      </Chip>
    );
    const chip = getByText('mounted');
    expect(chip.style.backgroundColor.replace(/\s/g, '')).toBe(GREEN.replace(/\s/g, ''));
    expect(chip.style.color).toBe('rgb(255, 255, 255)');
  });

  it('a chip with no colour falls through to the quiet register rather than a transparent fill', () => {
    const { getByText } = render(<Chip>4-bit</Chip>);
    expect(getByText('4-bit').style.backgroundColor).toBe('');
  });

  it('every palette constant resolves through a theme token — no bare hex, no node hue', () => {
    for (const c of [AZURE, GREEN, RED, SLATE]) {
      expect(c).toMatch(/^var\(--color-(action-solid|status-[a-z]+-solid), #[0-9a-f]{6}\)$/);
      expect(c).not.toContain('--color-node-');
    }
  });
});
