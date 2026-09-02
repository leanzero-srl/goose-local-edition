import { fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ConfirmCloseRunDialog } from './ConfirmCloseRunDialog';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { assertStudioClean } from '../lz/assertStudioClean';
import { SURFACE } from '../lz/tokens';

/**
 * Q2 (branch review, 2026-09-01): the traffic-light close button killed a live session run without
 * a word. main now keeps the window and asks THROUGH THIS DIALOG — a Studio overlay, never
 * window.confirm — and only "Stop run and close" lets the close through.
 */
const runs = [{ runId: '20260901-2302', runDir: '/proj', workingDir: '/proj' }];

function mount(overrides: Partial<Parameters<typeof ConfirmCloseRunDialog>[0]> = {}) {
  const onKeepRunning = vi.fn();
  const onStopAndClose = vi.fn();
  const utils = render(
    <ConfirmCloseRunDialog
      runs={runs}
      onKeepRunning={onKeepRunning}
      onStopAndClose={onStopAndClose}
      {...overrides}
    />,
    { wrapper: IntlTestWrapper }
  );
  return { ...utils, onKeepRunning, onStopAndClose };
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe('ConfirmCloseRunDialog', () => {
  it('is a modal Studio overlay: warn dot, the title, a body that says closing stops the run, the run named', () => {
    const { container } = mount();
    const dialog = screen.getByRole('dialog', { name: 'A swarm run is live in this window' });
    expect(dialog).toHaveAttribute('aria-modal', 'true');
    expect(dialog).toHaveAccessibleDescription(/Closing this window stops the run/);
    expect(screen.getByText('Run 20260901-2302 in /proj')).toBeInTheDocument();
    expect(screen.getByRole('img', { name: 'Warning' })).toBeInTheDocument();
    // The one elevation token, on the Panel surface.
    const panel = screen.getByTestId('lz-panel');
    for (const c of SURFACE.overlay.split(' ')) expect(panel.className).toContain(c);
    expect(screen.getByRole('button', { name: 'Keep running' })).toHaveAttribute(
      'data-variant',
      'secondary'
    );
    const stop = screen.getByRole('button', { name: 'Stop run and close' });
    expect(stop).toHaveAttribute('data-variant', 'destructive');
    expect(stop.className).toContain('bg-lz-err-solid');
    assertStudioClean(container);
  });

  it('opens with focus on the safe action, Keep running', () => {
    mount();
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Keep running' }));
  });

  it('Escape is Keep running — and never Stop', () => {
    const { onKeepRunning, onStopAndClose } = mount();
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(onKeepRunning).toHaveBeenCalledTimes(1);
    expect(onStopAndClose).not.toHaveBeenCalled();
  });

  it('Escape still keeps running when focus has left the dialog (the scrim was clicked)', () => {
    const { onKeepRunning } = mount();
    (document.activeElement as HTMLElement | null)?.blur();
    expect(document.activeElement).toBe(document.body);
    fireEvent.keyDown(document.body, { key: 'Escape' });
    expect(onKeepRunning).toHaveBeenCalledTimes(1);
  });

  it('Keep running by click replies keep', () => {
    const { onKeepRunning, onStopAndClose } = mount();
    fireEvent.click(screen.getByRole('button', { name: 'Keep running' }));
    expect(onKeepRunning).toHaveBeenCalledTimes(1);
    expect(onStopAndClose).not.toHaveBeenCalled();
  });

  it('Stop run and close fires the confirm ONCE, then disables both actions and ignores Escape', () => {
    const { onKeepRunning, onStopAndClose } = mount();
    const stop = screen.getByRole('button', { name: 'Stop run and close' });
    fireEvent.click(stop);
    expect(onStopAndClose).toHaveBeenCalledTimes(1);
    expect(screen.getByRole('button', { name: 'Stopping the run…' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Keep running' })).toBeDisabled();
    fireEvent.click(screen.getByRole('button', { name: 'Stopping the run…' }));
    expect(onStopAndClose).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(onKeepRunning).not.toHaveBeenCalled();
  });

  it('traps Tab inside the dialog in both directions', () => {
    mount();
    const dialog = screen.getByTestId('confirm-close-run-dialog');
    const keep = screen.getByRole('button', { name: 'Keep running' });
    const stop = screen.getByRole('button', { name: 'Stop run and close' });
    stop.focus();
    fireEvent.keyDown(dialog, { key: 'Tab' });
    expect(document.activeElement).toBe(keep);
    fireEvent.keyDown(dialog, { key: 'Tab', shiftKey: true });
    expect(document.activeElement).toBe(stop);
    // Focus that escaped comes back to the first control on the next Tab.
    stop.blur();
    fireEvent.keyDown(dialog, { key: 'Tab' });
    expect(document.activeElement).toBe(keep);
  });

  it('returns focus to where it was when the dialog goes away', () => {
    const outside = document.createElement('button');
    outside.textContent = 'composer';
    document.body.appendChild(outside);
    outside.focus();
    const { unmount } = mount();
    expect(document.activeElement).not.toBe(outside);
    unmount();
    expect(document.activeElement).toBe(outside);
    outside.remove();
  });

  it('names every live run when the window watches more than one', () => {
    mount({
      runs: [
        ...runs,
        { runId: 'bench', runDir: '/bench/app', workingDir: '/bench/app' },
      ],
    });
    expect(screen.getByTestId('confirm-close-run-runs').children).toHaveLength(2);
    expect(screen.getByText('Run bench in /bench/app')).toBeInTheDocument();
  });

  it('never reaches for a native primitive', () => {
    const confirm = vi.spyOn(window, 'confirm').mockReturnValue(true);
    const alert = vi.spyOn(window, 'alert').mockImplementation(() => undefined);
    const prompt = vi.spyOn(window, 'prompt').mockReturnValue(null);
    mount();
    fireEvent.keyDown(window, { key: 'Escape' });
    fireEvent.click(screen.getByRole('button', { name: 'Stop run and close' }));
    expect(confirm).not.toHaveBeenCalled();
    expect(alert).not.toHaveBeenCalled();
    expect(prompt).not.toHaveBeenCalled();
  });
});
