import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import {
  JumpToLatestButton,
  SessionBrand,
  SessionLoadErrorPanel,
  SubmitErrorBanner,
} from './BaseChat';
import { IntlTestWrapper } from '../i18n/test-utils';
import { LEANZERO_WEBSITE_URL } from '../branding';
import { allClasses, assertStudioClean } from './lz/assertStudioClean';
import { missingUtilities } from './lz/compileStudioCss';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { __unstable__loadDesignSystem as loadDesignSystem } from '@tailwindcss/node';

/**
 * Studio remake, surface C — the session chrome BaseChat owns:
 *  - the top-right brand is ONE pill (a solid accent mark + "LeanZero"), the whole pill the link;
 *  - the two notifications are Panels with a StatusDot — no hand-written amber, no faded red.
 * Handlers are the props; the chrome only presents them.
 */

const wrap = (ui: React.ReactElement) => render(<IntlTestWrapper>{ui}</IntlTestWrapper>);

/**
 * The pill's rendered height, from the REAL Tailwind pipeline: the class list is compiled against
 * main.css and the height declaration's token is resolved to px. jsdom lays nothing out, so a
 * getBoundingClientRect would answer 0 — the compiled rule is what the app paints.
 */
async function compiledHeightPx(classes: string[]): Promise<number | null> {
  const base = resolve(__dirname, '../styles');
  const css = readFileSync(resolve(base, 'main.css'), 'utf8');
  const design = await loadDesignSystem(css, { base });
  const rules = design.candidatesToCss(classes).filter((r): r is string => r != null);
  for (const rule of rules) {
    const decl = /(?:^|[;{\s])height:\s*([^;]+);/.exec(rule);
    if (decl == null) continue;
    // Tailwind's numeric scale: calc(var(--spacing) * N), --spacing being Tailwind's 0.25rem = 4px.
    const scale = /calc\(var\(--spacing\)\s*\*\s*([\d.]+)\)/.exec(decl[1]);
    if (scale) return Number.parseFloat(scale[1]) * 4;
    const token = /var\((--[\w-]+)\)/.exec(decl[1]);
    const value = token ? new RegExp(`${token[1]}:\\s*([\\d.]+)px`).exec(css)?.[1] : decl[1];
    return value == null ? null : Number.parseFloat(value);
  }
  return null;
}

describe('BaseChat chrome — SessionBrand (Q-191)', () => {
  it('Swarm edition: the pill reads "LeanZero" and the WHOLE pill is the leanzero.net link', () => {
    const { container } = wrap(<SessionBrand isLocal />);
    const link = screen.getByTestId('local-edition-badge');
    expect(link.tagName).toBe('A');
    expect(link.textContent).toBe('LeanZero');
    expect(screen.getByRole('link', { name: 'LeanZero' })).toBe(link);
    expect(link.getAttribute('href')).toBe(LEANZERO_WEBSITE_URL);
    expect(link.getAttribute('target')).toBe('_blank');
    expect(link.getAttribute('title')).toBe('Goose Swarm by LeanZero — open leanzero.net');
    expect(screen.queryByText('Goose Swarm')).toBeNull();
    // The anchor IS the pill: the outline, the surface fill and the hover step live on it, so the
    // hit target is the painted shape — no inner chip hugging a 20px box.
    expect(link.className).toContain('h-lz-row-dense');
    expect(link.className).toContain('border-lz-border-strong');
    expect(link.className).toContain('bg-lz-surface');
    expect(link.className).toContain('hover:bg-lz-surface-2');
    expect(link.className).not.toMatch(/border-l-|bg-opacity|\/\d0\b|uppercase|tracking-/);
    expect(link.querySelector('[data-testid="lz-chip"]')).toBeNull();
    const mark = screen.getByTestId('brand-mark');
    expect(link.contains(mark)).toBe(true);
    expect(mark.className).toContain('bg-lz-accent');
    expect(mark.querySelector('svg')).not.toBeNull();
    expect(screen.queryByText(/powered by/)).toBeNull();
    expect(container.querySelector('[style]')).toBeNull();
    assertStudioClean(container);
  });

  it('the hit target is at least 32px tall in the compiled CSS (it was a 20px chip)', async () => {
    wrap(<SessionBrand isLocal />);
    const classes = screen.getByTestId('local-edition-badge').className.split(/\s+/);
    const height = await compiledHeightPx(classes);
    expect(height).toBe(32);
    // The instrument's own control: the old chip's h-5 resolves to the 20px the owner found too small.
    expect(await compiledHeightPx(['h-5'])).toBe(20);
  }, 30_000);

  it('standard edition: the goose wordmark in the same pill, linking to the goose docs', async () => {
    const { container } = wrap(<SessionBrand isLocal={false} />);
    const link = screen.getByTestId('goose-brand');
    expect(link.textContent).toBe('goose');
    expect(link.getAttribute('href')).toBe('https://goose-docs.ai');
    expect(screen.queryByTestId('local-edition-badge')).toBeNull();
    expect(await compiledHeightPx(link.className.split(/\s+/))).toBeGreaterThanOrEqual(32);
    assertStudioClean(container);
  }, 30_000);
});

describe('BaseChat chrome — notifications', () => {
  it('a failed prompt is a Panel with a warn dot and a ghost Dismiss that calls the handler', () => {
    const onDismiss = vi.fn();
    const { container } = wrap(<SubmitErrorBanner error="socket closed" onDismiss={onDismiss} />);
    expect(screen.getByTestId('lz-panel')).toBeInTheDocument();
    expect(screen.getByTestId('submit-error-banner').getAttribute('role')).toBe('status');
    const dot = screen.getByTestId('lz-status-dot');
    expect(dot.className).toContain('bg-lz-warn');
    expect(dot.getAttribute('aria-label')).toBe('That message did not go through');
    expect(screen.getByText('That message did not go through')).toBeInTheDocument();
    expect(screen.getByText('socket closed')).toBeInTheDocument();
    expect(screen.getByText('Your conversation is safe. Send again to retry.')).toBeInTheDocument();
    const dismiss = screen.getByRole('button', { name: 'Dismiss' });
    expect(dismiss.getAttribute('data-variant')).toBe('ghost');
    fireEvent.click(dismiss);
    expect(onDismiss).toHaveBeenCalledTimes(1);
    expect(container.querySelector('[style]')).toBeNull();
    assertStudioClean(container);
  });

  it('a session that failed to load is a Panel with an err dot, an h2 title and the one way home', () => {
    const onGoHome = vi.fn();
    const { container } = wrap(<SessionLoadErrorPanel error="not found" onGoHome={onGoHome} />);
    expect(screen.getByTestId('lz-panel')).toBeInTheDocument();
    expect(screen.getByTestId('lz-status-dot').className).toContain('bg-lz-err');
    const title = screen.getByRole('heading', { level: 3 });
    expect(title.textContent).toBe('Failed to Load Session');
    expect(title.className).toContain('text-lz-h2');
    expect(screen.getByText('not found')).toBeInTheDocument();
    const home = screen.getByRole('button', { name: 'Go home' });
    expect(home.getAttribute('data-variant')).toBe('secondary');
    fireEvent.click(home);
    expect(onGoHome).toHaveBeenCalledTimes(1);
    assertStudioClean(container);
  });

  it('every class the chrome emits compiles to a real rule against main.css', async () => {
    const { container } = wrap(
      <>
        <SessionBrand isLocal />
        <SessionBrand isLocal={false} />
        <SubmitErrorBanner error="e" onDismiss={() => {}} />
        <SessionLoadErrorPanel error="e" onGoHome={() => {}} />
        <JumpToLatestButton onJump={() => {}} />
      </>
    );
    const classes = allClasses(container).filter(
      (c) => !c.startsWith('lucide') && c !== 'goose-icon-animation' && c !== 'no-drag'
    );
    expect(classes.length).toBeGreaterThan(20);
    expect(await missingUtilities(classes)).toEqual([]);
  }, 30_000);
});

describe('BaseChat chrome — Jump to latest (Q-496)', () => {
  it('a solid outlined button over the transcript that takes the chat back to its live edge', async () => {
    const onJump = vi.fn();
    const { container } = wrap(<JumpToLatestButton onJump={onJump} />);
    const button = screen.getByRole('button', { name: 'Jump to latest' });
    expect(button.getAttribute('data-testid')).toBe('jump-to-latest');
    expect(button.getAttribute('data-variant')).toBe('secondary');
    // Solid: the surface fill and the strong outline, lifted by the overlay shadow — no tint, no rail.
    expect(button.className).toContain('bg-lz-surface');
    expect(button.className).toContain('border-lz-border-strong');
    expect(button.className).toContain('shadow-lz-overlay');
    expect(button.className).not.toMatch(/border-l-|bg-opacity|\/\d0\b/);
    expect(button.className).toContain('pointer-events-auto');
    // The overlay row lets clicks through to the transcript everywhere but the button.
    const row = container.firstElementChild as HTMLElement;
    expect(row.className).toContain('pointer-events-none');
    expect(row.className).toContain('h-0');
    fireEvent.click(button);
    await waitFor(() => expect(onJump).toHaveBeenCalledTimes(1));
    assertStudioClean(container);
  });
});
