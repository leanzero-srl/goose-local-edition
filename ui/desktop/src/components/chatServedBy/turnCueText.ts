import type { IntlShape } from 'react-intl';
import { defineMessages } from '../../i18n';
import { compactTokens, formatElapsed, formatRate } from '../leanzero-swarm/mlxLiveStats';
import { lostSinceTime } from './peerGoneText';
import type { TurnCue } from './turnStatus';

const i18n = defineMessages({
  reconnecting: {
    id: 'turnCue.reconnecting',
    defaultMessage: 'Waiting for {mac} to reconnect…',
  },
  gone: {
    id: 'turnCue.gone',
    defaultMessage:
      '{because, select, quit {{mac}’s goose isn’t running — open goose there, or run chat on this Mac} other {{mac} hasn’t answered since {time} — its goose may be closed, or it’s offline}}',
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
  writing: {
    id: 'turnCue.writing',
    defaultMessage: 'Writing for {elapsed} · {tokens} tokens · {rate} tok/s',
  },
  writingNoRate: {
    id: 'turnCue.writingNoRate',
    defaultMessage: 'Writing for {elapsed} · {tokens} tokens',
  },
});

function readingText(intl: IntlShape, cue: Extract<TurnCue, { kind: 'reading' }>): string {
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

/** The status line's words for a turn cue (turnStatus.ts). */
export function turnCueText(intl: IntlShape, cue: TurnCue): string {
  switch (cue.kind) {
    case 'reconnecting':
      return intl.formatMessage(i18n.reconnecting, { mac: cue.mac });
    case 'gone':
      return intl.formatMessage(i18n.gone, {
        mac: cue.mac,
        because: cue.gone.because === 'said-quit' ? 'quit' : 'silent',
        time: lostSinceTime(intl, cue.gone),
      });
    case 'checking':
      return intl.formatMessage(i18n.checking, { mac: cue.mac });
    case 'silent':
      return intl.formatMessage(i18n.silent, { mac: cue.mac });
    case 'reading':
      return readingText(intl, cue);
    case 'writing': {
      const values = {
        elapsed: formatElapsed(cue.elapsedS),
        tokens: compactTokens(cue.tokens),
      };
      return cue.tps != null && cue.tps > 0
        ? intl.formatMessage(i18n.writing, { ...values, rate: formatRate(cue.tps, intl.locale) })
        : intl.formatMessage(i18n.writingNoRate, values);
    }
  }
}
