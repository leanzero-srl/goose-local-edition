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
});

interface ExtensionItemProps {
  extension: FixedExtensionEntry;
  onToggle: (extension: FixedExtensionEntry) => Promise<boolean | void> | void;
  onConfigure?: (extension: FixedExtensionEntry) => void;
  isStatic?: boolean; // to not allow users to edit configuration
}

export default function ExtensionItem({
  extension,
  onToggle,
  onConfigure,
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

  return (
    <Card
      id={`extension-${kebabCase(extension.name)}`}
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
    </Card>
  );
}
