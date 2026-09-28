import type { IntlShape } from 'react-intl';
import { defineMessages } from '../../i18n';

const i18n = defineMessages({
  phaseWaitingForLoad: {
    id: 'nodes.phaseWaitingForLoad',
    defaultMessage: 'Waiting for another load to finish',
  },
  phaseMakingRoom: { id: 'mlxStateTile.makingRoom', defaultMessage: 'Making room' },
  phaseStarting: { id: 'mlxStateTile.startingEngine', defaultMessage: 'Starting the engine' },
  phaseLoading: { id: 'mlxStateTile.loadingWeights', defaultMessage: 'Loading weights' },
  phaseWarming: { id: 'mlxStateTile.warming', defaultMessage: 'Warming up' },
  phaseUnnamed: { id: 'nodes.phaseUnnamed', defaultMessage: 'Loading' },
});

/**
 * A way's load phase as goosed names it (`NodesServingWayDto.loadPhase`, the loader's `loading`
 * mark), in the person's words — the ONE table the node cards and the composer's loader line read.
 * null = no phase reported yet.
 */
export function loadPhaseWord(intl: IntlShape, phase: string | null): string {
  switch (phase) {
    case 'waitingForLoad':
      return intl.formatMessage(i18n.phaseWaitingForLoad);
    case 'makingRoom':
      return intl.formatMessage(i18n.phaseMakingRoom);
    case 'starting':
      return intl.formatMessage(i18n.phaseStarting);
    case 'loading':
      return intl.formatMessage(i18n.phaseLoading);
    case 'warming':
      return intl.formatMessage(i18n.phaseWarming);
    case null:
      return intl.formatMessage(i18n.phaseUnnamed);
    default:
      // A phase this build has no words for is shown as goose named it, never hidden.
      return phase;
  }
}
