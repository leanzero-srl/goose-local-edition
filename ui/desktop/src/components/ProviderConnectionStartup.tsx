import { useEffect } from 'react';
import { acpListProviderDetails } from '../acp/providers';
import { toastError } from '../toasts';
import { useEdition } from '../contexts/EditionContext';
import { defineMessages, useIntl } from '../i18n';
import { errorMessage } from '../utils/conversionUtils';

const messages = defineMessages({
  attention: {
    id: 'providerConnections.attention',
    defaultMessage: 'Provider connections need attention',
  },
});

export function ProviderConnectionStartup() {
  const { isLocal } = useEdition();
  const intl = useIntl();
  useEffect(() => {
    if (!isLocal) return;
    let active = true;
    void acpListProviderDetails()
      .then((providers) => {
        const failed = providers.filter((provider) => provider.connection_error);
        if (active && failed.length) {
          toastError({
            title: intl.formatMessage(messages.attention),
            msg: failed
              .map((provider) => `${provider.metadata.display_name}: ${provider.connection_error}`)
              .join('\n'),
          });
        }
      })
      .catch((error: unknown) => {
        if (active)
          toastError({ title: intl.formatMessage(messages.attention), msg: errorMessage(error) });
      });
    return () => {
      active = false;
    };
  }, [isLocal, intl]);
  return null;
}
