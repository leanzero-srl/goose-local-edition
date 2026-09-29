import type { IntlShape } from 'react-intl';
import { defineMessages } from '../../../../i18n';
import type { ChatServedBy } from '../../../chatServedBy/chatServedBy';
import { peerGoneOf, peerGoneText } from '../../../chatServedBy/peerGoneText';
import { shortModelName } from '../../../noNodeNotice/mlxMount';
import { displacedText, loaderText, type ChatLoader } from '../../../chatServedBy/loaderText';
import { fellBackText } from '../../../chatServedBy/turnLine';

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
  ownNodes: { id: 'modelsBottomBar.ownNodes', defaultMessage: 'This chat’s nodes' },
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
  /** This chat's node was stopped for another chat's (§8.7) — the composer notice's words. */
  displacedWords: string | null;
  /** The last turn ran on a later entry of its chain (§8.7 `nodes.fellBack`) — the composer's words. */
  fellBackWords: string | null;
  /** The chip's label; null = the chip keeps the model it was given. */
  chipLabel: string | null;
}

/**
 * The node this chat's in-flight turn waits on the loader for: the node its wait loads, or the load
 * that is for this chat. A behind-switch wait (Q-442) carries on on the chat's own node — its
 * target. null = the loader holds no turn of this chat's (a refusal, another chat's load).
 */
function loaderNodeOfThisTurn(loader: ChatLoader | null): string | null {
  if (loader?.kind === 'waiting') return loader.wait.target.name;
  if (loader?.kind === 'loading' && loader.forThisChat) return loader.swap.target.name;
  return null;
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
  const displaced = !isModelLoading ? (served?.displaced ?? null) : null;
  const displacedWords = displaced ? displacedText(intl, displaced) : null;
  const fellBack = !isModelLoading ? (served?.fellBack ?? null) : null;
  const fellBackWords = fellBack ? fellBackText(intl, fellBack) : null;
  // A `node:` / `strategy:` chat names what it runs on by the Nodes page's names (design §8.5,
  // Q-255): the node, or the strategy and the node its turns go to — never `node:<id>`.
  const route = !isModelLoading ? (served?.route ?? null) : null;
  // A chat's own node set is "This chat's nodes", never the name goosed generates for it (Q-379).
  const routeName =
    route?.kind === 'strategy' && route.own ? intl.formatMessage(i18n.ownNodes) : route?.name;
  // While this chat's turn waits on the loader, it goes to the node the loader is for — never the
  // node its last turn ran on (Q-460, 3.0.74: "… · deepseek" while the turn waited for the Studio).
  const routeNode =
    route?.kind === 'strategy' ? (loaderNodeOfThisTurn(loader) ?? route.node) : null;
  const routeLabel =
    route == null
      ? null
      : routeNode == null
        ? (routeName ?? null)
        : intl.formatMessage(i18n.routeChip, { route: routeName, node: routeNode });
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
  return {
    servedModel,
    servedWhere,
    servedRunning,
    splitStop,
    goneWords,
    loaderWords,
    displacedWords,
    fellBackWords,
    chipLabel,
  };
}
