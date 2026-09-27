import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { DiagnosticsModal } from './Diagnostics';

vi.mock('../../acp/diagnostics', () => ({ getDiagnosticsReport: vi.fn() }));

const mount = (onClose = vi.fn()) =>
  render(
    <IntlTestWrapper>
      <DiagnosticsModal isOpen onClose={onClose} sessionId="s1" />
    </IntlTestWrapper>
  );

/** The layer painted over the app behind the dialog: the first fixed full-screen element. */
const backdrop = () =>
  Array.from(document.querySelectorAll<HTMLElement>('.fixed')).find((el) =>
    el.className.includes('inset-0')
  );

describe('Report a problem — a real dialog (Q-21)', () => {
  it('Escape closes it, and it is announced as a modal dialog named by its title', () => {
    const onClose = vi.fn();
    mount(onClose);
    const dialog = screen.getByRole('dialog');
    expect(dialog).toHaveAccessibleName('Report a Problem');
    fireEvent.keyDown(dialog, { key: 'Escape' });
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});

describe('Report a problem — the backdrop (Q-23)', () => {
  it('dims the app at half strength — never solid black (bg-opacity-* does nothing on Tailwind 4)', () => {
    mount();
    const layer = backdrop();
    expect(layer).toBeDefined();
    expect(layer!.className).toContain('bg-black/50');
    expect(layer!.className).not.toMatch(/bg-opacity-/);
    expect(screen.getByRole('button', { name: /Download/ })).toBeInTheDocument();
  });
});
