import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { ChatInputCard } from './ChatInputCard';
import { allClasses, assertStudioClean } from './lz/assertStudioClean';
import { missingUtilities } from './lz/compileStudioCss';

describe('ChatInputCard (Studio)', () => {
  it('is the Studio card: lz-surface, a 1px lz hairline, the card radius, no shadow', async () => {
    const { container } = render(
      <ChatInputCard className="mt-2">
        <span>composer</span>
      </ChatInputCard>
    );
    const card = screen.getByText('composer').parentElement as HTMLElement;
    for (const c of ['bg-lz-surface', 'border', 'border-lz-border', 'rounded-lz-card', 'overflow-hidden', 'mt-2']) {
      expect(card.className).toContain(c);
    }
    expect(card.className).not.toMatch(/shadow|rounded-2xl|border-border-primary|bg-background-primary/);
    assertStudioClean(container);
    expect(await missingUtilities(allClasses(container))).toEqual([]);
  }, 30_000);
});
