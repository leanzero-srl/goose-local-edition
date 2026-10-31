// Each split node's macOS GPU wired limit, and what raising it would buy: the preflight plans the
// split again with that node's GPU ceiling raised (goose-sidecar preflight wired_what_ifs), so the
// layers and context below are the planner's own arithmetic. "Raise on <Mac>" goes to THAT Mac's
// goose app (LeanZero Link), which shows the macOS administrator prompt to whoever is at it.
import { useState } from 'react';
import { Loader2 } from 'lucide-react';
import type { IntlShape } from 'react-intl';
import { defineMessages, useIntl } from '../../i18n';
import { Button, Chip, TNUM, TONE_TEXT, TYPE, WEIGHT, SURFACE, cx } from '../lz';
import { StudioSwitch, ToneBanner } from './studio';
import { mlxEngineWiredLimitApply } from '../../acp/mlx-engine';
import type { MlxDistributedNodePreflight } from '../../acp/mlx-distributed';
import { mlxErrorMessage } from './mlxErrorMessage';
import { gb1, gib } from './mlxDistributed';

const i18n = defineMessages({
  title: { id: 'mlxDistWired.title', defaultMessage: 'GPU wired limits' },
  intro: {
    id: 'mlxDistWired.intro',
    defaultMessage:
      'A node’s budget is the smaller of its free memory and its GPU ceiling, which macOS sets from the wired limit. Where the ceiling binds, the preflight planned the split again with a raised limit.',
  },
  limitDefault: {
    id: 'mlxDistWired.limitDefault',
    defaultMessage: 'wired limit 0 (macOS default) · GPU ceiling {ceiling} GB',
  },
  limitRaised: {
    id: 'mlxDistWired.limitRaised',
    defaultMessage: 'wired limit {mb} MB · GPU ceiling {ceiling} GB',
  },
  limitUnread: { id: 'mlxDistWired.limitUnread', defaultMessage: 'wired limit not read' },
  binds: { id: 'mlxDistWired.binds', defaultMessage: 'Ceiling binds' },
  buys: {
    id: 'mlxDistWired.buys',
    defaultMessage: 'Raise to {mb} MB on {mac}: {layers}max context {now} → ~{then}',
  },
  buysLayers: {
    id: 'mlxDistWired.buysLayers',
    defaultMessage:
      '{count, plural, one {+# layer moves} other {+# layers move}} to {mac} → ',
  },
  buysNothing: {
    id: 'mlxDistWired.buysNothing',
    defaultMessage:
      'Raising {mac} to {mb} MB buys nothing here: the planner still fits max context {now} (another limit binds).',
  },
  keeps: {
    id: 'mlxDistWired.keeps',
    defaultMessage: 'macOS and other apps on {mac} keep {keep} GB',
  },
  beyond: {
    id: 'mlxDistWired.beyond',
    defaultMessage:
      'That is the most the reserve allows, above the suggestion from what {mac} uses now: expect memory pressure there.',
  },
  raise: { id: 'mlxDistWired.raise', defaultMessage: 'Raise on {mac}' },
  keepAfterRestart: { id: 'mlxDistWired.keepAfterRestart', defaultMessage: 'Keep after restart' },
  waiting: {
    id: 'mlxDistWired.waiting',
    defaultMessage:
      'The administrator prompt is up on {mac}: whoever is at it approves or cancels. It takes effect at the split’s next start.',
  },
  done: { id: 'mlxDistWired.done', defaultMessage: 'Raised' },
  doneText: {
    id: 'mlxDistWired.doneText',
    defaultMessage: '{mac} reads {mb} MB now. Run the preflight again to plan with it.',
  },
  failed: { id: 'mlxDistWired.failed', defaultMessage: 'Not changed' },
  noApp: {
    id: 'mlxDistWired.noApp',
    defaultMessage:
      '{mac} is reached over ssh, not through a goose app, so nobody there can be asked from here. Run this in Terminal on {mac}:',
  },
  fallback: {
    id: 'mlxDistWired.fallback',
    defaultMessage:
      'If no goose app is running on {mac}, or nobody there can approve, run this in Terminal on {mac}:',
  },
});

/** Where a node's admin prompt can be asked for: this Mac, a Link peer's app, or nowhere (ssh). */
export type WiredRoute = { kind: 'self' } | { kind: 'link'; nodeId: string } | { kind: 'ssh' };

export function wiredRoute(host: string | null | undefined): WiredRoute {
  if (!host) return { kind: 'self' };
  if (host.startsWith('link:') && host.length > 5) return { kind: 'link', nodeId: host.slice(5) };
  return { kind: 'ssh' };
}

/** "262k" for a context length. */
export function contextK(intl: IntlShape, tokens: number | null | undefined): string {
  if (tokens == null) return '—';
  return `${intl.formatNumber(Math.round(tokens / 1000))}k`;
}

