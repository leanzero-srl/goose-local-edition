import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { TooltipProvider } from '../ui/Tooltip';
import { ContextWindowIndicator } from './ContextWindowIndicator';

const mount = (ui: React.ReactElement) =>
  render(
    <IntlTestWrapper>
      <TooltipProvider>{ui}</TooltipProvider>
    </IntlTestWrapper>
  );

/**
 * Q-153, round live-1 (3.0.52): the counter read "40k / 262k" for 39 minutes while the split held
 * the 39,996-token prompt AND the 24,228 tokens it had written — goose reports usage only when a
 * turn ends. The engine's live request is what grows the context during the turn.
 */
describe('Q-153: the context counter grows with the answer being written', () => {
  it('shows the tokens written so far beside the context, and says what each number is', () => {
    mount(
      <ContextWindowIndicator
        totalTokens={39_996}
        tokenLimit={262_144}
        alerts={[]}
        liveTokens={24_228}
      />
    );
    const gauge = screen.getByTestId('context-window-indicator');
    expect(gauge).toHaveTextContent('40k + 24k / 262k');
    expect(screen.getByTestId('context-window-live')).toHaveTextContent('+ 24k');
    expect(gauge).toHaveAttribute(
      'title',
      '39,996 tokens of context + 24,228 being written now, of a 262,144-token window'
    );
  });

  it('colours by what the engine holds now, the written tokens included', () => {
    mount(
      <ContextWindowIndicator
        totalTokens={70_000}
        tokenLimit={100_000}
        alerts={[]}
        liveTokens={10_000}
      />
    );
    expect(screen.getByTestId('context-window-indicator').className).toContain('text-lz-warn');
  });

  it('between turns (nothing being written) it is the plain count', () => {
    mount(<ContextWindowIndicator totalTokens={39_996} tokenLimit={262_144} alerts={[]} />);
    const gauge = screen.getByTestId('context-window-indicator');
    expect(gauge).toHaveTextContent('40k / 262k');
    expect(screen.queryByTestId('context-window-live')).toBeNull();
    expect(gauge).not.toHaveAttribute('title');
  });
});

/**
 * Q-467: a strategy chat's turn answered by a node that reports no window (deepseek, which the
 * catalog lacks) showed "42k / 262k" — the pool's split window. The chip now says what it knows.
 */
describe('Q-467: a window the serving node did not report', () => {
  it('says "used · window unknown" beside the count, never a number for the window', () => {
    mount(
      <ContextWindowIndicator totalTokens={42_000} tokenLimit={0} alerts={[]} windowUnknown />
    );
    const gauge = screen.getByTestId('context-window-indicator');
    expect(gauge).toHaveTextContent('42k used · window unknown');
    expect(gauge.textContent).not.toContain('/');
    expect(gauge).toHaveAttribute('data-window', 'unknown');
  });

  it('a stale window never shows while the served node said none', () => {
    mount(
      <ContextWindowIndicator totalTokens={42_000} tokenLimit={262_144} alerts={[]} windowUnknown />
    );
    expect(screen.getByTestId('context-window-indicator').textContent).not.toContain('262k');
  });

  it('nothing used yet: no chip', () => {
    mount(<ContextWindowIndicator totalTokens={0} tokenLimit={0} alerts={[]} windowUnknown />);
    expect(screen.queryByTestId('context-window-indicator')).toBeNull();
  });
});
