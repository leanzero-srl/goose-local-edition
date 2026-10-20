import { useMemo } from 'react';
import CardContainer from './CardContainer';
import CardHeader from './CardHeader';
import CardBody from './CardBody';
import DefaultCardButtons from './buttons/DefaultCardButtons';
import type { ProviderDetails, ProviderMetadata } from '../../../../types/providers';
import { defineMessages, useIntl } from '../../../../i18n';
import { Chip, TYPE, cx } from '../../../lz';
import { providerRowState } from '../../../leanzero-swarm/cloudProviderState';

const i18n = defineMessages({
  noMetadata: {
    id: 'providerCard.noMetadata',
    defaultMessage: 'ProviderCard error: No metadata provided',
  },
  unknownProvider: {
    id: 'providerCard.unknownProvider',
    defaultMessage: 'Unknown Provider',
  },
  notRunning: { id: 'providerCard.notRunning', defaultMessage: 'Not running' },
  notRunningText: {
    id: 'providerCard.notRunningText',
    defaultMessage:
      'Nothing is running at {origin} yet. Start the local server, then recheck connections.',
  },
});

type ProviderCardProps = {
  provider: ProviderDetails;
  onConfigure: () => void;
  onLaunch: () => void;
  isOnboarding: boolean;
};

export const ProviderCard = function ProviderCard({
  provider,
  onConfigure,
  onLaunch,
  isOnboarding,
}: ProviderCardProps) {
  const intl = useIntl();
  // Safely access metadata with null checks
  const providerMetadata: ProviderMetadata | null = provider?.metadata || null;

  // Instead of useEffect for logging, use useMemo to memoize the metadata
  const metadata = useMemo(() => providerMetadata, [providerMetadata]);

  if (!metadata) {
    return <div>{intl.formatMessage(i18n.noMetadata)}</div>;
  }

  const state = providerRowState(provider);

  const handleCardClick = () => {
    if (!isOnboarding) {
      onConfigure();
    }
  };

  return (
    <CardContainer
      testId={`provider-card-${provider.name.toLowerCase()}`}
      grayedOut={!provider.is_configured && isOnboarding} // onboarding page will have grayed out cards if not configured
      onClick={handleCardClick}
      header={
        <CardHeader
          name={metadata.display_name || provider?.name || intl.formatMessage(i18n.unknownProvider)}
          description={metadata.description || ''}
          isConfigured={provider?.is_configured || false}
        />
      }
      body={
        <CardBody>
          {state === 'failed' && (
            <p role="alert" className="mb-3 text-sm text-text-danger break-words">
              {provider.connection_error}
            </p>
          )}
          {state === 'not-running' && (
            <div
              data-testid={`provider-card-not-running-${provider.name}`}
              className="mb-3 flex flex-col items-start gap-1"
            >
              <Chip tone="stopped">{intl.formatMessage(i18n.notRunning)}</Chip>
              <p className={cx('break-words', TYPE.meta)}>
                {intl.formatMessage(i18n.notRunningText, {
                  origin: provider.local_server_not_running,
                })}
              </p>
            </div>
          )}
          <DefaultCardButtons
            provider={provider}
            onConfigure={onConfigure}
            onLaunch={onLaunch}
            isOnboardingPage={isOnboarding}
          />
        </CardBody>
      }
    />
  );
};
