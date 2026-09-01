import { useState } from 'react';
import { ChevronDown, ChevronUp } from 'lucide-react';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from './ui/collapsible';
import { Button, FOCUS, MOTION, StatusDot, SURFACE, TONE_TEXT, TYPE, WEIGHT, cx } from './lz';
import { formatExtensionErrorMessage } from '../utils/extensionErrorUtils';
import { formatExtensionName } from './settings/extensions/subcomponents/ExtensionList';
import { defineMessages, useIntl } from '../i18n';

const i18n = defineMessages({
  loadingExtensions: {
    id: 'groupedExtensionLoadingToast.loadingExtensions',
    defaultMessage: 'Loading {count, plural, one {# extension} other {# extensions}}...',
  },
  successfullyLoaded: {
    id: 'groupedExtensionLoadingToast.successfullyLoaded',
    defaultMessage: 'Successfully loaded {count, plural, one {# extension} other {# extensions}}',
  },
  partiallyLoaded: {
    id: 'groupedExtensionLoadingToast.partiallyLoaded',
    defaultMessage: 'Loaded {successCount}/{totalCount, plural, one {# extension} other {# extensions}}',
  },
  failedToLoad: {
    id: 'groupedExtensionLoadingToast.failedToLoad',
    defaultMessage: '{count, plural, one {# extension} other {# extensions}} failed to load',
  },
  failedToAddExtension: {
    id: 'groupedExtensionLoadingToast.failedToAddExtension',
    defaultMessage: 'Failed to add extension',
  },
  copied: {
    id: 'groupedExtensionLoadingToast.copied',
    defaultMessage: 'Copied!',
  },
  copyError: {
    id: 'groupedExtensionLoadingToast.copyError',
    defaultMessage: 'Copy error',
  },
  showLess: {
    id: 'groupedExtensionLoadingToast.showLess',
    defaultMessage: 'Show less',
  },
  showDetails: {
    id: 'groupedExtensionLoadingToast.showDetails',
    defaultMessage: 'Show details',
  },
  collapseDetails: {
    id: 'groupedExtensionLoadingToast.collapseDetails',
    defaultMessage: 'Collapse details',
  },
  expandDetails: {
    id: 'groupedExtensionLoadingToast.expandDetails',
    defaultMessage: 'Expand details',
  },
  statusLoading: {
    id: 'groupedExtensionLoadingToast.statusLoading',
    defaultMessage: 'Loading',
  },
  statusLoaded: {
    id: 'groupedExtensionLoadingToast.statusLoaded',
    defaultMessage: 'Loaded',
  },
  statusPartial: {
    id: 'groupedExtensionLoadingToast.statusPartial',
    defaultMessage: 'Partially loaded',
  },
  statusFailed: {
    id: 'groupedExtensionLoadingToast.statusFailed',
    defaultMessage: 'Failed',
  },
});

export interface ExtensionLoadingStatus {
  name: string;
  status: 'loading' | 'success' | 'error';
  error?: string;
  recoverHints?: string;
}

interface ExtensionLoadingToastProps {
  extensions: ExtensionLoadingStatus[];
  totalCount: number;
  isComplete: boolean;
}

/**
 * The content of the extension-loading toast. The surface (lz-surface, hairline, the overlay
 * elevation) and the close are the toast container's; this composes the body on the same
 * tokens: a StatusDot carries each state, type carries the hierarchy, the details are a
 * quiet meta disclosure.
 */
