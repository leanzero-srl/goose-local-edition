import { fireEvent, render, screen } from '@testing-library/react';
import { useState } from 'react';
import { describe, expect, it } from 'vitest';
import { DetailModeChooser, nextDetailMode } from './SwarmRunPanel';
import type { SwarmLogMode } from './useVerboseSwarm';

function StatefulChooser() {
  const [mode, setMode] = useState<SwarmLogMode>('verbose');
  return <DetailModeChooser mode={mode} onChange={setMode} />;
}

describe('DetailModeChooser', () => {
  it('maps radio navigation keys onto the explicit detail choices', () => {
    expect(nextDetailMode('compact', 'ArrowRight')).toBe('verbose');
    expect(nextDetailMode('compact', 'ArrowLeft')).toBe('developer');
    expect(nextDetailMode('verbose', 'Home')).toBe('compact');
    expect(nextDetailMode('verbose', 'End')).toBe('developer');
    expect(nextDetailMode('verbose', 'Enter')).toBeNull();
  });

  // The old control was ONE button that cycled: the two modes you were not in were invisible, and reaching
  // Developer from Compact meant guessing that clicking twice would get you there.
  it('shows all three modes at once, with the current one marked', () => {
    render(<StatefulChooser />);
    const choices = screen.getAllByRole('radio');
    expect(choices.map((c) => c.textContent)).toEqual(['Compact', 'Verbose', 'Developer']);
    expect(choices[1]).toHaveAttribute('aria-checked', 'true');
  });

  it('uses roving tabindex, moves focus, and exposes a contrasting selected focus ring', () => {
    render(<StatefulChooser />);
    const choices = screen.getAllByRole('radio');

    expect(choices.map((choice) => choice.tabIndex)).toEqual([-1, 0, -1]);
    expect(choices[1]).toHaveClass('focus-visible:ring-white');

    choices[1].focus();
    fireEvent.keyDown(choices[1], { key: 'ArrowRight' });
    expect(choices[2]).toHaveAttribute('aria-checked', 'true');
    expect(choices[2]).toHaveFocus();

    fireEvent.keyDown(choices[2], { key: 'Home' });
    expect(choices[0]).toHaveAttribute('aria-checked', 'true');
    expect(choices[0]).toHaveFocus();

    fireEvent.keyDown(choices[0], { key: 'End' });
    expect(choices[2]).toHaveAttribute('aria-checked', 'true');
    expect(choices[2]).toHaveFocus();
  });
});
