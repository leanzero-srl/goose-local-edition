import { useCallback, useEffect, useState, type ReactNode } from 'react';
import { Loader2, Power, X, Zap } from 'lucide-react';
import type { IntlShape } from 'react-intl';
import { defineMessages, useIntl } from '../../i18n';
import { Button, Chip, SURFACE, TNUM, TONE_TEXT, TYPE, WEIGHT, cx, type NodeIndex } from '../lz';
import { OverlayDialog, OverlayDialogTitle } from '../ui/OverlayDialog';
import { useConfirmDialog } from '../ui/useConfirmDialog';
import { memoryFree, memoryHolders } from '../../acp/memfree';
import { errorMessage } from '../../utils/conversionUtils';
import {
  afterLine,
  gib,
  groupByCategory,
  quitOffer,
  verdictLine,
  type MemfreeGroup,
  type MemoryFreeResult,
  type MemoryHolders,
} from './memfree';

const i18n = defineMessages({
  title: { id: 'freeMemory.title', defaultMessage: 'Free up memory on {mac}' },
  loading: { id: 'freeMemory.loading', defaultMessage: 'Measuring what holds memory…' },
  readFailed: { id: 'freeMemory.readFailed', defaultMessage: 'Could not measure memory: {reason}' },
  budget: {
    id: 'freeMemory.budget',
    defaultMessage: '<b>{free} GB</b> free for a model, of {total} GB',
  },
  fitsByGooseOwned: {
    id: 'freeMemory.fitsByGooseOwned',
    defaultMessage: 'Freeing goose’s own memory (<b>{gb} GB</b>) is enough for this model.',
  },
  fitsWithApps: {
    id: 'freeMemory.fitsWithApps',
    defaultMessage:
      'This model fits after freeing <b>{gb} GB</b>: goose’s own memory and quitting {apps}.',
  },
  fitsWithAppsOnly: {
    id: 'freeMemory.fitsWithAppsOnly',
    defaultMessage: 'This model fits after quitting {apps}, which frees <b>{gb} GB</b>.',
  },
  cannotFit: {
    id: 'freeMemory.cannotFit',
    defaultMessage:
      'Even freeing everything listed here (<b>{gb} GB</b>) leaves this model <b>{short} GB</b> short.',
  },
  altSmaller: {
    id: 'freeMemory.altSmaller',
    defaultMessage: 'A smaller variant fits: {model} (needs <b>{gb} GB</b>)',
  },
  altPeer: {
    id: 'freeMemory.altPeer',
    defaultMessage: 'Run it on {mac} instead (<b>{gb} GB</b> free there)',
  },
  altPeerNoFree: {
    id: 'freeMemory.altPeerNoFree',
    defaultMessage: 'Run it on <b>{mac}</b> instead',
  },
  freeOwn: { id: 'freeMemory.freeOwn', defaultMessage: 'Free goose’s own ({gb} GB)' },
  nothingOwn: { id: 'freeMemory.nothingOwn', defaultMessage: 'goose holds no memory it can free' },
  quit: { id: 'freeMemory.quit', defaultMessage: 'Quit' },
  quitTitle: { id: 'freeMemory.quitTitle', defaultMessage: 'Quit {app}?' },
  quitBody: {
    id: 'freeMemory.quitBody',
    defaultMessage:
      'goose asks {app} on {mac} to quit the normal way. Save your work there first: anything unsaved may be lost.',
  },
  quitConfirm: { id: 'freeMemory.quitConfirm', defaultMessage: 'Quit {app}' },
  keep: { id: 'freeMemory.keep', defaultMessage: 'Keep it open' },
  remoteQuitOff: {
    id: 'freeMemory.remoteQuitOff',
    defaultMessage:
      '{mac} does not let other Macs quit its apps (Settings › App on that Mac). Quit them there.',
  },
  after: {
    id: 'freeMemory.after',
    defaultMessage:
      'Before <b>{before} GB</b> free, now <b>{after} GB</b> free (freed {freed} GB).',
  },
  afterFits: { id: 'freeMemory.afterFits', defaultMessage: 'The model fits now.' },
  afterShort: {
    id: 'freeMemory.afterShort',
    defaultMessage: 'Still <b>{gb} GB</b> short.',
  },
  itemFailed: { id: 'freeMemory.itemFailed', defaultMessage: '{label}: {message}' },
  freeFailed: { id: 'freeMemory.freeFailed', defaultMessage: 'Could not free memory: {reason}' },
  close: { id: 'freeMemory.close', defaultMessage: 'Close' },
  catGooseEngine: { id: 'freeMemory.cat.gooseEngine', defaultMessage: 'goose’s engines' },
  catGooseOrphan: { id: 'freeMemory.cat.gooseOrphan', defaultMessage: 'Left over by goose' },
  catAiServer: { id: 'freeMemory.cat.aiServer', defaultMessage: 'Model servers' },
  catJob: { id: 'freeMemory.cat.job', defaultMessage: 'Jobs' },
  catUserApp: { id: 'freeMemory.cat.userApp', defaultMessage: 'Apps' },
  catSystem: { id: 'freeMemory.cat.system', defaultMessage: 'macOS and the rest' },
});

