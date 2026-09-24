import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import { defineMessages, useIntl } from '../../i18n';
import { useFeatures } from '../../contexts/FeaturesContext';
import {
  leanzeroLinkNodes,
  leanzeroLinkStatus,
  linkErrorText,
  type LinkState,
  type NodesResponse,
} from '../../acp/leanzero-link';
import {
  mlxEngineDownload,
  mlxEngineDownloadCancel,
  mlxEngineDownloadPause,
  mlxEngineDownloadProgress,
  mlxEngineDownloadResume,
  mlxEngineModelDelete,
  mlxEngineModelsList,
  mlxEngineStatus,
  type MlxDownloadProgress,
  type MlxEngineStatus,
  type MlxLocalModel,
} from '../../acp/mlx-engine';
import {
  mlxEngineReplicaCancel,
  mlxEngineReplicaProgress,
  mlxEngineReplicaTargets,
  mlxEngineReplicate,
  type ReplicaLink,
  type ReplicaLinkKind,
  type ReplicaProgress,
  type ReplicaTargets,
} from '../../acp/mlx-replica';
import { mlxErrorMessage } from './mlxErrorMessage';
import { liveDecodeTps, mlxActivity, readMlxLiveStatus, type MlxActivity } from './mlxLiveStats';
import { touchLocalNetwork } from './LocalNetworkNotice';
import {
  SELF_KEY,
  macTarget,
  macsFrom,
  peerRefuses,
  refusedBy,
  type Mac,
  type Permission,
} from './macs';

/**
 * EVERY linked Mac's facts, read once for the whole Providers view: the Link roster (names,
 * switches), each Mac's engine status and models folder, the downloads and copies in flight. My
 * Macs, the Models table, Run it and the Engine tab all read THIS — so the same Mac never says two
 * things in two places. A peer whose owner turned "Load and download models" off is not asked at
 * all: its own roster entry already says why, and the surfaces say it in words.
 */

const i18n = defineMessages({
  thisMac: { id: 'macs.thisMac', defaultMessage: 'This Mac' },
  offThere: {
    id: 'macs.offThere',
    defaultMessage:
      '{what} is off on {name} — turn on “Let my other Macs use this Mac” there (Providers › My Macs)',
  },
  permManage: { id: 'macs.permission.manage', defaultMessage: 'Load and download models' },
  permChat: { id: 'macs.permission.chat', defaultMessage: 'Answer chat' },
  permSplit: { id: 'macs.permission.split', defaultMessage: 'Run part of a split model' },
  listFailed: { id: 'macs.listFailed', defaultMessage: 'The models folder could not be read.' },
  statusFailed: { id: 'macs.statusFailed', defaultMessage: 'The engine could not be read.' },
  linksFailed: { id: 'macs.linksFailed', defaultMessage: 'The links could not be read.' },
  copyFailed: { id: 'macs.copyFailed', defaultMessage: 'The copy did not start.' },
  cancelFailed: { id: 'macs.cancelFailed', defaultMessage: 'The copy could not be cancelled.' },
  downloadFailed: { id: 'macs.downloadFailed', defaultMessage: 'The download did not start.' },
  pauseFailed: { id: 'macs.pauseFailed', defaultMessage: 'Pause failed.' },
  resumeFailed: { id: 'macs.resumeFailed', defaultMessage: 'Resume failed.' },
  cancelDownloadFailed: { id: 'macs.cancelDownloadFailed', defaultMessage: 'Cancel failed.' },
  trackingFailed: {
    id: 'macs.trackingFailed',
    defaultMessage: 'The downloads in flight could not be reconnected.',
  },
});

export const PERMISSION_LABEL = {
  manage: i18n.permManage,
  chat: i18n.permChat,
  split: i18n.permSplit,
} as const;

export interface MacFacts {
  status: MlxEngineStatus | null;
  statusError: string | null;
  models: MlxLocalModel[] | null;
  modelsError: string | null;
  disk: { availableBytes: number; totalBytes: number } | null;
  /** This Mac only, while it runs: what its requests are doing and the live decode rate. */
  activity: MlxActivity | null;
  decodeTps: number | null;
}

const NO_FACTS: MacFacts = {
  status: null,
  statusError: null,
  models: null,
  modelsError: null,
  disk: null,
  activity: null,
  decodeTps: null,
};

