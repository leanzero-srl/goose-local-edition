import { useState, useEffect } from 'react';
import kebabCase from 'lodash/kebabCase';
import { Switch } from '../../../ui/switch';
import { Gear } from '../../../icons';
import { FixedExtensionEntry } from '../../../ConfigContext';
import { getSubtitle, getFriendlyTitle } from './ExtensionList';
import { Card, CardHeader, CardTitle, CardContent, CardAction } from '../../../ui/card';
import { defineMessages, useIntl } from '../../../../i18n';
import { inspectConfigExtension } from '../../../../acp/extensions';
import { McpCapabilities } from '../../../extensions/McpCapabilities';
import { Button } from '../../../lz';
import { TreeContextMenu } from '../../../Layout/tree';
import { useStartChatAbout } from '../../../Layout/useStartChatAbout';
import { Pencil, Power, Sparkles, Trash2 } from 'lucide-react';
import type { McpToolInfo } from '../../../../types/mcpSetup';

const i18n = defineMessages({
  configureExtension: {
    id: 'extensionItem.configureExtension',
    defaultMessage: 'Configure {name} Extension',
  },
  toggleExtension: {
    id: 'extensionItem.toggleExtension',
    defaultMessage: 'Toggle {name} extension On or Off',
  },
  menuEdit: { id: 'extensionItem.menuEdit', defaultMessage: 'Edit' },
  menuEnable: { id: 'extensionItem.menuEnable', defaultMessage: 'Enable' },
  menuDisable: { id: 'extensionItem.menuDisable', defaultMessage: 'Disable' },
  menuAsk: { id: 'extensionItem.menuAsk', defaultMessage: 'Start an AI session about this MCP' },
  menuRemove: { id: 'extensionItem.menuRemove', defaultMessage: 'Remove' },
  menuConfirmRemove: {
    id: 'extensionItem.menuConfirmRemove',
    defaultMessage: 'Confirm remove {name}',
  },
  menuNotEditable: {
    id: 'extensionItem.menuNotEditable',
    defaultMessage: 'Built-in and bundled extensions cannot be edited or removed',
  },
});

/** What is asked of the model when an MCP is opened as a chat about it: how it is launched, where
 *  its configuration lives, and the tools that enable, disable and rewrite it. */
export function askAboutExtensionPrompt(extension: FixedExtensionEntry): string {
  const kind = extension.type;
  const where =
    'cmd' in extension && typeof extension.cmd === 'string'
      ? ` (command: ${extension.cmd}${'args' in extension && Array.isArray(extension.args) ? ' ' + extension.args.join(' ') : ''})`
      : 'uri' in extension && typeof extension.uri === 'string'
        ? ` (${extension.uri})`
        : '';
  return [
    `I want to work on my goose MCP extension "${getFriendlyTitle(extension)}" — config name "${extension.name}", type ${kind}${where}.`,
    `Its configuration is the "${extension.name}" entry under extensions: in ~/.config/goose/config.yaml (name, type, cmd/args or uri, envs, timeout, enabled). Read that entry first with the developer tools.`,
    'You can change it in place by editing that entry, fork it by adding a new entry with a new name beside it, or add a brand-new MCP the same way; manage_extensions enables or disables an extension by name and search_available_extensions lists the ones goose knows about. Changes to config.yaml apply to the next session.',
    'Ask me what I want changed before you write anything, then make the change and show me the resulting entry.',
  ].join('\n');
}

interface ExtensionItemProps {
  extension: FixedExtensionEntry;
  onToggle: (extension: FixedExtensionEntry) => Promise<boolean | void> | void;
  onConfigure?: (extension: FixedExtensionEntry) => void;
  /** Remove the extension from the config (the list's context menu; confirmed in-menu). */
  onDelete?: (extension: FixedExtensionEntry) => void;
  isStatic?: boolean; // to not allow users to edit configuration
}

