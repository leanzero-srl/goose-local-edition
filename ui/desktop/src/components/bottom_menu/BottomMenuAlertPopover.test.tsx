import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import BottomMenuAlertPopover from './BottomMenuAlertPopover';
import { AlertType, type Alert } from '../alerts';

const alert = (type: AlertType): Alert => ({
  type,
  message: 'Context window',
  progress: { current: 1000, total: 128000 },
});

/** UX audit C1: the context chip's info alert painted a GREEN dot, which read as "ready" on a swarm
 *  whose only node could not answer. The dot is a warning mark only; readiness is the composer's
 *  readiness strip, from engine facts. */
describe('BottomMenuAlertPopover dot', () => {
  it('an info alert (context under 75%) shows no dot at all — never a green light', () => {
    render(
      <BottomMenuAlertPopover alerts={[alert(AlertType.Info)]}>
        <span>0 / 128k</span>
      </BottomMenuAlertPopover>
    );
    expect(screen.getByText('0 / 128k')).toBeInTheDocument();
    expect(screen.queryByTestId('alert-indicator-dot')).toBeNull();
  });

  it('a warning shows the warn dot and an error the err dot', () => {
    const { rerender } = render(<BottomMenuAlertPopover alerts={[alert(AlertType.Warning)]} />);
    expect(screen.getByTestId('alert-indicator-dot').className).toContain('text-lz-warn');
    rerender(<BottomMenuAlertPopover alerts={[alert(AlertType.Error)]} />);
    expect(screen.getByTestId('alert-indicator-dot').className).toContain('text-lz-err');
  });
});
