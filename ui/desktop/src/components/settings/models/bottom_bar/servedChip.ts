import type { IntlShape } from 'react-intl';
import { defineMessages } from '../../../../i18n';
import type { ChatServedBy } from '../../../chatServedBy/chatServedBy';
import { peerGoneOf, peerGoneText } from '../../../chatServedBy/peerGoneText';
import { shortModelName } from '../../../noNodeNotice/mlxMount';
import { loaderText } from '../../../chatServedBy/loaderText';

const i18n = defineMessages({
  servedChip: {
    id: 'modelsBottomBar.servedChip',
    defaultMessage: '{model} · {where}',
  },
  servedChipNotRunning: {
    id: 'modelsBottomBar.servedChipNotRunning',
    defaultMessage: '{model} · not running',
  },
  routeChip: { id: 'modelsBottomBar.routeChip', defaultMessage: '{route} · {node}' },
});

export interface ServedChipWords {
  /** The served model's short name; null when nothing names one (or the session is still read). */
  servedModel: string | null;
  /** The Macs that serve it, as one list; null when none are named. */
  servedWhere: string | null;
  servedRunning: boolean;
  splitStop: Extract<ChatServedBy['readiness'], { kind: 'split-stopped' }>['stop'] | null;
  /** A Mac that is away, in the composer bar's words (Q-111). */
  goneWords: string | null;
  /** The node loader's line while it is in this chat's way (Q-254) — the composer bar's words. */
  loaderWords: string | null;
  /** The chip's label; null = the chip keeps the model it was given. */
  chipLabel: string | null;
}

/**
 * What the model chip says serves this chat — THE derivation: the chip itself and every sentence
 * that names where a turn runs (the loop dialog's "Each tick is one turn on …") read it here, so
 * the two never name different things.
 */
export function servedChipWords(
  intl: IntlShape,
  served: ChatServedBy | null | undefined,
  isModelLoading: boolean
): ServedChipWords {
  // The MLX engine that serves this chat, as the one derivation names it. `where` is empty only
  // when nothing is named — the chip then keeps the provider's own label.
  const servedModel = !isModelLoading && served?.model ? shortModelName(served.model) : null;
  const servedWhere =
    served && served.where.length > 0
      ? intl.formatList(served.where, { type: 'conjunction' })
      : null;
  const servedRunning = served != null && served.engine !== 'none';
  // The split chat was on stopped by itself (Q-81): the chip names the split, never this Mac's
  // single engine "not running".
  const splitStop = served?.readiness.kind === 'split-stopped' ? served.readiness.stop : null;
  // A Mac that is away (its goose quit, or silent well past its comeback) is said in the composer
  // bar's words, and the chip then names only the model — the Mac is already in the words (Q-111).
  const away = served ? peerGoneOf(served) : null;
  const goneWords = away ? peerGoneText(intl, away.mac, away.gone) : null;
  const loader = !isModelLoading ? (served?.loader ?? null) : null;
  const loaderWords = loader ? loaderText(intl, loader) : null;
  // A `node:` / `strategy:` chat names what it runs on by the Nodes page's names (design §8.5,
  // Q-255): the node, or the strategy and the node its turns go to — never `node:<id>`.
  const route = !isModelLoading ? (served?.route ?? null) : null;
  const routeLabel =
    route == null
      ? null
      : route.kind === 'node' || route.node == null
        ? route.name
        : intl.formatMessage(i18n.routeChip, { route: route.name, node: route.node });
  const engineLabel =
    servedModel == null
      ? null
      : goneWords
        ? servedModel
        : (servedRunning || splitStop) && servedWhere
          ? intl.formatMessage(i18n.servedChip, { model: servedModel, where: servedWhere })
          : intl.formatMessage(i18n.servedChipNotRunning, { model: servedModel });
  // While the loader loads a node, the chip names that node: the stopped way is on its way out.
  const chipLabel =
    routeLabel ?? (loader?.kind === 'loading' ? loader.swap.target.name : engineLabel);
  return { servedModel, servedWhere, servedRunning, splitStop, goneWords, loaderWords, chipLabel };
}
