import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { Tabs, TabsContent, TabsList, TabsTrigger } from './tabs';
import { assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';

describe('ui/tabs (the app-wide base)', () => {
  it('a disabled trigger is a solid Studio state; the active trigger has no shadow; content never fades in', async () => {
    const { container } = render(
      <Tabs defaultValue="a">
        <TabsList>
          <TabsTrigger value="a">Alpha</TabsTrigger>
          <TabsTrigger value="b" disabled>
            Beta
          </TabsTrigger>
        </TabsList>
        <TabsContent value="a">alpha content</TabsContent>
      </Tabs>
    );
    const beta = screen.getByText('Beta') as HTMLButtonElement;
    expect(beta.disabled).toBe(true);
    for (const c of ['disabled:bg-lz-surface-2', 'disabled:text-lz-ink-4', 'disabled:cursor-not-allowed']) {
      expect(beta.className).toContain(c);
    }
    expect(beta.className).not.toMatch(/opacity|shadow/);
    const content = screen.getByText('alpha content');
    expect(content.className).not.toMatch(/animate-in|fade-in/);
    assertStudioClean(container);
    expect(
      await missingUtilities([
        'disabled:bg-lz-surface-2',
        'disabled:text-lz-ink-4',
        'disabled:cursor-not-allowed',
      ])
    ).toEqual([]);
  }, 30_000);
});
