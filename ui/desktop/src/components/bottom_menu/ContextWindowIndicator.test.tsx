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
      <ContextWindowIndicator totalTokens={70_000} tokenLimit={100_000} alerts={[]} liveTokens={10_000} />
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
