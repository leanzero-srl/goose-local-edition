import ExtensionItem from './ExtensionItem';
import builtInExtensionsData from '../../../../built-in-extensions.json';
import type { ExtensionConfig } from '../../../../types/extensions';
import { FixedExtensionEntry } from '../../../ConfigContext';
import { combineCmdAndArgs } from '../utils';
import { defineMessages, useIntl } from '../../../../i18n';
import { SectionHeader } from '../../../lz';

const i18n = defineMessages({
  defaultExtensionsTitle: {
    id: 'extensionList.defaultExtensionsTitle',
    defaultMessage: 'Default extensions',
  },
  availableExtensionsTitle: {
    id: 'extensionList.availableExtensionsTitle',
    defaultMessage: 'Available extensions',
  },
  noExtensions: {
    id: 'extensionList.noExtensions',
    defaultMessage: 'No extensions available',
  },
  builtInExtension: {
    id: 'extensionList.builtInExtension',
    defaultMessage: 'Built-in extension',
  },
});

interface ExtensionListProps {
  extensions: FixedExtensionEntry[];
  onToggle: (extension: FixedExtensionEntry) => Promise<boolean | void> | void;
  onConfigure?: (extension: FixedExtensionEntry) => void;
  onDelete?: (extension: FixedExtensionEntry) => void;
  isStatic?: boolean;
  disableConfiguration?: boolean;
  searchTerm?: string;
}

export default function ExtensionList({
  extensions,
  onToggle,
  onConfigure,
  onDelete,
  isStatic,
  disableConfiguration: _disableConfiguration,
  searchTerm = '',
}: ExtensionListProps) {
  const matchesSearch = (extension: FixedExtensionEntry): boolean => {
    if (!searchTerm) return true;

    const searchLower = searchTerm.toLowerCase();
    const title = getFriendlyTitle(extension).toLowerCase();
    const name = extension.name.toLowerCase();
    const subtitle = getSubtitle(extension);
    const description = subtitle.description?.toLowerCase() || '';

    return (
      title.includes(searchLower) || name.includes(searchLower) || description.includes(searchLower)
    );
  };

  const intl = useIntl();

  // Separate enabled and disabled extensions, then filter by search term
  const enabledExtensions = extensions.filter((ext) => ext.enabled && matchesSearch(ext));
  const disabledExtensions = extensions.filter((ext) => !ext.enabled && matchesSearch(ext));

  // Sort each group alphabetically by their friendly title
  const sortedEnabledExtensions = [...enabledExtensions].sort((a, b) =>
    getFriendlyTitle(a).localeCompare(getFriendlyTitle(b))
  );
  const sortedDisabledExtensions = [...disabledExtensions].sort((a, b) =>
    getFriendlyTitle(a).localeCompare(getFriendlyTitle(b))
  );

  return (
    <div className="space-y-8">
      {sortedEnabledExtensions.length > 0 && (
        <div>
          <SectionHeader
            className="mb-3"
            title={intl.formatMessage(i18n.defaultExtensionsTitle)}
            count={sortedEnabledExtensions.length}
          />
          <div className="grid grid-cols-1 lg:grid-cols-2 gap-3">
            {sortedEnabledExtensions.map((extension) => (
              <ExtensionItem
                key={extension.name}
                extension={extension}
                onToggle={onToggle}
                onConfigure={onConfigure}
                onDelete={onDelete}
                isStatic={isStatic}
              />
            ))}
          </div>
        </div>
      )}

      {sortedDisabledExtensions.length > 0 && (
        <div>
          <SectionHeader
            className="mb-3"
            title={intl.formatMessage(i18n.availableExtensionsTitle)}
            count={sortedDisabledExtensions.length}
          />
          <div className="grid grid-cols-1 lg:grid-cols-2 gap-3">
            {sortedDisabledExtensions.map((extension) => (
              <ExtensionItem
                key={extension.name}
                extension={extension}
                onToggle={onToggle}
                onConfigure={onConfigure}
                onDelete={onDelete}
                isStatic={isStatic}
              />
            ))}
          </div>
        </div>
      )}

      {extensions.length === 0 && (
        <div className="text-center text-lz-ink-2 py-8">
          {intl.formatMessage(i18n.noExtensions)}
        </div>
      )}
    </div>
  );
}

// Helper functions
export function formatExtensionName(name: string): string {
  return name
    .split(/[-_]/) // Split on hyphens and underscores
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(' ');
}

export function getFriendlyTitle(extension: FixedExtensionEntry): string {
  const builtin = extension.type === 'builtin' || extension.type === 'platform';
  if (builtin && extension.display_name) return formatExtensionName(extension.display_name);
  // A builtin entry written without display_name still has a real name in the bundled catalogue
  // ("computercontroller" is "Computer Controller") — never title-case the internal id instead.
  const known = builtin
    ? builtInExtensionsData.find(
        (ext) => normalizeExtensionName(ext.id) === normalizeExtensionName(extension.name)
      )
    : undefined;
  return formatExtensionName(known?.name ?? extension.name);
}

function normalizeExtensionName(name: string): string {
  return name.toLowerCase().replace(/\s+/g, '');
}

export function getSubtitle(config: ExtensionConfig) {
  switch (config.type) {
    case 'builtin': {
      const extensionData = builtInExtensionsData.find(
        (ext) => normalizeExtensionName(ext.name) === normalizeExtensionName(config.name)
      );
      return {
        description: extensionData?.description || config.description || 'Built-in extension',
        command: null,
      };
    }
    case 'sse':
    case 'streamable_http': {
      const label = config.type === 'sse' ? 'SSE' : 'HTTP';
      return {
        description: config.description ? `${label}: ${config.description}` : `${label} extension`,
        command: config.uri || null,
      };
    }

    default:
      return {
        description: config.description || null,
        command: 'cmd' in config ? combineCmdAndArgs(config.cmd, config.args ?? []) : null,
      };
  }
}
