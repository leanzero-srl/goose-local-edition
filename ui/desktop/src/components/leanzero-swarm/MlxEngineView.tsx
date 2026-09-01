import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Check,
  ChevronDown,
  Download,
  Folder,
  HardDrive,
  Laptop,
  Loader2,
  Pencil,
  Play,
  RefreshCw,
  Search,
  SlidersHorizontal,
  Square,
  Trash2,
  X,
} from 'lucide-react';
import { Button } from '../ui/button';
import { Input } from '../ui/input';
import { Select } from '../ui/Select';
import { ConfirmationModal } from '../ui/ConfirmationModal';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '../ui/dialog';
import { errorMessage } from '../../utils/conversionUtils';
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
import {
  AMBER,
  AZURE,
  Chip,
  DownloadProgressRow,
  GREEN,
  INK_DARK,
  RED,
  SLATE,
  SolidBanner,
  formatCount,
  formatDate,
  formatGb,
} from './primitives';
import { FilterCombobox } from './FilterCombobox';
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

// The ONE accent for every active tab and segment. The node ramp is node identity, never
// chrome — node-5 pink on a tab was the hot-pink bug. This window also runs in builds without
// `.local-edition`, where a bare var() resolves to NOTHING and a solid fill silently turns
// transparent (caught live 2026-08-31: the active tab label vanished); the token carries its
// fallback.
const SEGMENT_ACTIVE = AZURE;

// Engine state on the status triad: running = ok, mounting and stopped = the stopped neutral
// (mounting keeps its spinner, which is the difference), failed = err.
const STATE_COLOR: Record<MlxEngineState, string> = {
  running: GREEN,
  mounting: SLATE,
  failed: RED,
  stopped: SLATE,
};

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
// Small solid building blocks (Chip/SolidBanner/formatters live in ./primitives)
// ---------------------------------------------------------------------------

function StateBadge({ state }: { state: MlxEngineState }) {
  return (
    <span
      data-testid="mlx-state-badge"
      className="inline-flex items-center gap-1.5 rounded px-2 py-0.5 text-[11px] font-semibold text-white"
      style={{ backgroundColor: STATE_COLOR[state] }}
    >
      {state === 'mounting' && <Loader2 className="h-3 w-3 animate-spin" />}
      {state}
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
    <SolidBanner
      color={AMBER}
      label="Restart required"
      text="Settings changed — remount to apply."
      action={
        <Button
          size="sm"
          onClick={onRemount}
          disabled={engineBusy || !(status.modelId ?? settings?.modelId)}
          className="shrink-0 rounded font-bold text-white hover:opacity-90"
          style={{ backgroundColor: INK_DARK }}
        >
          <RefreshCw className="w-3.5 h-3.5" />
          Remount
        </Button>
      }
    />
  );
}

/** Strong memory bar: solid azure used-fill on a bordered track, bold numbers beside it. */
function MemoryBar({ availableGb, totalGb }: { availableGb: number; totalGb: number }) {
  const usedGb = Math.max(0, totalGb - availableGb);
  const pct = totalGb > 0 ? Math.min(100, (usedGb / totalGb) * 100) : 0;
  const tight = totalGb > 0 && availableGb / totalGb < 0.15;
  return (
    <div className="flex min-w-0 items-center gap-3">
      <div
        className="h-2.5 flex-1 overflow-hidden rounded border border-border-primary"
        role="progressbar"
        aria-valuenow={Math.round(pct)}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-label="Unified memory in use"
      >
        <div
          className="h-full"
          style={{ width: `${pct}%`, backgroundColor: tight ? AMBER : AZURE }}
        />
      </div>
      <span
        className="shrink-0 text-xs font-bold tabular-nums"
        style={{ color: tight ? AMBER : AZURE }}
      >
        {availableGb.toFixed(1)} GB free
      </span>
      <span className="shrink-0 text-xs tabular-nums text-text-secondary">
        of {totalGb.toFixed(1)} GB
      </span>
    </div>
  );
}

/**
 * Disk space on the models dir's volume, styled like the Engine tab's memory bar: solid
 * used-fill on a bordered track, bold "{free} free of {total}" beside it. Numbers come
 * from the modelsList response (statvfs), never fabricated.
 */
function DiskBar({ availableBytes, totalBytes }: { availableBytes: number; totalBytes: number }) {
  const usedBytes = Math.max(0, totalBytes - availableBytes);
  const pct = totalBytes > 0 ? Math.min(100, (usedBytes / totalBytes) * 100) : 0;
  const tight = totalBytes > 0 && availableBytes / totalBytes < 0.1;
  return (
    <div className="flex min-w-0 items-center gap-3" data-testid="mlx-disk-bar">
      <HardDrive
        className="h-4 w-4 shrink-0 text-text-secondary"
        style={tight ? { color: AMBER } : undefined}
      />
      <div
        className="h-2.5 flex-1 overflow-hidden rounded border border-border-primary"
        role="progressbar"
        aria-valuenow={Math.round(pct)}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-label="Disk space used on the models volume"
      >
        <div
          className="h-full"
          style={{ width: `${pct}%`, backgroundColor: tight ? AMBER : AZURE }}
        />
      </div>
      <span
        className="shrink-0 text-xs font-bold tabular-nums"
        style={{ color: tight ? AMBER : AZURE }}
      >
        {formatGb(availableBytes)} free
      </span>
      <span className="shrink-0 text-xs tabular-nums text-text-secondary">
        of {formatGb(totalBytes)}
      </span>
    </div>
  );
}

