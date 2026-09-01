import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import EnvironmentBadge from './EnvironmentBadge';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { TooltipProvider } from '../ui/Tooltip';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';

describe('EnvironmentBadge (Studio)', () => {
  it('in a dev build it is a warn-tone Chip, never a hand-written orange dot', async () => {
    expect(import.meta.env.DEV).toBe(true);
    const { container } = render(
      <IntlTestWrapper>
        <TooltipProvider>
          <EnvironmentBadge className="ml-1" />
        </TooltipProvider>
      </IntlTestWrapper>
    );
    expect(screen.getByTestId('environment-badge').className).toContain('ml-1');
    const chip = screen.getByTestId('lz-chip');
    expect(chip.getAttribute('data-tone')).toBe('warn');
    expect(chip.className).toContain('bg-lz-warn-solid');
    expect(chip).toHaveTextContent('Dev');
    expect(container.innerHTML).not.toMatch(/orange/);
    assertStudioClean(container);
    // `no-drag` is the Electron drag-region class from main.css, not a Tailwind utility.
    expect(await missingUtilities(allClasses(container).filter((c) => c !== 'no-drag'))).toEqual(
      []
    );
  }, 30_000);
});