/** One copy of a model from one Mac to another; the RECEIVER owns its progress. */
export interface CopyJob {
  key: string;
  modelId: string;
  fromKey: string;
  toKey: string;
  linkKind: ReplicaLinkKind;
  /** null until the receiver's first answer. */
  progress: ReplicaProgress | null;
  /** The start or cancel failed — goose's words. */
  error: string | null;
}

export function copyKey(modelId: string, toKey: string): string {
  return `${modelId}|${toKey}`;
}

export function copyRunning(job: CopyJob): boolean {
  if (job.error != null) return false;
  return (
    job.progress == null || job.progress.state === 'queued' || job.progress.state === 'copying'
  );
}

export interface MacsValue {
  /** Link reads, when goose offers Link at all. */
  linkState: LinkState | null;
  linkError: string | null;
  refreshLink: () => Promise<void>;
  macs: Mac[];
  self: Mac;
  macByKey: (key: string) => Mac | null;
  facts: Record<string, MacFacts>;
  factsOf: (key: string) => MacFacts;
  refreshStatus: (key: string) => Promise<void>;
  refreshModels: (key: string) => Promise<void>;
  /** A refusal from another Mac in the person's words; anything else verbatim. */
  describeError: (mac: Mac, text: string) => string;
  offText: (mac: Mac, permission: Permission) => string;

  downloads: Record<string, Record<string, MlxDownloadProgress>>;
  downloadErrors: Record<string, Record<string, string>>;
  download: (macKey: string, repoId: string) => Promise<void>;
  pauseDownload: (macKey: string, repoId: string) => Promise<void>;
  resumeDownload: (macKey: string, repoId: string) => Promise<void>;
  cancelDownload: (macKey: string, repoId: string) => Promise<void>;
  deleteModel: (macKey: string, modelId: string) => Promise<void>;

  /** Per SENDING Mac: the direct paths from it to the others. */
  links: Record<string, ReplicaTargets>;
  linksError: Record<string, string>;
  refreshLinks: () => void;
  linkBetween: (fromKey: string, toKey: string) => ReplicaLink | null;
  whyNoLink: (fromKey: string, toKey: string) => string | null;
  copies: Record<string, CopyJob>;
  copy: (modelId: string, fromKey: string, toKey: string) => void;
  cancelCopy: (key: string) => Promise<void>;
  dismissCopy: (key: string) => void;
  /** A copy that finished reaching `toKey` runs `then` once — "copy first, then start". */
  whenCopied: (key: string, then: () => void) => void;
  /** The last measured copy rate (bytes/s) between the two, or null. */
  copyRate: (fromKey: string, toKey: string) => number | null;
}

const MacsContext = createContext<MacsValue | null>(null);

export function useMacs(): MacsValue {
  const value = useContext(MacsContext);
  if (!value) throw new Error('useMacs is read inside <MacsProvider>');
  return value;
}

/** Renders `children` inside the view's MacsProvider, or inside its own when none is above. */
export function WithMacs({ children }: { children: ReactNode }) {
  const outer = useContext(MacsContext);
  return outer ? <>{children}</> : <MacsProvider>{children}</MacsProvider>;
}

export const LINK_POLL_MS = 5000;
export const STATUS_POLL_MS = 5000;
export const MODELS_POLL_MS = 20000;
const DOWNLOAD_POLL_MS = 1000;
const COPY_POLL_MS = 1000;
const COPIES_STORE = 'mlx-copies';

/** The downloads this window started on a Mac, so a return to the view reconnects them. */
function trackingKey(macKey: string): string {
  return `mlx-downloads:${macKey === SELF_KEY ? 'local' : macKey}`;
}

export function readTracked(macKey: string): Set<string> {
  const stored = sessionStorage.getItem(trackingKey(macKey));
  if (!stored) return new Set();
  const ids: unknown = JSON.parse(stored);
  if (!Array.isArray(ids) || !ids.every((id) => typeof id === 'string')) {
    throw new Error('Saved download tracking is invalid.');
  }
  return new Set(ids);
}

function writeTracked(macKey: string, ids: Set<string>): void {
  sessionStorage.setItem(trackingKey(macKey), JSON.stringify([...ids]));
}

function rateKey(fromKey: string, toKey: string): string {
  return `mlx-copy-rate:${fromKey}>${toKey}`;
}