export default function ExtensionItem({
  extension,
  onToggle,
  onConfigure,
  onDelete,
  isStatic,
}: ExtensionItemProps) {
  const intl = useIntl();
  // Add local state to track the visual toggle state
  const [visuallyEnabled, setVisuallyEnabled] = useState(extension.enabled);
  // Track if we're in the process of toggling
  const [isToggling, setIsToggling] = useState(false);
  const [error, setError] = useState('');
  const [tools, setTools] = useState<McpToolInfo[] | null>(null);

  const handleToggle = async (ext: FixedExtensionEntry) => {
    // Prevent multiple toggles while one is in progress
    if (isToggling) return;

    setIsToggling(true);
    setError('');

    // Immediately update visual state
    const newState = !ext.enabled;
    setVisuallyEnabled(newState);

    try {
      // Call the actual toggle function that performs the async operation
      const result = await onToggle(ext);
      if (result === false) throw new Error('The extension could not be updated.');
      // Success case is handled by the useEffect below when extension.enabled changes
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      // If there was an error, revert the visual state
      setVisuallyEnabled(!newState);
    } finally {
      setIsToggling(false);
    }
  };

  // Update visual state when the actual extension state changes
  useEffect(() => {
    if (!isToggling) {
      setVisuallyEnabled(extension.enabled);
    }
  }, [extension.enabled, isToggling]);

  const renderSubtitle = () => {
    const { description, command } = getSubtitle(extension);
    return (
      <>
        {description && <span>{description}</span>}
        {command && (
          <details className="mt-3">
            <summary className="cursor-pointer text-xs font-medium">Connection details</summary>
            <code className="mt-2 block break-all text-xs">{command}</code>
          </details>
        )}
      </>
    );
  };

  // Bundled extensions and builtins are not editable
  // Over time we can take the first part of the conditional away as people have bundled: true in their config.yaml entries

  // allow configuration editing if extension is not a builtin/bundled extension AND isStatic = false
  const editable =
    !(extension.type === 'builtin' || ('bundled' in extension && extension.bundled)) && !isStatic;

  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const startChat = useStartChatAbout();
  const title = getFriendlyTitle(extension);
  return (
    <Card
      id={`extension-${kebabCase(extension.name)}`}
      onContextMenu={(e) => {
        e.preventDefault();
        setMenu({ x: e.clientX, y: e.clientY });
      }}
      className="transition-colors duration-200 min-h-[120px] overflow-hidden border-lz-border bg-lz-surface text-lz-ink"
    >
      <CardHeader>
        <CardTitle>{getFriendlyTitle(extension)}</CardTitle>

        <CardAction>
          <div className="flex items-center justify-end gap-2">
            {editable && (
              <button
                className="text-lz-ink-2 hover:text-lz-ink"
                aria-label={intl.formatMessage(i18n.configureExtension, {
                  name: getFriendlyTitle(extension),
                })}
                onClick={() => onConfigure?.(extension)}
              >
                <Gear className="w-4 h-4" />
              </button>
            )}
            <Switch
              checked={visuallyEnabled}
              onCheckedChange={() => handleToggle(extension)}
              disabled={isToggling}
              variant="mono"
              aria-label={intl.formatMessage(i18n.toggleExtension, {
                name: getFriendlyTitle(extension),
              })}
            />
          </div>
        </CardAction>
      </CardHeader>
      <CardContent className="px-4 overflow-hidden text-sm break-words text-lz-ink-2">
        {renderSubtitle()}
        {['stdio', 'sse', 'streamable_http'].includes(extension.type) && (
          <div className="mt-4 space-y-3">
            <Button
              onClick={async () => {
                setError('');
                setTools(null);
                try {
                  const result = await inspectConfigExtension(extension.name);
                  setTools(result.tools as unknown as McpToolInfo[]);
                } catch (e) {
                  setError(e instanceof Error ? e.message : String(e));
                }
              }}
            >
              Test connection & discover tools
            </Button>
            {tools && <McpCapabilities tools={tools} />}
          </div>
        )}
        {error && (
          <p role="alert" className="mt-2 text-lz-err">
            {error}
          </p>
        )}
      </CardContent>
      {menu && (
        <TreeContextMenu
          x={menu.x}
          y={menu.y}
          testId="extension-context-menu"
          onClose={() => setMenu(null)}
          items={[
            {
              key: 'edit',
              label: intl.formatMessage(i18n.menuEdit),
              icon: <Pencil />,
              disabled: !editable || !onConfigure,
              title: editable ? undefined : intl.formatMessage(i18n.menuNotEditable),
              onClick: () => {
                setMenu(null);
                onConfigure?.(extension);
              },
            },
            {
              key: 'toggle',
              label: intl.formatMessage(visuallyEnabled ? i18n.menuDisable : i18n.menuEnable),
              icon: <Power />,
              disabled: isToggling,
              onClick: () => {
                setMenu(null);
                void handleToggle(extension);
              },
            },
            {
              key: 'ask',
              label: intl.formatMessage(i18n.menuAsk),
              icon: <Sparkles />,
              onClick: () => {
                setMenu(null);
                void startChat(askAboutExtensionPrompt(extension));
              },
            },
            {
              key: 'remove',
              label: intl.formatMessage(i18n.menuRemove),
              icon: <Trash2 />,
              danger: true,
              separator: true,
              disabled: !editable || !onDelete,
              title: editable ? undefined : intl.formatMessage(i18n.menuNotEditable),
              confirmLabel: intl.formatMessage(i18n.menuConfirmRemove, { name: title }),
              onClick: () => {
                setMenu(null);
                onDelete?.(extension);
              },
            },
          ]}
        />
      )}
    </Card>
  );
}
