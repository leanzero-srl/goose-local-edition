import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { SystemNotificationInline } from './SystemNotificationInline';

const renderLine = (msg: string) =>
  render(
    <IntlTestWrapper>
      <SystemNotificationInline notification={{ msg, notificationType: 'inlineMessage' }} />
    </IntlTestWrapper>
  );

/**
 * Q-173, critic round 2 (3.0.57): "goose check: the answer says both … they cannot both hold." was
 * 12 px grey-400 — faded, against the house rule — and "Suggested skill …" the same.
 */
describe('SystemNotificationInline (Q-173)', () => {
  it('a goose check leads with a SOLID warning chip and reads as body copy', () => {
    renderLine(
      'goose check: the answer says both "Every fact … nothing dropped." and "One thing I flagged …"; they cannot both hold.'
    );
    const row = screen.getByTestId('system-notification-check');
    const chip = screen.getByTestId('lz-chip');
    expect(chip).toHaveAttribute('data-tone', 'warn');
    expect(chip.className).toContain('bg-lz-warn-solid');
    expect(chip.textContent).toBe('goose check');
    expect(row.textContent).toContain('they cannot both hold.');
    expect(row.textContent).not.toContain('goose check:');
    expect(row.innerHTML).not.toMatch(/gray-400|text-xs|opacity-/);
  });

  it('any other line (a suggested skill) is body copy in ink-2, never faded grey', () => {
    renderLine('Suggested skill atlassian-migration-scripts-skill');
    const line = screen.getByTestId('system-notification-inline');
    expect(line.className).toContain('text-lz-ink-2');
    expect(line.className).toContain('text-lz-body');
    expect(line.className).not.toMatch(/gray-400|text-xs/);
    expect(screen.queryByTestId('lz-chip')).toBeNull();
  });
});
