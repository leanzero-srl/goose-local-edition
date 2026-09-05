import { useState, useEffect, useMemo } from 'react';
import { acpCreateCustomProviderFromRequest, acpListProviderDetails } from '../../acp/providers';
import type { ProviderDetails, UpdateCustomProviderRequest } from '../../types/providers';
import { Select } from '../ui/Select';
import ProviderConfigForm from './ProviderConfigForm';
import LocalModelPicker from './LocalModelPicker';
import CustomProviderForm from '../settings/providers/modal/subcomponents/forms/CustomProviderForm';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '../ui/dialog';
import { HardDrive, Key, Plus } from 'lucide-react';
import { defineMessages, useIntl } from '../../i18n';
import { useFeatures } from '../../contexts/FeaturesContext';
import { useEdition } from '../../contexts/EditionContext';
import { LeanZero } from '../icons';
import { SWARM_DISPLAY_NAME, SWARM_PROVIDER_ID } from '../../branding';
import { CLOUD_PROVIDERS } from '../leanzero-swarm/cloudProviders';
import {
  isLocalEditionCloudProvider,
  SWARM_CHAT_MODEL_ID,
} from '../settings/models/leanzeroSelectorPolicy';

const i18n = defineMessages({
  useLocalModel: {
    id: 'providerSelector.useLocalModel',
    defaultMessage: 'Use a Local Model',
  },
  localModelDescription: {
    id: 'providerSelector.localModelDescription',
    defaultMessage: 'Download a model and run it on this device. No API key or account needed.',
  },
  connectProvider: {
    id: 'providerSelector.connectProvider',
    defaultMessage: 'Connect to a Provider',
  },
  connectProviderDescription: {
    id: 'providerSelector.connectProviderDescription',
    defaultMessage: 'Connect OpenAI, Anthropic, Google, etc',
  },
  selectProvider: {
    id: 'providerSelector.selectProvider',
    defaultMessage: 'Select a provider',
  },
  addCustomProvider: {
    id: 'providerSelector.addCustomProvider',
    defaultMessage: 'Add a custom provider',
  },
  addCustomProviderTitle: {
    id: 'providerSelector.addCustomProviderTitle',
    defaultMessage: 'Add Custom Provider',
  },
  useSwarm: {
    id: 'providerSelector.useSwarm',
    defaultMessage: 'Use {name}',
  },
  useSwarmDescription: {
    id: 'providerSelector.useSwarmDescription',
    defaultMessage: 'Chat and builds run on the nodes of your own pool. No API key needed.',
  },
  connectCloudProviderDescription: {
    id: 'providerSelector.connectCloudProviderDescription',
    defaultMessage: 'Connect {providers}',
  },
});

const LOCAL_MODEL = 'local-model' as const;
const OWN_PROVIDER = 'own-provider' as const;

type SelectedPath = typeof LOCAL_MODEL | typeof OWN_PROVIDER | null;

interface ProviderOption {
  value: string;
  label: string;
  provider: ProviderDetails;
}

interface ProviderSelectorProps {
  onConfigured: (providerName: string, modelId?: string) => void;
  onFirstSelection?: () => void;
}