// Benchmark-style card scaffolding: full border, bg-background-secondary header strip with
// a micro-caps label, optional footer strip in mono for captions.

function Card({ children }: { children: React.ReactNode }) {
  return <div className="overflow-hidden rounded border border-border-primary">{children}</div>;
}

function CardHeader({
  label,
  right,
  children,
}: {
  label: string;
  right?: React.ReactNode;
  children?: React.ReactNode;
}) {
  return (
    <div className="flex flex-wrap items-center gap-2 border-b border-border-primary bg-background-secondary px-3 py-2">
      <span className="text-[11px] font-semibold uppercase tracking-[0.08em] text-text-secondary">
        {label}
      </span>
      {children}
      {right != null && <span className="ml-auto flex items-center gap-2">{right}</span>}
    </div>
  );
}

function CardFooter({ children }: { children: React.ReactNode }) {
  return (
    <div className="border-t border-border-primary bg-background-secondary px-3 py-2 font-mono text-[11px] text-text-secondary">
      {children}
    </div>
  );
}

/** Segmented control in the benchmark register: bordered strip, solid active fill. */
function Segmented<T extends string>({
  options,
  value,
  onChange,
  activeColor = SEGMENT_ACTIVE,
  disabled,
}: {
  options: Array<{ value: T; label: React.ReactNode; title?: string }>;
  value: T;
  onChange: (v: T) => void;
  activeColor?: string;
  disabled?: boolean;
}) {
  return (
    <div className="flex self-start overflow-hidden rounded border border-border-primary">
      {options.map((opt) => {
        const active = opt.value === value;
        return (
          <button
            key={opt.value}
            type="button"
            onClick={() => onChange(opt.value)}
            disabled={disabled}
            aria-pressed={active}
            title={opt.title}
            className={`flex items-center gap-2 px-3 py-1.5 text-sm font-bold transition-colors ${
              active
                ? 'text-white'
                : 'bg-background-secondary text-text-secondary hover:text-text-primary'
            }`}
            style={active ? { backgroundColor: activeColor } : undefined}
          >
            {opt.label}
          </button>
        );
      })}
    </div>
  );
}

