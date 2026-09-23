import { describe, it, expect, vi } from 'vitest';
import { render, type RenderOptions, screen, fireEvent, waitFor } from '@testing-library/react';
import ExtensionItem from './ExtensionItem';
import { IntlTestWrapper } from '../../../../i18n/test-utils';
import type { FixedExtensionEntry } from '../../../ConfigContext';

vi.mock('../../../Layout/useStartChatAbout', () => ({ useStartChatAbout: () => vi.fn() }));
vi.mock('./ExtensionList', () => ({
  getSubtitle: (ext: { cmd?: string }) => ({ description: '', command: ext.cmd ?? '' }),
  getFriendlyTitle: (ext: { name: string }) => ext.name,
}));

const renderWithIntl = (ui: React.ReactElement, options?: RenderOptions) =>
  render(ui, { wrapper: IntlTestWrapper, ...options });

const makeExtension = (enabled: boolean): FixedExtensionEntry =>
  ({ name: 'developer', type: 'builtin', enabled }) as unknown as FixedExtensionEntry;

describe('ExtensionItem', () => {
  it('reflects the toggle as OFF immediately when disabling, before the async toggle resolves', async () => {
    // onToggle stays pending so we observe the in-flight (optimistic) state
    const onToggle = vi.fn(() => new Promise<void>(() => {}));
    renderWithIntl(<ExtensionItem extension={makeExtension(true)} onToggle={onToggle} />);

    const toggle = screen.getByRole('switch');
    expect(toggle).toHaveAttribute('aria-checked', 'true');

    fireEvent.click(toggle);

    await waitFor(() => {
      expect(screen.getByRole('switch')).toHaveAttribute('aria-checked', 'false');
    });
  });

  it('reflects the toggle as ON immediately when enabling, before the async toggle resolves', async () => {
    const onToggle = vi.fn(() => new Promise<void>(() => {}));
    renderWithIntl(<ExtensionItem extension={makeExtension(false)} onToggle={onToggle} />);

    const toggle = screen.getByRole('switch');
    expect(toggle).toHaveAttribute('aria-checked', 'false');

    fireEvent.click(toggle);

    await waitFor(() => {
      expect(screen.getByRole('switch')).toHaveAttribute('aria-checked', 'true');
    });
  });
});

describe('ExtensionItem state and details (UX audit M1)', () => {
  it('states enabled/off as a solid chip beside the name, and "Updating" while a toggle is in flight', async () => {
    const onToggle = vi.fn(() => new Promise<void>(() => {}));
    renderWithIntl(<ExtensionItem extension={makeExtension(true)} onToggle={onToggle} />);
    const chip = screen.getByText('Enabled');
    expect(chip.className).toContain('bg-lz-ok-solid');
    fireEvent.click(screen.getByRole('switch'));
    await waitFor(() => expect(screen.getByText('Updating').className).toContain('bg-lz-accent'));
  });

  it('an off extension carries the stopped fill, never grey text', () => {
    renderWithIntl(<ExtensionItem extension={makeExtension(false)} onToggle={vi.fn()} />);
    expect(screen.getByText('Off').className).toContain('bg-lz-stopped-solid');
  });

  it('connection details are an lz disclosure, not a native <details> triangle', () => {
    const ext = {
      name: 'fetch',
      type: 'stdio',
      enabled: true,
      cmd: 'uvx mcp-server-fetch',
    } as unknown as FixedExtensionEntry;
    const { container } = renderWithIntl(<ExtensionItem extension={ext} onToggle={vi.fn()} />);
    expect(container.querySelector('details, summary')).toBeNull();
    const toggle = screen.getByRole('button', { name: 'Connection details' });
    expect(screen.getByText('uvx mcp-server-fetch')).not.toBeVisible();
    fireEvent.click(toggle);
    expect(screen.getByText('uvx mcp-server-fetch')).toBeVisible();
  });
});
