import { useEffect, useState, useCallback, useMemo } from 'react';
import { Button } from '../../ui/button';
import { Plus } from 'lucide-react';
import { GPSIcon } from '../../ui/icons';
import { useConfig, FixedExtensionEntry } from '../../ConfigContext';
import { defineMessages, useIntl } from '../../../i18n';
import ExtensionList from './subcomponents/ExtensionList';
import ExtensionModal from './modal/ExtensionModal';
import {
  createExtensionConfig,
  ExtensionFormData,
  extensionToFormData,
  getDefaultFormData,
  nameToKey,
} from './utils';

import { activateExtensionDefault, deleteExtension, toggleExtensionDefault } from './index';
import type { ExtensionConfig } from '../../../types/extensions';

const i18n = defineMessages({
  addCustomExtension: {
    id: 'extensionsSection.addCustomExtension',
    defaultMessage: 'Add custom extension',
  },
  browseExtensions: {
    id: 'extensionsSection.browseExtensions',
    defaultMessage: 'Browse extensions',
  },
  updateExtension: {
    id: 'extensionsSection.updateExtension',
    defaultMessage: 'Update Extension',
  },
  saveChanges: {
    id: 'extensionsSection.saveChanges',
    defaultMessage: 'Save Changes',
  },
  addExtension: {
    id: 'extensionsSection.addExtension',
    defaultMessage: 'Add Extension',
  },
});

interface ExtensionSectionProps {
  deepLinkConfig?: ExtensionConfig;
  showEnvVars?: boolean;
  hideButtons?: boolean;
  disableConfiguration?: boolean;
  customToggle?: (extension: FixedExtensionEntry) => Promise<boolean | void>;
  selectedExtensions?: string[]; // Add controlled state
  onModalClose?: (extensionName: string) => void;
  searchTerm?: string;
  excludeNames?: string[];
}

