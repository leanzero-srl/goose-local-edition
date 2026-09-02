import { fireEvent, render, screen } from '@testing-library/react';
import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest';
import { Clipped, RevealGlyph } from './Clipped';
import { assertStudioClean } from '../lz/assertStudioClean';

/**
 * THE CLIPPED-TEXT CONTRACT (Mihai, 2026-09-02: "have a way to click on that element and bring it up").
 * jsdom lays nothing out, so overflow is simulated the way the browser reports it — scrollWidth beyond
 * clientWidth on the measured span — and switched off to prove a short line gains no chrome.
 */

let overflowing = false;
beforeAll(() => {
  Object.defineProperty(HTMLElement.prototype, 'scrollWidth', {
    configurable: true,
    get: () => (overflowing ? 1000 : 100),
  });
  Object.defineProperty(HTMLElement.prototype, 'clientWidth', {
    configurable: true,
    get: () => 100,
  });
});
afterAll(() => {
  delete (HTMLElement.prototype as unknown as Record<string, unknown>).scrollWidth;
  delete (HTMLElement.prototype as unknown as Record<string, unknown>).clientWidth;
});

const BRIEF =
  'Build the ledger store: a SQLite-backed append-only table of payments with (id, vendor, amount_cents, ' +
  'currency, posted_at) and a `Store.add(payment)` that rejects a duplicate id with StoreError, ' +
  'a `Store.list(vendor=None, since=None)` returning rows newest-first, and a `Store.export_csv(path)` ' +
  'that writes the RFC 4180 form the vendor docs at /v3/docs describe. Owns store.py and tests/test_store.py; ' +
  'the api slice imports Store from it and nothing else.';

describe('Clipped', () => {
  it('a clipped line is a button: title, glyph, the reveal with the full text, copy, Escape, focus back', () => {
    overflowing = true;
    const { container } = render(
      <Clipped
        text={BRIEF}
        label="Task brief"
        context={[
          { label: 'task', value: 'store' },
          { label: 'phase', value: 'build' },
        ]}
      />
    );
    const control = screen.getByTestId('clipped-text');
    expect(control).toHaveAttribute('data-clipped', 'true');
    expect(control).toHaveAttribute('role', 'button');
    expect(control).toHaveAttribute('title', BRIEF);
    expect(control.querySelector('svg')).not.toBeNull();
    assertStudioClean(container);

    fireEvent.click(control);
    const dialog = screen.getByRole('dialog');
    expect(screen.getByTestId('reveal-body')).toHaveTextContent(BRIEF);
    expect(dialog).toHaveTextContent('Task brief');
    expect(dialog).toHaveTextContent('store');
    expect(dialog).toHaveTextContent('build');
    assertStudioClean(dialog);
    expect(document.activeElement).toBe(screen.getByTestId('reveal-close'));

    fireEvent.click(screen.getByTestId('reveal-copy'));
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith(BRIEF);

    fireEvent.keyDown(document.activeElement ?? document.body, { key: 'Escape' });
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(document.activeElement).toBe(control);
  });

  it('a line that fits is a plain span — no role, no title, no glyph', () => {
    overflowing = false;
    render(<Clipped text="short" label="Task brief" />);
    const control = screen.getByTestId('clipped-text');
    expect(control).toHaveAttribute('data-clipped', 'false');
    expect(control).not.toHaveAttribute('role');
    expect(control).not.toHaveAttribute('title');
    expect(control.querySelector('svg')).toBeNull();
    fireEvent.click(control);
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('a summary derived from a longer brief is clipped even when it fits, and the reveal holds the brief', () => {
    overflowing = false;
    render(<Clipped text={BRIEF.slice(0, 87) + '…'} full={BRIEF} label="Task brief" />);
    const control = screen.getByTestId('clipped-text');
    expect(control).toHaveAttribute('data-clipped', 'true');
    expect(control).toHaveAttribute('title', BRIEF);
    fireEvent.keyDown(control, { key: 'Enter' });
    expect(screen.getByTestId('reveal-body')).toHaveTextContent(BRIEF);
  });

  it('opening the reveal never fires the row it sits in; Escape never reaches the inspector', () => {
    overflowing = true;
    const rowClick = vi.fn();
    const rowKey = vi.fn();
    const windowEscape = vi.fn();
    window.addEventListener('keydown', windowEscape);
    render(
      <div role="button" tabIndex={0} onClick={rowClick} onKeyDown={rowKey}>
        <Clipped text={BRIEF} label="Task brief" />
      </div>
    );
    const control = screen.getByTestId('clipped-text');
    fireEvent.click(control);
    expect(rowClick).not.toHaveBeenCalled();
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    fireEvent.keyDown(document.activeElement ?? document.body, { key: 'Escape' });
    expect(windowEscape).not.toHaveBeenCalled();
    expect(screen.queryByRole('dialog')).toBeNull();

    fireEvent.keyDown(control, { key: ' ' });
    expect(rowKey).not.toHaveBeenCalled();
    expect(screen.getByRole('dialog')).toBeInTheDocument();
    window.removeEventListener('keydown', windowEscape);
  });

  it('a mono reveal keeps the monospace body', () => {
    overflowing = true;
    render(<Clipped text="$ pytest -q tests/" label="Working on" mono />);
    fireEvent.click(screen.getByTestId('clipped-text'));
    expect(screen.getByTestId('reveal-body').className).toContain('font-mono');
  });
});

describe('RevealGlyph', () => {
  it('is a real button that opens the reveal and takes focus back on close, without firing its row', () => {
    const rowClick = vi.fn();
    const { container } = render(
      <div onClick={rowClick}>
        <RevealGlyph spec={{ label: 'Working on', text: BRIEF, mono: true, context: [{ label: 'node', value: 'A' }] }} />
      </div>
    );
    const glyph = screen.getByTestId('reveal-glyph');
    expect(glyph.tagName).toBe('BUTTON');
    expect(glyph).toHaveAttribute('title', 'Show the full working on');
    assertStudioClean(container);
    fireEvent.click(glyph);
    expect(rowClick).not.toHaveBeenCalled();
    expect(screen.getByTestId('reveal-body')).toHaveTextContent(BRIEF);
    fireEvent.click(screen.getByTestId('reveal-backdrop'));
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(document.activeElement).toBe(glyph);
  });
});
