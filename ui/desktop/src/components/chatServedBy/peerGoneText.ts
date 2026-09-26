import type { IntlShape } from 'react-intl';
import { defineMessages } from '../../i18n';
import { routePeerName } from '../leanzero-swarm/macs';
import type { ChatServedBy } from './chatServedBy';

/**
 * The words for a route's Mac whose goose is gone (served-by `reconnecting` with `gone`, Q-111) —
 * ONE message the composer bar's headline and the model chip both say, so the two can never name
 * the state differently. The tray says the same in main's English (mlxTray `peerGoneText`).
 */
const i18n = defineMessages({
  peerGone: {
    id: 'chatServedBy.peerGone',
    defaultMessage: '{peer}’s goose isn’t running',
  },
});

/** The Mac whose goose is gone, by its one name; null while contact is a blip or not lost. */
export function peerGoneMac(served: ChatServedBy): string | null {
  const { readiness } = served;
  return readiness.kind === 'reconnecting' && readiness.gone
    ? routePeerName(readiness.status)
    : null;
}

export function peerGoneText(intl: IntlShape, peer: string): string {
  return intl.formatMessage(i18n.peerGone, { peer });
}