export default function ExtensionsSection({
  deepLinkConfig,
  showEnvVars,
  hideButtons,
  disableConfiguration,
  customToggle,
  selectedExtensions = [],
  onModalClose,
  searchTerm = '',
  excludeNames,
}: ExtensionSectionProps) {
  const intl = useIntl();
  const { getExtensions, addExtension, removeExtension, setExtensionEnabled, extensionsList } =
    useConfig();
  const [selectedExtension, setSelectedExtension] = useState<FixedExtensionEntry | null>(null);
  const [isModalOpen, setIsModalOpen] = useState(false);
  const [isAddModalOpen, setIsAddModalOpen] = useState(false);
  const [deepLinkConfigStateVar, setDeepLinkConfigStateVar] = useState<
    ExtensionConfig | undefined | null
  >(deepLinkConfig);
  const [showEnvVarsStateVar, setShowEnvVarsStateVar] = useState<boolean | undefined | null>(
    showEnvVars
  );

  useEffect(() => {
    setDeepLinkConfigStateVar(deepLinkConfig);
    setShowEnvVarsStateVar(showEnvVars);
  }, [deepLinkConfig, showEnvVars]);

  const extensions = useMemo(() => {
    if (extensionsList.length === 0) return [];

    return extensionsList
      .filter((entry) => !excludeNames?.includes(entry.name))
      .sort((a, b) => {
        // First sort by builtin
        if (a.type === 'builtin' && b.type !== 'builtin') return -1;
        if (a.type !== 'builtin' && b.type === 'builtin') return 1;

        // Then sort by bundled (handle null/undefined cases)
        const aBundled = 'bundled' in a && a.bundled === true;
        const bBundled = 'bundled' in b && b.bundled === true;
        if (aBundled && !bBundled) return -1;
        if (!aBundled && bBundled) return 1;

        // Finally sort alphabetically within each group
        return a.name.localeCompare(b.name);
      })
      .map((ext) => ({
        ...ext,
        // Use selectedExtensions to determine enabled state in recipe editor
        enabled: disableConfiguration ? selectedExtensions.includes(ext.name) : ext.enabled,
      }));
  }, [extensionsList, disableConfiguration, selectedExtensions, excludeNames]);

  const fetchExtensions = useCallback(async () => {
    await getExtensions(true); // Force refresh - this will update the context
  }, [getExtensions]);

  const handleExtensionToggle = async (extensionConfig: FixedExtensionEntry) => {
    if (customToggle) {
      await customToggle(extensionConfig);
      return true;
    }

    const toggleDirection = extensionConfig.enabled ? 'toggleOff' : 'toggleOn';
    const configKey = extensionConfig.configKey ?? nameToKey(extensionConfig.name);

    await toggleExtensionDefault({
      toggle: toggleDirection,
      extensionConfig: extensionConfig,
      setEnabled: (enabled) => setExtensionEnabled(configKey, enabled),
    });

    await fetchExtensions();
    return true;
  };

  const handleConfigureClick = (extension: FixedExtensionEntry) => {
    setSelectedExtension(extension);
    setIsModalOpen(true);
  };

  const handleAddExtension = async (formData: ExtensionFormData) => {
    await activateExtensionDefault({
      addToConfig: addExtension,
      extensionConfig: createExtensionConfig(formData),
    });
    await fetchExtensions();
    onModalClose?.(formData.name);
  };

  const handleUpdateExtension = async (formData: ExtensionFormData) => {
    if (!selectedExtension) throw new Error('Select an extension to update.');
    const extensionConfig = createExtensionConfig(formData);
    await addExtension(extensionConfig.name, extensionConfig, formData.enabled);
    if (selectedExtension.name !== extensionConfig.name)
      await removeExtension(selectedExtension.name);
    await fetchExtensions();
  };

  const handleDeleteExtension = async (name: string) => {
    await deleteExtension({ name, removeFromConfig: removeExtension });
    await fetchExtensions();
  };

  const handleModalClose = () => {
    setDeepLinkConfigStateVar(null);
    setShowEnvVarsStateVar(null);

    setIsModalOpen(false);
    setIsAddModalOpen(false);
    setSelectedExtension(null);

    // Clear any navigation state that might be cached
    if (window.history.state?.deepLinkConfig) {
      window.history.replaceState({}, '', window.location.hash);
    }
  };

  return (
    <section id="extensions">
      <div className="">
        <ExtensionList
          extensions={extensions}
          onToggle={handleExtensionToggle}
          onConfigure={handleConfigureClick}
          onDelete={(extension) => void handleDeleteExtension(extension.name)}
          disableConfiguration={disableConfiguration}
          searchTerm={searchTerm}
        />

        {!hideButtons && (
          <div className="flex gap-4 pt-4 w-full">
            <Button
              className="flex items-center gap-2 justify-center"
              variant="default"
              onClick={() => setIsAddModalOpen(true)}
            >
              <Plus className="h-4 w-4" />
              {intl.formatMessage(i18n.addCustomExtension)}
            </Button>
            <Button
              className="flex items-center gap-2 justify-center"
              variant="secondary"
              onClick={() => window.open('https://goose-docs.ai/v1/extensions/', '_blank')}
            >
              <GPSIcon size={12} />
              {intl.formatMessage(i18n.browseExtensions)}
            </Button>
          </div>
        )}

        {/* Modal for updating an existing extension */}
        {isModalOpen && selectedExtension && (
          <ExtensionModal
            title={intl.formatMessage(i18n.updateExtension)}
            initialData={extensionToFormData(selectedExtension)}
            onClose={handleModalClose}
            onSubmit={handleUpdateExtension}
            onDelete={handleDeleteExtension}
            submitLabel={intl.formatMessage(i18n.saveChanges)}
            modalType={'edit'}
          />
        )}

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

        {/* Modal for adding extension from deeplink*/}
        {deepLinkConfigStateVar && showEnvVarsStateVar && (
          <ExtensionModal
            title={intl.formatMessage(i18n.addCustomExtension)}
            initialData={extensionToFormData({
              ...deepLinkConfig,
              enabled: true,
            } as FixedExtensionEntry)}
            onClose={handleModalClose}
            onSubmit={handleAddExtension}
            submitLabel={intl.formatMessage(i18n.addExtension)}
            modalType={'add'}
          />
        )}
      </div>
    </section>
  );
}