/**
 * Labeled numeric stepper with an honest "engine default" state: blank field = the key is
 * omitted and the engine's own default applies; a typed 0 is sent as 0. The chip states
 * which of the two is true right now.
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
  const stepBtn =
    'h-7 w-7 flex items-center justify-center rounded border border-border-primary text-text-secondary hover:text-text-primary hover:border-text-secondary transition-colors leading-none text-sm';
  const bump = (dir: 1 | -1) => {
    const base = isSet && !Number.isNaN(Number(text)) ? Number(text) : 0;
    let next = base + dir * spec.step;
    if (spec.integer) next = Math.round(next);
    // Float steps accumulate representation noise (0.1 + 0.05 = 0.15000000000000002).
    const rounded = spec.integer ? next : Number(next.toFixed(4));
    onText(String(rounded));
  };
  return (
    <div className="flex min-w-0 flex-col gap-1">
      <div className="flex items-center justify-between gap-2">
        <span className="truncate text-xs font-medium text-text-primary">{spec.label}</span>
        {isSet ? (
          <button
            type="button"
            onClick={() => onText('')}
            className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px] font-semibold text-white hover:opacity-90"
            style={{ backgroundColor: AZURE }}
            title="Clear — fall back to the engine default"
          >
            set <X className="w-2.5 h-2.5" />
          </button>
        ) : (
          <Chip quiet title="No value sent — the engine uses its own default">
            engine default
          </Chip>
        )}
      </div>
      <div className="flex items-center gap-1.5">
        <button
          type="button"
          className={stepBtn}
          aria-label={`Decrease ${spec.label}`}
          onClick={() => bump(-1)}
        >
          −
        </button>
        <Input
          type="number"
          step={spec.step}
          value={text}
          placeholder="engine default"
          onChange={(e) => onText(e.target.value)}
          className="h-7 rounded text-right text-sm tabular-nums"
          aria-label={spec.label}
        />
        <button
          type="button"
          className={stepBtn}
          aria-label={`Increase ${spec.label}`}
          onClick={() => bump(1)}
        >
          +
        </button>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Model pickers — the app's custom react-select wrapper, never a native <select>.
// ---------------------------------------------------------------------------

interface ModelOption {
  value: string;
  label: string;
  model: MlxLocalModel;
}

function ModelOptionLabel({ option }: { option: ModelOption }) {
  return (
    <span className="flex min-w-0 items-center gap-2">
      <span className="truncate font-mono text-sm">{option.model.id}</span>
      <span className="shrink-0 text-xs tabular-nums text-text-secondary">
        {formatGb(option.model.sizeBytes)}
      </span>
      {!option.model.complete && (
        <Chip color={AMBER} ink={INK_DARK}>
          partial download
        </Chip>
      )}
    </span>
  );
}

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
  }));
  const selected = options.find((o) => o.value === value) ?? null;
  return (
    <Select
      options={options}
      value={selected}
      isDisabled={disabled}
      placeholder={
        models.length === 0 ? 'No models in the models folder yet' : 'Pick a model to mount'
      }
      isOptionDisabled={(o) => !(o as ModelOption).model.complete}
      formatOptionLabel={(o) => <ModelOptionLabel option={o as ModelOption} />}
      onChange={(o) => onChange(o ? (o as ModelOption).value : null)}
      isClearable
    />
  );
}

interface ProfileModelOption {
  value: string;
  label: string;
  local: boolean;
  hasProfile: boolean;
}

function ProfileModelOptionLabel({ option }: { option: ProfileModelOption }) {
  return (
    <span className="flex min-w-0 items-center gap-2">
      <span className="truncate font-mono text-sm">{option.value}</span>
      {option.hasProfile && <Chip quiet>profile</Chip>}
      {!option.local && (
        <Chip color={SLATE} title="A saved profile for a model that is not in the models folder">
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
    <Select
      aria-label="Sampling model"
      options={options}
      value={selected}
      placeholder={options.length === 0 ? 'No local models yet' : 'Pick a model to tune'}
      formatOptionLabel={(o) => <ProfileModelOptionLabel option={o as ProfileModelOption} />}
      onChange={(o) => onChange(o ? (o as ProfileModelOption).value : null)}
    />
  );
}

// ---------------------------------------------------------------------------
// ENGINE tab
// ---------------------------------------------------------------------------

function StatusFact({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex min-w-0 flex-col gap-0.5">
      <span className="text-[11px] font-medium text-text-secondary">{label}</span>
      <span className="min-w-0 truncate text-sm text-text-primary">{children}</span>
    </div>
  );
}

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

  return (
    <div className="flex flex-col gap-4 pb-8">
      {statusError && <SolidBanner color={RED} label="Engine unreachable" text={statusError} />}
      {status?.gateMessage && status.gateVerdict === 'block' && (
        <SolidBanner color={RED} label="Mount blocked" text={status.gateMessage} />
      )}
      {status?.gateMessage && status.gateVerdict === 'warn' && (
        <SolidBanner color={AMBER} label="Memory pressure" text={status.gateMessage} />
      )}
      {strayPort != null && (
        <SolidBanner
          color={AMBER}
          label="Unsupervised engine"
          text={`unsupervised engine on port ${strayPort} — Unmount reclaims it`}
        />
      )}
      {mountError && <SolidBanner color={RED} label="Mount failed" text={mountError} />}
      {status?.state === 'failed' && status.lastError && status.lastError !== mountError && (
        <SolidBanner color={RED} label="Engine failed" text={status.lastError} />
      )}
      <RestartRequiredBanner
        status={status}
        settings={settings}
        engineBusy={engineBusy}
        onRemount={onRemount}
      />

      {/* Status card */}
      <Card>
        <CardHeader label="Engine">
          {status ? <StateBadge state={status.state} /> : <Chip color={SLATE}>loading</Chip>}
          {status?.modelId ? (
            <span className="truncate font-mono text-sm font-semibold text-text-primary">
              {status.modelId}
            </span>
          ) : (
            <span className="text-sm text-text-secondary">no model mounted</span>
          )}
          {status?.toolCallParser && (
            <Chip quiet title="Tool-call parser reported by the live engine">
              {status.toolCallParser}
            </Chip>
          )}
        </CardHeader>
        <div className="flex flex-col gap-4 px-3 py-3">
          <div className="grid grid-cols-2 gap-x-6 gap-y-3 md:grid-cols-4">
            <StatusFact label="Context window">
              {status?.contextWindow != null ? (
                <span className="font-semibold tabular-nums text-text-primary">
                  {status.contextWindow.toLocaleString()}
                </span>
              ) : (
                <span className="text-text-secondary">—</span>
              )}
            </StatusFact>
            <StatusFact label="PID">
              {status?.pid != null ? (
                <span className="font-mono tabular-nums">{status.pid}</span>
              ) : (
                <span className="text-text-secondary">—</span>
              )}
            </StatusFact>
            <StatusFact label="Base URL">
              {status?.baseUrl ? (
                <span className="font-mono text-xs">{status.baseUrl}</span>
              ) : (
                <span className="text-text-secondary">—</span>
              )}
            </StatusFact>
            <StatusFact label="Port (configured)">
              {settings ? (
                <span className="font-mono tabular-nums">{settings.port}</span>
              ) : (
                <span className="text-text-secondary">—</span>
              )}
            </StatusFact>
          </div>

          {status?.probeError && (
            <div className="break-words text-xs font-semibold" style={{ color: RED }}>
              Probe failed: {status.probeError}
            </div>
          )}

          {status && (
            <MemoryBar availableGb={status.availableMemoryGb} totalGb={status.totalMemoryGb} />
          )}
        </div>
      </Card>

      {/* Mount controls */}
      <Card>
        <CardHeader label="Mount a model" />
        {/* flex-wrap + a min width on the picker: at ~800px the two buttons otherwise
            crushed the model picker into unreadability. */}
        <div className="flex flex-wrap items-start gap-2 px-3 py-3">
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
            <Button
              disabled
              className="rounded font-bold text-white"
              style={{ backgroundColor: SLATE }}
            >
              <Loader2 className="w-4 h-4 animate-spin" />
              Mounting
            </Button>
          ) : selectionIsMounted ? (
            <Button
              disabled
              className="rounded font-bold text-white"
              style={{ backgroundColor: GREEN }}
            >
              <Check className="w-4 h-4" />
              Mounted
            </Button>
          ) : state === 'running' ? (
            <Button
              onClick={onMount}
              disabled={!canSwitch}
              className="rounded font-bold text-white hover:opacity-90"
              style={{ backgroundColor: AZURE }}
            >
              <Play className="w-4 h-4" />
              Switch model
            </Button>
          ) : (
            <Button
              onClick={onMount}
              disabled={!canMount}
              className="rounded font-bold text-white hover:opacity-90"
              style={{ backgroundColor: AZURE }}
            >
              <Play className="w-4 h-4" />
              Mount
            </Button>
          )}
          <Button
            onClick={onUnmount}
            disabled={!canUnmount}
            variant="outline"
            className="rounded font-bold"
          >
            <Square className="w-4 h-4" />
            Unmount
          </Button>
        </div>
        <CardFooter>
          Mount returns immediately and the engine flips to mounting; the card above follows the
          live status every 2 seconds. Each model mounts with its own sampling profile.
        </CardFooter>
      </Card>

      {/* Spawn command — visible, not editable here: the owner sees exactly what would run. */}
      {settings && (
        <Card>
          <CardHeader label="Spawn command" />
          <div className="px-3 py-2.5">
            <span className="break-all font-mono text-xs text-text-primary">
              {settings.spawnCommand.join(' ')}
            </span>
          </div>
        </Card>
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
        <span className="text-sm text-text-secondary">
          Sampling is PER MODEL: each model mounts with the flags from its own profile, and
          per-request values sent by goose override them.
        </span>
        <span className="text-sm">
          {status?.modelId ? (
            <>
              <span className="text-text-secondary">Currently mounted: </span>
              <span className="font-mono font-semibold text-text-primary">{status.modelId}</span>
            </>
          ) : (
            <span className="text-text-secondary">no model mounted</span>
          )}
        </span>
      </div>

      <Card>
        <CardHeader
          label="Model profile"
          right={
            <>
              {dirty && (
                <Chip color={AMBER} ink={INK_DARK}>
                  unsaved
                </Chip>
              )}
              <Button
                size="sm"
                onClick={onSaveSettings}
                disabled={!dirty || saving || !settings || !selectedModelId}
                className="rounded font-bold text-white hover:opacity-90"
                style={{ backgroundColor: AZURE }}
              >
                {saving ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : null}
                Save
              </Button>
            </>
          }
        >
          {selectedIsMounted && <Chip color={GREEN}>mounted</Chip>}
        </CardHeader>
        <div className="flex flex-col gap-4 px-3 py-3">
          <div className="max-w-2xl">
            <SamplingModelPicker
              models={models}
              profileIds={profileIds}
              value={selectedModelId}
              onChange={onSelectModel}
            />
          </div>
          {saveError && <SolidBanner color={RED} label="Save failed" text={saveError} />}
          {!settings ? (
            <span className="text-sm text-text-secondary">Loading settings…</span>
          ) : !selectedModelId || !drafts ? (
            <span className="text-sm text-text-secondary">
              Pick a model above to edit its sampling profile.
            </span>
          ) : (
            <>
              <div className="grid grid-cols-2 gap-x-6 gap-y-4 md:grid-cols-3">
                {SAMPLING_FIELDS.map((spec) => (
                  <NumericField
                    key={spec.key}
                    spec={spec}
                    text={drafts[spec.key]}
                    onText={(v) => setDraft(spec.key, v)}
                  />
                ))}
              </div>
              <div className="max-w-xs">
                <NumericField
                  spec={CONTEXT_LIMIT_FIELD}
                  text={drafts.contextLimit}
                  onText={(v) => setDraft('contextLimit', v)}
                />
              </div>
            </>
          )}
        </div>
        <CardFooter>
          A blank field sends nothing — the engine keeps its own default. Profiles apply at mount,
          per model: saving never touches a live process, and the status reports restart required
          until the mounted model is remounted.
        </CardFooter>
      </Card>
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
        <Input
          value={value}
          onChange={(e) => setValue(e.target.value)}
          className="rounded font-mono text-sm"
          placeholder="/path/to/mlx-models"
          aria-label="Models folder path"
        />
        {error && <SolidBanner color={RED} label="Save failed" text={error} />}
        <DialogFooter>
          <Button variant="outline" onClick={onClose} disabled={saving}>
            Cancel
          </Button>
          <Button
            onClick={() => onSave(value.trim())}
            disabled={saving || value.trim() === ''}
            className="rounded font-bold text-white hover:opacity-90"
            style={{ backgroundColor: AZURE }}
          >
            {saving ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : null}
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

/** The browser's column template — ONE grid shared by the header row and every hit so the
 *  attributes line up as a table, not a pile of chips. The id keeps a 220px floor (at narrow
 *  widths the old chip cluster squeezed it to "lmstudi…", caught live 2026-08-31); the list
 *  scrolls sideways under that width instead of squeezing. */
const HIT_COLUMNS = 'minmax(220px, 1fr) 132px 48px 80px 60px 64px 48px 88px 100px';
const HIT_CELL = 'min-w-0 truncate text-[11px] tabular-nums text-text-secondary';
const HIT_NUM = `${HIT_CELL} text-right`;

function BrowseHeaderRow() {
  return (
    <div
      className="grid items-center gap-x-3 border-t border-border-primary px-3 py-1.5"
      style={{ gridTemplateColumns: HIT_COLUMNS }}
      data-testid="mlx-browse-header"
    >
      <span className={HIT_CELL}>Model</span>
      <span className={HIT_CELL}>Publisher</span>
      <span className={HIT_CELL}>Quant</span>
      <span className={HIT_CELL}>Arch</span>
      <span className={HIT_NUM}>Size</span>
      <span className={HIT_NUM}>Downloads</span>
      <span className={HIT_NUM}>Likes</span>
      <span className={HIT_NUM}>Created</span>
      <span />
    </div>
  );
}

interface BrowseHitRowProps {
  hit: MlxBrowseHit;
  sort: MlxBrowseSort;
  progress: MlxDownloadProgress | undefined;
  startError: string | undefined;
  handlers: DownloadHandlers;
  onOpenCard: () => void;
}

function BrowseHitRow({
  hit,
  sort,
  progress,
  startError,
  handlers,
  onOpenCard,
}: BrowseHitRowProps) {
  return (
    <div
      className="cursor-pointer border-t border-border-primary transition-colors hover:bg-background-secondary"
      onClick={onOpenCard}
      role="button"
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === 'Enter' && e.target === e.currentTarget) onOpenCard();
      }}
      aria-label={`Open model card for ${hit.id}`}
    >
      {/* Every attribute is an aligned, tabular, secondary-text column; the ONE coloured element
          on a row is its primary action. "—" states an absent value — never a guessed one. */}
      <div className="grid items-center gap-x-3 px-3 py-2" style={{ gridTemplateColumns: HIT_COLUMNS }}>
        <span className="min-w-0 truncate font-mono text-[13px] font-medium text-text-primary" title={hit.id}>
          {hit.id}
        </span>
        <span className={HIT_CELL} title={`Published by ${hit.author}`}>
          {hit.author}
        </span>
        <span className={HIT_CELL} title={hit.quant ? "Derived from the repo's tags or name" : undefined}>
          {hit.quant ?? '—'}
        </span>
        <span className={HIT_CELL} title={hit.arch ? "Derived from the repo's tags or name" : undefined}>
          {hit.arch ?? '—'}
        </span>
        <span
          className={HIT_NUM}
          title={
            hit.sizeBytesEstimate != null
              ? 'estimated from tensor dtypes; exact size on the model card'
              : 'no size estimate for this repo'
          }
        >
          {hit.sizeBytesEstimate != null ? `~${formatGb(hit.sizeBytesEstimate)}` : '—'}
        </span>
        <span className={HIT_NUM} title={`${hit.downloads.toLocaleString()} downloads`}>
          {formatCount(hit.downloads)}
        </span>
        <span className={HIT_NUM} title={`${hit.likes.toLocaleString()} likes`}>
          {formatCount(hit.likes)}
        </span>
        <span
          className={`${HIT_NUM} ${sort === 'newest' ? 'font-semibold text-text-primary' : ''}`}
          title={hit.createdAt ? `Created ${hit.createdAt}` : undefined}
        >
          {hit.createdAt ? formatDate(hit.createdAt) : '—'}
        </span>
        <span className="flex justify-end">
          {!progress && (
            <Button
              size="xs"
              onClick={(e) => {
                e.stopPropagation();
                handlers.onDownload(hit.id);
              }}
              className="shrink-0 rounded font-semibold text-white hover:opacity-90"
              style={{ backgroundColor: AZURE }}
              aria-label={`Download ${hit.id}`}
            >
              <Download className="w-3 h-3" />
              Download
            </Button>
          )}
        </span>
      </div>
      {startError && (
        <div className="break-words px-3 pb-2 text-xs font-semibold" style={{ color: RED }}>
          {startError}
        </div>
      )}
      {progress && (
        <div className="px-3 pb-2">
          <DownloadProgressRow
            repoId={hit.id}
            progress={progress}
            onPause={() => handlers.onPause(hit.id)}
            onResume={() => handlers.onResume(hit.id)}
            onCancel={() => handlers.onCancel(hit.id)}
          />
        </div>
      )}
    </div>
  );
}