export function GroupedExtensionLoadingToast({
  extensions,
  totalCount,
  isComplete,
}: ExtensionLoadingToastProps) {
  const [isOpen, setIsOpen] = useState(false);
  const [copiedExtension, setCopiedExtension] = useState<string | null>(null);
  const intl = useIntl();

  const successCount = extensions.filter((ext) => ext.status === 'success').length;
  const errorCount = extensions.filter((ext) => ext.status === 'error').length;

  const statusDot = (status: ExtensionLoadingStatus['status'], size: 8 | 10 = 8) => {
    switch (status) {
      case 'loading':
        return (
          <StatusDot
            tone="accent"
            live
            size={size}
            label={intl.formatMessage(i18n.statusLoading)}
          />
        );
      case 'success':
        return <StatusDot tone="ok" size={size} label={intl.formatMessage(i18n.statusLoaded)} />;
      case 'error':
        return <StatusDot tone="err" size={size} label={intl.formatMessage(i18n.statusFailed)} />;
    }
  };

  const getSummaryText = () => {
    if (!isComplete) {
      return intl.formatMessage(i18n.loadingExtensions, { count: totalCount });
    }

    if (errorCount === 0) {
      return intl.formatMessage(i18n.successfullyLoaded, { count: successCount });
    }

    return intl.formatMessage(i18n.partiallyLoaded, { successCount, totalCount });
  };

  const summaryDot = !isComplete ? (
    statusDot('loading', 10)
  ) : errorCount === 0 ? (
    statusDot('success', 10)
  ) : (
    <StatusDot tone="warn" size={10} label={intl.formatMessage(i18n.statusPartial)} />
  );

  return (
    <div className="w-full" data-testid="extension-loading-toast">
      <Collapsible open={isOpen} onOpenChange={setIsOpen}>
        <div className="flex flex-col">
          {/* Main summary section - clickable */}
          <CollapsibleTrigger asChild>
            <div
              className={cx(
                'flex cursor-pointer items-start gap-3 rounded-lz-control',
                SURFACE.hover,
                MOTION
              )}
            >
              <span className="flex h-5 shrink-0 items-center">{summaryDot}</span>
              <div className="min-w-0 flex-1">
                <div className={cx(TYPE.body, WEIGHT.semibold)}>{getSummaryText()}</div>
                {errorCount > 0 && (
                  <div className={cx('text-lz-meta', TONE_TEXT.err)}>
                    {intl.formatMessage(i18n.failedToLoad, { count: errorCount })}
                  </div>
                )}
              </div>
            </div>
          </CollapsibleTrigger>

          {/* Expanded details section */}
          <CollapsibleContent className="overflow-hidden">
            <div className={cx('mt-3 border-t pt-3', SURFACE.hairline)}>
              <div className="max-h-64 space-y-3 overflow-y-auto pr-2">
                {extensions.map((ext) => {
                  const friendlyName = formatExtensionName(ext.name);

                  return (
                    <div key={ext.name} className="flex flex-col gap-2">
                      <div className="flex items-center gap-3">
                        {statusDot(ext.status)}
                        <div className={cx('min-w-0 flex-1 truncate', TYPE.body)}>
                          {friendlyName}
                        </div>
                      </div>
                      {ext.status === 'error' && ext.error && (
                        <div className="ml-5 flex flex-col gap-2">
                          <div className="break-words text-lz-meta text-lz-ink-2">
                            {formatExtensionErrorMessage(
                              ext.error,
                              intl.formatMessage(i18n.failedToAddExtension)
                            )}
                          </div>
                          {/* Pass D (owner): the "Ask goose" recovery button is gone — it
                              silently created a project-less session. Copy error remains. */}
                          <div className="flex gap-2">
                            <Button
                              size="sm"
                              onClick={(e) => {
                                e.stopPropagation();
                                navigator.clipboard.writeText(ext.error!);
                                setCopiedExtension(ext.name);
                                setTimeout(() => setCopiedExtension(null), 2000);
                              }}
                            >
                              {copiedExtension === ext.name
                                ? intl.formatMessage(i18n.copied)
                                : intl.formatMessage(i18n.copyError)}
                            </Button>
                          </div>
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            </div>
          </CollapsibleContent>

          {/* Toggle: the details disclosure, in the meta register */}
          {totalCount > 0 && (
            <CollapsibleTrigger asChild>
              <button
                type="button"
                data-testid="extension-loading-toggle"
                className={cx(
                  'mt-2 flex h-7 w-full items-center justify-center gap-1 rounded-lz-control [&_svg]:size-3',
                  TYPE.meta,
                  'hover:text-lz-ink',
                  SURFACE.hover,
                  FOCUS,
                  MOTION
                )}
                aria-label={
                  isOpen
                    ? intl.formatMessage(i18n.collapseDetails)
                    : intl.formatMessage(i18n.expandDetails)
                }
              >
                {isOpen ? (
                  <>
                    <span>{intl.formatMessage(i18n.showLess)}</span>
                    <ChevronUp />
                  </>
                ) : (
                  <>
                    <span>{intl.formatMessage(i18n.showDetails)}</span>
                    <ChevronDown />
                  </>
                )}
              </button>
            </CollapsibleTrigger>
          )}
        </div>
      </Collapsible>
    </div>
  );
}
