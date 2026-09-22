import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { IntlProvider } from 'react-intl';
import ExtensionItem, {
  askAboutExtensionPrompt,
} from '../settings/extensions/subcomponents/ExtensionItem';
import type { FixedExtensionEntry } from '../ConfigContext';

/** Skills, memories and MCPs share the one context menu: edit / delete through the surface that
 *  already owns them, and "start an AI session about this" with the item in the first message. */
const chat = vi.hoisted(() => ({ startChat: vi.fn() }));
vi.mock('./useStartChatAbout', () => ({ useStartChatAbout: () => chat.startChat }));
vi.mock('../settings/extensions/subcomponents/ExtensionList', () => ({
  getSubtitle: () => 'sub',
  getFriendlyTitle: (e: { name: string }) => e.name,
}));
vi.mock('../../acp/extensions', () => ({ inspectConfigExtension: vi.fn() }));

const wrap = (ui: React.ReactElement) =>
  render(
    <IntlProvider locale="en" messages={{}}>
      {ui}
    </IntlProvider>
  );

beforeEach(() => vi.clearAllMocks());

describe('the MCP card context menu', () => {
  const ext = {
    name: 'jira',
    type: 'stdio',
    cmd: 'npx',
    args: ['jira-mcp'],
    enabled: true,
    configKey: 'jira',
  } as unknown as FixedExtensionEntry;

  it('edit opens the configure modal, disable toggles, ask starts a chat naming the MCP, remove confirms first', async () => {
    const onConfigure = vi.fn();
    const onToggle = vi.fn().mockResolvedValue(true);
    const onDelete = vi.fn();
    wrap(
      <ExtensionItem
        extension={ext}
        onToggle={onToggle}
        onConfigure={onConfigure}
        onDelete={onDelete}
      />
    );
    fireEvent.contextMenu(screen.getByText('jira'));
    let menu = await screen.findByTestId('extension-context-menu');
    fireEvent.click(within(menu).getByText('Edit'));
    expect(onConfigure).toHaveBeenCalledWith(ext);

    fireEvent.contextMenu(screen.getByText('jira'));
    menu = await screen.findByTestId('extension-context-menu');
    fireEvent.click(within(menu).getByText('Disable'));
    await waitFor(() => expect(onToggle).toHaveBeenCalledWith(ext));

    fireEvent.contextMenu(screen.getByText('jira'));
    menu = await screen.findByTestId('extension-context-menu');
    fireEvent.click(within(menu).getByText('Start an AI session about this MCP'));
    expect(chat.startChat).toHaveBeenCalledWith(askAboutExtensionPrompt(ext));
    expect(askAboutExtensionPrompt(ext)).toContain('npx jira-mcp');

    fireEvent.contextMenu(screen.getByText('jira'));
    menu = await screen.findByTestId('extension-context-menu');
    fireEvent.click(within(menu).getByText('Remove'));
    expect(onDelete).not.toHaveBeenCalled();
    fireEvent.click(within(menu).getByText('Confirm remove jira'));
    expect(onDelete).toHaveBeenCalledWith(ext);
  });

  it('a builtin MCP cannot be edited or removed from the menu, and says why', async () => {
    const builtin = {
      name: 'developer',
      type: 'builtin',
      enabled: true,
    } as unknown as FixedExtensionEntry;
    wrap(
      <ExtensionItem
        extension={builtin}
        onToggle={vi.fn()}
        onConfigure={vi.fn()}
        onDelete={vi.fn()}
      />
    );
    fireEvent.contextMenu(screen.getByText('developer'));
    const menu = await screen.findByTestId('extension-context-menu');
    expect(within(menu).getByText('Edit').closest('button')).toBeDisabled();
    expect(within(menu).getByText('Remove').closest('button')).toBeDisabled();
    expect(within(menu).getByText('Remove').closest('button')?.getAttribute('title')).toMatch(
      /cannot be edited or removed/
    );
  });
});