/** One solid hue per category (the node ramp: distinct, saturated, with its measured ink). */
const CATEGORY_HUE: Record<string, NodeIndex> = {
  gooseEngine: 1,
  gooseOrphan: 2,
  aiServer: 3,
  job: 4,
  userApp: 5,
  system: 6,
};

function categoryLabel(intl: IntlShape, category: string): string {
  switch (category) {
    case 'gooseEngine':
      return intl.formatMessage(i18n.catGooseEngine);
    case 'gooseOrphan':
      return intl.formatMessage(i18n.catGooseOrphan);
    case 'aiServer':
      return intl.formatMessage(i18n.catAiServer);
    case 'job':
      return intl.formatMessage(i18n.catJob);
    case 'userApp':
      return intl.formatMessage(i18n.catUserApp);
    default:
      return intl.formatMessage(i18n.catSystem);
  }
}

const bold = { b: (chunks: ReactNode) => <strong className={WEIGHT.semibold}>{chunks}</strong> };

export interface FreeMemoryDialogProps {
  /** The Link node id of a peer Mac; omitted for this Mac. */
  target?: string;
  macName: string;
  /** The model the verdict is about (the card's model); omitted from the per-Mac entry. */
  fitModelId?: string | null;
  onClose: () => void;
}

/**
 * "Free up memory": who holds memory on one Mac (measured by the engine), the honest verdict for the
 * model, one click to free goose's own share, and a Quit per user app behind the house confirm
 * dialog. Every number is the engine's; after each free the dialog re-reads and shows the new verdict.
 */
