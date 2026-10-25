import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react';
import { ArrowLeft, Eraser, Loader2, Power, ShieldCheck, Unplug, X, Zap } from 'lucide-react';
import type { IntlShape } from 'react-intl';
import { defineMessages, useIntl } from '../../i18n';
import {
  Button,
  Checkbox,
  Chip,
  SURFACE,
  TNUM,
  TONE_TEXT,
  TYPE,
  WEIGHT,
  cx,
  type NodeIndex,
} from '../lz';
import { OverlayDialog, OverlayDialogTitle } from '../ui/OverlayDialog';
import { useConfirmDialog } from '../ui/useConfirmDialog';
import { memoryFree, memoryHolders, readNeverTouchExtras } from '../../acp/memfree';
import { errorMessage } from '../../utils/conversionUtils';
import {
  DEFAULT_NEVER_TOUCH,
  afterLine,
  effectiveNeverTouch,
  forceStopBundle,
  gib,
  groupByCategory,
  isGooseOwnGroup,
  neverTouchHit,
  nuclearRequest,
  nuclearRows,
  quitOffer,
  unloadOffer,
  verdictLine,
  type MemfreeGroup,
  type MemfreeItem,
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
  forceStop: { id: 'freeMemory.forceStop', defaultMessage: 'Force stop' },
  forceStopTitle: { id: 'freeMemory.forceStopTitle', defaultMessage: 'Force stop {app}?' },
  forceStopBody: {
    id: 'freeMemory.forceStopBody',
    defaultMessage:
      '{app} did not quit when asked, maybe because it has unsaved work. Force stop ends it on {mac} without saving. Unsaved work in {app} is lost.',
  },
  forceStopConfirm: { id: 'freeMemory.forceStopConfirm', defaultMessage: 'Force stop {app}' },
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
  unload: { id: 'freeMemory.unload', defaultMessage: 'Unload' },
  unloadTitle: { id: 'freeMemory.unloadTitle', defaultMessage: 'Unload {server}?' },
  unloadBody: {
    id: 'freeMemory.unloadBody',
    defaultMessage:
      'goose unloads the models {server} holds on {mac} (an MLX engine is asked to stop). Whatever uses it has to load them again.',
  },
  unloadInUse: {
    id: 'freeMemory.unloadInUse',
    defaultMessage: 'In use right now, this stops their work:',
  },
  unloadConfirm: { id: 'freeMemory.unloadConfirm', defaultMessage: 'Unload {server}' },
  keepRunning: { id: 'freeMemory.keepRunning', defaultMessage: 'Keep it running' },
  usedBy: { id: 'freeMemory.usedBy', defaultMessage: 'In use: {who}' },
  protectedChip: { id: 'freeMemory.protectedChip', defaultMessage: 'Protected' },
  neverTouchReason: {
    id: 'freeMemory.neverTouchReason',
    defaultMessage: 'on the never-touch list ({entry})',
  },
  leftAlone: { id: 'freeMemory.leftAlone', defaultMessage: 'Left alone' },
  nuclearOpen: {
    id: 'freeMemory.nuclearOpen',
    defaultMessage: 'Free everything except protected',
  },
  nuclearIntro: {
    id: 'freeMemory.nuclearIntro',
    defaultMessage:
      'Everything nobody is using is ticked. Untick what you want to keep. Protected items are never touched.',
  },
  nuclearGooseOwn: { id: 'freeMemory.nuclearGooseOwn', defaultMessage: 'goose’s own memory' },
  nuclearTotal: { id: 'freeMemory.nuclearTotal', defaultMessage: 'Ticked: <b>{gb} GB</b>' },
  nuclearFree: { id: 'freeMemory.nuclearFree', defaultMessage: 'Free {gb} GB' },
  nuclearBack: { id: 'freeMemory.nuclearBack', defaultMessage: 'Back' },
  nuclearProtectedHead: { id: 'freeMemory.nuclearProtectedHead', defaultMessage: 'Never touched' },
  nuclearConfirmTitle: {
    id: 'freeMemory.nuclearConfirmTitle',
    defaultMessage: 'Free {gb} GB on {mac}?',
  },
  nuclearConfirmBody: {
    id: 'freeMemory.nuclearConfirmBody',
    defaultMessage:
      'goose asks each ticked app to quit and unloads each ticked model server. Apps are asked to quit. An app that does not quit is left running and reported, not stopped. Protected items are never touched.',
  },
  nuclearConfirmKeep: { id: 'freeMemory.nuclearConfirmKeep', defaultMessage: 'Keep everything' },
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
  /** Defaults + this Mac's additions; sent with every call so a peer honours them too. */
  const [neverTouch, setNeverTouch] = useState<string[] | null>(null);
  const [view, setView] = useState<'list' | 'nuclear'>('list');
  const [ticked, setTicked] = useState<Set<string>>(new Set());
  const [gooseOwnTicked, setGooseOwnTicked] = useState(true);
  const fmt = useCallback(
    (bytes: number) =>
      intl.formatNumber(gib(bytes), { minimumFractionDigits: 1, maximumFractionDigits: 1 }),
    [intl]
  );

  useEffect(() => {
    let alive = true;
    readNeverTouchExtras().then(
      (extras) => alive && setNeverTouch(effectiveNeverTouch(extras)),
      () => alive && setNeverTouch([...DEFAULT_NEVER_TOUCH])
    );
    return () => {
      alive = false;
    };
  }, []);

  const read = useCallback(async () => {
    if (!neverTouch) return;
    try {
      setHolders(await memoryHolders({ nodeId: target, fitModelId, neverTouch }));
      setError(null);
    } catch (e) {
      setError(intl.formatMessage(i18n.readFailed, { reason: errorMessage(e, String(e)) }));
    }
  }, [target, fitModelId, intl, neverTouch]);

  useEffect(() => {
    void read();
  }, [read]);

  const free = async (
    key: string,
    what: {
      gooseOwned: boolean;
      quitApps: string[];
      unloadServers: string[];
      seenUsedBy?: Record<string, string[]>;
      forceStopApps?: string[];
      nuclear: boolean;
    }
  ) => {
    setBusy(key);
    try {
      setResult(
        await memoryFree({
          nodeId: target,
          gooseOwned: what.gooseOwned,
          quitApps: what.quitApps,
          unloadServers: what.unloadServers,
          seenUsedBy: what.seenUsedBy ?? {},
          forceStopApps: what.forceStopApps ?? [],
          nuclear: what.nuclear,
          neverTouch: neverTouch ?? [],
          fitModelId,
        })
      );
      setError(null);
      setView('list');
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
    if (ok) {
      await free(group.id, {
        gooseOwned: false,
        quitApps: [group.bundleId],
        unloadServers: [],
        nuclear: false,
      });
    }
  };

  const unload = async (group: MemfreeGroup) => {
    const usedBy = group.usedBy ?? [];
    const ok = await confirm({
      title: intl.formatMessage(i18n.unloadTitle, { server: group.label }),
      message: intl.formatMessage(i18n.unloadBody, { server: group.label, mac: macName }),
      detail:
        usedBy.length > 0 ? (
          <div data-testid="free-memory-unload-used-by" className="flex flex-col gap-0.5">
            <p className={WEIGHT.semibold}>{intl.formatMessage(i18n.unloadInUse)}</p>
            {usedBy.map((who) => (
              <p key={who} className="break-words">
                {who}
              </p>
            ))}
          </div>
        ) : undefined,
      confirmLabel: intl.formatMessage(i18n.unloadConfirm, { server: group.label }),
      cancelLabel: intl.formatMessage(i18n.keepRunning),
      confirmVariant: 'destructive',
    });
    if (ok) {
      await free(group.id, {
        gooseOwned: false,
        quitApps: [],
        unloadServers: [group.id],
        seenUsedBy: { [group.id]: [...usedBy] },
        nuclear: false,
      });
    }
  };

  const forceStop = async (item: MemfreeItem, bundleId: string) => {
    const ok = await confirm({
      title: intl.formatMessage(i18n.forceStopTitle, { app: item.label }),
      message: intl.formatMessage(i18n.forceStopBody, { app: item.label, mac: macName }),
      confirmLabel: intl.formatMessage(i18n.forceStopConfirm, { app: item.label }),
      cancelLabel: intl.formatMessage(i18n.keep),
      confirmVariant: 'destructive',
    });
    if (ok) {
      await free(item.id, {
        gooseOwned: false,
        quitApps: [],
        unloadServers: [],
        forceStopApps: [bundleId],
        nuclear: true,
      });
    }
  };

  const rows = useMemo(
    () => (holders && neverTouch ? nuclearRows(holders.groups, neverTouch) : []),
    [holders, neverTouch]
  );
  const hasGooseOwn = !!holders && holders.groups.some(isGooseOwnGroup);
  const remoteOff = !!holders && !holders.quitAppsAllowed && target != null;

  const openNuclear = () => {
    setTicked(
      new Set(
        rows.filter((r) => r.kind === 'offer' && r.ticked && !remoteOff).map((r) => r.group.id)
      )
    );
    setGooseOwnTicked(hasGooseOwn);
    setView('nuclear');
  };

  const toggle = (id: string, on: boolean) =>
    setTicked((prev) => {
      const next = new Set(prev);
      if (on) next.add(id);
      else next.delete(id);
      return next;
    });

  const tickedBytes =
    rows
      .filter((r) => r.kind === 'offer' && ticked.has(r.group.id))
      .reduce((sum, r) => sum + r.group.bytes, 0) +
    (gooseOwnTicked && hasGooseOwn && holders ? holders.gooseOwnedBytes : 0);
  const tickedCount =
    rows.filter((r) => r.kind === 'offer' && ticked.has(r.group.id)).length +
    (gooseOwnTicked && hasGooseOwn ? 1 : 0);

  const freeNuclear = async () => {
    const request = nuclearRequest(rows, ticked, gooseOwnTicked && hasGooseOwn, neverTouch ?? []);
    const labels = rows
      .filter((r) => r.kind === 'offer' && ticked.has(r.group.id))
      .map((r) => r.group.label);
    if (request.gooseOwned) labels.unshift(intl.formatMessage(i18n.nuclearGooseOwn));
    const ok = await confirm({
      title: intl.formatMessage(i18n.nuclearConfirmTitle, { gb: fmt(tickedBytes), mac: macName }),
      message: intl.formatMessage(i18n.nuclearConfirmBody),
      detail: (
        <p data-testid="free-memory-nuclear-confirm-list" className="break-words">
          {intl.formatList(labels, { type: 'conjunction' })}
        </p>
      ),
      confirmLabel: intl.formatMessage(i18n.nuclearFree, { gb: fmt(tickedBytes) }),
      cancelLabel: intl.formatMessage(i18n.nuclearConfirmKeep),
      confirmVariant: 'destructive',
    });
    if (!ok) return;
    const { gooseOwned, quitApps, unloadServers, seenUsedBy, forceStopApps, nuclear } = request;
    await free('nuclear', {
      gooseOwned,
      quitApps,
      unloadServers,
      seenUsedBy,
      forceStopApps,
      nuclear,
    });
  };

  const protectedReason = (g: MemfreeGroup): string | null => {
    if (g.protectedWhy) return g.protectedWhy;
    const hit = neverTouch ? neverTouchHit(g, neverTouch) : null;
    return hit ? intl.formatMessage(i18n.neverTouchReason, { entry: hit }) : null;
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
                  .map((it) => {
                    const bundle = forceStopBundle(it, holders?.groups ?? [], neverTouch ?? []);
                    return (
                      <div key={it.id} className="flex flex-wrap items-center gap-2">
                        <p className={cx(WEIGHT.semibold, TONE_TEXT.err)}>
                          {intl.formatMessage(i18n.itemFailed, {
                            label: it.label,
                            message: it.message,
                          })}
                        </p>
                        {bundle && (
                          <Button
                            variant="destructive"
                            size="sm"
                            data-testid={`free-memory-force-${bundle}`}
                            disabled={remoteOff || busy != null}
                            title={
                              remoteOff
                                ? intl.formatMessage(i18n.remoteQuitOff, { mac: macName })
                                : undefined
                            }
                            icon={busy === it.id ? spinner : <Power />}
                            onClick={() => void forceStop(it, bundle)}
                          >
                            {intl.formatMessage(i18n.forceStop)}
                          </Button>
                        )}
                      </div>
                    );
                  })}
              </div>
            )}

            <div className="flex flex-wrap items-center gap-2">
              <Button
                variant="primary"
                size="sm"
                data-testid="free-memory-own"
                disabled={busy != null || holders.gooseOwnedBytes <= 0}
                icon={busy === 'own' ? spinner : <Zap />}
                onClick={() =>
                  void free('own', {
                    gooseOwned: true,
                    quitApps: [],
                    unloadServers: [],
                    nuclear: false,
                  })
                }
              >
                {holders.gooseOwnedBytes > 0
                  ? intl.formatMessage(i18n.freeOwn, { gb: fmt(holders.gooseOwnedBytes) })
                  : intl.formatMessage(i18n.nothingOwn)}
              </Button>
              {view === 'list' && (
                <Button
                  variant="secondary"
                  size="sm"
                  data-testid="free-memory-nuclear-open"
                  disabled={busy != null || neverTouch == null}
                  icon={<Eraser />}
                  onClick={openNuclear}
                >
                  {intl.formatMessage(i18n.nuclearOpen)}
                </Button>
              )}
            </div>

            {!holders.quitAppsAllowed && target != null && (
              <p data-testid="free-memory-remote-off" className={TYPE.meta}>
                {intl.formatMessage(i18n.remoteQuitOff, { mac: macName })}
              </p>
            )}

            {view === 'nuclear' && (
              <section data-testid="free-memory-nuclear" className="flex flex-col gap-2">
                <div className="flex items-center justify-between gap-2">
                  <h3 className={cx(TYPE.body, WEIGHT.semibold)}>
                    {intl.formatMessage(i18n.nuclearOpen)}
                  </h3>
                  <Button
                    variant="ghost"
                    size="sm"
                    data-testid="free-memory-nuclear-back"
                    icon={<ArrowLeft />}
                    disabled={busy != null}
                    onClick={() => setView('list')}
                  >
                    {intl.formatMessage(i18n.nuclearBack)}
                  </Button>
                </div>
                <p className={TYPE.meta}>{intl.formatMessage(i18n.nuclearIntro)}</p>
                <ul className="flex flex-col">
                  {hasGooseOwn && (
                    <li className="flex items-center justify-between gap-3 border-b border-lz-border py-1.5">
                      <Checkbox
                        testId="free-memory-tick-gooseOwned"
                        checked={gooseOwnTicked}
                        onChange={setGooseOwnTicked}
                        disabled={busy != null}
                        label={intl.formatMessage(i18n.nuclearGooseOwn)}
                      />
                      <strong className={cx(TYPE.body, WEIGHT.semibold, TNUM)}>
                        {fmt(holders.gooseOwnedBytes)} GB
                      </strong>
                    </li>
                  )}
                  {rows.map((r) =>
                    r.kind === 'offer' ? (
                      <li
                        key={r.group.id}
                        className="flex items-center justify-between gap-3 border-b border-lz-border py-1.5"
                      >
                        <Checkbox
                          testId={`free-memory-tick-${r.group.id}`}
                          checked={ticked.has(r.group.id)}
                          onChange={(on) => toggle(r.group.id, on)}
                          disabled={busy != null || remoteOff}
                          label={r.group.label}
                          description={
                            (r.group.usedBy ?? []).length > 0
                              ? intl.formatMessage(i18n.usedBy, {
                                  who: (r.group.usedBy ?? []).join('; '),
                                })
                              : undefined
                          }
                        />
                        <strong className={cx(TYPE.body, WEIGHT.semibold, TNUM)}>
                          {fmt(r.group.bytes)} GB
                        </strong>
                      </li>
                    ) : null
                  )}
                </ul>
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <p data-testid="free-memory-nuclear-total" className={cx(TYPE.body, TNUM)}>
                    {intl.formatMessage(i18n.nuclearTotal, { gb: fmt(tickedBytes), ...bold })}
                  </p>
                  <Button
                    variant="destructive"
                    size="sm"
                    data-testid="free-memory-nuclear-free"
                    disabled={busy != null || tickedCount === 0}
                    icon={busy === 'nuclear' ? spinner : <Eraser />}
                    onClick={() => void freeNuclear()}
                  >
                    {intl.formatMessage(i18n.nuclearFree, { gb: fmt(tickedBytes) })}
                  </Button>
                </div>
                {rows.some((r) => r.kind !== 'offer') && (
                  <>
                    <h4 className={cx(TYPE.meta, WEIGHT.semibold)}>
                      {intl.formatMessage(i18n.nuclearProtectedHead)}
                    </h4>
                    <ul className="flex flex-col">
                      {rows.map((r) =>
                        r.kind === 'offer' ? null : (
                          <li
                            key={r.group.id}
                            data-testid={`free-memory-kept-${r.group.id}`}
                            className="flex items-center justify-between gap-3 border-b border-lz-border py-1.5"
                          >
                            <div className="flex min-w-0 flex-col gap-0.5">
                              <div className="flex flex-wrap items-center gap-2">
                                <span className={cx(TYPE.body, 'break-words')}>
                                  {r.group.label}
                                </span>
                                <Chip
                                  tone={r.kind === 'protected' ? 'accent' : 'stopped'}
                                  icon={r.kind === 'protected' ? <ShieldCheck /> : undefined}
                                >
                                  {intl.formatMessage(
                                    r.kind === 'protected' ? i18n.protectedChip : i18n.leftAlone
                                  )}
                                </Chip>
                              </div>
                              {(r.kind === 'protected' ? protectedReason(r.group) : r.reason) && (
                                <p className={cx(TYPE.meta, 'break-words')}>
                                  {r.kind === 'protected' ? protectedReason(r.group) : r.reason}
                                </p>
                              )}
                            </div>
                            <strong className={cx(TYPE.body, WEIGHT.semibold, TNUM)}>
                              {fmt(r.group.bytes)} GB
                            </strong>
                          </li>
                        )
                      )}
                    </ul>
                  </>
                )}
              </section>
            )}

            {view === 'list' && (
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
                        const offer = quitOffer(g, holders.quitAppsAllowed, neverTouch ?? []);
                        const unloadable = unloadOffer(
                          g,
                          holders.quitAppsAllowed,
                          neverTouch ?? []
                        );
                        const kept = protectedReason(g);
                        return (
                          <li
                            key={g.id}
                            data-testid={`free-memory-holder-${g.id}`}
                            className="flex items-center justify-between gap-3 border-b border-lz-border py-1.5"
                          >
                            <div className="min-w-0">
                              <div className="flex flex-wrap items-center gap-2">
                                <span className={cx(TYPE.body, 'break-words')}>{g.label}</span>
                                {kept && (
                                  <Chip tone="accent" icon={<ShieldCheck />} title={kept}>
                                    {intl.formatMessage(i18n.protectedChip)}
                                  </Chip>
                                )}
                              </div>
                              {(kept ?? g.why) && (
                                <p className={cx(TYPE.meta, 'break-words')}>{kept ?? g.why}</p>
                              )}
                              {(g.usedBy ?? []).length > 0 && (
                                <p className={cx(TYPE.meta, 'break-words')}>
                                  {intl.formatMessage(i18n.usedBy, {
                                    who: (g.usedBy ?? []).join('; '),
                                  })}
                                </p>
                              )}
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
                              {unloadable.show && (
                                <Button
                                  variant="secondary"
                                  size="sm"
                                  data-testid={`free-memory-unload-${g.id}`}
                                  disabled={unloadable.disabled || busy != null}
                                  title={
                                    unloadable.disabled
                                      ? intl.formatMessage(i18n.remoteQuitOff, { mac: macName })
                                      : undefined
                                  }
                                  icon={busy === g.id ? spinner : <Unplug />}
                                  onClick={() => void unload(g)}
                                >
                                  {intl.formatMessage(i18n.unload)}
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
            )}
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
