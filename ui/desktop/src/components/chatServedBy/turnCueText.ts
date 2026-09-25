import type { IntlShape } from 'react-intl';
import { defineMessages } from '../../i18n';
import { compactTokens, formatElapsed, formatRate } from '../leanzero-swarm/mlxLiveStats';
import type { TurnCue } from './turnStatus';

const i18n = defineMessages({
  reconnecting: {
    id: 'turnCue.reconnecting',
    defaultMessage: 'Waiting for {mac} to reconnect…',
  },
  checking: {
    id: 'turnCue.checking',
    defaultMessage: 'Checking whether {mac} still has your answer…',
  },
  silent: {
    id: 'turnCue.silent',
    defaultMessage: 'No words from {mac} for a moment — checking…',
  },
  readingPercent: {
    id: 'turnCue.readingPercent',
    defaultMessage: '{mac} is reading your prompt ({tokens} tokens) — {percent}%',
  },
  readingEta: {
    id: 'turnCue.readingEta',
    defaultMessage:
      '{mac} is reading your prompt ({tokens} tokens) — {elapsed} of about {expected} at its measured {rate} tok/s',
  },
  readingElapsed: {
    id: 'turnCue.readingElapsed',
    defaultMessage: '{mac} is reading your prompt ({tokens} tokens) — {elapsed} so far',
  },
  readingPlain: {
    id: 'turnCue.readingPlain',
    defaultMessage: '{mac} is reading your prompt ({tokens} tokens)…',
  },
});

/** The status line's words for a turn cue (turnStatus.ts). */
export function turnCueText(intl: IntlShape, cue: TurnCue): string {
  switch (cue.kind) {
    case 'reconnecting':
      return intl.formatMessage(i18n.reconnecting, { mac: cue.mac });
    case 'checking':
      return intl.formatMessage(i18n.checking, { mac: cue.mac });
    case 'silent':
      return intl.formatMessage(i18n.silent, { mac: cue.mac });
    case 'reading': {
      const tokens = compactTokens(cue.promptTokens);
      switch (cue.progress) {
        case 'percent':
          return intl.formatMessage(i18n.readingPercent, {
            mac: cue.mac,
            tokens,
            percent: cue.percent,
          });
        case 'eta':
          return intl.formatMessage(i18n.readingEta, {
            mac: cue.mac,
            tokens,
            elapsed: formatElapsed(cue.elapsedS),
            expected: formatElapsed(cue.expectedS),
            rate: formatRate(cue.rate, intl.locale),
          });
        case 'elapsed':
          return intl.formatMessage(i18n.readingElapsed, {
            mac: cue.mac,
            tokens,
            elapsed: formatElapsed(cue.elapsedS),
          });
        case 'plain':
          return intl.formatMessage(i18n.readingPlain, { mac: cue.mac, tokens });
      }
    }
  }
}