export function FreeMemoryDialog({ target, macName, fitModelId, onClose }: FreeMemoryDialogProps) {
  const intl = useIntl();
  const { confirm, dialog } = useConfirmDialog();
  const [holders, setHolders] = useState<MemoryHolders | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [result, setResult] = useState<MemoryFreeResult | null>(null);
  const fmt = useCallback(
    (bytes: number) =>
      intl.formatNumber(gib(bytes), { minimumFractionDigits: 1, maximumFractionDigits: 1 }),
    [intl]
  );

  const read = useCallback(async () => {
    try {
      setHolders(await memoryHolders({ nodeId: target, fitModelId }));
      setError(null);
    } catch (e) {
      setError(intl.formatMessage(i18n.readFailed, { reason: errorMessage(e, String(e)) }));
    }
  }, [target, fitModelId, intl]);

  useEffect(() => {
    void read();
  }, [read]);

  const free = async (key: string, gooseOwned: boolean, quitApps: string[]) => {
    setBusy(key);
    try {
      setResult(await memoryFree({ nodeId: target, gooseOwned, quitApps, fitModelId }));
      setError(null);
      await read();
    } catch (e) {
      setError(intl.formatMessage(i18n.freeFailed, { reason: errorMessage(e, String(e)) }));
    } finally {
      setBusy(null);
    }
  };

  const quit = async (group: MemfreeGroup) => {
    if (!group.bundleId) return;
    const ok = await confirm({
      title: intl.formatMessage(i18n.quitTitle, { app: group.label }),
      message: intl.formatMessage(i18n.quitBody, { app: group.label, mac: macName }),
      confirmLabel: intl.formatMessage(i18n.quitConfirm, { app: group.label }),
      cancelLabel: intl.formatMessage(i18n.keep),
      confirmVariant: 'destructive',
    });
    if (ok) await free(group.id, false, [group.bundleId]);
  };

  const line = holders ? verdictLine(holders) : null;
  const after = result ? afterLine(result) : null;
  const n1 = (v: number) =>
    intl.formatNumber(v, { minimumFractionDigits: 1, maximumFractionDigits: 1 });
  const spinner = <Loader2 className="animate-spin" />;

  return (
    <OverlayDialog
      open
      onClose={onClose}
      panelClassName={cx(
        'flex max-h-[85vh] w-[40rem] max-w-[90vw] flex-col gap-4 overflow-auto rounded-none p-5',
        SURFACE.overlay
      )}
    >
      <div data-testid="free-memory-dialog" className="flex flex-col gap-3">
        <OverlayDialogTitle asChild>
          <h2 className={cx('break-words', TYPE.h2)}>
            {intl.formatMessage(i18n.title, { mac: macName })}
          </h2>
        </OverlayDialogTitle>

        {!holders && !error && (
          <p className={cx(TYPE.bodyMuted, 'flex items-center gap-2')}>
            <Loader2 className="size-4 animate-spin" />
            {intl.formatMessage(i18n.loading)}
          </p>
        )}

        {holders && (
          <>
            <p data-testid="free-memory-budget" className={cx(TYPE.body, TNUM)}>
              {intl.formatMessage(i18n.budget, {
                free: fmt(holders.budgetBytes),
                total: fmt(holders.totalBytes),
                ...bold,
              })}
            </p>
            {line && (
              <div
                data-testid="free-memory-verdict"
                data-verdict={line.kind}
                className="flex flex-col gap-1"
              >
                <p className={cx(TYPE.body, TNUM, line.kind === 'cannotFit' && TONE_TEXT.err)}>
                  {line.kind === 'fitsByGooseOwned' &&
                    intl.formatMessage(i18n.fitsByGooseOwned, { gb: n1(line.freeGb), ...bold })}
                  {line.kind === 'fitsWithApps' &&
                    intl.formatMessage(line.withGoose ? i18n.fitsWithApps : i18n.fitsWithAppsOnly, {
                      gb: n1(line.freeGb),
                      apps: intl.formatList(line.apps, { type: 'conjunction' }),
                      ...bold,
                    })}
                  {line.kind === 'cannotFit' &&
                    intl.formatMessage(i18n.cannotFit, {
                      gb: n1(line.everythingGb),
                      short: n1(line.shortGb),
                      ...bold,
                    })}
                </p>
                {line.kind === 'cannotFit' && line.alternatives.length > 0 && (
                  <ul
                    data-testid="free-memory-alternatives"
                    className={cx(TYPE.body, TNUM, 'flex flex-col gap-0.5')}
                  >
                    {line.alternatives.map((a) => (
                      <li key={a.kind === 'smallerVariant' ? a.modelId : a.nodeId}>
                        {a.kind === 'smallerVariant'
                          ? intl.formatMessage(i18n.altSmaller, {
                              model: a.modelId,
                              gb: fmt(a.needBytes),
                              ...bold,
                            })
                          : a.budgetBytes === undefined
                            ? intl.formatMessage(i18n.altPeerNoFree, { mac: a.label, ...bold })
                            : intl.formatMessage(i18n.altPeer, {
                                mac: a.label,
                                gb: fmt(a.budgetBytes),
                                ...bold,
                              })}
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            )}

            {after && (
              <div
                data-testid="free-memory-after"
                className={cx(TYPE.body, TNUM, 'flex flex-col gap-0.5')}
              >
                <p>
                  {intl.formatMessage(i18n.after, {
                    before: n1(after.beforeGb),
                    after: n1(after.afterGb),
                    freed: n1(after.freedGb),
                    ...bold,
                  })}{' '}
                  {after.fits === true && (
                    <span className={cx(WEIGHT.semibold, TONE_TEXT.ok)}>
                      {intl.formatMessage(i18n.afterFits)}
                    </span>
                  )}
                  {after.shortGb != null && (
                    <span className={TONE_TEXT.err}>
                      {intl.formatMessage(i18n.afterShort, { gb: n1(after.shortGb), ...bold })}
                    </span>
                  )}
                </p>
                {result?.items
                  .filter((it) => !it.ok)
                  .map((it) => (
                    <p key={it.id} className={cx(WEIGHT.semibold, TONE_TEXT.err)}>
                      {intl.formatMessage(i18n.itemFailed, {
                        label: it.label,
                        message: it.message,
                      })}
                    </p>
                  ))}
              </div>
            )}

            <div>
              <Button
                variant="primary"
                size="sm"
                data-testid="free-memory-own"
                disabled={busy != null || holders.gooseOwnedBytes <= 0}
                icon={busy === 'own' ? spinner : <Zap />}
                onClick={() => void free('own', true, [])}
              >
                {holders.gooseOwnedBytes > 0
                  ? intl.formatMessage(i18n.freeOwn, { gb: fmt(holders.gooseOwnedBytes) })
                  : intl.formatMessage(i18n.nothingOwn)}
              </Button>
            </div>

            {!holders.quitAppsAllowed && target != null && (
              <p data-testid="free-memory-remote-off" className={TYPE.meta}>
                {intl.formatMessage(i18n.remoteQuitOff, { mac: macName })}
              </p>
            )}

            <div className="flex flex-col gap-3">
              {groupByCategory(holders.groups).map((block) => (
                <section
                  key={block.category}
                  data-testid={`free-memory-cat-${block.category}`}
                  className="flex flex-col gap-1"
                >
                  <div className="flex items-center gap-2">
                    <Chip node={CATEGORY_HUE[block.category] ?? 6}>
                      {categoryLabel(intl, block.category)}
                    </Chip>
                    <strong className={cx(TYPE.body, WEIGHT.semibold, TNUM)}>
                      {fmt(block.bytes)} GB
                    </strong>
                  </div>
                  <ul className="flex flex-col">
                    {block.groups.map((g) => {
                      const offer = quitOffer(g, holders.quitAppsAllowed);
                      return (
                        <li
                          key={g.id}
                          data-testid={`free-memory-holder-${g.id}`}
                          className="flex items-center justify-between gap-3 border-b border-lz-border py-1.5"
                        >
                          <div className="min-w-0">
                            <p className={cx(TYPE.body, 'break-words')}>{g.label}</p>
                            {g.why && <p className={cx(TYPE.meta, 'break-words')}>{g.why}</p>}
                          </div>
                          <div className="flex shrink-0 items-center gap-2">
                            <strong className={cx(TYPE.body, WEIGHT.semibold, TNUM)}>
                              {fmt(g.bytes)} GB
                            </strong>
                            {offer.show && (
                              <Button
                                variant="secondary"
                                size="sm"
                                data-testid={`free-memory-quit-${g.id}`}
                                disabled={offer.disabled || busy != null}
                                title={
                                  offer.disabled
                                    ? intl.formatMessage(i18n.remoteQuitOff, { mac: macName })
                                    : undefined
                                }
                                icon={busy === g.id ? spinner : <Power />}
                                onClick={() => void quit(g)}
                              >
                                {intl.formatMessage(i18n.quit)}
                              </Button>
                            )}
                          </div>
                        </li>
                      );
                    })}
                  </ul>
                </section>
              ))}
            </div>
          </>
        )}

        {error && (
          <p
            data-testid="free-memory-error"
            className={cx(TYPE.body, WEIGHT.semibold, TONE_TEXT.err)}
          >
            {error}
          </p>
        )}
      </div>
      <div className="flex justify-end">
        <Button size="sm" variant="secondary" icon={<X />} onClick={onClose}>
          {intl.formatMessage(i18n.close)}
        </Button>
      </div>
      {dialog}
    </OverlayDialog>
  );
}
