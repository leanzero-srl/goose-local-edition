import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { SessionBrand, SessionLoadErrorPanel, SubmitErrorBanner } from './BaseChat';
import { IntlTestWrapper } from '../i18n/test-utils';
import { LEANZERO_WEBSITE_URL } from '../branding';
import { allClasses, assertStudioClean } from './lz/assertStudioClean';
import { missingUtilities } from './lz/compileStudioCss';

/**
 * Studio remake, surface C — the session chrome BaseChat owns:
 *  - the top-right brand is ONE quiet chip (wordmark + a solid accent mark), not the
 *    "goose SWARM powered by LeanZero" cluster;
 *  - the two notifications are Panels with a StatusDot — no hand-written amber, no faded red.
 * Handlers are the props; the chrome only presents them.
 */

const wrap = (ui: React.ReactElement) => render(<IntlTestWrapper>{ui}</IntlTestWrapper>);

describe('BaseChat chrome — SessionBrand', () => {
  it('Flock edition: one quiet chip "LeanZero Flock" with a solid accent mark, linking to leanzero.net', () => {
    const { container } = wrap(<SessionBrand isLocal />);
    const chips = screen.getAllByTestId('lz-chip');
    expect(chips).toHaveLength(1);
    expect(chips[0].textContent).toBe('LeanZero Flock');
    expect(chips[0].getAttribute('data-tone')).toBeNull();
    expect(chips[0].className).toContain('border-lz-border-strong');
    expect(chips[0].className).not.toMatch(/uppercase|tracking-/);
    const mark = screen.getByTestId('brand-mark');
    expect(mark.className).toContain('bg-lz-accent');
    expect(mark.querySelector('svg')).not.toBeNull();
    const link = screen.getByTestId('local-edition-badge');
    expect(link.tagName).toBe('A');
    expect(link.getAttribute('href')).toBe(LEANZERO_WEBSITE_URL);
    expect(link.getAttribute('target')).toBe('_blank');
    expect(screen.queryByText(/powered by/)).toBeNull();
    expect(container.querySelector('[style]')).toBeNull();
    assertStudioClean(container);
  });

  it('standard edition: the goose wordmark as the same quiet chip, linking to the goose docs', () => {
    const { container } = wrap(<SessionBrand isLocal={false} />);
    const chip = screen.getByTestId('lz-chip');
    expect(chip.textContent).toBe('goose');
    expect(screen.getByTestId('goose-brand').getAttribute('href')).toBe('https://goose-docs.ai');
    expect(screen.queryByTestId('local-edition-badge')).toBeNull();
    assertStudioClean(container);
  });
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
      </>
    );
    const classes = allClasses(container).filter(
      (c) => !c.startsWith('lucide') && c !== 'goose-icon-animation' && c !== 'no-drag'
    );
    expect(classes.length).toBeGreaterThan(20);
    expect(await missingUtilities(classes)).toEqual([]);
  }, 30_000);
});
