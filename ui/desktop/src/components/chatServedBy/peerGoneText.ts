import type { IntlShape } from 'react-intl';
import { defineMessages } from '../../i18n';
import type { PeerGone } from '../../utils/routeContact';
import { routePeerName } from '../leanzero-swarm/macs';
import type { ChatServedBy } from './chatServedBy';

/**
 * The words for a route's Mac that is away (served-by `reconnecting` with `gone`, Q-111) — ONE
 * message the composer bar's headline and the model chip both say, so the two can never name the
 * state differently. "isn't running" ONLY when its goose said it quit; a silence says what is
 * known — no answer since when, closed or offline — because a network outage is not a closed app.
 * The tray says the same in main's English (mlxTray `peerGoneText`).
 */
const i18n = defineMessages({
  peerGone: {
    id: 'chatServedBy.peerGone',
    defaultMessage:
      '{because, select, quit {{peer}’s goose isn’t running} other {{peer} hasn’t answered since {time} — its goose may be closed, or it’s offline}}',
  },
});

/** The Mac that is away and why, by its one name; null while contact is a blip or not lost. */
export function peerGoneOf(served: ChatServedBy): { mac: string; gone: PeerGone } | null {
  const { readiness } = served;
  return readiness.kind === 'reconnecting' && readiness.gone
    ? { mac: routePeerName(readiness.status), gone: readiness.gone }
    : null;
}

/** The clock time contact was lost at, in the reader's locale. */
export function lostSinceTime(intl: IntlShape, gone: PeerGone): string {
  return gone.because === 'silent'
    ? intl.formatTime(gone.lostSinceMs, { hour: 'numeric', minute: '2-digit' })
    : '';
}

export function peerGoneText(intl: IntlShape, peer: string, gone: PeerGone): string {
  return intl.formatMessage(i18n.peerGone, {
    peer,
    because: gone.because === 'said-quit' ? 'quit' : 'silent',
    time: lostSinceTime(intl, gone),
  });
}
