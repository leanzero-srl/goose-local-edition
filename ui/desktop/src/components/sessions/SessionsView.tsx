import React, { useCallback, useState } from 'react';
import SessionListView from './SessionListView';
import { useNavigation } from '../../hooks/useNavigation';
import { useEdition } from '../../contexts/EditionContext';
import { defineMessages, useIntl } from '../../i18n';
import { Button } from '../ui/button';
import LinkedSessionsView from './LinkedSessionsView';

const messages = defineMessages({
  local: { id: 'sessions.scope.local', defaultMessage: 'This Mac' },
  linked: { id: 'sessions.scope.linked', defaultMessage: 'Linked Macs' },
});

const SessionsView: React.FC = () => {
  const setView = useNavigation();
  const intl = useIntl();
  const { isLocal } = useEdition();
  const [linked, setLinked] = useState(false);

  const handleSelectSession = useCallback(
    async (sessionId: string) => {
      setView('pair', {
        disableAnimation: true,
        resumeSessionId: sessionId,
      });
    },
    [setView]
  );

  const navigation = isLocal && (
    <div className="flex gap-2">
      <Button
        variant={linked ? 'outline' : 'default'}
        aria-pressed={!linked}
        onClick={() => setLinked(false)}
      >
        {intl.formatMessage(messages.local)}
      </Button>
      <Button
        variant={linked ? 'default' : 'outline'}
        aria-pressed={linked}
        onClick={() => setLinked(true)}
      >
        {intl.formatMessage(messages.linked)}
      </Button>
    </div>
  );
  return linked && isLocal ? (
    <LinkedSessionsView navigation={navigation} />
  ) : (
    <SessionListView onSelectSession={handleSelectSession} navigation={navigation} />
  );
};

export default SessionsView;