function useVisibleInterval(fn: () => void, ms: number, enabled: boolean): void {
  const ref = useRef(fn);
  useEffect(() => {
    ref.current = fn;
  }, [fn]);
  useEffect(() => {
    if (!enabled) return undefined;
    let timer: ReturnType<typeof setInterval> | null = null;
    const start = () => {
      if (timer != null) return;
      ref.current();
      timer = setInterval(() => ref.current(), ms);
    };
    const stop = () => {
      if (timer != null) clearInterval(timer);
      timer = null;
    };
    const onVisibility = () => (document.visibilityState === 'visible' ? start() : stop());
    onVisibility();
    document.addEventListener('visibilitychange', onVisibility);
    return () => {
      stop();
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, [ms, enabled]);
}

export function MacsProvider({ children }: { children: ReactNode }) {
  const intl = useIntl();
  const { leanzeroLink } = useFeatures();
  const [linkState, setLinkState] = useState<LinkState | null>(null);
  const [linkError, setLinkError] = useState<string | null>(null);
  const [nodes, setNodes] = useState<NodesResponse | null>(null);
  const [facts, setFacts] = useState<Record<string, MacFacts>>({});
  const [downloads, setDownloads] = useState<Record<string, Record<string, MlxDownloadProgress>>>(
    {}
  );
  const [downloadErrors, setDownloadErrors] = useState<Record<string, Record<string, string>>>({});
  const [links, setLinks] = useState<Record<string, ReplicaTargets>>({});
  const [linksError, setLinksError] = useState<Record<string, string>>({});
  const [copies, setCopies] = useState<Record<string, CopyJob>>({});
  const afterCopy = useRef<Map<string, () => void>>(new Map());
  const disposed = useRef(false);
  useEffect(() => {
    disposed.current = false;
    return () => {
      disposed.current = true;
    };
  }, []);

  const thisMac = intl.formatMessage(i18n.thisMac);
  const macs = useMemo(() => macsFrom(nodes, thisMac), [nodes, thisMac]);
  const macsRef = useRef(macs);
  useEffect(() => {
    macsRef.current = macs;
  }, [macs]);
  const self = macs[0];
  const macByKey = useCallback(
    (key: string) => macsRef.current.find((m) => m.key === key) ?? null,
    []
  );

  const offText = useCallback(
    (mac: Mac, permission: Permission) =>
      intl.formatMessage(i18n.offThere, {
        what: intl.formatMessage(PERMISSION_LABEL[permission]),
        name: mac.name,
      }),
    [intl]
  );
  const describeError = useCallback(
    (mac: Mac, text: string) => {
      const permission = mac.isSelf ? null : refusedBy(text);
      return permission ? offText(mac, permission) : text;
    },
    [offText]
  );

  // ---- the roster -------------------------------------------------------------------------
  const refreshLink = useCallback(async () => {
    if (!leanzeroLink) return;
    try {
      const state = await leanzeroLinkStatus();
      if (disposed.current) return;
      setLinkState(state);
      setLinkError(null);
    } catch (e) {
      if (!disposed.current) setLinkError(linkErrorText(e));
      return;
    }
    try {
      const next = await leanzeroLinkNodes();
      if (!disposed.current) setNodes(next);
    } catch {
      // A failed roster read keeps the last roster: a blip must not make a Mac vanish.
    }
  }, [leanzeroLink]);
  useVisibleInterval(() => void refreshLink(), LINK_POLL_MS, leanzeroLink);

  const patchFacts = useCallback((key: string, patch: Partial<MacFacts>) => {
    setFacts((prev) => ({ ...prev, [key]: { ...(prev[key] ?? NO_FACTS), ...patch } }));
  }, []);

  // ---- per-Mac engine status and models ---------------------------------------------------
  const refreshStatus = useCallback(
    async (key: string) => {
      const mac = macByKey(key);
      if (!mac || !mac.online || peerRefuses(mac, 'manage')) return;
      let status: MlxEngineStatus;
      try {
        status = await mlxEngineStatus(macTarget(mac));
      } catch (e) {
        if (!disposed.current)
          patchFacts(key, {
            statusError: mlxErrorMessage(e, intl.formatMessage(i18n.statusFailed)),
          });
        return;
      }
      if (disposed.current) return;
      patchFacts(key, { status, statusError: null });
      if (!mac.isSelf) return;
      if (status.state !== 'running' || !status.baseUrl) {
        patchFacts(key, { activity: null, decodeTps: null });
        return;
      }
      const read = await readMlxLiveStatus(status.baseUrl);
      if (disposed.current) return;
      patchFacts(
        key,
        read.ok
          ? { activity: mlxActivity(read.stats), decodeTps: liveDecodeTps(read.stats) }
          : { activity: null, decodeTps: null }
      );
    },
    [intl, macByKey, patchFacts]
  );

  const refreshModels = useCallback(
    async (key: string) => {
      const mac = macByKey(key);
      if (!mac || !mac.online || peerRefuses(mac, 'manage')) return;
      try {
        const list = await mlxEngineModelsList(macTarget(mac));
        if (disposed.current) return;
        patchFacts(key, {
          models: list.models,
          modelsError: null,
          disk: { availableBytes: list.diskAvailableBytes, totalBytes: list.diskTotalBytes },
        });
      } catch (e) {
        if (!disposed.current)
          patchFacts(key, {
            modelsError: mlxErrorMessage(e, intl.formatMessage(i18n.listFailed)),
          });
      }
    },
    [intl, macByKey, patchFacts]
  );

  const macKeys = macs
    .filter((m) => m.online && !peerRefuses(m, 'manage'))
    .map((m) => m.key)
    .join(',');
  useVisibleInterval(
    () => {
      for (const key of macKeys.split(',')) void refreshStatus(key);
    },
    STATUS_POLL_MS,
    true
  );
  useVisibleInterval(
    () => {
      for (const key of macKeys.split(',')) void refreshModels(key);
    },
    MODELS_POLL_MS,
    true
  );
  // A Mac joining the roster (or turning its switch on) is read at once, not at the next tick.
  useEffect(() => {
    for (const key of macKeys.split(',')) {
      void refreshStatus(key);
      void refreshModels(key);
    }
  }, [macKeys, refreshStatus, refreshModels]);

  // ---- downloads ---------------------------------------------------------------------------
  const setDownload = useCallback(
    (macKey: string, repoId: string, progress: MlxDownloadProgress | null) => {
      setDownloads((prev) => {
        const mine = { ...(prev[macKey] ?? {}) };
        if (progress) mine[repoId] = progress;
        else delete mine[repoId];
        return { ...prev, [macKey]: mine };
      });
    },
    []
  );
  const setDownloadError = useCallback((macKey: string, repoId: string, message: string | null) => {
    setDownloadErrors((prev) => {
      const mine = { ...(prev[macKey] ?? {}) };
      if (message == null && !(repoId in mine)) return prev;
      if (message == null) delete mine[repoId];
      else mine[repoId] = message;
      return { ...prev, [macKey]: mine };
    });
  }, []);

  const syncProgress = useCallback(
    async (macKey: string, repoId: string, opts: { dropIfUntracked?: boolean } = {}) => {
      const mac = macByKey(macKey);
      if (!mac) return;
      let progress: MlxDownloadProgress | null;
      try {
        progress = await mlxEngineDownloadProgress(repoId, macTarget(mac));
      } catch {
        return; // a transient poll failure keeps the last real numbers
      }
      if (disposed.current) return;
      if (!progress) {
        if (opts.dropIfUntracked) setDownload(macKey, repoId, null);
        return;
      }
      if (progress.state === 'cancelled') {
        setDownload(macKey, repoId, null);
        void refreshModels(macKey);
        return;
      }
      setDownload(macKey, repoId, progress);
      if (progress.state === 'done') void refreshModels(macKey);
    },
    [macByKey, refreshModels, setDownload]
  );

  // Reconnect the downloads this window started, per Mac, when the Mac appears.
  const reconnected = useRef<Set<string>>(new Set());
  useEffect(() => {
    for (const mac of macs) {
      if (reconnected.current.has(mac.key)) continue;
      reconnected.current.add(mac.key);
      let ids: Set<string>;
      try {
        ids = readTracked(mac.key);
      } catch (e) {
        setDownloadError(mac.key, '', mlxErrorMessage(e, intl.formatMessage(i18n.trackingFailed)));
        continue;
      }
      for (const id of ids) void syncProgress(mac.key, id);
    }
  }, [macs, intl, setDownloadError, syncProgress]);

  const activeDownloads = useMemo(
    () =>
      Object.entries(downloads)
        .flatMap(([macKey, byRepo]) =>
          Object.entries(byRepo)
            .filter(([, p]) => p.state === 'queued' || p.state === 'downloading')
            .map(([repo]) => `${macKey}\t${repo}`)
        )
        .sort()
        .join('\n'),
    [downloads]
  );
  useEffect(() => {
    if (activeDownloads === '') return undefined;
    const pairs = activeDownloads.split('\n').map((l) => l.split('\t') as [string, string]);
    const timer = setInterval(() => {
      for (const [macKey, repo] of pairs) void syncProgress(macKey, repo);
    }, DOWNLOAD_POLL_MS);
    return () => clearInterval(timer);
  }, [activeDownloads, syncProgress]);

  const track = useCallback((macKey: string, repoId: string) => {
    let ids: Set<string>;
    try {
      ids = readTracked(macKey);
    } catch {
      ids = new Set();
    }
    ids.add(repoId);
    writeTracked(macKey, ids);
  }, []);

  const download = useCallback(
    async (macKey: string, repoId: string) => {
      const mac = macByKey(macKey);
      if (!mac) return;
      track(macKey, repoId);
      setDownloadError(macKey, repoId, null);
      setDownload(macKey, repoId, { state: 'queued', totalBytes: 0, downloadedBytes: 0 });
      try {
        await mlxEngineDownload(repoId, macTarget(mac));
      } catch (e) {
        setDownload(macKey, repoId, null);
        setDownloadError(
          macKey,
          repoId,
          describeError(mac, mlxErrorMessage(e, intl.formatMessage(i18n.downloadFailed)))
        );
      }
    },
    [describeError, intl, macByKey, setDownload, setDownloadError, track]
  );

  const pauseDownload = useCallback(
    async (macKey: string, repoId: string) => {
      const mac = macByKey(macKey);
      if (!mac) return;
      try {
        await mlxEngineDownloadPause(repoId, macTarget(mac));
      } catch (e) {
        setDownloadError(macKey, repoId, mlxErrorMessage(e, intl.formatMessage(i18n.pauseFailed)));
      }
      await syncProgress(macKey, repoId);
    },
    [intl, macByKey, setDownloadError, syncProgress]
  );

  const resumeDownload = useCallback(
    async (macKey: string, repoId: string) => {
      const mac = macByKey(macKey);
      if (!mac) return;
      track(macKey, repoId);
      setDownloadError(macKey, repoId, null);
      setDownloads((prev) =>
        prev[macKey]?.[repoId] != null
          ? prev
          : {
              ...prev,
              [macKey]: {
                ...(prev[macKey] ?? {}),
                [repoId]: { state: 'queued', totalBytes: 0, downloadedBytes: 0 },
              },
            }
      );
      try {
        await mlxEngineDownloadResume(repoId, macTarget(mac));
      } catch (e) {
        setDownloadError(macKey, repoId, mlxErrorMessage(e, intl.formatMessage(i18n.resumeFailed)));
      }
      await syncProgress(macKey, repoId, { dropIfUntracked: true });
    },
    [intl, macByKey, setDownloadError, syncProgress, track]
  );

  const cancelDownload = useCallback(
    async (macKey: string, repoId: string) => {
      const mac = macByKey(macKey);
      if (!mac) return;
      try {
        await mlxEngineDownloadCancel(repoId, macTarget(mac));
      } catch (e) {
        setDownloadError(
          macKey,
          repoId,
          mlxErrorMessage(e, intl.formatMessage(i18n.cancelDownloadFailed))
        );
        return;
      }
      await syncProgress(macKey, repoId, { dropIfUntracked: true });
    },
    [intl, macByKey, setDownloadError, syncProgress]
  );

  const deleteModel = useCallback(
    async (macKey: string, modelId: string) => {
      const mac = macByKey(macKey);
      if (!mac) return;
      await mlxEngineModelDelete(modelId, macTarget(mac));
      // A deleted model's finished download row would keep saying "done".
      setDownload(macKey, modelId, null);
      setDownloadError(macKey, modelId, null);
      await refreshModels(macKey);
    },
    [macByKey, refreshModels, setDownload, setDownloadError]
  );

  // ---- copies between Macs -----------------------------------------------------------------
  const senders = macs
    .filter((m) => m.online && !peerRefuses(m, 'manage'))
    .map((m) => m.key)
    .join(',');
  const multiMac = macs.length > 1;
  const linksSeq = useRef(0);
  const refreshLinks = useCallback(() => {
    if (!multiMac) return;
    const seq = ++linksSeq.current;
    for (const key of senders.split(',')) {
      const mac = macByKey(key);
      if (!mac) continue;
      void (async () => {
        try {
          const next = await mlxEngineReplicaTargets(macTarget(mac));
          if (disposed.current || seq !== linksSeq.current) return;
          setLinks((prev) => ({ ...prev, [key]: next }));
          setLinksError((prev) => {
            if (!(key in prev)) return prev;
            const rest = { ...prev };
            delete rest[key];
            return rest;
          });
        } catch (e) {
          if (disposed.current || seq !== linksSeq.current) return;
          setLinksError((prev) => ({
            ...prev,
            [key]: describeError(mac, mlxErrorMessage(e, intl.formatMessage(i18n.linksFailed))),
          }));
        }
      })();
    }
  }, [describeError, intl, macByKey, multiMac, senders]);
  useEffect(() => {
    refreshLinks();
  }, [refreshLinks]);

  const linkBetween = useCallback(
    (fromKey: string, toKey: string): ReplicaLink | null => {
      const to = macByKey(toKey);
      if (!to?.nodeId) return null;
      return links[fromKey]?.targets.find((t) => t.nodeId === to.nodeId)?.link ?? null;
    },
    [links, macByKey]
  );
  const whyNoLink = useCallback(
    (fromKey: string, toKey: string): string | null => {
      const to = macByKey(toKey);
      if (linksError[fromKey]) return linksError[fromKey];
      if (!to?.nodeId) return null;
      return links[fromKey]?.targets.find((t) => t.nodeId === to.nodeId)?.unavailable ?? null;
    },
    [links, linksError, macByKey]
  );

  const persistCopies = useCallback((next: Record<string, CopyJob>) => {
    const running = Object.values(next)
      .filter(copyRunning)
      .map(({ modelId, fromKey, toKey, linkKind }) => ({ modelId, fromKey, toKey, linkKind }));
    sessionStorage.setItem(COPIES_STORE, JSON.stringify(running));
  }, []);
  useEffect(() => persistCopies(copies), [copies, persistCopies]);
  // A copy started before the view was left is followed again.
  useEffect(() => {
    const stored = sessionStorage.getItem(COPIES_STORE);
    if (!stored) return;
    let parsed: unknown;
    try {
      parsed = JSON.parse(stored);
    } catch {
      return;
    }
    if (!Array.isArray(parsed)) return;
    const restored: Record<string, CopyJob> = {};
    for (const raw of parsed) {
      const j = raw as Partial<CopyJob>;
      if (!j.modelId || !j.fromKey || !j.toKey) continue;
      const key = copyKey(j.modelId, j.toKey);
      restored[key] = {
        key,
        modelId: j.modelId,
        fromKey: j.fromKey,
        toKey: j.toKey,
        linkKind: j.linkKind ?? 'thunderbolt',
        progress: null,
        error: null,
      };
    }
    if (Object.keys(restored).length > 0) setCopies((prev) => ({ ...restored, ...prev }));
  }, []);

  const setCopy = useCallback((key: string, apply: (job: CopyJob) => CopyJob) => {
    setCopies((prev) => (prev[key] ? { ...prev, [key]: apply(prev[key]) } : prev));
  }, []);

  const copy = useCallback(
    (modelId: string, fromKey: string, toKey: string) => {
      const from = macByKey(fromKey);
      const to = macByKey(toKey);
      const link = linkBetween(fromKey, toKey);
      if (!from || !to?.nodeId || !link) return;
      const key = copyKey(modelId, toKey);
      setCopies((prev) => ({
        ...prev,
        [key]: { key, modelId, fromKey, toKey, linkKind: link.kind, progress: null, error: null },
      }));
      const receiver = to.nodeId;
      void (async () => {
        try {
          await touchLocalNetwork();
          await mlxEngineReplicate(modelId, receiver, macTarget(from));
        } catch (e) {
          setCopy(key, (job) => ({
            ...job,
            error: describeError(from, mlxErrorMessage(e, intl.formatMessage(i18n.copyFailed))),
          }));
        }
      })();
    },
    [describeError, intl, linkBetween, macByKey, setCopy]
  );

  const cancelCopy = useCallback(
    async (key: string) => {
      const job = copies[key];
      const to = job ? macByKey(job.toKey) : null;
      if (!job || !to?.nodeId) return;
      afterCopy.current.delete(key);
      try {
        await mlxEngineReplicaCancel(job.modelId, to.nodeId);
      } catch (e) {
        setCopy(key, (j) => ({
          ...j,
          error: mlxErrorMessage(e, intl.formatMessage(i18n.cancelFailed)),
        }));
      }
    },
    [copies, intl, macByKey, setCopy]
  );

  const dismissCopy = useCallback((key: string) => {
    afterCopy.current.delete(key);
    setCopies((prev) => {
      const next = { ...prev };
      delete next[key];
      return next;
    });
  }, []);

  const whenCopied = useCallback((key: string, then: () => void) => {
    afterCopy.current.set(key, then);
  }, []);

  const copyRate = useCallback((fromKey: string, toKey: string): number | null => {
    const stored = Number(localStorage.getItem(rateKey(fromKey, toKey)));
    return Number.isFinite(stored) && stored > 0 ? stored : null;
  }, []);

  const runningCopies = useMemo(
    () =>
      Object.values(copies)
        .filter(copyRunning)
        .map((j) => j.key)
        .sort()
        .join('\n'),
    [copies]
  );
  const copiesRef = useRef(copies);
  useEffect(() => {
    copiesRef.current = copies;
  }, [copies]);
  useEffect(() => {
    if (runningCopies === '') return undefined;
    const keys = runningCopies.split('\n');
    const timer = setInterval(() => {
      for (const key of keys) {
        const job = copiesRef.current[key];
        const to = job ? macByKey(job.toKey) : null;
        if (!job || !to?.nodeId) continue;
        const receiver = to.nodeId;
        void (async () => {
          let progress: ReplicaProgress | null;
          try {
            progress = await mlxEngineReplicaProgress(job.modelId, receiver);
          } catch {
            return; // a transient poll failure keeps the last real numbers
          }
          if (!progress || disposed.current) return;
          const done = progress;
          setCopy(key, (j) => ({ ...j, progress: done }));
          if (done.state === 'done') {
            if (done.wireMillis > 0) {
              const rate = (done.wireBytes / done.wireMillis) * 1000;
              localStorage.setItem(rateKey(job.fromKey, job.toKey), String(rate));
            }
            void refreshModels(job.toKey);
            const then = afterCopy.current.get(key);
            afterCopy.current.delete(key);
            then?.();
          }
          if (done.state === 'failed' || done.state === 'cancelled') {
            afterCopy.current.delete(key);
          }
        })();
      }
    }, COPY_POLL_MS);
    return () => clearInterval(timer);
  }, [runningCopies, macByKey, refreshModels, setCopy]);

  const factsOf = useCallback((key: string) => facts[key] ?? NO_FACTS, [facts]);

  const value = useMemo<MacsValue>(
    () => ({
      linkState,
      linkError,
      refreshLink,
      macs,
      self,
      macByKey,
      facts,
      factsOf,
      refreshStatus,
      refreshModels,
      describeError,
      offText,
      downloads,
      downloadErrors,
      download,
      pauseDownload,
      resumeDownload,
      cancelDownload,
      deleteModel,
      links,
      linksError,
      refreshLinks,
      linkBetween,
      whyNoLink,
      copies,
      copy,
      cancelCopy,
      dismissCopy,
      whenCopied,
      copyRate,
    }),
    [
      linkState,
      linkError,
      refreshLink,
      macs,
      self,
      macByKey,
      facts,
      factsOf,
      refreshStatus,
      refreshModels,
      describeError,
      offText,
      downloads,
      downloadErrors,
      download,
      pauseDownload,
      resumeDownload,
      cancelDownload,
      deleteModel,
      links,
      linksError,
      refreshLinks,
      linkBetween,
      whyNoLink,
      copies,
      copy,
      cancelCopy,
      dismissCopy,
      whenCopied,
      copyRate,
    ]
  );

  return <MacsContext.Provider value={value}>{children}</MacsContext.Provider>;
}
