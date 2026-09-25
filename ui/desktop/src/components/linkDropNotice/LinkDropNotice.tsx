import { useEffect, useSyncExternalStore } from 'react';
import { RotateCcw, Unplug } from 'lucide-react';
import {
  latestMlxRemoteSingleStatus,
  subscribeMlxRemoteSingleStatus,
} from '../../acp/mlx-remote-single';
import { defineMessages, useIntl } from '../../i18n';
import { routePeerName } from '../leanzero-swarm/macs';
import { Button, Disclosure, SPACE, SURFACE, TONE_TEXT, TYPE, WEIGHT, cx } from '../lz';
import type { LinkDrop } from './parseLinkDrop';
import { rememberDropName, storedDropName } from './dropNames';

const i18n = defineMessages({
  midAnswer: {
    id: 'linkDropNotice.midAnswer',
    defaultMessage:
      '{cause, select, quit {{mac} quit goose mid-answer} restart {{mac} restarted goose mid-answer} other {{mac} stopped answering mid-reply}} — the answer above stops there.',
  },
  noAnswer: {
    id: 'linkDropNotice.noAnswer',
    defaultMessage:
      '{cause, select, quit {{mac} quit goose} restart {{mac} restarted goose} other {{mac} stopped answering}} — this turn got no answer.',
  },
  unnamed: { id: 'linkDropNotice.unnamed', defaultMessage: 'The linked Mac' },
  retry: { id: 'linkDropNotice.retry', defaultMessage: 'Retry' },
  details: { id: 'linkDropNotice.details', defaultMessage: 'Details' },
});

/**
 * The Mac a dropped turn was served by, by its one name, stored with the drop (Q-62): the drop's
 * own words when they carry it ("Work's Mac Studio quit goose"), else the name kept for this
 * message, else the route this window reads when its node id is the one the relay reported — then
 * kept. Otherwise nothing here knows it: "the linked Mac", never the node id.
 */
function useDroppedMacName(messageId: string, drop: LinkDrop): string | null {
  const route = useSyncExternalStore(subscribeMlxRemoteSingleStatus, latestMlxRemoteSingleStatus);
  const fromRoute =
    drop.peerId != null && route?.peer === drop.peerId ? routePeerName(route) : null;
  const name = drop.macName ?? storedDropName(messageId) ?? fromRoute;
  useEffect(() => {
    if (name) rememberDropName(messageId, name);
  }, [messageId, name]);
  return name;
}

/**
 * A turn the serving Mac dropped (Q-49): said in words, below whatever the model had written, with
 * Retry (the last user turn's text, as the other failure notices resend it) and the error itself
 * behind Details.
 */
export default function LinkDropNotice({
  messageId,
  drop,
  hasAnswer,
  live,
  retryText,
  onRetry,
}: {
  /** The message the drop ended — the name is stored with it. */
  messageId: string;
  drop: LinkDrop;
  /** The model wrote something before the drop — it is shown above this notice. */
  hasAnswer: boolean;
  /** This is the chat's latest message: a retry sends the next turn. */
  live: boolean;
  retryText: string | null;
  onRetry: (text: string) => void;
}) {
  const intl = useIntl();
  const mac = useDroppedMacName(messageId, drop) ?? intl.formatMessage(i18n.unnamed);
  const cause = drop.cause ?? 'none';
  return (
    <div
      role="alert"
      data-testid="link-drop-notice"
      className={cx(SURFACE.card, SPACE.card, 'mt-2 flex flex-col gap-3')}
    >
      <div className="flex items-start gap-2.5">
        <Unplug aria-hidden className={cx('mt-0.5 size-5 shrink-0', TONE_TEXT.err)} />
        <p
          data-testid="link-drop-headline"
          className={cx('min-w-0 flex-1', TYPE.body, WEIGHT.semibold)}
        >
          {intl.formatMessage(hasAnswer ? i18n.midAnswer : i18n.noAnswer, { mac, cause })}
        </p>
      </div>
      {live && retryText != null && (
        <div className="flex flex-wrap items-center gap-2">
          <Button
            variant="primary"
            size="sm"
            icon={<RotateCcw />}
            data-testid="link-drop-retry"
            onClick={() => onRetry(retryText)}
          >
            {intl.formatMessage(i18n.retry)}
          </Button>
        </div>
      )}
      <Disclosure
        variant="plain"
        testId="link-drop-details"
        title={intl.formatMessage(i18n.details)}
      >
        <p data-testid="link-drop-raw" className={cx(TYPE.meta, 'break-words font-mono')}>
          {drop.raw}
        </p>
      </Disclosure>
    </div>
  );
}
