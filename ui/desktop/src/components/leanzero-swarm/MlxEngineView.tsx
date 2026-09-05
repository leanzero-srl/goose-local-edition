import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Check,
  Download,
  Folder,
  HardDrive,
  Laptop,
  Loader2,
  Minus,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  Search,
  SlidersHorizontal,
  Square,
  Trash2,
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
import {
  mlxEngineBrowse,
  mlxEngineBrowseFilters,
  mlxEngineDownload,
  mlxEngineDownloadCancel,
  mlxEngineDownloadPause,
  mlxEngineDownloadProgress,
  mlxEngineDownloadResume,
  mlxEngineModelDelete,
  mlxEngineModelsList,
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
import { DownloadProgressRow, formatCount, formatDate, formatGb } from './primitives';
import { FilterCombobox } from './FilterCombobox';
import { INPUT, StudioSelect, ToneBanner, type StudioSelectOption } from './studio';
import { ModelCardModal } from './ModelCardModal';
import { useFeatures } from '../../contexts/FeaturesContext';
import {
  leanzeroLinkNodes,
  leanzeroLinkStatus,
  type NodeStatus,
  type NodesResponse,
} from '../../acp/leanzero-link';

// Formatters stay importable from this module — tests and older callers reach them here.
export { formatBytesShort, formatCount, formatDate, formatGb } from './primitives';

// Engine state on the status triad: running = ok, failed = err, stopped = the stopped neutral,
// and mounting is the same neutral with a LIVE dot (in flight) — the difference is motion.
const STATE_TONE: Record<MlxEngineState, Tone> = {
  running: 'ok',
  mounting: 'stopped',
  failed: 'err',
  stopped: 'stopped',
};

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

export type NumericDrafts = Record<NumericSettingKey, string>;

const NUMERIC_KEYS: NumericSettingKey[] = [...SAMPLING_FIELDS, CONTEXT_LIMIT_FIELD].map(
  (f) => f.key
);

export function draftsFromProfile(profile: MlxModelProfile | undefined): NumericDrafts {
  const drafts = {} as NumericDrafts;
  for (const key of NUMERIC_KEYS) {
    const value = profile?.[key];
    drafts[key] = value == null ? '' : String(value);
  }
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
  return profile;
}

export function profileHasValues(profile: MlxModelProfile): boolean {
  return NUMERIC_KEYS.some((key) => profile[key] != null);
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
  return NUMERIC_KEYS.every((key) => a[key].trim() === b[key].trim());
}

// ---------------------------------------------------------------------------
// Small building blocks on the Studio tokens (banner/progress row live in ./primitives)
// ---------------------------------------------------------------------------

/** The engine's state: a solid dot (pulsing while a mount is in flight) beside a toned chip. */
function StateBadge({ state }: { state: MlxEngineState }) {
  const tone = STATE_TONE[state];
  return (
    <span className="inline-flex items-center gap-2" data-testid="mlx-state-badge">
      <StatusDot tone={tone} live={state === 'mounting'} label={`Engine ${state}`} />
      <Chip
        tone={tone}
        icon={state === 'mounting' ? <Loader2 className="animate-spin" /> : undefined}
      >
        {state}
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
}

function ModelOptionLabel({ option }: { option: ModelOption }) {
  return (
    <span className="flex min-w-0 items-center gap-2">
      <span className="truncate font-mono text-lz-mono">{option.model.id}</span>
      <span className={cx('shrink-0', TYPE.meta, TNUM)}>{formatGb(option.model.sizeBytes)}</span>
      {!option.model.complete && <Chip tone="warn">partial download</Chip>}
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
}: {
  models: MlxLocalModel[];
  value: string | null;
  onChange: (id: string | null) => void;
  disabled: boolean;
}) {
  const options: ModelOption[] = models.map((model) => ({
    value: model.id,
    label: model.id,
    model,
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
}

function EngineSection(props: EngineSectionProps) {
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
  } = props;

  const state = status?.state ?? null;
  const mountedModelId = status?.modelId ?? null;
  const strayPort = state === 'stopped' ? status?.strayListenerPort : undefined;
  const selectionIsMounted =
    state === 'running' && !!mountModelId && mountModelId === mountedModelId;
  const canMount =
    !!mountModelId && !engineBusy && (state === 'stopped' || state === 'failed' || state === null);
  const canSwitch =
    state === 'running' && !!mountModelId && mountModelId !== mountedModelId && !engineBusy;
  const canUnmount =
    !engineBusy && (state === 'running' || state === 'mounting' || strayPort != null);

  const memoryTight =
    status != null &&
    status.totalMemoryGb > 0 &&
    status.availableMemoryGb / status.totalMemoryGb < 0.15;

  // Every row is backend truth or an honest "—"; nothing here is fabricated.
  const facts: KeyValueItem[] = [
    {
      key: 'model',
      label: 'Served model',
      value: status?.modelId ?? <span className="text-lz-ink-4">no model mounted</span>,
      mono: status?.modelId != null,
    },
    {
      key: 'state',
      label: 'State',
      value: status ? <StateBadge state={status.state} /> : <Absent />,
    },
    {
      key: 'context',
      label: 'Context length',
      value: status?.contextWindow != null ? status.contextWindow.toLocaleString() : <Absent />,
    },
    {
      key: 'memory',
      label: 'Memory headroom',
      value: status ? (
        `${status.availableMemoryGb.toFixed(1)} GB free of ${status.totalMemoryGb.toFixed(1)} GB`
      ) : (
        <Absent />
      ),
      tone: memoryTight ? 'warn' : undefined,
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

  return (
    <div className="flex flex-col gap-4 pb-8">
      {statusError && <ToneBanner tone="err" label="Engine unreachable" text={statusError} />}
      {status?.gateMessage && status.gateVerdict === 'block' && (
        <ToneBanner tone="err" label="Mount blocked" text={status.gateMessage} />
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
      {mountError && <ToneBanner tone="err" label="Mount failed" text={mountError} />}
      {status?.state === 'failed' && status.lastError && status.lastError !== mountError && (
        <ToneBanner tone="err" label="Engine failed" text={status.lastError} />
      )}
      <RestartRequiredBanner
        status={status}
        settings={settings}
        engineBusy={engineBusy}
        onRemount={onRemount}
      />

      {/* Status panel: label / value rows, every value right-aligned in tabular figures. */}
      <Panel title="Engine" padded={false}>
        <div className="px-4">
          <KeyValue items={facts} aria-label="Engine status" />
        </div>
        {status?.probeError && (
          <p
            className={cx(
              'break-words border-t px-4 py-3 text-lz-body',
              WEIGHT.semibold,
              TONE_TEXT.err,
              SURFACE.hairline
            )}
          >
            Probe failed: {status.probeError}
          </p>
        )}
        {status && (
          <div className={cx('border-t px-4 py-3', SURFACE.hairline)}>
            <MemoryBar availableGb={status.availableMemoryGb} totalGb={status.totalMemoryGb} />
          </div>
        )}
      </Panel>

      {/* Mount controls: the picker, ONE primary action, Unmount as the secondary. */}
      <Panel title="Mount a model">
        {/* flex-wrap + a min width on the picker: at ~800px the two buttons otherwise
            crushed the model picker into unreadability. */}
        <div className="flex flex-wrap items-start gap-2">
          <div className="min-w-[220px] flex-1">
            <ModelPicker
              models={models}
              value={mountModelId}
              onChange={setMountModelId}
              disabled={engineBusy || state === 'mounting'}
            />
          </div>
          {/* The primary button tells the truth about the LIVE engine, not just mount intent:
              mounting -> spinner; selection already mounted -> "Mounted" as a disabled status;
              a different selection while running -> "Switch model" (the backend shuts the old
              model down); otherwise the plain Mount action. */}
          {state === 'mounting' ? (
            <Button variant="primary" disabled icon={<Loader2 className="animate-spin" />}>
              Mounting
            </Button>
          ) : selectionIsMounted ? (
            <Button variant="secondary" disabled icon={<Check />}>
              Mounted
            </Button>
          ) : state === 'running' ? (
            <Button variant="primary" icon={<Play />} onClick={onMount} disabled={!canSwitch}>
              Switch model
            </Button>
          ) : (
            <Button variant="primary" icon={<Play />} onClick={onMount} disabled={!canMount}>
              Mount
            </Button>
          )}
          <Button variant="secondary" icon={<Square />} onClick={onUnmount} disabled={!canUnmount}>
            Unmount
          </Button>
        </div>
        <p className={cx('mt-3', TYPE.meta)}>
          Mount returns immediately and the engine flips to mounting; the status above follows the
          live engine every 2 seconds. Each model mounts with its own sampling profile.
        </p>
      </Panel>

      {/* Spawn command — visible, not editable here: the owner sees exactly what would run. */}
      {settings && (
        <Panel title="Spawn command">
          <code className={cx('block break-all', TYPE.mono)}>
            {settings.spawnCommand.join(' ')}
          </code>
        </Panel>
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
  status: MlxEngineStatus | null;
  settings: MlxEngineSettings | null;
  engineBusy: boolean;
  onRemount: () => void;
  models: MlxLocalModel[];
  selectedModelId: string | null;
  onSelectModel: (id: string | null) => void;
  drafts: NumericDrafts | null;
  savedDrafts: NumericDrafts | null;
  setDraft: (key: NumericSettingKey, text: string) => void;
  onSaveSettings: () => void;
  saving: boolean;
  saveError: string | null;
}

function SamplingSection(props: SamplingSectionProps) {
  const {
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
            </div>
          )}
          <p className={TYPE.meta}>
            A blank field sends nothing — the engine keeps its own default. Profiles apply at mount,
            per model: saving never touches a live process, and the status reports restart required
            until the mounted model is remounted.
          </p>
        </div>
      </Panel>
    </div>
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

function useHfBrowserState(nodeId?: string): HfBrowserState {
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
        const page = await mlxEngineBrowse(baseParams, nodeId);
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
  }, [baseParams, nodeId]);

  const loadMore = useCallback(() => {
    if (!nextCursor) return;
    const id = epoch.current;
    setLoadingMore(true);
    void (async () => {
      try {
        const page = await mlxEngineBrowse({ ...baseParams, cursor: nextCursor }, nodeId);
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
  }, [baseParams, nextCursor, nodeId]);

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
              title="estimated from tensor dtypes; exact size on the model card"
            >
              ~{formatGb(hit.sizeBytesEstimate)}
            </span>
          ) : (
            <span className="text-lz-ink-4" title="no size estimate for this repo">
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

type ModelsSubTab = 'hf' | 'downloaded';

interface ModelsSectionProps {
  settings: MlxEngineSettings | null;
  models: MlxLocalModel[];
  disk: { availableBytes: number; totalBytes: number } | null;
  mountedModelId: string | null;
  refreshModels: () => void;
  saveSettings: (next: MlxEngineSettings) => Promise<void>;
  onOpenSampling: (modelId: string) => void;
  downloads: Record<string, MlxDownloadProgress>;
  downloadErrors: Record<string, string>;
  downloadHandlers: DownloadHandlers;
  onModelDeleted: (modelId: string) => void;
  filters: MlxBrowseFilters | null;
  filtersError: string | null;
  /** The device every model op targets — undefined = local, byte-identical to before. */
  nodeId?: string;
  /** Hostname of the selected REMOTE device, or null when local — names the delete confirm. */
  remoteHostname: string | null;
}

function ModelsSection({
  settings,
  models,
  disk,
  mountedModelId,
  refreshModels,
  saveSettings,
  onOpenSampling,
  downloads,
  downloadErrors,
  downloadHandlers,
  onModelDeleted,
  filters,
  filtersError,
  nodeId,
  remoteHostname,
}: ModelsSectionProps) {
  // Owner amendment: the Models area splits into [Hugging Face | Downloaded] — the local models
  // used to sit at the bottom of one long column and were hard to see. The browser's state lives
  // in the section (useHfBrowserState) so switching sub-tabs never loses query/filters/pages.
  const [view, setView] = useState<ModelsSubTab>('hf');
  const browser = useHfBrowserState(nodeId);

  const [dirDialogOpen, setDirDialogOpen] = useState(false);
  const [dirSaving, setDirSaving] = useState(false);
  const [dirError, setDirError] = useState<string | null>(null);

  const [cardRepoId, setCardRepoId] = useState<string | null>(null);

  const [pendingDelete, setPendingDelete] = useState<MlxLocalModel | null>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [deleting, setDeleting] = useState(false);

  // Every tracked download must be visible on the ACTIVE sub-tab: rows already shown inline
  // (browse hits on Hugging Face, local models on Downloaded) stay where they are; the rest
  // render in the Active downloads card above the pane content.
  const hfOrphanDownloads = useMemo(() => {
    const hitIds = new Set((browser.hits ?? []).map((h) => h.id));
    return Object.entries(downloads).filter(([repoId]) => !hitIds.has(repoId));
  }, [browser.hits, downloads]);
  const downloadedOrphanDownloads = useMemo(() => {
    const localIds = new Set(models.map((m) => m.id));
    return Object.entries(downloads).filter(([repoId]) => !localIds.has(repoId));
  }, [models, downloads]);

  const saveDir = useCallback(
    async (dir: string) => {
      if (!settings) return;
      setDirSaving(true);
      setDirError(null);
      try {
        await saveSettings({ ...sanitizeSettingsForWrite(settings), modelsDir: dir });
        setDirDialogOpen(false);
        refreshModels();
      } catch (error) {
        setDirError(mlxErrorMessage(error, 'Could not save the models folder.'));
      } finally {
        setDirSaving(false);
      }
    },
    [settings, saveSettings, refreshModels]
  );

  const confirmDelete = useCallback(async () => {
    if (!pendingDelete) return;
    setDeleting(true);
    setDeleteError(null);
    try {
      await mlxEngineModelDelete(pendingDelete.id, nodeId);
      onModelDeleted(pendingDelete.id);
      setPendingDelete(null);
      refreshModels();
    } catch (error) {
      setDeleteError(mlxErrorMessage(error, `Could not delete ${pendingDelete.id}.`));
    } finally {
      setDeleting(false);
    }
  }, [pendingDelete, refreshModels, onModelDeleted, nodeId]);

  const isIncomplete = (model: MlxLocalModel) => model.missingFiles > 0 || !model.complete;

  // The local library as a table: model | state (a tone, or "—") | size; the actions ride the
  // trailing slot. Errors and live downloads sit under the model id, as on the browser.
  const localColumns = useMemo<DataTableColumn<MlxLocalModel>[]>(
    () => [
      {
        key: 'model',
        header: 'Model',
        className: 'min-w-[220px]',
        cell: (model) => (
          <div className="flex min-w-0 flex-col gap-1 py-1">
            <span className="flex min-w-0 items-center gap-2">
              <HardDrive
                className={cx(
                  'size-4 shrink-0',
                  isIncomplete(model) ? TONE_TEXT.warn : 'text-lz-ink-3'
                )}
              />
              <span className="truncate font-mono text-lz-mono text-lz-ink" title={model.id}>
                {model.id}
              </span>
            </span>
            {downloadErrors[model.id] && (
              <span className={cx('break-words text-lz-meta', WEIGHT.semibold, TONE_TEXT.err)}>
                {downloadErrors[model.id]}
              </span>
            )}
            {downloads[model.id] && (
              <DownloadProgressRow
                repoId={model.id}
                progress={downloads[model.id]}
                onPause={() => downloadHandlers.onPause(model.id)}
                onResume={() => downloadHandlers.onResume(model.id)}
                onCancel={() => downloadHandlers.onCancel(model.id)}
              />
            )}
          </div>
        ),
      },
      {
        key: 'state',
        header: 'State',
        cell: (model) => {
          const mounted = model.id === mountedModelId;
          const incomplete = isIncomplete(model);
          if (!mounted && !incomplete) return <Absent />;
          return (
            <span className="inline-flex flex-wrap items-center gap-1">
              {mounted && <Chip tone="ok">mounted</Chip>}
              {incomplete && (
                <Chip
                  tone="warn"
                  title="Files the repo's safetensors index names are absent or unfinished — Resume continues the download"
                >
                  incomplete — missing {model.missingFiles} file(s)
                </Chip>
              )}
            </span>
          );
        },
      },
      {
        key: 'size',
        header: 'Size',
        numeric: true,
        cell: (model) => <span className={META}>{formatGb(model.sizeBytes)}</span>,
      },
    ],
    [downloads, downloadErrors, downloadHandlers, mountedModelId]
  );

  return (
    <div className="flex flex-col gap-4 pb-8">
      {/* Second-level switch (owner): the browser and the local library are separate tabs so
          neither buries the other. */}
      <Segmented<ModelsSubTab>
        aria-label="Models view"
        options={[
          { value: 'hf', label: 'Hugging Face' },
          {
            value: 'downloaded',
            label: (
              <>
                Downloaded
                <span className={cx('text-lz-meta', TNUM)}>{models.length}</span>
              </>
            ),
          },
        ]}
        value={view}
        onChange={setView}
      />

      {view === 'hf' && (
        <>
          <ActiveDownloadsCard
            entries={hfOrphanDownloads}
            errors={downloadErrors}
            handlers={downloadHandlers}
          />
          <HfBrowser
            browser={browser}
            downloads={downloads}
            downloadErrors={downloadErrors}
            handlers={downloadHandlers}
            filters={filters}
            filtersError={filtersError}
            onOpenCard={setCardRepoId}
          />
        </>
      )}

      {view === 'downloaded' && (
        <>
          <ActiveDownloadsCard
            entries={downloadedOrphanDownloads}
            errors={downloadErrors}
            handlers={downloadHandlers}
          />

          {/* Local models */}
          <Panel
            title="Downloaded models"
            count={models.length}
            headerRight={
              <Button size="sm" variant="ghost" icon={<RefreshCw />} onClick={refreshModels}>
                Refresh
              </Button>
            }
            padded={false}
          >
            {deleteError && (
              <div className={cx('border-b px-4 py-3', SURFACE.hairline)}>
                <ToneBanner tone="err" label="Delete failed" text={deleteError} />
              </div>
            )}
            <DataTable
              aria-label="Downloaded models"
              columns={localColumns}
              rows={models}
              rowKey={(model) => model.id}
              rowAction={(model) => (
                <span className="inline-flex items-center gap-1">
                  {isIncomplete(model) ? (
                    <Button
                      size="sm"
                      variant="secondary"
                      icon={<Play />}
                      onClick={() => downloadHandlers.onResume(model.id)}
                      aria-label={`Resume ${model.id}`}
                      title="Resume the download — complete files are skipped, partials continue"
                    >
                      Resume
                    </Button>
                  ) : (
                    <Button
                      size="sm"
                      variant="ghost"
                      icon={<SlidersHorizontal />}
                      onClick={() => onOpenSampling(model.id)}
                      aria-label={`Sampling for ${model.id}`}
                      title="This model's sampling profile — opens the Sampling tab"
                    >
                      Sampling
                    </Button>
                  )}
                  <Button
                    size="sm"
                    variant="ghost"
                    icon={<Trash2 />}
                    onClick={() => {
                      setDeleteError(null);
                      setPendingDelete(model);
                    }}
                    aria-label={`Delete ${model.id}`}
                    title="Delete from the models folder"
                  />
                </span>
              )}
              empty={
                <EmptyState
                  icon={<HardDrive />}
                  title="Nothing downloaded yet"
                  body="Browse the Hugging Face tab to download an MLX model."
                />
              }
            />
          </Panel>

          {/* Models folder + disk — the local library's home, so it lives on the Downloaded tab. */}
          <Panel title="Models folder">
            <div className="flex items-center gap-2">
              <Folder className="size-4 shrink-0 text-lz-ink-3" />
              <span
                className={cx(
                  'min-w-0 flex-1 truncate px-3 py-1.5 font-mono text-lz-mono text-lz-ink',
                  SURFACE.inset,
                  RADIUS.control
                )}
                title={settings?.modelsDir}
              >
                {settings?.modelsDir ?? '…'}
              </span>
              <Button
                size="sm"
                variant="secondary"
                icon={<Pencil />}
                onClick={() => {
                  setDirError(null);
                  setDirDialogOpen(true);
                }}
                disabled={!settings}
              >
                Edit
              </Button>
            </div>
            {disk && (
              <div className="mt-3">
                <DiskBar availableBytes={disk.availableBytes} totalBytes={disk.totalBytes} />
              </div>
            )}
            <p className={cx('mt-3', TYPE.meta)}>
              One directory used by downloads and mounts alike; the bar is the free space on its
              volume.
            </p>
          </Panel>
        </>
      )}

      {cardRepoId != null && (
        <ModelCardModal
          repoId={cardRepoId}
          nodeId={nodeId}
          onClose={() => setCardRepoId(null)}
          progress={downloads[cardRepoId]}
          startError={downloadErrors[cardRepoId]}
          onDownload={() => downloadHandlers.onDownload(cardRepoId)}
          onPause={() => downloadHandlers.onPause(cardRepoId)}
          onResume={() => downloadHandlers.onResume(cardRepoId)}
          onCancel={() => downloadHandlers.onCancel(cardRepoId)}
        />
      )}

      <ModelsDirDialog
        open={dirDialogOpen}
        initial={settings?.modelsDir ?? ''}
        saving={dirSaving}
        error={dirError}
        onSave={(dir) => void saveDir(dir)}
        onClose={() => setDirDialogOpen(false)}
      />

      <ConfirmationModal
        isOpen={pendingDelete !== null}
        title="Delete model"
        message={
          pendingDelete
            ? `Delete ${pendingDelete.id} (${formatGb(pendingDelete.sizeBytes)})${
                remoteHostname ? ` on ${remoteHostname}` : ''
              } from the models folder? This removes the files from ${
                remoteHostname ? "that device's disk" : 'disk'
              }.`
            : ''
        }
        confirmLabel="Delete"
        confirmVariant="destructive"
        isSubmitting={deleting}
        onConfirm={() => void confirmDelete()}
        onCancel={() => setPendingDelete(null)}
      />
    </div>
  );
}

// ---------------------------------------------------------------------------
// Device target picker — manage models on ANY linked device, not just the local
// one. Node list comes from `leanzeroLink/nodes` (self + peers). Never a native
// <select>: the hub's StudioSelect listbox.
// ---------------------------------------------------------------------------

interface DeviceTarget {
  /** null = THIS device (local). A peer carries its `node_id`. */
  nodeId: string | null;
  hostname: string;
  isSelf: boolean;
  /** Peers carry a live status for the idle/busy chip; self shows none. */
  status?: NodeStatus;
}

function deviceLabel(t: DeviceTarget): string {
  return t.isSelf ? 'This device' : t.hostname;
}

const NODE_STATUS: Record<NodeStatus['type'], { tone: Tone; label: string }> = {
  Idle: { tone: 'ok', label: 'idle' },
  Busy: { tone: 'warn', label: 'busy' },
  Offline: { tone: 'stopped', label: 'offline' },
};

function NodeStatusChip({ status }: { status: NodeStatus }) {
  const v = NODE_STATUS[status.type] ?? NODE_STATUS.Offline;
  return <Chip tone={v.tone}>{v.label}</Chip>;
}

interface DeviceOption extends StudioSelectOption {
  target: DeviceTarget;
}

/**
 * The node dropdown — a Studio listbox. "This device" (self) is always first and default;
 * each connected peer follows with its hostname and a live idle/busy chip. A peer stays
 * selectable even when busy — model management (list/browse/download) works while a node
 * runs a session; an unreachable/offline peer surfaces its backend error in the ops below.
 */
function DeviceTargetPicker({
  targets,
  value,
  onChange,
  disabled,
}: {
  targets: DeviceTarget[];
  value: string | null;
  onChange: (nodeId: string | null) => void;
  disabled?: boolean;
}) {
  const options: DeviceOption[] = targets.map((t) => ({
    value: t.nodeId ?? 'self',
    label: deviceLabel(t),
    target: t,
  }));
  const selected = options.find((o) => o.target.nodeId === value) ?? options[0] ?? null;
  return (
    <StudioSelect
      className="min-w-[240px]"
      aria-label="Manage models on device"
      options={options}
      value={selected}
      disabled={disabled}
      placeholder="This device"
      renderOption={(o) => (
        <span className="flex min-w-0 items-center gap-2 [&_svg]:size-4 [&_svg]:shrink-0">
          <Laptop className="text-lz-ink-3" />
          <span className={cx('min-w-0 truncate', WEIGHT.medium)}>{o.label}</span>
          {o.target.status && <NodeStatusChip status={o.target.status} />}
        </span>
      )}
      optionTestId={(o) => `mlx-device-target-option-${o.target.nodeId ?? 'self'}`}
      onChange={(o) => onChange(o ? o.target.nodeId : null)}
    />
  );
}

/**
 * Poll the mesh roster for the device picker. Gated on the `leanzeroLink` capability and a
 * `connected` auth state — the common case right now (no worker deployed) yields no peers, so
 * the picker is hidden and the whole view behaves exactly as before. A transient status blip
 * keeps the last roster rather than yanking the user's selection back to local; a definitive
 * "not connected" clears the peers.
 */
function useLinkNodes(enabled: boolean): NodesResponse | null {
  const [nodes, setNodes] = useState<NodesResponse | null>(null);
  useEffect(() => {
    if (!enabled) {
      setNodes(null);
      return undefined;
    }
    let disposed = false;
    let timer: ReturnType<typeof setInterval> | null = null;
    const poll = async () => {
      try {
        const st = await leanzeroLinkStatus();
        if (disposed) return;
        if (st.auth.state === 'connected') {
          try {
            const n = await leanzeroLinkNodes();
            if (!disposed) setNodes(n);
          } catch {
            // Keep the last roster on a transient nodes() failure — "connected" still holds.
          }
        } else {
          setNodes(null);
        }
      } catch {
        // Status read failed (worker unreachable): keep the last roster, don't yank selection.
      }
    };
    const start = () => {
      if (timer != null) return;
      void poll();
      timer = setInterval(() => void poll(), 5000);
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
      disposed = true;
      stop();
      document.removeEventListener('visibilitychange', onVisibility);
    };
  }, [enabled]);
  return nodes;
}

// ---------------------------------------------------------------------------
// The view
// ---------------------------------------------------------------------------

type MlxTab = 'engine' | 'models' | 'sampling';

const STATUS_POLL_MS = 2000;

const MlxEngineView: React.FC = () => {
  const [tab, setTab] = useState<MlxTab>('engine');

  // Which linked device every op targets. `targetNodeId === null` means THIS device (local):
  // activeNodeId is then `undefined`, omitted from the wire, so the local path is byte-identical.
  const { leanzeroLink } = useFeatures();
  const linkNodes = useLinkNodes(leanzeroLink);
  const peers = useMemo(() => linkNodes?.peers ?? [], [linkNodes]);
  const [targetNodeId, setTargetNodeId] = useState<string | null>(null);
  const [nodeSwitching, setNodeSwitching] = useState(false);

  const selectedPeer = useMemo(
    () => (targetNodeId != null ? (peers.find((p) => p.node_id === targetNodeId) ?? null) : null),
    [peers, targetNodeId]
  );
  const activeNodeId = selectedPeer ? selectedPeer.node_id : undefined;
  const remoteHostname = selectedPeer ? selectedPeer.hostname : null;

  const deviceTargets = useMemo<DeviceTarget[]>(() => {
    const self: DeviceTarget = {
      nodeId: null,
      hostname: linkNodes?.self.hostname ?? 'This device',
      isSelf: true,
    };
    return [
      self,
      ...peers.map((p) => ({
        nodeId: p.node_id,
        hostname: p.hostname,
        isSelf: false,
        status: p.status,
      })),
    ];
  }, [linkNodes, peers]);

  // A selected peer that drops off the roster (disabled / removed) falls back to This device —
  // never leave a stale peer id driving the ops once it is gone.
  useEffect(() => {
    if (targetNodeId != null && !peers.some((p) => p.node_id === targetNodeId)) {
      setTargetNodeId(null);
    }
  }, [peers, targetNodeId]);

  // The live target, mirrored into a ref so an in-flight fetch that resolves AFTER a device
  // switch is DROPPED rather than writing the previous node's data over the new one (TRUTH LAYER).
  const activeNodeRef = useRef(activeNodeId);
  useEffect(() => {
    activeNodeRef.current = activeNodeId;
  }, [activeNodeId]);

  const [status, setStatus] = useState<MlxEngineStatus | null>(null);
  const [statusError, setStatusError] = useState<string | null>(null);
  const [settings, setSettings] = useState<MlxEngineSettings | null>(null);
  const [models, setModels] = useState<MlxLocalModel[]>([]);
  const [disk, setDisk] = useState<{ availableBytes: number; totalBytes: number } | null>(null);

  // Download tracking lives in the VIEW SHELL, not the Models tab: switching tabs
  // mid-download must keep the rows live and the poll running while the view is open.
  const [downloads, setDownloads] = useState<Record<string, MlxDownloadProgress>>({});
  const [downloadErrors, setDownloadErrors] = useState<Record<string, string>>({});
  // A cancel DELETES the partial on disk. On a REMOTE device that is another machine's disk, so
  // it goes through a confirm that names the device; a local cancel stays immediate (as before).
  const [pendingCancel, setPendingCancel] = useState<string | null>(null);

  const [browseFilters, setBrowseFilters] = useState<MlxBrowseFilters | null>(null);
  const [browseFiltersError, setBrowseFiltersError] = useState<string | null>(null);
  // The device whose filter vocabulary is loaded (null = none yet). A node switch re-crawls
  // for the new device — a peer's backend keeps its own cache.
  const browseFiltersFor = useRef<{ node: string | undefined } | null>(null);

  const [mountModelId, setMountModelId] = useState<string | null>(null);
  const [mountError, setMountError] = useState<string | null>(null);
  const [engineBusy, setEngineBusy] = useState(false);

  // Per-model sampling: ONLY models the user actually edited live here, keyed by model id
  // — so two models keep separate unsaved drafts and both survive tab/model switches.
  const [profileDrafts, setProfileDrafts] = useState<Record<string, NumericDrafts>>({});
  const [samplingModelId, setSamplingModelId] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  const defaultedPicker = useRef(false);
  const userPickedModel = useRef(false);

  const pickMountModel = useCallback((id: string | null) => {
    userPickedModel.current = true;
    setMountModelId(id);
  }, []);

  const refreshStatus = useCallback(async () => {
    try {
      const next = await mlxEngineStatus(activeNodeId);
      if (activeNodeRef.current !== activeNodeId) return; // switched away mid-flight — drop
      setStatus(next);
      setStatusError(null);
    } catch (error) {
      if (activeNodeRef.current !== activeNodeId) return;
      setStatusError(mlxErrorMessage(error, 'Could not read the engine status.'));
    }
  }, [activeNodeId]);

  // Poll status every 2s while this window is actually visible; stop when hidden.
  useEffect(() => {
    let timer: ReturnType<typeof setInterval> | null = null;
    const start = () => {
      if (timer != null) return;
      void refreshStatus();
      timer = setInterval(() => void refreshStatus(), STATUS_POLL_MS);
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

  const refreshModels = useCallback(() => {
    void (async () => {
      try {
        const list = await mlxEngineModelsList(activeNodeId);
        if (activeNodeRef.current !== activeNodeId) return; // switched away mid-flight — drop
        setModels(list.models);
        setDisk({ availableBytes: list.diskAvailableBytes, totalBytes: list.diskTotalBytes });
      } catch (error) {
        if (activeNodeRef.current !== activeNodeId) return;
        // The models list failing is a real fact; show it where models are picked.
        setMountError(mlxErrorMessage(error, 'Could not list local models.'));
      } finally {
        // The node's models have landed (or failed loudly) — the switch is done.
        if (activeNodeRef.current === activeNodeId) setNodeSwitching(false);
      }
    })();
  }, [activeNodeId]);

  // Filter vocabularies load once per (device, view-open) (cached backend-side), on the first
  // visit to the Models tab; a failure leaves free text working and says so. Switching the
  // device re-loads for the new node.
  useEffect(() => {
    if (tab !== 'models') return;
    if (browseFiltersFor.current && browseFiltersFor.current.node === activeNodeId) return;
    browseFiltersFor.current = { node: activeNodeId };
    setBrowseFilters(null);
    setBrowseFiltersError(null);
    void (async () => {
      try {
        setBrowseFilters(await mlxEngineBrowseFilters(activeNodeId));
      } catch (error) {
        setBrowseFiltersError(mlxErrorMessage(error, 'Could not load the filter vocabularies.'));
      }
    })();
  }, [tab, activeNodeId]);

  const setDownloadError = useCallback((repoId: string, message: string) => {
    setDownloadErrors((prev) => ({ ...prev, [repoId]: message }));
  }, []);

  const clearDownloadError = useCallback((repoId: string) => {
    setDownloadErrors((prev) => {
      if (!(repoId in prev)) return prev;
      const next = { ...prev };
      delete next[repoId];
      return next;
    });
  }, []);

  /**
   * Pull one download's real progress. "cancelled" DROPS the row — the backend deleted
   * its partial repo dir — and refreshes the local list so the dir's absence shows.
   */
  const syncProgress = useCallback(
    async (repoId: string, opts: { dropIfUntracked?: boolean } = {}) => {
      try {
        const progress = await mlxEngineDownloadProgress(repoId, activeNodeId);
        if (!progress) {
          if (opts.dropIfUntracked) {
            setDownloads((prev) => {
              const next = { ...prev };
              delete next[repoId];
              return next;
            });
          }
          return;
        }
        if (progress.state === 'cancelled') {
          setDownloads((prev) => {
            const next = { ...prev };
            delete next[repoId];
            return next;
          });
          refreshModels();
          return;
        }
        setDownloads((prev) => ({ ...prev, [repoId]: progress }));
        if (progress.state === 'done') refreshModels();
      } catch {
        // transient poll failure — keep the last real numbers rather than inventing any
      }
    },
    [refreshModels, activeNodeId]
  );

  const startDownload = useCallback(
    async (repoId: string) => {
      clearDownloadError(repoId);
      setDownloads((prev) => ({
        ...prev,
        [repoId]: { state: 'queued', totalBytes: 0, downloadedBytes: 0 },
      }));
      try {
        await mlxEngineDownload(repoId, activeNodeId);
      } catch (error) {
        setDownloads((prev) => {
          const next = { ...prev };
          delete next[repoId];
          return next;
        });
        setDownloadError(repoId, mlxErrorMessage(error, 'Download failed to start.'));
      }
    },
    [clearDownloadError, setDownloadError, activeNodeId]
  );

  const pauseDownload = useCallback(
    async (repoId: string) => {
      try {
        await mlxEngineDownloadPause(repoId, activeNodeId);
      } catch (error) {
        setDownloadError(repoId, mlxErrorMessage(error, 'Pause failed.'));
      }
      await syncProgress(repoId);
    },
    [setDownloadError, syncProgress, activeNodeId]
  );

  /** Also the entry point for UNTRACKED partial residue on disk (incomplete local models). */
  const resumeDownload = useCallback(
    async (repoId: string) => {
      clearDownloadError(repoId);
      setDownloads((prev) =>
        prev[repoId] != null
          ? prev
          : { ...prev, [repoId]: { state: 'queued', totalBytes: 0, downloadedBytes: 0 } }
      );
      try {
        await mlxEngineDownloadResume(repoId, activeNodeId);
      } catch (error) {
        setDownloadError(repoId, mlxErrorMessage(error, 'Resume failed.'));
      }
      // Real state replaces the optimistic entry; a refused untracked resume drops it.
      await syncProgress(repoId, { dropIfUntracked: true });
    },
    [clearDownloadError, setDownloadError, syncProgress, activeNodeId]
  );

  const cancelDownload = useCallback(
    async (repoId: string) => {
      try {
        await mlxEngineDownloadCancel(repoId, activeNodeId);
      } catch (error) {
        setDownloadError(repoId, mlxErrorMessage(error, 'Cancel failed.'));
        return;
      }
      // Paused/failed cancels delete synchronously — this sync sees "cancelled" and the
      // row disappears now. An active cancel stops between chunks; the 1s poll below
      // keeps following it until the backend reports "cancelled".
      await syncProgress(repoId, { dropIfUntracked: true });
    },
    [setDownloadError, syncProgress, activeNodeId]
  );

  const downloadHandlers = useMemo<DownloadHandlers>(
    () => ({
      onDownload: (repoId) => void startDownload(repoId),
      onPause: (repoId) => void pauseDownload(repoId),
      onResume: (repoId) => void resumeDownload(repoId),
      // A cancel deletes the partial from disk. On a remote device that is ANOTHER machine's
      // disk — confirm first, naming the device. Local stays immediate, byte-identical.
      onCancel: (repoId) => {
        if (remoteHostname) setPendingCancel(repoId);
        else void cancelDownload(repoId);
      },
    }),
    [startDownload, pauseDownload, resumeDownload, cancelDownload, remoteHostname]
  );

  /**
   * Deleting a model orphans any finished download row for it (caught live: a deleted
   * model's row kept saying "done" and the Download action never came back). Drop it.
   */
  const clearDownloadFor = useCallback(
    (repoId: string) => {
      setDownloads((prev) => {
        if (!(repoId in prev)) return prev;
        const next = { ...prev };
        delete next[repoId];
        return next;
      });
      clearDownloadError(repoId);
    },
    [clearDownloadError]
  );

  // Poll live downloads every second — real bytes, never a fake animation. This runs at
  // the shell so it survives tab switches; it stops only when nothing is active.
  const activeDownloadKey = useMemo(
    () =>
      Object.entries(downloads)
        .filter(([, p]) => p.state === 'queued' || p.state === 'downloading')
        .map(([id]) => id)
        .sort()
        .join('\n'),
    [downloads]
  );
  useEffect(() => {
    if (activeDownloadKey === '') return undefined;
    const repoIds = activeDownloadKey.split('\n');
    const timer = setInterval(() => {
      for (const repoId of repoIds) void syncProgress(repoId);
    }, 1000);
    return () => clearInterval(timer);
  }, [activeDownloadKey, syncProgress]);

  // First load AND every device switch. On a switch the previous device's data is dropped so
  // nothing stale reads as the new node's truth (TRUTH LAYER); a loading state shows until this
  // node's models + settings + status land. On first mount there is nothing to drop — the local
  // load is byte-identical to before.
  const nodeInitialized = useRef(false);
  useEffect(() => {
    if (nodeInitialized.current) {
      setNodeSwitching(true);
      setStatus(null);
      setSettings(null);
      setModels([]);
      setDisk(null);
      setStatusError(null);
      setMountError(null);
      setSaveError(null);
      setDownloads({});
      setDownloadErrors({});
      setPendingCancel(null);
      setMountModelId(null);
      setSamplingModelId(null);
      setProfileDrafts({});
      defaultedPicker.current = false;
      userPickedModel.current = false;
    }
    nodeInitialized.current = true;
    refreshModels();
    void (async () => {
      try {
        const next = await mlxEngineSettingsRead(activeNodeId);
        if (activeNodeRef.current !== activeNodeId) return; // switched away mid-flight — drop
        setSettings(next);
      } catch (error) {
        if (activeNodeRef.current !== activeNodeId) return;
        setSaveError(mlxErrorMessage(error, 'Could not read the engine settings.'));
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeNodeId]);

  // Picker follows truth: while the engine is running or mounting and the user has not
  // explicitly picked something else this visit, the picker shows the mounted model — so a
  // window opened onto an already-running engine reads "Mounted", never a stale "Mount".
  // An explicit user selection is never overridden. With the engine down, the picker defaults
  // once to the persisted model.
  useEffect(() => {
    if (userPickedModel.current) return;
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
  }, [status?.state, status?.modelId, settings?.modelId]);

  // Sampling picker default: the mounted model when running, else the last-mounted
  // settings.modelId, else the first complete local model. Once set (default, explicit
  // pick, or the Models-tab shortcut) it is never yanked from under the user.
  useEffect(() => {
    if (samplingModelId != null) return;
    const candidate =
      (status?.state === 'running' && status.modelId) ||
      settings?.modelId ||
      models.find((m) => m.complete)?.id ||
      null;
    if (candidate) setSamplingModelId(candidate);
  }, [samplingModelId, status?.state, status?.modelId, settings?.modelId, models]);

  const onMount = useCallback(() => {
    if (!mountModelId) return;
    void (async () => {
      setEngineBusy(true);
      setMountError(null);
      try {
        await mlxEngineMount(mountModelId, activeNodeId);
      } catch (error) {
        setMountError(mlxErrorMessage(error, 'Mount failed.'));
      } finally {
        setEngineBusy(false);
        void refreshStatus();
      }
    })();
  }, [mountModelId, refreshStatus, activeNodeId]);

  const onUnmount = useCallback(() => {
    void (async () => {
      setEngineBusy(true);
      setMountError(null);
      try {
        await mlxEngineUnmount(activeNodeId);
      } catch (error) {
        setMountError(mlxErrorMessage(error, 'Unmount failed.'));
      } finally {
        setEngineBusy(false);
        void refreshStatus();
      }
    })();
  }, [refreshStatus, activeNodeId]);

  const onRemount = useCallback(() => {
    const modelId = status?.modelId ?? settings?.modelId;
    if (!modelId) return;
    void (async () => {
      setEngineBusy(true);
      setMountError(null);
      try {
        await mlxEngineUnmount(activeNodeId);
        await mlxEngineMount(modelId, activeNodeId);
      } catch (error) {
        setMountError(mlxErrorMessage(error, 'Remount failed.'));
      } finally {
        setEngineBusy(false);
        void refreshStatus();
      }
    })();
  }, [status?.modelId, settings?.modelId, refreshStatus, activeNodeId]);

  const savedDraftsForSelected = useMemo(
    () =>
      settings && samplingModelId
        ? draftsFromProfile(settings.modelProfiles?.[samplingModelId])
        : null,
    [settings, samplingModelId]
  );
  const draftsForSelected =
    samplingModelId != null ? (profileDrafts[samplingModelId] ?? savedDraftsForSelected) : null;

  const setProfileDraft = useCallback(
    (key: NumericSettingKey, text: string) => {
      if (!samplingModelId || !settings) return;
      setProfileDrafts((prev) => {
        const base =
          prev[samplingModelId] ?? draftsFromProfile(settings.modelProfiles?.[samplingModelId]);
        return { ...prev, [samplingModelId]: { ...base, [key]: text } };
      });
    },
    [samplingModelId, settings]
  );

  const saveSettings = useCallback(
    async (next: MlxEngineSettings) => {
      const saved = await mlxEngineSettingsUpdate(next, activeNodeId);
      setSettings(saved);
      void refreshStatus();
    },
    [refreshStatus, activeNodeId]
  );

  const onSaveProfile = useCallback(() => {
    if (!settings || !samplingModelId) return;
    const drafts = profileDrafts[samplingModelId];
    if (!drafts) return;
    void (async () => {
      setSaving(true);
      setSaveError(null);
      try {
        await saveSettings(settingsWithProfile(settings, samplingModelId, drafts));
        // This model's edits are now the saved truth; other models keep their own drafts.
        setProfileDrafts((prev) => {
          const next = { ...prev };
          delete next[samplingModelId];
          return next;
        });
      } catch (error) {
        setSaveError(mlxErrorMessage(error, 'Could not save settings.'));
      } finally {
        setSaving(false);
      }
    })();
  }, [settings, samplingModelId, profileDrafts, saveSettings]);

  const openSamplingFor = useCallback((modelId: string) => {
    setSamplingModelId(modelId);
    setTab('sampling');
  }, []);

  // The page shell (MainPanelLayout, the Goose Swarm header, the top-level tab bar and the
  // scroll area) belongs to LeanZeroSwarmView — this component is the LeanZero MLX tab's content:
  // the engine sub-tabs (Engine / Models / Sampling) plus everything under them, unchanged.
  return (
    <div className="flex flex-col gap-4">
      {/* Device picker — only when there ARE peers (capability present + connected + a peer on
          the mesh). With no worker deployed the mesh is not connected, so this is hidden and the
          view behaves exactly as before, all ops on THIS device. */}
      {peers.length > 0 && (
        <div className="flex flex-wrap items-center gap-2">
          <span className={TYPE.meta}>Manage on</span>
          <DeviceTargetPicker
            targets={deviceTargets}
            value={targetNodeId}
            onChange={setTargetNodeId}
            disabled={engineBusy || nodeSwitching}
          />
          {nodeSwitching && (
            <Chip tone="warn" icon={<Loader2 className="animate-spin" />}>
              switching device
            </Chip>
          )}
        </div>
      )}
      {selectedPeer && (
        <ToneBanner
          tone="accent"
          label="Remote"
          text={`Managing models on ${selectedPeer.hostname} (remote)`}
        />
      )}

      <div className="flex flex-wrap items-center gap-3">
        <Segmented<MlxTab>
          aria-label="Engine sections"
          options={[
            { value: 'engine', label: 'Engine' },
            {
              value: 'models',
              label: (
                <>
                  Models
                  <span className={cx('text-lz-meta', TNUM)}>{models.length}</span>
                </>
              ),
            },
            { value: 'sampling', label: 'Sampling' },
          ]}
          value={tab}
          onChange={setTab}
        />
        {status && <StateBadge state={status.state} />}
        {/* pr-3: without it the ScrollArea's right edge shaved the final glyph off "Rapid-MLX"
            (caught live on the packaged build, 2026-08-31). */}
        <span className={cx('ml-auto shrink-0 pr-3', TYPE.meta)}>Powered by Rapid-MLX</span>
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
        />
      )}
      {tab === 'models' && (
        <ModelsSection
          settings={settings}
          models={models}
          disk={disk}
          mountedModelId={status?.modelId ?? null}
          refreshModels={refreshModels}
          saveSettings={saveSettings}
          onOpenSampling={openSamplingFor}
          downloads={downloads}
          downloadErrors={downloadErrors}
          downloadHandlers={downloadHandlers}
          onModelDeleted={clearDownloadFor}
          filters={browseFilters}
          filtersError={browseFiltersError}
          nodeId={activeNodeId}
          remoteHostname={remoteHostname}
        />
      )}
      {tab === 'sampling' && (
        <SamplingSection
          status={status}
          settings={settings}
          engineBusy={engineBusy}
          onRemount={onRemount}
          models={models}
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

      {/* Cancelling a remote download deletes the partial from THAT device's disk — a destructive
          op on another machine, so it names the device and asks first (a local cancel does not). */}
      <ConfirmationModal
        isOpen={pendingCancel !== null}
        title="Cancel download"
        message={
          pendingCancel
            ? `Cancel the download of ${pendingCancel}${
                remoteHostname ? ` on ${remoteHostname}` : ''
              } and delete its partial files from ${
                remoteHostname ? "that device's disk" : 'disk'
              }?`
            : ''
        }
        confirmLabel="Cancel download"
        cancelLabel="Keep"
        confirmVariant="destructive"
        onConfirm={() => {
          if (pendingCancel) void cancelDownload(pendingCancel);
          setPendingCancel(null);
        }}
        onCancel={() => setPendingCancel(null)}
      />
    </div>
  );
};

export default MlxEngineView;