function NodeWiredRow({ node }: { node: MlxDistributedNodePreflight }) {
  const intl = useIntl();
  const w = node.wiredWhatIf ?? null;
  const route = wiredRoute(node.host);
  const [persist, setPersist] = useState(false);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null);
  const mac = node.name;
  const ceiling = node.ceilingBytes != null ? gb1(gib(node.ceilingBytes)) : '—';
  const limit =
    node.wiredLimitMb == null
      ? intl.formatMessage(i18n.limitUnread)
      : node.wiredLimitMb === 0
        ? intl.formatMessage(i18n.limitDefault, { ceiling })
        : intl.formatMessage(i18n.limitRaised, {
            mb: intl.formatNumber(node.wiredLimitMb),
            ceiling,
          });
  const layerDelta = w && w.layersThen != null && w.layersNow != null ? w.layersThen - w.layersNow : 0;
  const gains =
    w != null &&
    w.maxContextThen != null &&
    (w.maxContextNow == null || w.maxContextThen > w.maxContextNow || layerDelta > 0);

  const raise = () => {
    if (!w || route.kind === 'ssh') return;
    setBusy(true);
    setResult(null);
    void mlxEngineWiredLimitApply(
      w.mb,
      persist,
      route.kind === 'link' ? route.nodeId : undefined
    )
      .then((r) =>
        setResult({
          ok: true,
          text: intl.formatMessage(i18n.doneText, {
            mac,
            mb: intl.formatNumber(r.wiredLimitMb ?? w.mb),
          }),
        })
      )
      .catch((e) => setResult({ ok: false, text: mlxErrorMessage(e, '') }))
      .finally(() => setBusy(false));
  };

  return (
    <div
      data-testid="mlx-dist-wired-node"
      data-node={mac}
      data-route={route.kind}
      className={cx('flex min-w-0 flex-col gap-2 p-3', SURFACE.card)}
    >
      <div className="flex flex-wrap items-center gap-2">
        <span className={cx(TYPE.body, WEIGHT.semibold)}>{mac}</span>
        <span className={cx(TYPE.meta, TNUM)}>{limit}</span>
        {w && <Chip tone="warn">{intl.formatMessage(i18n.binds)}</Chip>}
      </div>
      {w && (
        <>
          <span
            data-testid="mlx-dist-wired-buys"
            className={cx(TYPE.body, WEIGHT.semibold, TNUM, gains ? TONE_TEXT.ok : undefined)}
          >
            {gains
              ? intl.formatMessage(i18n.buys, {
                  mb: intl.formatNumber(w.mb),
                  mac,
                  layers:
                    layerDelta > 0
                      ? intl.formatMessage(i18n.buysLayers, { count: layerDelta, mac })
                      : '',
                  now: contextK(intl, w.maxContextNow),
                  then: contextK(intl, w.maxContextThen),
                })
              : intl.formatMessage(i18n.buysNothing, {
                  mac,
                  mb: intl.formatNumber(w.mb),
                  now: contextK(intl, w.maxContextNow),
                })}
          </span>
          <span className={cx(TYPE.meta, TNUM, w.beyondSuggestion ? TONE_TEXT.warn : undefined)}>
            {intl.formatMessage(i18n.keeps, {
              mac,
              keep: intl.formatNumber(w.keepBytes / 1024 ** 3, { maximumFractionDigits: 1 }),
            })}
            {w.beyondSuggestion ? ` · ${intl.formatMessage(i18n.beyond, { mac })}` : ''}
          </span>
          {route.kind === 'ssh' ? (
            <div className="flex flex-col gap-1" data-testid="mlx-dist-wired-fallback">
              <span className={TYPE.meta}>{intl.formatMessage(i18n.noApp, { mac })}</span>
              <code className={TYPE.mono}>{w.fallbackCommand}</code>
            </div>
          ) : (
            gains && (
              <div className="flex flex-wrap items-center gap-2">
                <Button
                  size="sm"
                  variant="primary"
                  disabled={busy}
                  icon={busy ? <Loader2 className="animate-spin" /> : undefined}
                  onClick={raise}
                  data-testid="mlx-dist-wired-raise"
                >
                  {intl.formatMessage(i18n.raise, { mac })}
                </Button>
                <StudioSwitch
                  checked={persist}
                  onChange={setPersist}
                  aria-label={intl.formatMessage(i18n.keepAfterRestart)}
                  disabled={busy}
                />
                <span className={TYPE.meta}>{intl.formatMessage(i18n.keepAfterRestart)}</span>
              </div>
            )
          )}
          {busy && (
            <ToneBanner
              tone="warn"
              live
              label={intl.formatMessage(i18n.raise, { mac })}
              text={intl.formatMessage(i18n.waiting, { mac })}
              testId="mlx-dist-wired-waiting"
            />
          )}
          {result && (
            <ToneBanner
              tone={result.ok ? 'ok' : 'err'}
              label={intl.formatMessage(result.ok ? i18n.done : i18n.failed)}
              text={result.text}
              testId="mlx-dist-wired-result"
            />
          )}
          {result && !result.ok && (
            <div className="flex flex-col gap-1" data-testid="mlx-dist-wired-fallback">
              <span className={TYPE.meta}>{intl.formatMessage(i18n.fallback, { mac })}</span>
              <code className={TYPE.mono}>{w.fallbackCommand}</code>
            </div>
          )}
        </>
      )}
    </div>
  );
}

export function DistributedWiredLimits({
  nodes,
}: {
  nodes: readonly MlxDistributedNodePreflight[];
}) {
  const intl = useIntl();
  if (nodes.length === 0) return null;
  return (
    <div data-testid="mlx-dist-wired" className="flex flex-col gap-2">
      <span className={TYPE.zone}>{intl.formatMessage(i18n.title)}</span>
      <p className={TYPE.meta}>{intl.formatMessage(i18n.intro)}</p>
      <div className="grid grid-cols-1 gap-2 xl:grid-cols-2">
        {nodes.map((n) => (
          <NodeWiredRow key={`${n.rank}|${n.name}`} node={n} />
        ))}
      </div>
    </div>
  );
}
