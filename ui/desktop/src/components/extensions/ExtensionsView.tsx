import { BundledMcps, BUNDLED_NAMES } from './BundledMcps';
import { View, ViewOptions } from '../../utils/navigationUtils';
import ExtensionsSection from '../settings/extensions/ExtensionsSection';
import type { ExtensionConfig } from '../../types/extensions';
import { MainPanelLayout } from '../Layout/MainPanelLayout';
import { Button, PageHeader } from '../lz';
import { Plus } from 'lucide-react';
import { GPSIcon } from '../ui/icons';
import { useState, useEffect } from 'react';
import kebabCase from 'lodash/kebabCase';
import ExtensionModal from '../settings/extensions/modal/ExtensionModal';
import {
  getDefaultFormData,
  ExtensionFormData,
  createExtensionConfig,
} from '../settings/extensions/utils';
import { activateExtensionDefault } from '../settings/extensions';
import { useConfig } from '../ConfigContext';
import { SearchView } from '../conversation/SearchView';
import { defineMessages, useIntl } from '../../i18n';

const i18n = defineMessages({
  heading: {
    id: 'extensionsView.heading',
    defaultMessage: 'MCPs',
  },
  subtitle: {
    id: 'extensionsView.subtitle',
    defaultMessage:
      'Tools your agents can use. Extensions switched on here load in every new chat.',
  },
  addCustomExtension: {
    id: 'extensionsView.addCustomExtension',
    defaultMessage: 'Add custom extension',
  },
  browseExtensions: {
    id: 'extensionsView.browseExtensions',
    defaultMessage: 'Browse extensions',
  },
  searchPlaceholder: {
    id: 'extensionsView.searchPlaceholder',
    defaultMessage: 'Search extensions...',
  },
  addExtension: {
    id: 'extensionsView.addExtension',
    defaultMessage: 'Add Extension',
  },
});

export type ExtensionsViewOptions = {
  deepLinkConfig?: ExtensionConfig;
  showEnvVars?: boolean;
};

export default function ExtensionsView({
  viewOptions,
}: {
  onClose: () => void;
  setView: (view: View, viewOptions?: ViewOptions) => void;
  viewOptions: ExtensionsViewOptions;
}) {
  const intl = useIntl();
  const [isAddModalOpen, setIsAddModalOpen] = useState(false);
  const [refreshKey, setRefreshKey] = useState(0);
  const [searchTerm, setSearchTerm] = useState('');
  const { addExtension } = useConfig();

  // Only trigger refresh when deep link config changes AND we don't need to show env vars
  useEffect(() => {
    if (viewOptions.deepLinkConfig && !viewOptions.showEnvVars) {
      setRefreshKey((prevKey) => prevKey + 1);
    }
  }, [viewOptions.deepLinkConfig, viewOptions.showEnvVars]);

  const scrollToExtension = (extensionName: string) => {
    setTimeout(() => {
      const element = document.getElementById(`extension-${kebabCase(extensionName)}`);
      if (element) {
        element.scrollIntoView({
          behavior: 'smooth',
          block: 'center',
        });
        // Mark the card it scrolled to with the solid accent ring (never an alpha glow).
        const ring = ['ring-2', 'ring-lz-accent'];
        element.classList.add(...ring);
        setTimeout(() => {
          element.classList.remove(...ring);
        }, 2000);
      }
    }, 200);
  };

  // Scroll to extension whenever extensionId is provided (after refresh)
  useEffect(() => {
    if (viewOptions.deepLinkConfig?.name && refreshKey > 0) {
      scrollToExtension(viewOptions.deepLinkConfig?.name);
    }
  }, [viewOptions.deepLinkConfig?.name, refreshKey]);

  const handleModalClose = () => {
    setIsAddModalOpen(false);
  };

  const handleAddExtension = async (formData: ExtensionFormData) => {
    await activateExtensionDefault({
      addToConfig: addExtension,
      extensionConfig: createExtensionConfig(formData),
    });
    setRefreshKey((key) => key + 1);
  };

  return (
    <MainPanelLayout backgroundColor="bg-lz-bg text-lz-ink">
      <div
        className="flex flex-col min-w-0 flex-1 overflow-y-auto relative"
        data-search-scroll-area
      >
        <div className="bg-lz-bg px-6 pb-4 pt-8">
          <div className="flex flex-col page-transition">
            <PageHeader
              className="mb-6"
              title={intl.formatMessage(i18n.heading)}
              subtitle={intl.formatMessage(i18n.subtitle)}
              actions={
                <>
                  <Button
                    variant="secondary"
                    icon={<GPSIcon size={12} />}
                    onClick={() => window.open('https://goose-docs.ai/v1/extensions/', '_blank')}
                  >
                    {intl.formatMessage(i18n.browseExtensions)}
                  </Button>
                  <Button variant="primary" icon={<Plus />} onClick={() => setIsAddModalOpen(true)}>
                    {intl.formatMessage(i18n.addCustomExtension)}
                  </Button>
                </>
              }
            />
            <div className="mb-6">
              <BundledMcps />
            </div>
            <label className="mb-5 block">
              <span className="mb-1.5 block text-lz-meta font-lz-medium text-lz-ink-2">
                Find an extension
              </span>
              <input
                className="h-8 w-full rounded-lz-control border border-lz-border-strong bg-lz-surface px-3 text-lz-body text-lz-ink placeholder:text-lz-ink-4"
                value={searchTerm}
                onChange={(event) => setSearchTerm(event.target.value)}
                placeholder="Search installed extensions"
              />
            </label>
          </div>
        </div>

        <div className="px-8 pb-16">
          <SearchView
            onSearch={(term) => setSearchTerm(term)}
            placeholder={intl.formatMessage(i18n.searchPlaceholder)}
          >
            <ExtensionsSection
              key={refreshKey}
              excludeNames={BUNDLED_NAMES}
              deepLinkConfig={viewOptions.deepLinkConfig}
              showEnvVars={viewOptions.showEnvVars}
              hideButtons={true}
              searchTerm={searchTerm}
              onModalClose={(extensionName: string) => {
                scrollToExtension(extensionName);
              }}
            />
          </SearchView>
        </div>

        {/* Bottom padding space - same as in hub.tsx */}
        <div className="block h-8" />
      </div>

      {/* Modal for adding a new extension */}
      {isAddModalOpen && (
        <ExtensionModal
          title={intl.formatMessage(i18n.addCustomExtension)}
          initialData={getDefaultFormData()}
          onClose={handleModalClose}
          onSubmit={handleAddExtension}
          submitLabel={intl.formatMessage(i18n.addExtension)}
          modalType={'add'}
        />
      )}
    </MainPanelLayout>
  );
}