/**
 * The Hugging Face browser's state, LIFTED into ModelsSection so the [Hugging Face | Downloaded]
 * sub-tab switch can unmount the browser's markup without losing query/filters/hits/pagination.
 * It still resets when the Models tab itself is left — exactly the pre-split behavior.
 */
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
        setError(errorMessage(e, 'Hugging Face browse failed.'));
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
        setError(errorMessage(e, 'Loading the next page failed.'));
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

  return (
    <Card>
      <CardHeader
        label="Hugging Face — MLX models"
        right={
          <>
            {loading && (
              <Chip color={SLATE}>
                <Loader2 className="h-2.5 w-2.5 animate-spin" />
                loading
              </Chip>
            )}
            <Chip quiet>{hits?.length ?? 0} loaded</Chip>
          </>
        }
      />
      <div className="flex flex-col gap-2.5 px-3 py-3">
        <div className="flex flex-wrap items-center gap-2">
          <div className="flex min-w-[220px] flex-1 items-center gap-2">
            <Input
              value={queryText}
              onChange={(e) => setQueryText(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') commitQuery();
              }}
              placeholder="Search MLX models by name…"
              className="rounded text-sm"
              aria-label="Search Hugging Face"
            />
            <Button
              onClick={commitQuery}
              className="shrink-0 rounded font-bold text-white hover:opacity-90"
              style={{ backgroundColor: AZURE }}
              aria-label="Search"
            >
              <Search className="w-4 h-4" />
            </Button>
          </div>
          <Segmented<MlxBrowseSort>
            options={[
              { value: 'downloads', label: 'Top downloads', title: 'Most downloaded first' },
              { value: 'newest', label: 'Latest', title: 'Newest first (created date)' },
            ]}
            value={sort}
            onChange={setSort}
          />
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <FilterCombobox
            label="Provider"
            value={author}
            options={filters?.authors ?? []}
            onChange={setAuthor}
            chipColor={AZURE}
          />
          <FilterCombobox
            label="Quant"
            value={quant}
            options={filters?.quants ?? []}
            onChange={setQuant}
            chipColor={AZURE}
          />
          <FilterCombobox
            label="Arch"
            value={arch}
            options={filters?.archs ?? []}
            onChange={setArch}
            chipColor={AZURE}
          />
          {filters?.refreshError != null && (
            <Chip
              color={AMBER}
              ink={INK_DARK}
              title={`Vocabulary refresh failed — serving the previous crawl. ${filters.refreshError}`}
            >
              vocabulary may be stale
            </Chip>
          )}
          {filtersError != null && (
            <Chip color={AMBER} ink={INK_DARK} title={filtersError}>
              filter vocabulary unavailable — free text still works
            </Chip>
          )}
        </div>
        {error && <SolidBanner color={RED} label="Browse failed" text={error} />}
      </div>

      {hits != null && hits.length === 0 && !loading && !error && (
        <div className="border-t border-border-primary px-3 py-3 text-sm text-text-secondary">
          No MLX models match these filters.
        </div>
      )}
      {hits != null && hits.length > 0 && (
        <div className="overflow-x-auto">
          <div className="min-w-[940px]">
            <BrowseHeaderRow />
            {hits.map((hit) => (
              <BrowseHitRow
                key={hit.id}
                hit={hit}
                sort={sort}
                progress={downloads[hit.id]}
                startError={downloadErrors[hit.id]}
                handlers={handlers}
                onOpenCard={() => onOpenCard(hit.id)}
              />
            ))}
          </div>
        </div>
      )}
      {nextCursor != null && !loading && (
        <button
          type="button"
          onClick={loadMore}
          disabled={loadingMore}
          className="flex w-full items-center justify-center gap-2 border-t border-border-primary bg-background-secondary py-2.5 text-sm font-semibold text-text-secondary transition-colors hover:text-text-primary disabled:opacity-60"
        >
          {loadingMore ? <Loader2 className="h-4 w-4 animate-spin" /> : null}
          Load more
        </button>
      )}
      <CardFooter>
        Filters match Hugging Face tags server-side
        {filters != null
          ? ` — vocabularies sampled live from ${filters.sampledRepos} MLX repos; type in a filter to search them, or apply any free text.`
          : ' — type in a filter to search its vocabulary, or apply any free text.'}{' '}
        A model whose quant appears only in its name is excluded by those filters but still findable
        via search. Click a row for its full model card.
      </CardFooter>
    </Card>
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
    <Card>
      <CardHeader label="Active downloads">
        <Chip quiet>{entries.length}</Chip>
      </CardHeader>
      <div>
        {entries.map(([repoId, progress]) => (
          <div key={repoId} className="border-t border-border-primary px-3 py-2.5 first:border-t-0">
            <span className="min-w-0 truncate font-mono text-sm text-text-primary">{repoId}</span>
            {errors[repoId] && (
              <div className="mt-1 break-words text-xs font-semibold" style={{ color: RED }}>
                {errors[repoId]}
              </div>
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
      </div>
    </Card>
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
        setDirError(errorMessage(error, 'Could not save the models folder.'));
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
      setDeleteError(errorMessage(error, `Could not delete ${pendingDelete.id}.`));
    } finally {
      setDeleting(false);
    }
  }, [pendingDelete, refreshModels, onModelDeleted, nodeId]);

  return (
    <div className="flex flex-col gap-4 pb-8">
      {/* Second-level switch (owner): the browser and the local library are separate tabs so
          neither buries the other. Styled like every other segmented control in this view. */}
      <Segmented<ModelsSubTab>
        options={[
          { value: 'hf', label: 'Hugging Face', title: 'Browse and download MLX models' },
          {
            value: 'downloaded',
            label: (
              <>
                Downloaded
                <span className="text-[11px] font-medium tabular-nums">{models.length}</span>
              </>
            ),
            title: 'Local models, the models folder and disk space',
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
          <Card>
            <CardHeader
              label="Downloaded models"
              right={
                <Button size="xs" variant="outline" onClick={refreshModels} className="rounded">
                  <RefreshCw className="w-3 h-3" />
                  Refresh
                </Button>
              }
            >
              <Chip quiet>{models.length}</Chip>
            </CardHeader>
            {deleteError && (
              <div className="px-3 pt-3">
                <SolidBanner color={RED} label="Delete failed" text={deleteError} />
              </div>
            )}
            {models.length === 0 ? (
              <div className="px-3 py-3 text-sm text-text-secondary">
                Nothing downloaded yet — browse the Hugging Face tab.
              </div>
            ) : (
              <div>
                {models.map((model) => {
                  const incomplete = model.missingFiles > 0 || !model.complete;
                  return (
                    <div
                      key={model.id}
                      className="border-t border-border-primary px-3 py-2.5 first:border-t-0"
                    >
                      {/* Same wrap pattern as browse rows: the id keeps a readable floor and the
                      chip/action cluster wraps under it at narrow widths. */}
                      <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1">
                        <HardDrive
                          className="w-4 h-4 shrink-0 text-text-secondary"
                          style={incomplete ? { color: AMBER } : undefined}
                        />
                        <span className="min-w-[220px] flex-1 truncate font-mono text-sm text-text-primary">
                          {model.id}
                        </span>
                        {model.id === mountedModelId && <Chip color={GREEN}>mounted</Chip>}
                        {incomplete && (
                          <Chip
                            color={AMBER}
                            ink={INK_DARK}
                            title="Files the repo's safetensors index names are absent or unfinished — Resume continues the download"
                          >
                            incomplete — missing {model.missingFiles} file(s)
                          </Chip>
                        )}
                        <span className="shrink-0 text-xs tabular-nums text-text-secondary">
                          {formatGb(model.sizeBytes)}
                        </span>
                        {incomplete ? (
                          <Button
                            size="xs"
                            onClick={() => downloadHandlers.onResume(model.id)}
                            className="shrink-0 rounded font-bold text-white hover:opacity-90"
                            style={{ backgroundColor: AZURE }}
                            aria-label={`Resume ${model.id}`}
                            title="Resume the download — complete files are skipped, partials continue"
                          >
                            <Play className="w-3 h-3" />
                            Resume
                          </Button>
                        ) : (
                          <Button
                            size="xs"
                            onClick={() => onOpenSampling(model.id)}
                            variant="outline"
                            className="shrink-0 rounded font-bold"
                            aria-label={`Sampling for ${model.id}`}
                            title="This model's sampling profile — opens the Sampling tab"
                          >
                            <SlidersHorizontal className="w-3 h-3" />
                            Sampling
                          </Button>
                        )}
                        <Button
                          size="xs"
                          onClick={() => {
                            setDeleteError(null);
                            setPendingDelete(model);
                          }}
                          variant="outline"
                          className="shrink-0 rounded font-bold"
                          style={{ borderColor: RED, color: RED }}
                          aria-label={`Delete ${model.id}`}
                        >
                          <Trash2 className="w-3 h-3" />
                        </Button>
                      </div>
                      {downloadErrors[model.id] && (
                        <div
                          className="mt-1 break-words text-xs font-semibold"
                          style={{ color: RED }}
                        >
                          {downloadErrors[model.id]}
                        </div>
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
                  );
                })}
              </div>
            )}
          </Card>

          {/* Models folder + disk — the local library's home, so it lives on the Downloaded tab. */}
          <Card>
            <CardHeader label="Models folder" />
            <div className="flex flex-col gap-2 px-3 py-3">
              <div className="flex items-center gap-2">
                <Folder className="w-4 h-4 shrink-0 text-text-secondary" />
                <span
                  className="min-w-0 flex-1 truncate rounded border border-border-primary bg-background-secondary px-2.5 py-1.5 font-mono text-sm text-text-primary"
                  title={settings?.modelsDir}
                >
                  {settings?.modelsDir ?? '…'}
                </span>
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => {
                    setDirError(null);
                    setDirDialogOpen(true);
                  }}
                  disabled={!settings}
                  className="rounded"
                >
                  <Pencil className="w-3.5 h-3.5" />
                  Edit
                </Button>
              </div>
              {disk && (
                <DiskBar availableBytes={disk.availableBytes} totalBytes={disk.totalBytes} />
              )}
            </div>
            <CardFooter>
              One directory used by downloads and mounts alike; the bar is the free space on its
              volume.
            </CardFooter>
          </Card>
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
// <select>: the same custom-dropdown register as the Link tab's device picker.
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

const NODE_CHIP: Record<string, { color: string; label: string }> = {
  Idle: { color: GREEN, label: 'idle' },
  Busy: { color: AMBER, label: 'busy' },
  Offline: { color: SLATE, label: 'offline' },
};

function NodeStatusChip({ status }: { status: NodeStatus }) {
  const v = NODE_CHIP[status.type] ?? NODE_CHIP.Offline;
  return (
    <Chip color={v.color} ink={v.color === AMBER ? INK_DARK : '#ffffff'}>
      {v.label}
    </Chip>
  );
}

/**
 * Solid benchmark-styled node dropdown. "This device" (self) is always first and default;
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
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return undefined;
    const onDocMouseDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', onDocMouseDown);
    return () => document.removeEventListener('mousedown', onDocMouseDown);
  }, [open]);

  const selected = targets.find((t) => t.nodeId === value) ?? targets[0] ?? null;

  return (
    <div ref={rootRef} className="relative min-w-[240px]">
      <button
        type="button"
        data-testid="mlx-device-target"
        disabled={disabled}
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        className="flex w-full items-center gap-2 rounded border border-border-primary bg-background-secondary px-3 py-2 text-left text-sm font-semibold text-text-primary transition-colors hover:border-text-secondary disabled:opacity-60"
      >
        <Laptop className="h-4 w-4 shrink-0 text-text-secondary" />
        <span className="min-w-0 truncate">{selected ? deviceLabel(selected) : 'This device'}</span>
        {selected?.status && <NodeStatusChip status={selected.status} />}
        <ChevronDown className="ml-auto h-4 w-4 shrink-0 text-text-secondary" />
      </button>
      {open && (
        <div
          role="listbox"
          aria-label="Manage models on device"
          className="absolute left-0 top-full z-[60] mt-1 w-full overflow-hidden rounded border border-border-primary bg-background-primary shadow-lg"
        >
          {targets.map((t) => (
            <button
              key={t.nodeId ?? 'self'}
              type="button"
              role="option"
              aria-selected={t.nodeId === value}
              data-testid={`mlx-device-target-option-${t.nodeId ?? 'self'}`}
              title={deviceLabel(t)}
              onClick={() => {
                onChange(t.nodeId);
                setOpen(false);
              }}
              className={`flex w-full items-center gap-2 px-3 py-2 text-left text-sm ${
                t.nodeId === value ? 'bg-background-secondary' : 'hover:bg-background-secondary'
              }`}
            >
              <Laptop className="h-4 w-4 shrink-0 text-text-secondary" />
              <span className="min-w-0 truncate font-semibold text-text-primary">
                {deviceLabel(t)}
              </span>
              {t.status && <NodeStatusChip status={t.status} />}
            </button>
          ))}
        </div>
      )}
    </div>
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
      setStatusError(errorMessage(error, 'Could not read the engine status.'));
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
        setMountError(errorMessage(error, 'Could not list local models.'));
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
        setBrowseFiltersError(errorMessage(error, 'Could not load the filter vocabularies.'));
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
        setDownloadError(repoId, errorMessage(error, 'Download failed to start.'));
      }
    },
    [clearDownloadError, setDownloadError, activeNodeId]
  );

  const pauseDownload = useCallback(
    async (repoId: string) => {
      try {
        await mlxEngineDownloadPause(repoId, activeNodeId);
      } catch (error) {
        setDownloadError(repoId, errorMessage(error, 'Pause failed.'));
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
        setDownloadError(repoId, errorMessage(error, 'Resume failed.'));
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
        setDownloadError(repoId, errorMessage(error, 'Cancel failed.'));
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
        setSaveError(errorMessage(error, 'Could not read the engine settings.'));
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
        setMountError(errorMessage(error, 'Mount failed.'));
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
        setMountError(errorMessage(error, 'Unmount failed.'));
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
        setMountError(errorMessage(error, 'Remount failed.'));
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
        setSaveError(errorMessage(error, 'Could not save settings.'));
      } finally {
        setSaving(false);
      }
    })();
  }, [settings, samplingModelId, profileDrafts, saveSettings]);

  const openSamplingFor = useCallback((modelId: string) => {
    setSamplingModelId(modelId);
    setTab('sampling');
  }, []);

  const tabBtn = (t: MlxTab, label: string, extra?: React.ReactNode) => {
    const active = tab === t;
    return (
      <button
        type="button"
        onClick={() => setTab(t)}
        className={`flex items-center gap-2 px-4 py-2 text-sm font-bold transition-colors ${
          active
            ? 'text-white'
            : 'bg-background-secondary text-text-secondary hover:text-text-primary'
        }`}
        style={active ? { backgroundColor: SEGMENT_ACTIVE } : undefined}
        aria-pressed={active}
      >
        {label}
        {extra}
      </button>
    );
  };

  // The page shell (MainPanelLayout, the LeanZero Swarm header, the top-level tab bar and the
  // scroll area) belongs to LeanZeroSwarmView — this component is the LeanZero MLX tab's content:
  // the engine sub-tabs (Engine / Models / Sampling) plus everything under them, unchanged.
  return (
    <div className="flex flex-col gap-4">
      {/* Device picker — only when there ARE peers (capability present + connected + a peer on
          the mesh). With no worker deployed the mesh is not connected, so this is hidden and the
          view behaves exactly as before, all ops on THIS device. */}
      {peers.length > 0 && (
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-[11px] font-medium text-text-secondary">Manage on</span>
          <DeviceTargetPicker
            targets={deviceTargets}
            value={targetNodeId}
            onChange={setTargetNodeId}
            disabled={engineBusy || nodeSwitching}
          />
          {nodeSwitching && (
            <Chip color={AMBER} ink={INK_DARK}>
              <Loader2 className="h-2.5 w-2.5 animate-spin" />
              switching device
            </Chip>
          )}
        </div>
      )}
      {selectedPeer && (
        <SolidBanner
          color={AZURE}
          label="Remote"
          text={`Managing models on ${selectedPeer.hostname} (remote)`}
        />
      )}

      <div className="flex flex-wrap items-center gap-3">
        <div className="flex self-start overflow-hidden rounded border border-border-primary">
          {tabBtn('engine', 'Engine')}
          {tabBtn(
            'models',
            'Models',
            <span className="text-[11px] font-medium tabular-nums">{models.length}</span>
          )}
          {tabBtn('sampling', 'Sampling')}
        </div>
        {status && <StateBadge state={status.state} />}
        {/* pr-3: without it the ScrollArea's right edge shaved the final glyph off "Rapid-MLX"
            (caught live on the packaged build, 2026-08-31). */}
        <span className="ml-auto shrink-0 pr-3 text-sm font-bold" style={{ color: AZURE }}>
          Powered by Rapid-MLX
        </span>
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