export default function ProviderSelector({
  onConfigured,
  onFirstSelection,
}: ProviderSelectorProps) {
  const intl = useIntl();
  const { localInference: localInferenceCapability } = useFeatures();
  // Goose Swarm (local) edition: onboarding offers Swarm straight through (no credentials) and the
  // four swarm cloud families — no local-model download, no custom provider, no upstream catalog.
  const { isLocal } = useEdition();
  const localInference = localInferenceCapability && !isLocal;
  const [providerList, setProviderList] = useState<ProviderDetails[]>([]);
  const [selectedOption, setSelectedOption] = useState<ProviderOption | null>(null);
  const [selectedPath, setSelectedPath] = useState<SelectedPath>(null);
  const [showCustomModal, setShowCustomModal] = useState(false);

  useEffect(() => {
    const load = async () => {
      try {
        const list = await acpListProviderDetails();
        setProviderList(list);
      } catch (err) {
        console.error('Failed to fetch providers:', err);
      }
    };
    load();
  }, []);

  const options: ProviderOption[] = useMemo(() => {
    return [...providerList]
      .filter((p) => !isLocal || isLocalEditionCloudProvider(p.name))
      .sort((a, b) => {
        const aPreferred = a.provider_type === 'Preferred' ? 0 : 1;
        const bPreferred = b.provider_type === 'Preferred' ? 0 : 1;
        if (aPreferred !== bPreferred) return aPreferred - bPreferred;
        return a.metadata.display_name.localeCompare(b.metadata.display_name);
      })
      .map((provider) => ({
        value: provider.name,
        label: provider.metadata.display_name,
        provider,
      }));
  }, [providerList, isLocal]);

  const fuzzyFilterOption = (option: { label: string; value: string }, inputValue: string) => {
    const normalize = (s: string) => s.toLowerCase().replace(/[\s_-]/g, '');
    return (
      normalize(option.label).includes(normalize(inputValue)) ||
      normalize(option.value).includes(normalize(inputValue))
    );
  };

  const handleLocalModelClick = () => {
    setSelectedPath(LOCAL_MODEL);
    setSelectedOption(null);
    onFirstSelection?.();
  };

  const handleOwnProviderClick = () => {
    setSelectedPath(OWN_PROVIDER);
    onFirstSelection?.();
  };

  // Swarm needs no key: SWARM_COMMAND defaults, so the defaults write alone completes onboarding.
  const handleSwarmClick = () => {
    onFirstSelection?.();
    onConfigured(SWARM_PROVIDER_ID, SWARM_CHAT_MODEL_ID);
  };

  const handleProviderSelect = (option: ProviderOption | null) => {
    setSelectedOption(option);
    if (option) onFirstSelection?.();
  };

  const handleCreateCustomProvider = async (data: UpdateCustomProviderRequest) => {
    const result = await acpCreateCustomProviderFromRequest(data);
    setShowCustomModal(false);
    if (result.provider_name) {
      onConfigured(result.provider_name);
    }
  };

  const selectedProvider = selectedOption?.provider ?? null;

  return (
    <div>
      <div
        className={`grid ${localInference || isLocal ? 'grid-cols-2' : 'grid-cols-1'} gap-3 mb-6`}
      >
        {isLocal && (
          <div
            onClick={handleSwarmClick}
            data-testid="onboarding-use-swarm"
            className="p-4 border rounded-xl transition-all duration-200 cursor-pointer group border-border-default bg-background-muted hover:border-blue-400"
          >
            <LeanZero className="size-5 text-text-muted mb-2" />
            <span className="font-medium text-text-default text-base block">
              {intl.formatMessage(i18n.useSwarm, { name: SWARM_DISPLAY_NAME })}
            </span>
            <p className="text-text-muted text-sm mt-1">
              {intl.formatMessage(i18n.useSwarmDescription)}
            </p>
          </div>
        )}
        {localInference && (
          <div
            onClick={handleLocalModelClick}
            className={`p-4 border rounded-xl transition-all duration-200 cursor-pointer group ${
              selectedPath === LOCAL_MODEL
                ? 'border-blue-400 bg-background-muted'
                : 'border-border-default bg-background-muted hover:border-blue-400'
            }`}
          >
            <HardDrive size={20} className="text-text-muted mb-2" />
            <span className="font-medium text-text-default text-base block">
              {intl.formatMessage(i18n.useLocalModel)}
            </span>
            <p className="text-text-muted text-sm mt-1">
              {intl.formatMessage(i18n.localModelDescription)}
            </p>
          </div>
        )}

        <div
          onClick={handleOwnProviderClick}
          className={`p-4 border rounded-xl transition-all duration-200 cursor-pointer group ${
            selectedPath === OWN_PROVIDER
              ? 'border-blue-400 bg-background-muted'
              : 'border-border-default bg-background-muted hover:border-blue-400'
          }`}
        >
          <Key size={20} className="text-text-muted mb-2" />
          <span className="font-medium text-text-default text-base block">
            {intl.formatMessage(i18n.connectProvider)}
          </span>
          <p className="text-text-muted text-sm mt-1">
            {isLocal
              ? intl.formatMessage(i18n.connectCloudProviderDescription, {
                  providers: CLOUD_PROVIDERS.map((c) => c.label).join(', '),
                })
              : intl.formatMessage(i18n.connectProviderDescription)}
          </p>
        </div>
      </div>

      {localInference && selectedPath === LOCAL_MODEL && (
        <div className="animate-in fade-in slide-in-from-top-2 duration-300">
          <LocalModelPicker onConfigured={onConfigured} />
        </div>
      )}

      {selectedPath === OWN_PROVIDER && (
        <div className="animate-in fade-in slide-in-from-top-2 duration-300">
          <div className="mb-4">
            <Select
              options={options}
              value={selectedOption}
              onChange={(option) => handleProviderSelect(option as ProviderOption | null)}
              placeholder={intl.formatMessage(i18n.selectProvider)}
              isClearable
              isSearchable
              autoFocus
              filterOption={fuzzyFilterOption}
            />
          </div>

          {!isLocal && (
            <button
              onClick={() => setShowCustomModal(true)}
              className="flex items-center gap-1 text-sm text-text-muted hover:text-text-default transition-colors mb-6"
            >
              <Plus size={14} />
              <span>{intl.formatMessage(i18n.addCustomProvider)}</span>
            </button>
          )}

          {selectedProvider && (
            <ProviderConfigForm
              key={selectedProvider.name}
              provider={selectedProvider}
              onConfigured={onConfigured}
            />
          )}
        </div>
      )}

      <Dialog open={showCustomModal} onOpenChange={setShowCustomModal}>
        <DialogContent className="sm:max-w-[600px] max-h-[90vh] overflow-y-auto">
          <DialogHeader>
            <DialogTitle>{intl.formatMessage(i18n.addCustomProviderTitle)}</DialogTitle>
          </DialogHeader>
          <CustomProviderForm
            initialData={null}
            isEditable={true}
            onSubmit={handleCreateCustomProvider}
            onCancel={() => setShowCustomModal(false)}
          />
        </DialogContent>
      </Dialog>
    </div>
  );
}
