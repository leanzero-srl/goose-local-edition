import React, {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from 'react';
import {
  Download,
  Folder,
  HardDrive,
  Loader2,
  Network,
  Minus,
  Pencil,
  Plus,
  RefreshCw,
  Search,
  Square,
  X,
} from 'lucide-react';
import { ConfirmationModal } from '../ui/ConfirmationModal';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '../ui/dialog';
import {
  Button,
  Chip,
  DataTable,
  Disclosure,
  EmptyState,
  KeyValue,
  Panel,
  Segmented,
  StatusDot,
  Toolbar,
  FOCUS,
  MOTION,
  RADIUS,
  SURFACE,
  TNUM,
  TONE_DOT,
  TONE_TEXT,
  TYPE,
  WEIGHT,
  cx,
  type DataTableColumn,
  type KeyValueItem,
  type Tone,
} from '../lz';
import { mlxErrorMessage } from './mlxErrorMessage';
import { MlxThinkingFields } from './MlxThinkingFields';
import { MlxKvCacheFields } from './MlxKvCacheFields';
import {
  mlxEngineBrowse,
  mlxEngineBrowseFilters,
  mlxEngineMount,
  mlxEngineSettingsRead,
  mlxEngineSettingsUpdate,
  mlxEngineStatus,
  mlxEngineUnmount,
  type MlxBrowseFilters,
  type MlxBrowseHit,
  type MlxBrowseSort,
  type MlxDownloadProgress,
  type MlxEngineSettings,
  type MlxEngineState,
  type MlxEngineStatus,
  type MlxLocalModel,
  type MlxModelProfile,
} from '../../acp/mlx-engine';
import {
  DownloadProgressRow,
  formatBytesShort,
  formatCount,
  formatDate,
  formatGb,
} from './primitives';
import { FilterCombobox } from './FilterCombobox';
import { INPUT, StudioSelect, StudioSwitch, ToneBanner, type StudioSelectOption } from './studio';
import { ModelCardModal } from './ModelCardModal';
import { ModelMatrix, matrixRowCount } from './ModelMatrix';
import { SELF_KEY, macTarget, peerRefuses, routePeerName, type Mac } from './macs';
import { WithMacs, useMacs } from './useMacs';
import { MlxStateTile, servingEngine } from './MlxStateTile';
import { MlxRestoreBanner } from './MlxRestoreLine';
import { settleRestoreLine } from './mlxRestore';
import type { MlxServing } from '../../utils/mlxServing';
import {
  EMPTY_BOOK,
  advanceRateBook,
  advanceMountWatch,
  liveDecodeTps,
  mountCostOf,
  mountFill,
  MLX_STATUS_POLL_MS,
  singleLoad,
  pushSample,
  readMlxLiveStatus,
  readMlxServing,
  type MlxLiveStats,
  type RateBook,
  type MlxLiveRead,
  type MountWatch,
  type SingleLoad,
  type TpsSample,
} from './mlxLiveStats';
import { useFeatures } from '../../contexts/FeaturesContext';
import { defineMessages, useIntl } from '../../i18n';
import type { MlxDistributedStatus } from '../../acp/mlx-distributed';
import { DistributedEngineSection } from './DistributedEngineSection';
import { modeSummary, ownsTheMac } from './mlxDistributed';
import { formatMlxMode, formatRemoteMode } from './mlxModeLabel';
import {
  latestMlxRemoteSingleStatus,
  mlxRemoteSingleStop,
  remoteRouteUp,
  subscribeMlxRemoteSingleStatus,
  type MlxRemoteSingleStatus,
} from '../../acp/mlx-remote-single';
import { useMlxDistributedStatus } from './useMlxDistributedStatus';
import { PlacementBadge, PlacementCard, badgesOf, usePlacementPlans } from './PlacementCard';
import type { PlacementBadge as PlacementBadgeDto } from '../../acp/mlx-placement';

// Formatters stay importable from this module — tests and older callers reach them here.
export { formatBytesShort, formatCount, formatDate, formatGb } from './primitives';

const i18n = defineMessages({
  distributedOwns: { id: 'mlxEngineView.distributedOwns', defaultMessage: 'Distributed' },
  distributedOwnsText: {
    id: 'mlxEngineView.distributedOwnsText',
    defaultMessage:
      'The distributed engine owns this Mac: the single engine cannot mount until it is stopped in the Distributed engine section below.',
  },
  servingDistributed: {
    id: 'mlxEngineView.servingDistributed',
    defaultMessage: 'Serving across Macs',
  },
  servingRemote: { id: 'mlxEngineView.servingRemote', defaultMessage: 'Serving on {peer}' },
  stopRemote: { id: 'mlxEngineView.stopRemote', defaultMessage: 'Stop' },
  stopRemoteFailed: {
    id: 'mlxEngineView.stopRemoteFailed',
    defaultMessage: 'Could not stop serving from the other Mac.',
  },
  mountBlocked: { id: 'mlxEngineView.mountBlocked', defaultMessage: 'Mount blocked' },
  mountFailed: { id: 'mlxEngineView.mountFailed', defaultMessage: 'Mount failed' },
  pickThenRun: {
    id: 'mlxEngineView.pickThenRun',
    defaultMessage:
      'Pick a model, then start it in Run it below — on this Mac, on another of your Macs, or split across them.',
  },
});

/**
 * The mount failure banners, ONE per failure. The sidecar's refusal of a mount the memory gate
 * blocks arrives TWICE — the mount call rejects with "memory gate BLOCK for '<model>': <message>"
 * and the status keeps that gate's `<message>` as `gateMessage` — so a mount error that carries the
 * blocking gate's message is the same failure and renders once, as the gate's banner.
 */
export function mountFailureBanners(
  status: Pick<MlxEngineStatus, 'gateVerdict' | 'gateMessage'> | null,
  mountError: string | null
): { gateBlock: string | null; mountError: string | null } {
  const gateBlock =
    status?.gateVerdict === 'block' && status.gateMessage ? status.gateMessage : null;
  const same = mountError != null && gateBlock != null && mountError.includes(gateBlock);
  return { gateBlock, mountError: same ? null : mountError };
}

/** A quiet table cell: the meta register in tabular figures. */
const META = cx(TYPE.meta, TNUM);

/** The "—" of an absent value: ink-4, never information. */
function Absent() {
  return <span className="text-lz-ink-4">—</span>;
}

// ---------------------------------------------------------------------------
// Per-model sampling profiles: text drafts, where '' means "engine default".
// A cleared field OMITS the key from the profile; an explicit 0 sends 0.
// The two are different facts and the payload must keep them apart.
// ---------------------------------------------------------------------------

export type NumericSettingKey =
  | 'temperature'
  | 'topP'
  | 'topK'
  | 'minP'
  | 'repetitionPenalty'
  | 'presencePenalty'
  | 'frequencyPenalty'
  | 'contextLimit';

export interface NumericFieldSpec {
  key: NumericSettingKey;
  label: string;
  step: number;
  integer?: boolean;
}

export const SAMPLING_FIELDS: NumericFieldSpec[] = [
  { key: 'temperature', label: 'Temperature', step: 0.05 },
  { key: 'topP', label: 'Top P', step: 0.05 },
  { key: 'topK', label: 'Top K', step: 1, integer: true },
  { key: 'minP', label: 'Min P', step: 0.01 },
  { key: 'repetitionPenalty', label: 'Repetition penalty', step: 0.05 },
  { key: 'presencePenalty', label: 'Presence penalty', step: 0.1 },
  { key: 'frequencyPenalty', label: 'Frequency penalty', step: 0.1 },
];

export const CONTEXT_LIMIT_FIELD: NumericFieldSpec = {
  key: 'contextLimit',
  label: 'Context limit (tokens)',
  step: 1024,
  integer: true,
};

/**
 * The serving-lane overrides, drafted as text like the numbers so one drafts map carries the
 * whole profile: `speculative` is '' (auto) | 'mtp' | 'off'; `adapterPath` is the directory or
 * ''; `textOnly` is '' (auto = text lane) | 'true' | 'false'.
 */
export type LaneSettingKey = 'speculative' | 'adapterPath' | 'textOnly';
/** `thinking` is '' (auto) | 'on' | 'off'; `reasoningEffort` is '' (template default) | a level. */
export type ThinkingSettingKey = 'thinking' | 'reasoningEffort';
/** `kvCache` is '' (off — the engine's bf16 cache) | 'int8' | 'int4'. */
export type KvCacheSettingKey = 'kvCache';
export type ProfileDraftKey =
  | NumericSettingKey
  | LaneSettingKey
  | ThinkingSettingKey
  | KvCacheSettingKey;

export type NumericDrafts = Record<ProfileDraftKey, string>;

const NUMERIC_KEYS: NumericSettingKey[] = [...SAMPLING_FIELDS, CONTEXT_LIMIT_FIELD].map(
  (f) => f.key
);
const LANE_KEYS: LaneSettingKey[] = ['speculative', 'adapterPath', 'textOnly'];
const THINKING_KEYS: ThinkingSettingKey[] = ['thinking', 'reasoningEffort'];
const PROFILE_KEYS: ProfileDraftKey[] = [
  ...NUMERIC_KEYS,
  ...LANE_KEYS,
  ...THINKING_KEYS,
  'kvCache',
];

export function draftsFromProfile(profile: MlxModelProfile | undefined): NumericDrafts {
  const drafts = {} as NumericDrafts;
  for (const key of NUMERIC_KEYS) {
    const value = profile?.[key];
    drafts[key] = value == null ? '' : String(value);
  }
  drafts.speculative = profile?.speculative ?? '';
  drafts.adapterPath = profile?.adapterPath ?? '';
  drafts.textOnly = profile?.textOnly == null ? '' : String(profile.textOnly);
  drafts.thinking = profile?.thinking ?? '';
  drafts.reasoningEffort = profile?.reasoningEffort ?? '';
  drafts.kvCache = profile?.kvCache ?? '';
  return drafts;
}

/** A blank draft leaves its key ABSENT (engine default); a "0" draft sends the number 0. */
export function profileFromDrafts(drafts: NumericDrafts): MlxModelProfile {
  const profile: MlxModelProfile = {};
  for (const key of NUMERIC_KEYS) {
    const text = drafts[key].trim();
    if (text === '') continue;
    const n = Number(text);
    if (Number.isNaN(n)) continue;
    profile[key] = n;
  }
  const speculative = drafts.speculative.trim();
  if (speculative === 'mtp' || speculative === 'off') profile.speculative = speculative;
  const adapterPath = drafts.adapterPath.trim();
  if (adapterPath !== '') profile.adapterPath = adapterPath;
  if (drafts.textOnly === 'true') profile.textOnly = true;
  else if (drafts.textOnly === 'false') profile.textOnly = false;
  if (drafts.thinking === 'on' || drafts.thinking === 'off') profile.thinking = drafts.thinking;
  const effort = drafts.reasoningEffort.trim();
  if (effort !== '') profile.reasoningEffort = effort;
  if (drafts.kvCache === 'int8' || drafts.kvCache === 'int4') profile.kvCache = drafts.kvCache;
  return profile;
}

export function profileHasValues(profile: MlxModelProfile): boolean {
  return PROFILE_KEYS.some((key) => profile[key] != null);
}

/**
 * The ONLY shape update payloads are built from. The legacy flat sampling fields the
 * backend still echoes (pre-migration) are STRIPPED: the backend migrates any it receives
 * into `modelProfiles[modelId]`, so writing them back would clobber profile edits.
 * `servedModelName` passes through — dropping it would silently un-alias the engine.
 */
export function sanitizeSettingsForWrite(settings: MlxEngineSettings): MlxEngineSettings {
  const next: MlxEngineSettings = {
    modelsDir: settings.modelsDir,
    port: settings.port,
    spawnCommand: settings.spawnCommand,
    modelProfiles: { ...(settings.modelProfiles ?? {}) },
  };
  if (settings.modelId != null) next.modelId = settings.modelId;
  if (settings.servedModelName != null) next.servedModelName = settings.servedModelName;
  return next;
}

/**
 * Full settings payload with ONE model's profile rebuilt from its drafts. Every other
 * profile passes through untouched; an all-blank draft set removes the model's entry
 * entirely (no profile = every field at engine default).
 */
export function settingsWithProfile(
  settings: MlxEngineSettings,
  modelId: string,
  drafts: NumericDrafts
): MlxEngineSettings {
  const next = sanitizeSettingsForWrite(settings);
  const profile = profileFromDrafts(drafts);
  if (profileHasValues(profile)) next.modelProfiles[modelId] = profile;
  else delete next.modelProfiles[modelId];
  return next;
}

export function draftsEqual(a: NumericDrafts, b: NumericDrafts): boolean {
  return PROFILE_KEYS.every((key) => a[key].trim() === b[key].trim());
}

// ---------------------------------------------------------------------------
// Small building blocks on the Studio tokens (banner/progress row live in ./primitives)
// ---------------------------------------------------------------------------

/**
 * The engine's state on the tabs without the tile: a solid dot (pulsing while a mount is in
 * flight) beside a chip, both in the engine-phase palette the tile uses (mlxPhase.ts).
 */
/**
 * The Models and Sampling tabs' badge: what serves this Mac's chat — the route to a linked Mac, the
 * split, or this Mac's engine — in the tile's own words and colour (`servingEngine`, one source).
 */
function StateBadge(props: {
  state: MlxEngineState;
  live: MlxLiveRead | null;
  distributed: MlxDistributedStatus | null;
  remote: MlxRemoteSingleStatus | null;
  load: SingleLoad | null;
}) {
  const intl = useIntl();
  const engine = servingEngine(intl, { ...props, unreachable: false });
  const moving = engine.phase === 'loading';
  return (
    <span
      className="inline-flex items-center gap-2"
      data-testid="mlx-state-badge"
      data-mode={engine.mode}
      data-state={engine.stateKey}
      data-phase={engine.phase}
    >
      <StatusDot phase={engine.phase} live={moving} label={engine.wordText} />
      <Chip phase={engine.phase} icon={moving ? <Loader2 className="animate-spin" /> : undefined}>
        {engine.wordText}
      </Chip>
    </span>
  );
}

/**
 * The restart-required banner is a fact about the whole engine, not one tab: settings were
 * saved but the live process still runs the old ones. It renders on BOTH the Engine and
 * Sampling tabs from the same status + handlers — one truth, two viewports.
 */
function RestartRequiredBanner({
  status,
  settings,
  engineBusy,
  onRemount,
}: {
  status: MlxEngineStatus | null;
  settings: MlxEngineSettings | null;
  engineBusy: boolean;
  onRemount: () => void;
}) {
  if (!status?.restartRequired) return null;
  return (
    <ToneBanner
      tone="warn"
      label="Restart required"
      text="Settings changed — remount to apply."
      action={
        <Button
          size="sm"
          variant="secondary"
          icon={<RefreshCw />}
          onClick={onRemount}
          disabled={engineBusy || !(status.modelId ?? settings?.modelId)}
        >
          Remount
        </Button>
      }
    />
  );
}

/** A solid used-fill on a surface-2 track: the accent, or warn when the headroom is tight. */
function UsageBar({ pct, tight, label }: { pct: number; tight: boolean; label: string }) {
  return (
    <div
      className={cx('h-2 min-w-[120px] flex-1 overflow-hidden', RADIUS.pill, SURFACE.inset)}
      role="progressbar"
      aria-valuenow={Math.round(pct)}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-label={label}
    >
      <div
        className={cx('h-full', tight ? TONE_DOT.warn : TONE_DOT.accent)}
        style={{ width: `${pct}%` }}
      />
    </div>
  );
}

/** Unified memory in use, under the Engine facts; the numbers live in the KeyValue row above. */
function MemoryBar({ availableGb, totalGb }: { availableGb: number; totalGb: number }) {
  const usedGb = Math.max(0, totalGb - availableGb);
  const pct = totalGb > 0 ? Math.min(100, (usedGb / totalGb) * 100) : 0;
  const tight = totalGb > 0 && availableGb / totalGb < 0.15;
  return (
    <div className="flex min-w-0 items-center gap-3">
      <span className={cx('shrink-0', TYPE.meta)}>Unified memory in use</span>
      <UsageBar pct={pct} tight={tight} label="Unified memory in use" />
      <span
        className={cx(
          'shrink-0 text-lz-meta',
          WEIGHT.semibold,
          TNUM,
          tight ? TONE_TEXT.warn : 'text-lz-ink'
        )}
      >
        {Math.round(pct)}%
      </span>
    </div>
  );
}

/**
 * Disk space on the models dir's volume: solid used-fill on a track, "{free} free of {total}"
 * beside it. Numbers come from the modelsList response (statvfs), never fabricated.
 */
function DiskBar({ availableBytes, totalBytes }: { availableBytes: number; totalBytes: number }) {
  const usedBytes = Math.max(0, totalBytes - availableBytes);
  const pct = totalBytes > 0 ? Math.min(100, (usedBytes / totalBytes) * 100) : 0;
  const tight = totalBytes > 0 && availableBytes / totalBytes < 0.1;
  return (
    <div className="flex min-w-0 items-center gap-3" data-testid="mlx-disk-bar">
      <HardDrive className={cx('size-4 shrink-0', tight ? TONE_TEXT.warn : 'text-lz-ink-3')} />
      <UsageBar pct={pct} tight={tight} label="Disk space used on the models volume" />
      <span
        className={cx(
          'shrink-0 text-lz-meta',
          WEIGHT.semibold,
          TNUM,
          tight ? TONE_TEXT.warn : 'text-lz-ink'
        )}
      >
        {formatGb(availableBytes)} free
      </span>
      <span className={cx('shrink-0', TYPE.meta, TNUM)}>of {formatGb(totalBytes)}</span>
    </div>
  );
}

const STEP_BUTTON = cx(
  'flex size-7 shrink-0 items-center justify-center bg-lz-surface text-lz-ink-2 hover:bg-lz-surface-2 hover:text-lz-ink [&_svg]:size-3.5',
  SURFACE.outline,
  RADIUS.control,
  FOCUS,
  MOTION
);

/**
 * One row of the sampling form — label | control — with an honest "engine default" state:
 * a blank field means the key is omitted and the engine's own default applies (said in quiet
 * text beside the field); a typed 0 is sent as 0 and a Clear action takes it back to blank.
 */
function NumericField({
  spec,
  text,
  onText,
}: {
  spec: NumericFieldSpec;
  text: string;
  onText: (v: string) => void;
}) {
  const isSet = text.trim() !== '';
  const bump = (dir: 1 | -1) => {
    const base = isSet && !Number.isNaN(Number(text)) ? Number(text) : 0;
    let next = base + dir * spec.step;
    if (spec.integer) next = Math.round(next);
    // Float steps accumulate representation noise (0.1 + 0.05 = 0.15000000000000002).
    const rounded = spec.integer ? next : Number(next.toFixed(4));
    onText(String(rounded));
  };
  return (
    <div
      className={cx(
        'grid grid-cols-[minmax(160px,240px)_1fr] items-center gap-4 border-t py-2 first:border-t-0',
        SURFACE.hairline
      )}
    >
      <span className={cx('truncate', TYPE.body)}>{spec.label}</span>
      <div className="flex min-w-0 flex-wrap items-center gap-2">
        <button
          type="button"
          className={STEP_BUTTON}
          aria-label={`Decrease ${spec.label}`}
          onClick={() => bump(-1)}
        >
          <Minus />
        </button>
        <input
          type="number"
          step={spec.step}
          value={text}
          onChange={(e) => onText(e.target.value)}
          className={cx(INPUT, 'w-36 text-right', TNUM)}
          aria-label={spec.label}
        />
        <button
          type="button"
          className={STEP_BUTTON}
          aria-label={`Increase ${spec.label}`}
          onClick={() => bump(1)}
        >
          <Plus />
        </button>
        {isSet ? (
          <Button
            size="sm"
            variant="ghost"
            icon={<X />}
            onClick={() => onText('')}
            title="Clear — fall back to the engine default"
          >
            Clear
          </Button>
        ) : (
          <span className={TYPE.meta} title="No value sent — the engine uses its own default">
            engine default
          </span>
        )}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Model pickers — the hub's StudioSelect listbox, never a native <select>.
// ---------------------------------------------------------------------------

interface ModelOption extends StudioSelectOption {
  model: MlxLocalModel;
  badge?: PlacementBadgeDto;
}

function ModelOptionLabel({ option }: { option: ModelOption }) {
  return (
    <span className="flex min-w-0 items-center gap-2">
      <span className="truncate font-mono text-lz-mono">{option.model.id}</span>
      <span className={cx('shrink-0', TYPE.meta, TNUM)}>{formatGb(option.model.sizeBytes)}</span>
      {!option.model.complete && <Chip tone="warn">partial download</Chip>}
      {option.badge && <PlacementBadge badge={option.badge} />}
    </span>
  );
}

/**
 * The mount picker: a Studio listbox where an incomplete model stays visible but cannot be
 * picked; the ghost ✕ beside it clears the selection (the picker has always been clearable).
 */
function ModelPicker({
  models,
  value,
  onChange,
  disabled,
  badges,
}: {
  models: MlxLocalModel[];
  value: string | null;
  onChange: (id: string | null) => void;
  disabled: boolean;
  /** Where each model fits (the placement planner), by model id. */
  badges?: Map<string, PlacementBadgeDto>;
}) {
  const options: ModelOption[] = models.map((model) => ({
    value: model.id,
    label: model.id,
    model,
    badge: badges?.get(model.id),
    disabled: !model.complete,
  }));
  const selected = options.find((o) => o.value === value) ?? null;
  return (
    <div className="flex items-center gap-2">
      <StudioSelect
        className="min-w-0 flex-1"
        aria-label="Model to mount"
        options={options}
        value={selected}
        disabled={disabled}
        placeholder={
          models.length === 0 ? 'No models in the models folder yet' : 'Pick a model to mount'
        }
        renderOption={(o) => <ModelOptionLabel option={o} />}
        onChange={(o) => onChange(o ? o.value : null)}
      />
      {selected && !disabled && (
        <Button
          size="sm"
          variant="ghost"
          icon={<X />}
          onClick={() => onChange(null)}
          aria-label="Clear model selection"
          title="Clear the selection"
        />
      )}
    </div>
  );
}

interface ProfileModelOption extends StudioSelectOption {
  local: boolean;
  hasProfile: boolean;
}

function ProfileModelOptionLabel({ option }: { option: ProfileModelOption }) {
  return (
    <span className="flex min-w-0 items-center gap-2">
      <span className="truncate font-mono text-lz-mono">{option.value}</span>
      {option.hasProfile && <Chip>profile</Chip>}
      {!option.local && (
        <Chip tone="stopped" title="A saved profile for a model that is not in the models folder">
          not downloaded
        </Chip>
      )}
    </span>
  );
}

/**
 * Sampling model picker: every local model plus any model that only exists as a saved
 * profile key — a profile must never become unreachable because its files were deleted.
 */
function SamplingModelPicker({
  models,
  profileIds,
  value,
  onChange,
}: {
  models: MlxLocalModel[];
  profileIds: string[];
  value: string | null;
  onChange: (id: string | null) => void;
}) {
  const localIds = new Set(models.map((m) => m.id));
  const options: ProfileModelOption[] = [
    ...models.map((m) => ({
      value: m.id,
      label: m.id,
      local: true,
      hasProfile: profileIds.includes(m.id),
    })),
    ...profileIds
      .filter((id) => !localIds.has(id))
      .map((id) => ({ value: id, label: id, local: false, hasProfile: true })),
  ];
  const selected = options.find((o) => o.value === value) ?? null;
  return (
    <StudioSelect
      aria-label="Sampling model"
      options={options}
      value={selected}
      placeholder={options.length === 0 ? 'No local models yet' : 'Pick a model to tune'}
      renderOption={(o) => <ProfileModelOptionLabel option={o} />}
      onChange={(o) => onChange(o ? o.value : null)}
    />
  );
}

// ---------------------------------------------------------------------------
// ENGINE tab
// ---------------------------------------------------------------------------

const GATE_TONE: Record<NonNullable<MlxEngineStatus['gateVerdict']>, Tone> = {
  allow: 'ok',
  warn: 'warn',
  block: 'err',
};

interface EngineSectionProps {
  status: MlxEngineStatus | null;
  statusError: string | null;
  settings: MlxEngineSettings | null;
  models: MlxLocalModel[];
  mountModelId: string | null;
  setMountModelId: (id: string | null) => void;
  mountError: string | null;
  engineBusy: boolean;
  onMount: () => void;
  onUnmount: () => void;
  onRemount: () => void;
  /** The tile's live instrument while running — the last Rapid-MLX /v1/status read. */
  live: MlxLiveRead | null;
  tpsHistory: readonly TpsSample[];
  /** Every run the live read caught on this engine — the tile's median and range. */
  rates: RateBook;
  /** Who the engine is serving (main's read of goose's in-flight list). */
  serving: MlxServing | null;
  /** The memory watch across an in-flight mount. */
  mountWatch: MountWatch | null;
  /** The distributed engine (this Mac only); while it owns the Mac the single engine cannot mount. */
  distributed: MlxDistributedStatus | null;
  /** goose offers the split (the `mlxDistributed` capability). */
  distributedCapability: boolean;
  /** The split's own controls, folded under Run it's split row. */
  splitDetails: ReactNode;
  /** Which engine owns this Mac, in words — on the tile. */
  modeLabel: string;
  /** The route serving this Mac's chat from a linked Mac's engine, while it is up. */
  remote: MlxRemoteSingleStatus | null;
  /** Stop that route (and unmount the model there). */
  onStopRemote: () => void;
  /** Why the last Stop of that route did not finish — goose's words. */
  remoteStopError: string | null;
}

function EngineSection(props: EngineSectionProps) {
  const intl = useIntl();
  const {
    status,
    statusError,
    settings,
    models,
    mountModelId,
    setMountModelId,
    mountError,
    engineBusy,
    onMount,
    onUnmount,
    onRemount,
    live,
    tpsHistory,
    rates,
    serving,
    mountWatch,
    distributed,
    distributedCapability,
    splitDetails,
    modeLabel,
    remote,
    onStopRemote,
    remoteStopError,
  } = props;
  const [detailsOpen, setDetailsOpen] = useState(false);
  // Re-planned whenever the model list or what this Mac serves changes: a mount moves every fit.
  const plans = usePlacementPlans(
    [...models.map((m) => m.id), status?.state ?? '', status?.modelId ?? ''].join('\n')
  );
  const badges = useMemo(() => badgesOf(plans), [plans]);
  // goose refuses a single mount while the distributed engine owns the Mac.
  const distributedOwns = ownsTheMac(distributed);
  const banners = mountFailureBanners(status, mountError);

  const state = status?.state ?? null;
  const running = state === 'running';
  const mountedModelId = status?.modelId ?? null;
  const strayPort = state === 'stopped' ? status?.strayListenerPort : undefined;
  // Starting and stopping belong to Run it; the one thing left here is reclaiming an engine a
  // previous goose left listening on the port.
  const offerReclaim = strayPort != null;
  // What Run it is about: the split's model while the split owns this Mac, the model a linked Mac
  // serves this Mac's chat with while that route is up, else the picked one.
  const runModelId = distributedOwns
    ? (distributed?.modelId ?? null)
    : (remote?.modelId ?? mountModelId);
  const failedError =
    state === 'failed' && status?.lastError && status.lastError !== mountError
      ? status.lastError
      : null;

  // Available = free pages plus reclaimable file cache (the sidecar's measure), so a full file
  // cache after a big download no longer reads as memory pressure.
  const memoryTight =
    status != null &&
    status.memoryError == null &&
    status.totalMemoryGb > 0 &&
    status.availableMemoryGb / status.totalMemoryGb < 0.15;

  // Every row is backend truth or an honest "—"; nothing here is fabricated. The state, the served
  // model and the memory headroom live in the hero above — these are the running engine's facts.
  const facts: KeyValueItem[] = [
    {
      key: 'context',
      label: 'Context length',
      value: status?.contextWindow != null ? status.contextWindow.toLocaleString() : <Absent />,
    },
    {
      // Rapid-MLX's own num_running + num_waiting, read by the sidecar on the status probe. An
      // absent count is never a fabricated 0: the engine did not say, and `activeRequestsError`
      // says why — a mute /v1/status is what made the swarm's fleet zone read the node idle
      // while it generated, with the reason printed nowhere.
      key: 'inflight',
      label: 'In flight',
      value:
        status?.state !== 'running' ? (
          <Absent />
        ) : typeof status.activeRequests === 'number' ? (
          <span data-testid="mlx-inflight-count">{status.activeRequests}</span>
        ) : (
          <span data-testid="mlx-inflight-unknown" className="break-words">
            unknown{status.activeRequestsError ? ` — ${status.activeRequestsError}` : ''}
          </span>
        ),
      tone:
        status?.state === 'running' && typeof status.activeRequests !== 'number'
          ? 'err'
          : status?.state === 'running' && (status.activeRequests as number) > 0
            ? 'ok'
            : undefined,
    },
    {
      key: 'gate',
      label: 'Mount gate',
      value: status?.gateVerdict ? (
        <Chip tone={GATE_TONE[status.gateVerdict]} title={status.gateMessage}>
          {status.gateVerdict}
        </Chip>
      ) : (
        <Absent />
      ),
    },
    {
      key: 'parser',
      label: 'Tool-call parser',
      value: status?.toolCallParser ?? <Absent />,
    },
    { key: 'pid', label: 'PID', value: status?.pid ?? <Absent />, mono: status?.pid != null },
    {
      key: 'url',
      label: 'Base URL',
      value: status?.baseUrl ?? <Absent />,
      mono: status?.baseUrl != null,
    },
    {
      key: 'port',
      label: 'Port (configured)',
      value: settings?.port ?? <Absent />,
      mono: settings != null,
    },
  ];

  const details = (
    <div className="flex flex-col gap-4">
      <KeyValue items={facts} aria-label="Engine status" />
      {settings && (
        // Spawn command — visible, not editable here: the owner sees exactly what would run.
        <div className="flex flex-col gap-1.5">
          <span className={TYPE.meta}>Spawn command</span>
          <code className={cx('block break-all', TYPE.mono)}>
            {settings.spawnCommand.join(' ')}
          </code>
        </div>
      )}
    </div>
  );

  // The tile's instrument inputs — each a measured fact or absent, never a stand-in.
  const sizeOf = (id: string | null | undefined) =>
    (id ? models.find((m) => m.id === id)?.sizeBytes : undefined) ?? null;
  const mount =
    state === 'mounting' && status
      ? mountFill(mountWatch, status.availableMemoryGb, sizeOf(mountedModelId))
      : null;
  const fit = status?.mountFit;
  const cost = state === 'stopped' && fit && fit.modelId === mountModelId ? mountCostOf(fit) : null;

  return (
    <div className="flex flex-col gap-4 pb-8">
      <MlxRestoreBanner />
      {statusError && <ToneBanner tone="err" label="Engine unreachable" text={statusError} />}
      {banners.gateBlock && (
        <ToneBanner
          tone="err"
          label={intl.formatMessage(i18n.mountBlocked)}
          text={banners.gateBlock}
          testId="mlx-mount-blocked"
        />
      )}
      {status?.gateMessage && status.gateVerdict === 'warn' && (
        <ToneBanner tone="warn" label="Memory pressure" text={status.gateMessage} />
      )}
      {strayPort != null && (
        <ToneBanner
          tone="warn"
          label="Unsupervised engine"
          text={`unsupervised engine on port ${strayPort} — Unmount reclaims it`}
        />
      )}
      {banners.mountError && (
        <ToneBanner
          tone="err"
          label={intl.formatMessage(i18n.mountFailed)}
          text={banners.mountError}
          testId="mlx-mount-failed"
        />
      )}
      {remoteStopError && (
        <ToneBanner
          tone="err"
          label={intl.formatMessage(i18n.stopRemoteFailed)}
          text={remoteStopError}
          testId="mlx-remote-stop-failed"
        />
      )}
      {distributedOwns && (
        <ToneBanner
          tone="accent"
          label={intl.formatMessage(i18n.distributedOwns)}
          text={intl.formatMessage(i18n.distributedOwnsText)}
          testId="mlx-distributed-owns"
        />
      )}
      <RestartRequiredBanner
        status={status}
        settings={settings}
        engineBusy={engineBusy}
        onRemount={onRemount}
      />

      {/* The status hero: the state tile (a live instrument for the state), what is served, the
          headroom a mount has, and the picker. Side by side from lg; stacked below it, where the
          instrument would crush the picker. */}
      <section
        aria-label="Engine"
        data-testid="mlx-engine-hero"
        className={cx('flex flex-col gap-4 p-4 lg:flex-row', SURFACE.card)}
      >
        <MlxStateTile
          state={state}
          unreachable={statusError != null && status == null}
          live={live}
          history={tpsHistory}
          rates={rates}
          serving={serving}
          mount={mount}
          load={singleLoad(status)}
          cost={cost}
          failedError={failedError}
          action={
            remote ? (
              <Button
                variant="secondary"
                icon={<Square />}
                onClick={onStopRemote}
                disabled={engineBusy}
                data-testid="mlx-remote-stop"
              >
                {intl.formatMessage(i18n.stopRemote)}
              </Button>
            ) : null
          }
          modeLabel={modeLabel}
          distributed={distributed}
          remote={remote}
        />
        <div className="flex min-w-0 flex-1 flex-col gap-3">
          <div className="flex min-w-0 flex-col gap-1">
            <span className={TYPE.meta}>
              {distributedOwns
                ? intl.formatMessage(i18n.servingDistributed)
                : remote
                  ? intl.formatMessage(i18n.servingRemote, { peer: routePeerName(remote) })
                  : running || state === 'mounting'
                    ? 'Serving'
                    : 'Served model'}
            </span>
            {distributedOwns && distributed?.modelId ? (
              <span className={cx('break-all font-mono text-lz-h2 text-lz-ink')}>
                {distributed.modelId}
              </span>
            ) : remote?.modelId ? (
              <span className={cx('break-all font-mono text-lz-h2 text-lz-ink')}>
                {remote.modelId}
              </span>
            ) : mountedModelId && (running || state === 'mounting') ? (
              <span className={cx('break-all font-mono text-lz-h2 text-lz-ink')}>
                {mountedModelId}
              </span>
            ) : (
              <span className={cx('text-lz-h2 text-lz-ink-2')}>no model mounted</span>
            )}
          </div>
          {status?.probeError && (
            <p className={cx('break-words text-lz-body', WEIGHT.semibold, TONE_TEXT.err)}>
              Probe failed: {status.probeError}
            </p>
          )}
          {status?.memoryError != null && (
            <p className={cx('break-words text-lz-body', WEIGHT.semibold, TONE_TEXT.err)}>
              Memory unmeasured: {status.memoryError}
            </p>
          )}
          {status && status.memoryError == null && (
            <div className="flex min-w-0 flex-col gap-1.5">
              <span
                className={cx(
                  'text-lz-body',
                  TNUM,
                  memoryTight ? cx(WEIGHT.semibold, TONE_TEXT.warn) : 'text-lz-ink'
                )}
              >
                {`${status.availableMemoryGb.toFixed(1)} GB available of ${status.totalMemoryGb.toFixed(1)} GB`}
                {status.reclaimableCacheGb != null &&
                  ` (${status.reclaimableCacheGb.toFixed(1)} GB is reclaimable file cache)`}
              </span>
              <MemoryBar availableGb={status.availableMemoryGb} totalGb={status.totalMemoryGb} />
            </div>
          )}
          {/* flex-wrap + a min width on the picker: at ~800px the buttons otherwise crushed the
              model picker into unreadability. */}
          <div className="flex flex-wrap items-start gap-2">
            <div className="min-w-[220px] flex-1">
              <ModelPicker
                models={models}
                value={mountModelId}
                onChange={setMountModelId}
                disabled={engineBusy || state === 'mounting' || distributedOwns}
                badges={badges}
              />
            </div>
            {offerReclaim && (
              <Button
                variant="secondary"
                icon={<Square />}
                onClick={onUnmount}
                disabled={engineBusy}
              >
                Unmount
              </Button>
            )}
          </div>
          <p className={TYPE.meta}>{intl.formatMessage(i18n.pickThenRun)}</p>
        </div>
      </section>

      {runModelId && (
        <PlacementCard
          modelId={runModelId}
          single={status}
          distributed={distributed}
          onMountHere={onMount}
          onStopHere={onUnmount}
          mountBusy={engineBusy || state === 'mounting' || distributedOwns}
          distributedCapability={distributedCapability}
          splitDetails={splitDetails}
        />
      )}

      {/* The running engine's facts are the point of the page once it runs; before that they are
          all "—", so they fold away under a disclosure instead of leading the page. */}
      {running ? (
        <Panel title="Engine details">{details}</Panel>
      ) : (
        <Disclosure
          title="Engine details"
          open={detailsOpen}
          onOpenChange={setDetailsOpen}
          testId="mlx-engine-details"
        >
          {details}
        </Disclosure>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// SAMPLING tab — per-model profiles. The drafts map lives in the shell keyed by
// model id, so unsaved edits survive tab AND model switches; this section only
// renders the selected model's drafts.
// ---------------------------------------------------------------------------

interface SamplingSectionProps {
  /** Which Mac's profiles are edited — shown when more than one Mac can be managed. */
  macPicker: ReactNode;
  status: MlxEngineStatus | null;
  settings: MlxEngineSettings | null;
  engineBusy: boolean;
  onRemount: () => void;
  models: MlxLocalModel[];
  selectedModelId: string | null;
  onSelectModel: (id: string | null) => void;
  drafts: NumericDrafts | null;
  savedDrafts: NumericDrafts | null;
  setDraft: (key: ProfileDraftKey, text: string) => void;
  onSaveSettings: () => void;
  saving: boolean;
  saveError: string | null;
}

function SamplingSection(props: SamplingSectionProps) {
  const {
    macPicker,
    status,
    settings,
    engineBusy,
    onRemount,
    models,
    selectedModelId,
    onSelectModel,
    drafts,
    savedDrafts,
    setDraft,
    onSaveSettings,
    saving,
    saveError,
  } = props;

  const dirty = drafts != null && savedDrafts != null && !draftsEqual(drafts, savedDrafts);
  const profileIds = Object.keys(settings?.modelProfiles ?? {});
  const selectedIsMounted =
    status?.modelId != null && selectedModelId != null && status.modelId === selectedModelId;

  return (
    <div className="flex flex-col gap-4 pb-8">
      {macPicker}
      <RestartRequiredBanner
        status={status}
        settings={settings}
        engineBusy={engineBusy}
        onRemount={onRemount}
      />

      <div className="flex flex-col gap-1">
        <p className={TYPE.bodyMuted}>
          Sampling is PER MODEL: each model mounts with the flags from its own profile, and
          per-request values sent by goose override them.
        </p>
        <p className={TYPE.body}>
          {status?.modelId ? (
            <>
              <span className="text-lz-ink-3">Currently mounted: </span>
              <span className={cx('font-mono text-lz-mono', WEIGHT.semibold)}>
                {status.modelId}
              </span>
            </>
          ) : (
            <span className="text-lz-ink-3">no model mounted</span>
          )}
        </p>
      </div>

      <Panel
        title="Model profile"
        headerRight={
          <>
            {selectedIsMounted && <Chip tone="ok">mounted</Chip>}
            {dirty && <Chip tone="warn">unsaved</Chip>}
            <Button
              size="sm"
              variant="primary"
              onClick={onSaveSettings}
              disabled={!dirty || saving || !settings || !selectedModelId}
              icon={saving ? <Loader2 className="animate-spin" /> : undefined}
            >
              Save
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-4">
          <div className="max-w-2xl">
            <SamplingModelPicker
              models={models}
              profileIds={profileIds}
              value={selectedModelId}
              onChange={onSelectModel}
            />
          </div>
          {saveError && <ToneBanner tone="err" label="Save failed" text={saveError} />}
          {!settings ? (
            <p className={TYPE.meta}>Loading settings…</p>
          ) : !selectedModelId || !drafts ? (
            <p className={TYPE.bodyMuted}>Pick a model above to edit its sampling profile.</p>
          ) : (
            <div className="flex flex-col">
              {SAMPLING_FIELDS.map((spec) => (
                <NumericField
                  key={spec.key}
                  spec={spec}
                  text={drafts[spec.key]}
                  onText={(v) => setDraft(spec.key, v)}
                />
              ))}
              <NumericField
                spec={CONTEXT_LIMIT_FIELD}
                text={drafts.contextLimit}
                onText={(v) => setDraft('contextLimit', v)}
              />
              <LaneFields drafts={drafts} setDraft={setDraft} />
              <MlxThinkingFields
                model={models.find((m) => m.id === selectedModelId)}
                drafts={drafts}
                setDraft={setDraft}
              />
              <MlxKvCacheFields
                model={models.find((m) => m.id === selectedModelId)}
                drafts={drafts}
                setDraft={setDraft}
              />
            </div>
          )}
          <p className={TYPE.meta}>
            A blank field sends nothing — the engine keeps its own default. Profiles apply at mount,
            per model: saving never touches a live process, and the status reports restart required
            until the mounted model is remounted. The serving lane is read from the model folder at
            mount (an MTP head, a vision config), so a re-download can flip restart required on its
            own.
          </p>
        </div>
      </Panel>
    </div>
  );
}

// ---------------------------------------------------------------------------
// The serving-lane rows of the profile form: what the engine does with the checkpoint's own
// extras. Auto is the honest default everywhere — the model folder decides, the profile
// overrides. Same grid as NumericField so the form reads as one table.
// ---------------------------------------------------------------------------

interface SpeculativeOption extends StudioSelectOption {
  value: '' | 'mtp' | 'off';
}

const SPECULATIVE_OPTIONS: readonly SpeculativeOption[] = [
  { value: '', label: 'Auto — MTP when the model folder has mtp.safetensors' },
  { value: 'mtp', label: 'MTP — demand it (skipped with a warning if the head is missing)' },
  { value: 'off', label: 'Off — never speculate' },
];

function LaneRow({
  label,
  note,
  children,
}: {
  label: string;
  note: string;
  children: React.ReactNode;
}) {
  return (
    <div
      className={cx(
        'grid grid-cols-[minmax(160px,240px)_1fr] items-center gap-4 border-t py-2',
        SURFACE.hairline
      )}
    >
      <span className={cx('truncate', TYPE.body)}>{label}</span>
      <div className="flex min-w-0 flex-col gap-1">
        <div className="flex min-w-0 flex-wrap items-center gap-2">{children}</div>
        <span className={TYPE.meta}>{note}</span>
      </div>
    </div>
  );
}

function LaneFields({
  drafts,
  setDraft,
}: {
  drafts: NumericDrafts;
  setDraft: (key: ProfileDraftKey, text: string) => void;
}) {
  const speculative =
    SPECULATIVE_OPTIONS.find((o) => o.value === drafts.speculative.trim()) ??
    SPECULATIVE_OPTIONS[0];
  const textLane = drafts.textOnly.trim() !== 'false';
  const adapterSet = drafts.adapterPath.trim() !== '';
  return (
    <>
      <LaneRow
        label="Speculative decoding"
        note="MTP drafts 3 tokens per step from the head shipped next to the trunk; the config names the model folder because the engine resolves the head from it."
      >
        <div className="w-full max-w-md">
          <StudioSelect
            options={SPECULATIVE_OPTIONS}
            value={speculative}
            onChange={(o) => setDraft('speculative', o?.value ?? '')}
            placeholder="Auto"
            aria-label="Speculative decoding"
          />
        </div>
      </LaneRow>
      <LaneRow
        label="LoRA adapter folder"
        note="An mlx-lm adapter (adapter_config.json + adapters.safetensors) fused into the model at load. A folder missing either file fails the mount and says which."
      >
        <input
          type="text"
          value={drafts.adapterPath}
          onChange={(e) => setDraft('adapterPath', e.target.value)}
          placeholder="~/adapters/my-lora"
          spellCheck={false}
          className={cx(INPUT, 'w-full max-w-md font-mono')}
          aria-label="LoRA adapter folder"
        />
        {adapterSet ? (
          <Button
            size="sm"
            variant="ghost"
            icon={<X />}
            onClick={() => setDraft('adapterPath', '')}
            title="Clear — mount the bare checkpoint"
          >
            Clear
          </Button>
        ) : (
          <span className={TYPE.meta} title="No adapter — the bare checkpoint is served">
            none
          </span>
        )}
      </LaneRow>
      <LaneRow
        label="Text-only lane"
        note="On: a vision-bearing checkpoint (qwen3_5 with a vision config) is pinned to the text lane, which batches. Off: the engine may route it to its serialized single-request vision lane."
      >
        <StudioSwitch
          checked={textLane}
          onChange={(v) => setDraft('textOnly', v ? '' : 'false')}
          aria-label="Text-only lane"
        />
        <span className={TYPE.meta}>{textLane ? 'on' : 'off — vision lane allowed'}</span>
      </LaneRow>
    </>
  );
}

// ---------------------------------------------------------------------------
// MODELS tab — models folder, the Hugging Face browser, local models.
// ---------------------------------------------------------------------------

function ModelsDirDialog({
  open,
  initial,
  saving,
  error,
  onSave,
  onClose,
}: {
  open: boolean;
  initial: string;
  saving: boolean;
  error: string | null;
  onSave: (dir: string) => void;
  onClose: () => void;
}) {
  const [value, setValue] = useState(initial);
  useEffect(() => {
    if (open) setValue(initial);
  }, [open, initial]);
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-[560px]">
        <DialogHeader>
          <DialogTitle>Models folder</DialogTitle>
          <DialogDescription>
            One directory for everything: Hugging Face downloads land here and mounts read from
            here.
          </DialogDescription>
        </DialogHeader>
        <input
          value={value}
          onChange={(e) => setValue(e.target.value)}
          className={cx(INPUT, 'w-full font-mono text-lz-mono')}
          placeholder="/path/to/mlx-models"
          aria-label="Models folder path"
          autoComplete="off"
          spellCheck={false}
        />
        {error && <ToneBanner tone="err" label="Save failed" text={error} />}
        <DialogFooter>
          <Button variant="secondary" onClick={onClose} disabled={saving}>
            Cancel
          </Button>
          <Button
            variant="primary"
            onClick={() => onSave(value.trim())}
            disabled={saving || value.trim() === ''}
            icon={saving ? <Loader2 className="animate-spin" /> : undefined}
          >
            Save
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

// ------------------------- Hugging Face browser ----------------------------

/**
 * Per-download lifecycle handlers keyed by repo id — ONE cluster passed from the shell
 * (where the tracking state lives) down through every surface that renders a download.
 */
export interface DownloadHandlers {
  onDownload: (repoId: string) => void;
  onPause: (repoId: string) => void;
  onResume: (repoId: string) => void;
  onCancel: (repoId: string) => void;
}

/**
 * The Model column of a browse row: the id is the row's open-card control (the row itself is
 * clickable too), and any start error or live download row sits under it so the table stays
 * the ONE place a download is followed from.
 */
function HitNameCell({
  hit,
  startError,
  progress,
  handlers,
  onOpenCard,
}: {
  hit: MlxBrowseHit;
  startError: string | undefined;
  progress: MlxDownloadProgress | undefined;
  handlers: DownloadHandlers;
  onOpenCard: () => void;
}) {
  return (
    <div className="flex min-w-0 flex-col gap-1 py-1">
      <button
        type="button"
        onClick={(e) => {
          e.stopPropagation();
          onOpenCard();
        }}
        aria-label={`Open model card for ${hit.id}`}
        title={hit.id}
        className={cx(
          'max-w-full truncate text-left font-mono text-lz-mono text-lz-ink hover:text-lz-accent',
          FOCUS,
          MOTION
        )}
      >
        {hit.id}
      </button>
      {startError && (
        <span className={cx('break-words text-lz-meta', WEIGHT.semibold, TONE_TEXT.err)}>
          {startError}
        </span>
      )}
      {progress && (
        <DownloadProgressRow
          repoId={hit.id}
          progress={progress}
          onPause={() => handlers.onPause(hit.id)}
          onResume={() => handlers.onResume(hit.id)}
          onCancel={() => handlers.onCancel(hit.id)}
        />
      )}
    </div>
  );
}

interface HfBrowserState {
  queryText: string;
  setQueryText: (v: string) => void;
  commitQuery: () => void;
  author: string | null;
  setAuthor: (v: string | null) => void;
  quant: string | null;
  setQuant: (v: string | null) => void;
  arch: string | null;
  setArch: (v: string | null) => void;
  sort: MlxBrowseSort;
  setSort: (v: MlxBrowseSort) => void;
  hits: MlxBrowseHit[] | null;
  nextCursor: string | null;
  loading: boolean;
  loadingMore: boolean;
  error: string | null;
  loadMore: () => void;
}

function useHfBrowserState(): HfBrowserState {
  const [queryText, setQueryText] = useState('');
  const [appliedQuery, setAppliedQuery] = useState('');
  const [author, setAuthor] = useState<string | null>(null);
  const [quant, setQuant] = useState<string | null>(null);
  const [arch, setArch] = useState<string | null>(null);
  const [sort, setSort] = useState<MlxBrowseSort>('downloads');

  const [hits, setHits] = useState<MlxBrowseHit[] | null>(null);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const epoch = useRef(0);

  const baseParams = useMemo(
    () => ({
      sort,
      query: appliedQuery || undefined,
      author: author ?? undefined,
      quant: quant ?? undefined,
      arch: arch ?? undefined,
      limit: 20,
    }),
    [sort, appliedQuery, author, quant, arch]
  );

  // Any filter/sort/search change lands here: fetch page 1, REPLACING the list — stale
  // in-flight responses (including a Load more) are dropped by the epoch guard.
  useEffect(() => {
    const id = ++epoch.current;
    setLoading(true);
    setError(null);
    setNextCursor(null);
    void (async () => {
      try {
        const page = await mlxEngineBrowse(baseParams);
        if (epoch.current !== id) return;
        setHits(page.hits);
        setNextCursor(page.nextCursor ?? null);
      } catch (e) {
        if (epoch.current !== id) return;
        setError(mlxErrorMessage(e, 'Hugging Face browse failed.'));
        setHits(null);
      } finally {
        if (epoch.current === id) setLoading(false);
      }
    })();
  }, [baseParams]);

  const loadMore = useCallback(() => {
    if (!nextCursor) return;
    const id = epoch.current;
    setLoadingMore(true);
    void (async () => {
      try {
        const page = await mlxEngineBrowse({ ...baseParams, cursor: nextCursor });
        if (epoch.current !== id) return;
        setHits((prev) => {
          const seen = new Set((prev ?? []).map((h) => h.id));
          return [...(prev ?? []), ...page.hits.filter((h) => !seen.has(h.id))];
        });
        setNextCursor(page.nextCursor ?? null);
      } catch (e) {
        if (epoch.current !== id) return;
        setError(mlxErrorMessage(e, 'Loading the next page failed.'));
      } finally {
        if (epoch.current === id) setLoadingMore(false);
      }
    })();
  }, [baseParams, nextCursor]);

  const commitQuery = useCallback(() => setAppliedQuery(queryText.trim()), [queryText]);

  return {
    queryText,
    setQueryText,
    commitQuery,
    author,
    setAuthor,
    quant,
    setQuant,
    arch,
    setArch,
    sort,
    setSort,
    hits,
    nextCursor,
    loading,
    loadingMore,
    error,
    loadMore,
  };
}

interface HfBrowserProps {
  browser: HfBrowserState;
  downloads: Record<string, MlxDownloadProgress>;
  downloadErrors: Record<string, string>;
  handlers: DownloadHandlers;
  filters: MlxBrowseFilters | null;
  filtersError: string | null;
  onOpenCard: (repoId: string) => void;
}

/**
 * Paginated MLX-only Hugging Face browser (presentation — state lives in useHfBrowserState).
 * Every filter is applied SERVER-side through `_goose/unstable/mlxEngine/browse`; changing any
 * filter/sort/search resets pagination (an epoch guard drops stale in-flight pages), and Load
 * more appends via `nextCursor`. Filter vocabularies come from the backend's live crawl
 * (`browseFilters`), loaded once per view-open by the shell; free text beyond them passes
 * through as-is.
 */
function HfBrowser({
  browser,
  downloads,
  downloadErrors,
  handlers,
  filters,
  filtersError,
  onOpenCard,
}: HfBrowserProps) {
  const {
    queryText,
    setQueryText,
    commitQuery,
    author,
    setAuthor,
    quant,
    setQuant,
    arch,
    setArch,
    sort,
    setSort,
    hits,
    nextCursor,
    loading,
    loadingMore,
    error,
    loadMore,
  } = browser;

  // Every attribute is an aligned, quiet column; the ONE coloured element on a row is its
  // action. "—" states an absent value — never a guessed one.
  const columns = useMemo<DataTableColumn<MlxBrowseHit>[]>(
    () => [
      {
        key: 'model',
        header: 'Model',
        className: 'min-w-[220px]',
        cell: (hit) => (
          <HitNameCell
            hit={hit}
            startError={downloadErrors[hit.id]}
            progress={downloads[hit.id]}
            handlers={handlers}
            onOpenCard={() => onOpenCard(hit.id)}
          />
        ),
      },
      {
        key: 'publisher',
        header: 'Publisher',
        cell: (hit) => (
          <span className={META} title={`Published by ${hit.author}`}>
            {hit.author}
          </span>
        ),
      },
      {
        key: 'quant',
        header: 'Quant',
        cell: (hit) =>
          hit.quant ? (
            <span className={META} title="Derived from the repo's tags or name">
              {hit.quant}
            </span>
          ) : (
            <Absent />
          ),
      },
      {
        key: 'arch',
        header: 'Arch',
        cell: (hit) =>
          hit.arch ? (
            <span className={META} title="Derived from the repo's tags or name">
              {hit.arch}
            </span>
          ) : (
            <Absent />
          ),
      },
      {
        key: 'size',
        header: 'Size',
        numeric: true,
        cell: (hit) =>
          hit.sizeBytesEstimate != null ? (
            <span
              className={META}
              title="Repository download size, including weights, tokenizer and configuration"
            >
              {formatBytesShort(hit.sizeBytesEstimate)}
            </span>
          ) : (
            <span className="text-lz-ink-4" title="Download size unavailable">
              —
            </span>
          ),
      },
      {
        key: 'downloads',
        header: 'Downloads',
        numeric: true,
        cell: (hit) => (
          <span className={META} title={`${hit.downloads.toLocaleString()} downloads`}>
            {formatCount(hit.downloads)}
          </span>
        ),
      },
      {
        key: 'likes',
        header: 'Likes',
        numeric: true,
        cell: (hit) => (
          <span className={META} title={`${hit.likes.toLocaleString()} likes`}>
            {formatCount(hit.likes)}
          </span>
        ),
      },
      {
        key: 'created',
        header: 'Created',
        numeric: true,
        cell: (hit) =>
          hit.createdAt ? (
            <span
              className={
                sort === 'newest' ? cx('text-lz-meta text-lz-ink', WEIGHT.semibold, TNUM) : META
              }
              title={`Created ${hit.createdAt}`}
            >
              {formatDate(hit.createdAt)}
            </span>
          ) : (
            <Absent />
          ),
      },
    ],
    [downloads, downloadErrors, handlers, onOpenCard, sort]
  );

  return (
    <Panel
      title="Hugging Face — MLX models"
      count={hits?.length}
      headerRight={
        loading ? <Chip icon={<Loader2 className="animate-spin" />}>loading</Chip> : undefined
      }
      padded={false}
    >
      {/* Enter in the search field submits (implicit submission); the filter comboboxes
          preventDefault their own Enter, so a pick never commits the query. */}
      <form
        className={cx('border-b px-4 py-3', SURFACE.hairline)}
        onSubmit={(e) => {
          e.preventDefault();
          commitQuery();
        }}
      >
        <Toolbar
          aria-label="Hugging Face browser"
          className="flex-wrap"
          search={{
            value: queryText,
            onChange: setQueryText,
            placeholder: 'Search MLX models by name…',
            'aria-label': 'Search Hugging Face',
          }}
          filters={
            <>
              <Button
                type="submit"
                variant="secondary"
                size="sm"
                icon={<Search />}
                aria-label="Search"
              >
                Search
              </Button>
              <FilterCombobox
                label="Provider"
                value={author}
                options={filters?.authors ?? []}
                onChange={setAuthor}
              />
              <FilterCombobox
                label="Quant"
                value={quant}
                options={filters?.quants ?? []}
                onChange={setQuant}
              />
              <FilterCombobox
                label="Arch"
                value={arch}
                options={filters?.archs ?? []}
                onChange={setArch}
              />
              {filters?.refreshError != null && (
                <Chip
                  tone="warn"
                  title={`Vocabulary refresh failed — serving the previous crawl. ${filters.refreshError}`}
                >
                  vocabulary may be stale
                </Chip>
              )}
              {filtersError != null && (
                <Chip tone="warn" title={filtersError}>
                  filter vocabulary unavailable — free text still works
                </Chip>
              )}
            </>
          }
          actions={
            <Segmented<MlxBrowseSort>
              aria-label="Sort"
              options={[
                { value: 'downloads', label: 'Top downloads' },
                { value: 'newest', label: 'Latest' },
              ]}
              value={sort}
              onChange={setSort}
            />
          }
        />
      </form>
      {error && (
        <div className={cx('border-b px-4 py-3', SURFACE.hairline)}>
          <ToneBanner tone="err" label="Browse failed" text={error} />
        </div>
      )}
      {hits != null && (
        <DataTable
          aria-label="Hugging Face MLX models"
          columns={columns}
          rows={hits}
          rowKey={(hit) => hit.id}
          onRowClick={(hit) => onOpenCard(hit.id)}
          rowAction={(hit) =>
            downloads[hit.id] ? null : (
              <Button
                variant="primary"
                size="sm"
                icon={<Download />}
                onClick={(e) => {
                  e.stopPropagation();
                  handlers.onDownload(hit.id);
                }}
                aria-label={`Download ${hit.id}`}
                title="Download"
              />
            )
          }
          empty={
            !loading && !error ? (
              <EmptyState
                icon={<Search />}
                title="No matches"
                body="No MLX models match these filters."
              />
            ) : undefined
          }
        />
      )}
      {nextCursor != null && !loading && (
        <div className={cx('flex justify-center border-t py-2', SURFACE.hairline)}>
          <Button
            variant="secondary"
            size="sm"
            onClick={loadMore}
            disabled={loadingMore}
            icon={loadingMore ? <Loader2 className="animate-spin" /> : undefined}
          >
            Load more
          </Button>
        </div>
      )}
      <p className={cx('border-t px-4 py-3', TYPE.meta, SURFACE.hairline)}>
        Filters match Hugging Face tags server-side
        {filters != null
          ? ` — vocabularies sampled live from ${filters.sampledRepos} MLX repos; type in a filter to search them, or apply any free text.`
          : ' — type in a filter to search its vocabulary, or apply any free text.'}{' '}
        A model whose quant appears only in its name is excluded by those filters but still findable
        via search. Click a row for its full model card.
      </p>
    </Panel>
  );
}

/**
 * Downloads with no inline row on the ACTIVE sub-tab still render here, so a running download is
 * visible from BOTH [Hugging Face | Downloaded] — same shell-owned state, one row per repo per
 * pane (the inactive pane is unmounted, so `mlx-download-*` testids stay unique).
 */
function ActiveDownloadsCard({
  entries,
  errors,
  handlers,
}: {
  entries: Array<[string, MlxDownloadProgress]>;
  errors: Record<string, string>;
  handlers: DownloadHandlers;
}) {
  if (entries.length === 0) return null;
  return (
    <Panel title="Active downloads" count={entries.length} padded={false}>
      {entries.map(([repoId, progress]) => (
        <div key={repoId} className={cx('border-t px-4 py-3 first:border-t-0', SURFACE.hairline)}>
          <span className="block min-w-0 truncate font-mono text-lz-mono text-lz-ink">
            {repoId}
          </span>
          {errors[repoId] && (
            <p className={cx('mt-1 break-words text-lz-meta', WEIGHT.semibold, TONE_TEXT.err)}>
              {errors[repoId]}
            </p>
          )}
          <DownloadProgressRow
            repoId={repoId}
            progress={progress}
            onPause={() => handlers.onPause(repoId)}
            onResume={() => handlers.onResume(repoId)}
            onCancel={() => handlers.onCancel(repoId)}
          />
        </div>
      ))}
    </Panel>
  );
}

type ModelsSubTab = 'macs' | 'hf';

const MODELS_I18N = defineMessages({
  onYourMacs: { id: 'mlxModels.onYourMacs', defaultMessage: 'On your Macs' },
  huggingFace: { id: 'mlxModels.huggingFace', defaultMessage: 'Hugging Face' },
  downloadTo: { id: 'mlxModels.downloadTo', defaultMessage: 'Download to' },
  folders: { id: 'mlxModels.folders', defaultMessage: 'Models folders' },
  foldersHint: {
    id: 'mlxModels.foldersHint',
    defaultMessage:
      'One folder per Mac, used by downloads, copies and loads alike; the bar is the free space on its volume.',
  },
  edit: { id: 'mlxModels.edit', defaultMessage: 'Edit' },
  folderUnread: { id: 'mlxModels.folderUnread', defaultMessage: 'Can’t read: {reason}' },
  settingsFailed: {
    id: 'mlxModels.settingsFailed',
    defaultMessage: 'The engine settings could not be read.',
  },
  cancelTitle: { id: 'mlxModels.cancelTitle', defaultMessage: 'Cancel download' },
  cancelMessage: {
    id: 'mlxModels.cancelMessage',
    defaultMessage: 'Cancel the download of {model} on {name} and delete its partial files there?',
  },
  keep: { id: 'mlxModels.keep', defaultMessage: 'Keep' },
});

/** One Mac's models folder: its path, its disk, and Edit — read from THAT Mac's settings. */
function FolderRow({
  mac,
  settings,
  saveSettings,
}: {
  mac: Mac;
  /** This Mac's settings (the view already holds them); a peer's are read here. */
  settings: MlxEngineSettings | null;
  saveSettings: (macKey: string, next: MlxEngineSettings) => Promise<MlxEngineSettings>;
}) {
  const intl = useIntl();
  const ctx = useMacs();
  const facts = ctx.factsOf(mac.key);
  const [peerSettings, setPeerSettings] = useState<MlxEngineSettings | null>(null);
  const [readError, setReadError] = useState<string | null>(null);
  const [open, setOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  useEffect(() => {
    if (mac.isSelf) return undefined;
    let live = true;
    mlxEngineSettingsRead(macTarget(mac))
      .then((s) => live && setPeerSettings(s))
      .catch(
        (e) =>
          live &&
          setReadError(
            ctx.describeError(
              mac,
              mlxErrorMessage(e, intl.formatMessage(MODELS_I18N.settingsFailed))
            )
          )
      );
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mac.key, mac.isSelf]);
  const current = mac.isSelf ? settings : peerSettings;
  const save = async (dir: string) => {
    if (!current) return;
    setSaving(true);
    setSaveError(null);
    try {
      const saved = await saveSettings(mac.key, {
        ...sanitizeSettingsForWrite(current),
        modelsDir: dir,
      });
      if (!mac.isSelf) setPeerSettings(saved);
      setOpen(false);
      void ctx.refreshModels(mac.key);
    } catch (e) {
      setSaveError(ctx.describeError(mac, mlxErrorMessage(e, String(e))));
    } finally {
      setSaving(false);
    }
  };
  return (
    <div
      className={cx('flex flex-col gap-2 border-t py-3 first:border-t-0', SURFACE.hairline)}
      data-testid={`models-folder-${mac.key}`}
    >
      <div className="flex min-w-0 items-center gap-2">
        <span className={cx('w-40 shrink-0 truncate', TYPE.body, WEIGHT.semibold)}>{mac.name}</span>
        <Folder className="size-4 shrink-0 text-lz-ink-3" />
        {readError ? (
          <span
            className={cx('min-w-0 flex-1 break-words', TYPE.body, WEIGHT.semibold, TONE_TEXT.err)}
          >
            {intl.formatMessage(MODELS_I18N.folderUnread, { reason: readError })}
          </span>
        ) : (
          <span
            className={cx(
              'min-w-0 flex-1 truncate px-3 py-1.5 font-mono text-lz-mono text-lz-ink',
              SURFACE.inset,
              RADIUS.control
            )}
            title={current?.modelsDir}
          >
            {current?.modelsDir ?? '…'}
          </span>
        )}
        <Button
          size="sm"
          variant="secondary"
          icon={<Pencil />}
          onClick={() => {
            setSaveError(null);
            setOpen(true);
          }}
          disabled={!current}
        >
          {intl.formatMessage(MODELS_I18N.edit)}
        </Button>
      </div>
      {facts.disk && (
        <DiskBar availableBytes={facts.disk.availableBytes} totalBytes={facts.disk.totalBytes} />
      )}
      <ModelsDirDialog
        open={open}
        initial={current?.modelsDir ?? ''}
        saving={saving}
        error={saveError}
        onSave={(dir) => void save(dir)}
        onClose={() => setOpen(false)}
      />
    </div>
  );
}

interface ModelsSectionProps {
  settings: MlxEngineSettings | null;
  saveSettings: (macKey: string, next: MlxEngineSettings) => Promise<MlxEngineSettings>;
  onOpenSampling: (macKey: string, modelId: string) => void;
  filters: MlxBrowseFilters | null;
  filtersError: string | null;
}

function ModelsSection({
  settings,
  saveSettings,
  onOpenSampling,
  filters,
  filtersError,
}: ModelsSectionProps) {
  const intl = useIntl();
  const ctx = useMacs();
  // [On your Macs | Hugging Face]: the models every Mac holds, one table; the browser apart. The
  // browser's state lives in the section so switching sub-tabs never loses query/filters/pages.
  const [view, setView] = useState<ModelsSubTab>('macs');
  const browser = useHfBrowserState();
  const readable = ctx.macs.filter((m) => m.online && !peerRefuses(m, 'manage'));
  const [target, setTarget] = useState<string>(SELF_KEY);
  const targetMac = readable.find((m) => m.key === target) ?? ctx.self;
  const targetKey = targetMac.key;
  const downloads = useMemo(() => ctx.downloads[targetKey] ?? {}, [ctx.downloads, targetKey]);
  const downloadErrors = useMemo(
    () => ctx.downloadErrors[targetKey] ?? {},
    [ctx.downloadErrors, targetKey]
  );
  const [pendingCancel, setPendingCancel] = useState<string | null>(null);
  const [cardRepoId, setCardRepoId] = useState<string | null>(null);

  const handlers = useMemo<DownloadHandlers>(
    () => ({
      onDownload: (repoId) => void ctx.download(targetKey, repoId),
      onPause: (repoId) => void ctx.pauseDownload(targetKey, repoId),
      onResume: (repoId) => void ctx.resumeDownload(targetKey, repoId),
      // A cancel deletes the partial from disk; on another Mac that is ITS disk — asked first.
      onCancel: (repoId) =>
        targetMac.isSelf ? void ctx.cancelDownload(targetKey, repoId) : setPendingCancel(repoId),
    }),
    [ctx, targetKey, targetMac.isSelf]
  );

  const hfOrphanDownloads = useMemo(() => {
    const hitIds = new Set((browser.hits ?? []).map((h) => h.id));
    return Object.entries(downloads).filter(([repoId]) => !hitIds.has(repoId));
  }, [browser.hits, downloads]);

  const rowCount = matrixRowCount(ctx);

  return (
    <div className="flex flex-col gap-4 pb-8">
      <Segmented<ModelsSubTab>
        aria-label="Models view"
        options={[
          {
            value: 'macs',
            label: (
              <>
                {intl.formatMessage(MODELS_I18N.onYourMacs)}
                <span className={cx('text-lz-meta', TNUM)}>{rowCount}</span>
              </>
            ),
          },
          { value: 'hf', label: intl.formatMessage(MODELS_I18N.huggingFace) },
        ]}
        value={view}
        onChange={setView}
      />

      {view === 'macs' && (
        <>
          <ModelMatrix onOpenSampling={onOpenSampling} />
          <Panel title={intl.formatMessage(MODELS_I18N.folders)}>
            {readable.map((mac) => (
              <FolderRow
                key={mac.key}
                mac={mac}
                settings={mac.isSelf ? settings : null}
                saveSettings={saveSettings}
              />
            ))}
            <p className={cx('mt-3', TYPE.meta)}>{intl.formatMessage(MODELS_I18N.foldersHint)}</p>
          </Panel>
        </>
      )}

      {view === 'hf' && (
        <>
          {readable.length > 1 && (
            <div className="flex flex-wrap items-center gap-2" data-testid="mlx-download-to">
              <span className={TYPE.meta}>{intl.formatMessage(MODELS_I18N.downloadTo)}</span>
              <Segmented<string>
                size="sm"
                aria-label={intl.formatMessage(MODELS_I18N.downloadTo)}
                options={readable.map((m) => ({ value: m.key, label: m.name }))}
                value={targetKey}
                onChange={setTarget}
              />
            </div>
          )}
          <ActiveDownloadsCard
            entries={hfOrphanDownloads}
            errors={downloadErrors}
            handlers={handlers}
          />
          <HfBrowser
            browser={browser}
            downloads={downloads}
            downloadErrors={downloadErrors}
            handlers={handlers}
            filters={filters}
            filtersError={filtersError}
            onOpenCard={setCardRepoId}
          />
        </>
      )}

      {cardRepoId != null && (
        <ModelCardModal
          repoId={cardRepoId}
          onClose={() => setCardRepoId(null)}
          progress={downloads[cardRepoId]}
          startError={downloadErrors[cardRepoId]}
          onDownload={() => handlers.onDownload(cardRepoId)}
          onPause={() => handlers.onPause(cardRepoId)}
          onResume={() => handlers.onResume(cardRepoId)}
          onCancel={() => handlers.onCancel(cardRepoId)}
        />
      )}

      <ConfirmationModal
        isOpen={pendingCancel !== null}
        title={intl.formatMessage(MODELS_I18N.cancelTitle)}
        message={
          pendingCancel
            ? intl.formatMessage(MODELS_I18N.cancelMessage, {
                model: pendingCancel,
                name: targetMac.name,
              })
            : ''
        }
        confirmLabel={intl.formatMessage(MODELS_I18N.cancelTitle)}
        cancelLabel={intl.formatMessage(MODELS_I18N.keep)}
        confirmVariant="destructive"
        onConfirm={() => {
          if (pendingCancel) void ctx.cancelDownload(targetKey, pendingCancel);
          setPendingCancel(null);
        }}
        onCancel={() => setPendingCancel(null)}
      />
    </div>
  );
}

// ---------------------------------------------------------------------------
// The view
// ---------------------------------------------------------------------------

type MlxTab = 'engine' | 'models' | 'sampling';

const VIEW_I18N = defineMessages({
  samplingOn: { id: 'mlxEngineView.samplingOn', defaultMessage: 'Profiles on' },
  settingsFailed: {
    id: 'mlxEngineView.settingsFailed',
    defaultMessage: 'Could not read the engine settings.',
  },
});

/** A draft's key: the Mac and the model (two Macs keep separate profiles for the same model). */
function draftKey(macKey: string, modelId: string): string {
  return `${macKey}\n${modelId}`;
}

/**
 * The run book outlives the page: one per engine source ('single', 'distributed', 'remote:<peer>')
 * for the window's life, so leaving the Engine tab and coming back keeps the runs it saw (the
 * page-held "last run" reset on every visit). A new source starts a new book; a restarted engine
 * starts one inside advanceRateBook.
 */
let rateBook: { source: string | null; book: RateBook } = { source: null, book: EMPTY_BOOK };

function ratesFor(source: string | null): RateBook {
  // No engine read yet (a remount's first tick, or stopped): show nothing, keep the book — a new
  // engine replaces it below, and a restarted one inside advanceRateBook.
  if (source === null) return EMPTY_BOOK;
  if (rateBook.source !== source) rateBook = { source, book: EMPTY_BOOK };
  return rateBook.book;
}

function foldRates(source: string, stats: MlxLiveStats): RateBook {
  rateBook = { source, book: advanceRateBook(ratesFor(source), stats) };
  return rateBook.book;
}

function MlxEngineViewBody() {
  const [tab, setTab] = useState<MlxTab>('engine');
  const { mlxDistributed } = useFeatures();
  const intl = useIntl();
  const macsCtx = useMacs();
  const selfFacts = macsCtx.factsOf(SELF_KEY);
  const models = useMemo(() => selfFacts.models ?? [], [selfFacts.models]);

  // The distributed engine is supervised by THIS Mac.
  const distributed = useMlxDistributedStatus(mlxDistributed);
  // A route serving this Mac's chat from a linked Mac's engine (remote single): while it is up, the
  // Engine tab's tile and mode speak for THAT engine — what actually answers chat.
  const remoteStatus = useSyncExternalStore(
    subscribeMlxRemoteSingleStatus,
    latestMlxRemoteSingleStatus
  );
  const remote =
    remoteRouteUp(remoteStatus) && !ownsTheMac(distributed.status) ? remoteStatus : null;
  const modeLabel = remote
    ? formatRemoteMode(intl, routePeerName(remote))
    : formatMlxMode(intl, modeSummary(distributed.status), null);

  const [status, setStatus] = useState<MlxEngineStatus | null>(null);
  const [statusError, setStatusError] = useState<string | null>(null);
  useEffect(() => {
    settleRestoreLine({ single: status, remote: remoteStatus, distributed: distributed.status });
  }, [status, remoteStatus, distributed.status]);
  const [settings, setSettings] = useState<MlxEngineSettings | null>(null);

  const [browseFilters, setBrowseFilters] = useState<MlxBrowseFilters | null>(null);
  const [browseFiltersError, setBrowseFiltersError] = useState<string | null>(null);
  const browseFiltersLoaded = useRef(false);

  const [mountModelId, setMountModelId] = useState<string | null>(null);
  const [mountError, setMountError] = useState<string | null>(null);
  const [engineBusy, setEngineBusy] = useState(false);

  // Per-model sampling, per Mac: ONLY profiles the user actually edited live here, keyed by Mac and
  // model — two models keep separate unsaved drafts and both survive tab/model/Mac switches.
  const [profileDrafts, setProfileDrafts] = useState<Record<string, NumericDrafts>>({});
  const [samplingMac, setSamplingMac] = useState<string>(SELF_KEY);
  const [samplingModelId, setSamplingModelId] = useState<string | null>(null);
  const [peerSettings, setPeerSettings] = useState<Record<string, MlxEngineSettings>>({});
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  const defaultedPicker = useRef(false);
  const userPickedModel = useRef(false);

  const pickMountModel = useCallback((id: string | null) => {
    userPickedModel.current = true;
    setMountModelId(id);
  }, []);

  // The state tile's live instrument (MlxStateTile): Rapid-MLX's own /v1/status while running, the
  // decode-rate history for the sparkline, and the memory watch across a mount. All of it rides the
  // SAME 2-second status poll below — no second clock.
  const [live, setLive] = useState<MlxLiveRead | null>(null);
  const [tpsHistory, setTpsHistory] = useState<TpsSample[]>([]);
  const [rates, setRates] = useState<RateBook>(() => rateBook.book);
  const [serving, setServing] = useState<MlxServing | null>(null);
  const [mountWatch, setMountWatch] = useState<MountWatch | null>(null);
  // Free memory at the last status that was NOT mounting: the baseline a mount's claim is measured
  // from. Null until this view has seen one (a view opened mid-mount has no baseline).
  const settledFreeGb = useRef<number | null>(null);
  // One live read at a time, so a slow read never lands after a newer one and reorders the samples.
  const liveInFlight = useRef(false);
  // While the distributed engine owns this Mac and is up, the live read is ITS rank 0's /v1/status
  // (the single engine's shape, rank_live.py) through the same parser — the tile's one activity code.
  const distributedNow = useRef(distributed.status);
  useEffect(() => {
    distributedNow.current = distributed.status;
  }, [distributed.status]);
  // While a route serves chat from a linked Mac, the live read is THAT engine's /v1/status through
  // goosed's loopback relay — the same parser and rates as this Mac's own engine.
  const remoteNow = useRef(remote);
  useEffect(() => {
    remoteNow.current = remote;
  }, [remote]);
  // The engine the live figures came from: a switch between engines starts the history afresh.
  const liveSource = useRef<string | null>(null);

  const refreshLive = useCallback(async (next: MlxEngineStatus) => {
    // The page manages this Mac only now (the Manage-on switch is gone): the live read is this Mac's
    // distributed rank 0 while that engine owns the Mac, else the single engine.
    const dist = distributedNow.current;
    const distUp =
      dist && ownsTheMac(dist) && (dist.state === 'ready' || dist.state === 'serving')
        ? dist
        : null;
    const route = remoteNow.current;
    const remoteUp = !distUp && route?.state === 'ready' ? route : null;
    const source = distUp
      ? 'distributed'
      : remoteUp
        ? `remote:${remoteUp.peer ?? ''}`
        : next.state === 'running'
          ? 'single'
          : null;
    if (source !== liveSource.current) {
      liveSource.current = source;
      setTpsHistory([]);
      setRates(ratesFor(source));
    }
    if (source === null) {
      setLive(null);
      setServing(null);
      return;
    }
    const baseUrl = distUp ? distUp.baseUrl : remoteUp ? remoteUp.baseUrl : next.baseUrl;
    if (!baseUrl) {
      setLive({
        ok: false,
        detail: distUp
          ? 'the distributed engine reported no base URL'
          : remoteUp
            ? 'this goose does not hand over the relay to the other Mac’s engine (update goose)'
            : 'the running engine reported no base URL',
      });
      return;
    }
    if (liveInFlight.current) return;
    liveInFlight.current = true;
    try {
      const [read, who] = await Promise.all([readMlxLiveStatus(baseUrl), readMlxServing()]);
      setLive(read);
      setServing(who);
      if (read.ok) {
        const stats = read.stats;
        setRates(foldRates(source, stats));
        if (stats.uptimeS != null) {
          const sample = { uptimeS: stats.uptimeS, tps: liveDecodeTps(stats) };
          setTpsHistory((h) => pushSample(h, sample));
        }
      }
    } finally {
      liveInFlight.current = false;
    }
  }, []);

  const refreshStatus = useCallback(async () => {
    let next: MlxEngineStatus;
    try {
      next = await mlxEngineStatus(undefined, mountModelId);
      setStatus(next);
      setStatusError(null);
    } catch (error) {
      setStatusError(mlxErrorMessage(error, 'Could not read the engine status.'));
      return;
    }
    if (next.state === 'mounting' && next.modelId) {
      const modelId = next.modelId;
      const baseline = settledFreeGb.current;
      setMountWatch((w) => advanceMountWatch(w, modelId, next.availableMemoryGb, baseline));
    } else {
      settledFreeGb.current = next.availableMemoryGb;
      setMountWatch(null);
    }
    await refreshLive(next);
  }, [refreshLive, mountModelId]);

  // Poll status every 2s while this window is actually visible; stop when hidden.
  useEffect(() => {
    let timer: ReturnType<typeof setInterval> | null = null;
    const start = () => {
      if (timer != null) return;
      void refreshStatus();
      timer = setInterval(() => void refreshStatus(), MLX_STATUS_POLL_MS);
    };
    const stop = () => {
      if (timer != null) {
        clearInterval(timer);
        timer = null;
      }
    };
    const onVisibility = () => {
      if (document.visibilityState === 'visible') start();
      else stop();
    };
    onVisibility();
    document.addEventListener('visibilitychange', onVisibility);
    return () => {
      stop();
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, [refreshStatus]);

  // Filter vocabularies load once per view-open (cached backend-side), on the first visit to the
  // Models tab; a failure leaves free text working and says so.
  useEffect(() => {
    if (tab !== 'models' || browseFiltersLoaded.current) return;
    browseFiltersLoaded.current = true;
    void (async () => {
      try {
        setBrowseFilters(await mlxEngineBrowseFilters());
      } catch (error) {
        setBrowseFiltersError(mlxErrorMessage(error, 'Could not load the filter vocabularies.'));
      }
    })();
  }, [tab]);

  useEffect(() => {
    void (async () => {
      try {
        setSettings(await mlxEngineSettingsRead());
      } catch (error) {
        setSaveError(mlxErrorMessage(error, intl.formatMessage(VIEW_I18N.settingsFailed)));
      }
    })();
  }, [intl]);

  // Picker follows truth: while the engine is running or mounting and the user has not
  // explicitly picked something else this visit, the picker shows the mounted model — so a
  // window opened onto an already-running engine reads the live model, never a stale pick.
  // An explicit user selection is never overridden. With the engine down, the picker defaults
  // once to the persisted model.
  useEffect(() => {
    if (userPickedModel.current) return;
    if (remote?.modelId) {
      defaultedPicker.current = true;
      setMountModelId(remote.modelId);
      return;
    }
    if ((status?.state === 'running' || status?.state === 'mounting') && status.modelId) {
      defaultedPicker.current = true;
      setMountModelId(status.modelId);
      return;
    }
    if (defaultedPicker.current) return;
    const candidate = status?.modelId ?? settings?.modelId;
    if (candidate) {
      defaultedPicker.current = true;
      setMountModelId(candidate);
    }
  }, [status?.state, status?.modelId, settings?.modelId, remote?.modelId]);

  // The Mac whose sampling profiles the Sampling tab edits: its settings, models and engine.
  const samplingMacObj = macsCtx.macByKey(samplingMac) ?? macsCtx.self;
  const samplingIsSelf = samplingMacObj.isSelf;
  const samplingSettings = samplingIsSelf ? settings : (peerSettings[samplingMac] ?? null);
  const samplingModels = useMemo(
    () => (samplingIsSelf ? models : (macsCtx.factsOf(samplingMac).models ?? [])),
    [samplingIsSelf, models, macsCtx, samplingMac]
  );
  const samplingStatus = samplingIsSelf ? status : macsCtx.factsOf(samplingMac).status;

  useEffect(() => {
    if (samplingIsSelf || peerSettings[samplingMac]) return;
    const mac = macsCtx.macByKey(samplingMac);
    if (!mac) return;
    let live = true;
    mlxEngineSettingsRead(macTarget(mac))
      .then((s) => live && setPeerSettings((prev) => ({ ...prev, [samplingMac]: s })))
      .catch(
        (e) =>
          live &&
          setSaveError(
            macsCtx.describeError(
              mac,
              mlxErrorMessage(e, intl.formatMessage(VIEW_I18N.settingsFailed))
            )
          )
      );
    return () => {
      live = false;
    };
  }, [samplingIsSelf, samplingMac, peerSettings, macsCtx, intl]);

  // Sampling picker default: the running model, else the last-mounted settings.modelId, else the
  // first complete model on that Mac. Once set (default, explicit pick, or the Models-tab shortcut)
  // it is never yanked from under the user.
  useEffect(() => {
    if (samplingModelId != null) return;
    const candidate =
      (samplingStatus?.state === 'running' && samplingStatus.modelId) ||
      samplingSettings?.modelId ||
      samplingModels.find((m) => m.complete)?.id ||
      null;
    if (candidate) setSamplingModelId(candidate);
  }, [samplingModelId, samplingStatus, samplingSettings?.modelId, samplingModels]);

  const onMount = useCallback(() => {
    if (!mountModelId) return;
    void (async () => {
      setEngineBusy(true);
      setMountError(null);
      try {
        await mlxEngineMount(mountModelId);
      } catch (error) {
        setMountError(mlxErrorMessage(error, 'Mount failed.'));
      } finally {
        setEngineBusy(false);
        void refreshStatus();
      }
    })();
  }, [mountModelId, refreshStatus]);

  const [remoteStopError, setRemoteStopError] = useState<string | null>(null);
  const onStopRemote = useCallback(() => {
    void (async () => {
      setEngineBusy(true);
      setRemoteStopError(null);
      try {
        const { unmountError } = await mlxRemoteSingleStop(false);
        if (unmountError) setRemoteStopError(unmountError);
      } catch (error) {
        setRemoteStopError(mlxErrorMessage(error, intl.formatMessage(i18n.stopRemoteFailed)));
      } finally {
        setEngineBusy(false);
        void refreshStatus();
      }
    })();
  }, [intl, refreshStatus]);

  const onUnmount = useCallback(() => {
    void (async () => {
      setEngineBusy(true);
      setMountError(null);
      try {
        await mlxEngineUnmount();
      } catch (error) {
        setMountError(mlxErrorMessage(error, 'Unmount failed.'));
      } finally {
        setEngineBusy(false);
        void refreshStatus();
      }
    })();
  }, [refreshStatus]);

  /** Remount the Mac whose profiles changed: this Mac's engine, or the other Mac's over Link. */
  const remountOn = useCallback(
    (macKey: string) => {
      const mac = macsCtx.macByKey(macKey) ?? macsCtx.self;
      const macStatus = mac.isSelf ? status : macsCtx.factsOf(mac.key).status;
      const macSettings = mac.isSelf ? settings : peerSettings[mac.key];
      const modelId = macStatus?.modelId ?? macSettings?.modelId;
      if (!modelId) return;
      void (async () => {
        setEngineBusy(true);
        setMountError(null);
        try {
          await mlxEngineUnmount(macTarget(mac));
          await mlxEngineMount(modelId, macTarget(mac));
        } catch (error) {
          setMountError(macsCtx.describeError(mac, mlxErrorMessage(error, 'Remount failed.')));
        } finally {
          setEngineBusy(false);
          if (mac.isSelf) void refreshStatus();
          else void macsCtx.refreshStatus(mac.key);
        }
      })();
    },
    [macsCtx, peerSettings, refreshStatus, settings, status]
  );
  const onRemount = useCallback(() => remountOn(SELF_KEY), [remountOn]);

  const saveSettingsOn = useCallback(
    async (macKey: string, next: MlxEngineSettings): Promise<MlxEngineSettings> => {
      const mac = macsCtx.macByKey(macKey) ?? macsCtx.self;
      const saved = await mlxEngineSettingsUpdate(next, macTarget(mac));
      if (mac.isSelf) {
        setSettings(saved);
        void refreshStatus();
      } else {
        setPeerSettings((prev) => ({ ...prev, [mac.key]: saved }));
        void macsCtx.refreshStatus(mac.key);
      }
      return saved;
    },
    [macsCtx, refreshStatus]
  );

  const savedDraftsForSelected = useMemo(
    () =>
      samplingSettings && samplingModelId
        ? draftsFromProfile(samplingSettings.modelProfiles?.[samplingModelId])
        : null,
    [samplingSettings, samplingModelId]
  );
  const draftsForSelected =
    samplingModelId != null
      ? (profileDrafts[draftKey(samplingMac, samplingModelId)] ?? savedDraftsForSelected)
      : null;

  const setProfileDraft = useCallback(
    (key: ProfileDraftKey, text: string) => {
      if (!samplingModelId || !samplingSettings) return;
      const k = draftKey(samplingMac, samplingModelId);
      setProfileDrafts((prev) => {
        const base =
          prev[k] ?? draftsFromProfile(samplingSettings.modelProfiles?.[samplingModelId]);
        return { ...prev, [k]: { ...base, [key]: text } };
      });
    },
    [samplingMac, samplingModelId, samplingSettings]
  );

  const onSaveProfile = useCallback(() => {
    if (!samplingSettings || !samplingModelId) return;
    const k = draftKey(samplingMac, samplingModelId);
    const drafts = profileDrafts[k];
    if (!drafts) return;
    void (async () => {
      setSaving(true);
      setSaveError(null);
      try {
        await saveSettingsOn(
          samplingMac,
          settingsWithProfile(samplingSettings, samplingModelId, drafts)
        );
        // This model's edits are now the saved truth; other models keep their own drafts.
        setProfileDrafts((prev) => {
          const next = { ...prev };
          delete next[k];
          return next;
        });
      } catch (error) {
        setSaveError(mlxErrorMessage(error, 'Could not save settings.'));
      } finally {
        setSaving(false);
      }
    })();
  }, [samplingSettings, samplingModelId, samplingMac, profileDrafts, saveSettingsOn]);

  const openSamplingFor = useCallback((macKey: string, modelId: string) => {
    setSamplingMac(macKey);
    setSamplingModelId(modelId);
    setTab('sampling');
  }, []);

  const managed = macsCtx.macs.filter((m) => m.online && !peerRefuses(m, 'manage'));
  const samplingMacPicker =
    managed.length > 1 ? (
      <div className="flex flex-wrap items-center gap-2" data-testid="mlx-sampling-mac">
        <span className={TYPE.meta}>{intl.formatMessage(VIEW_I18N.samplingOn)}</span>
        <Segmented<string>
          size="sm"
          aria-label={intl.formatMessage(VIEW_I18N.samplingOn)}
          options={managed.map((m) => ({ value: m.key, label: m.name }))}
          value={samplingMac}
          onChange={(key) => {
            setSamplingMac(key);
            setSamplingModelId(null);
          }}
        />
      </div>
    ) : null;

  const splitDetails = mlxDistributed ? (
    <DistributedEngineSection
      capability={mlxDistributed}
      embedded
      status={distributed.status}
      statusError={distributed.error}
      onRefresh={distributed.refresh}
      models={models}
      singleStatus={status}
      onSingleChanged={() => void refreshStatus()}
    />
  ) : null;

  // The page shell (MainPanelLayout, the Goose Swarm header, the top-level tab bar and the
  // scroll area) belongs to LeanZeroSwarmView — this component is the LeanZero MLX tab's content.
  return (
    <div className="flex flex-col gap-4">
      {/* The engine's own section switch sits UNDER the Providers strip, so it is the subordinate
          underline register, on the row's hairline — never a second solid strip that reads as
          another top nav. */}
      <div className={cx('flex flex-wrap items-end gap-3 border-b', SURFACE.hairline)}>
        <Segmented<MlxTab>
          variant="underline"
          aria-label="Engine sections"
          options={[
            { value: 'engine', label: 'Engine' },
            {
              value: 'models',
              label: (
                <>
                  Models
                  <span className={cx('text-lz-meta', TNUM)}>{matrixRowCount(macsCtx)}</span>
                </>
              ),
            },
            { value: 'sampling', label: 'Sampling' },
          ]}
          value={tab}
          onChange={setTab}
        />
        {/* The Engine tab's hero owns the state; the other tabs keep the badge in view. */}
        {status && tab !== 'engine' && (
          <span className="pb-2">
            <StateBadge
              state={status.state}
              live={live}
              distributed={distributed.status}
              remote={remote}
              load={singleLoad(status)}
            />
          </span>
        )}
        {/* Which engine owns this Mac, on every tab. */}
        <span className="pb-2">
          <Chip tone={ownsTheMac(distributed.status) ? 'accent' : undefined} icon={<Network />}>
            <span data-testid="mlx-mode-chip">{modeLabel}</span>
          </Chip>
        </span>
        {/* pr-3: without it the ScrollArea's right edge shaved the final glyph off "Rapid-MLX"
            (caught live on the packaged build, 2026-08-31). */}
        <span className={cx('ml-auto shrink-0 pb-2.5 pr-3', TYPE.meta)}>Powered by Rapid-MLX</span>
      </div>

      {tab === 'engine' && (
        <EngineSection
          status={status}
          statusError={statusError}
          settings={settings}
          models={models}
          mountModelId={mountModelId}
          setMountModelId={pickMountModel}
          mountError={mountError}
          engineBusy={engineBusy}
          onMount={onMount}
          onUnmount={onUnmount}
          onRemount={onRemount}
          live={live}
          tpsHistory={tpsHistory}
          rates={rates}
          serving={serving}
          mountWatch={mountWatch}
          distributed={distributed.status}
          distributedCapability={mlxDistributed}
          splitDetails={splitDetails}
          modeLabel={modeLabel}
          remote={remote}
          onStopRemote={onStopRemote}
          remoteStopError={remoteStopError}
        />
      )}
      {tab === 'models' && (
        <ModelsSection
          settings={settings}
          saveSettings={saveSettingsOn}
          onOpenSampling={openSamplingFor}
          filters={browseFilters}
          filtersError={browseFiltersError}
        />
      )}
      {tab === 'sampling' && (
        <SamplingSection
          macPicker={samplingMacPicker}
          status={samplingStatus}
          settings={samplingSettings}
          engineBusy={engineBusy}
          onRemount={() => remountOn(samplingMac)}
          models={samplingModels}
          selectedModelId={samplingModelId}
          onSelectModel={setSamplingModelId}
          drafts={draftsForSelected}
          savedDrafts={savedDraftsForSelected}
          setDraft={setProfileDraft}
          onSaveSettings={onSaveProfile}
          saving={saving}
          saveError={saveError}
        />
      )}
    </div>
  );
}

/** The LeanZero MLX tab: inside the Providers view's MacsProvider, or its own when rendered alone. */
const MlxEngineView: React.FC = () => (
  <WithMacs>
    <MlxEngineViewBody />
  </WithMacs>
);

export default MlxEngineView;
