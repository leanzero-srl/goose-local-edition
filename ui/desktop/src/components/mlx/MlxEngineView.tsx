import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Check,
  Cpu,
  Download,
  Folder,
  HardDrive,
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
import { MainPanelLayout } from '../Layout/MainPanelLayout';
import { ScrollArea } from '../ui/scroll-area';
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
  mlxEngineDownload,
  mlxEngineDownloadCancel,
  mlxEngineDownloadProgress,
  mlxEngineModelDelete,
  mlxEngineModelsList,
  mlxEngineMount,
  mlxEngineSettingsRead,
  mlxEngineSettingsUpdate,
  mlxEngineStatus,
  mlxEngineUnmount,
  type MlxBrowseHit,
  type MlxBrowseSort,
  type MlxDownloadProgress,
  type MlxEngineSettings,
  type MlxEngineState,
  type MlxEngineStatus,
  type MlxLocalModel,
  type MlxModelProfile,
} from '../../acp/mlx-engine';

// Solid saturated palette — the benchmark register (BenchmarkView/ScoringDetail): full
// borders, bg-background-secondary strips, solid chips. Never faded tints, never a left
// accent rail, never a native control.
const AZURE = '#2e8bff';
const GREEN = '#2ecc71';
const AMBER = '#f5a623';
const RED = '#e5484d';
const SLATE = '#64748b';
const VIOLET = '#7c3aed';
const TEAL = 'var(--color-block-teal, #13bbaf)';
// The node ramp lives under `.local-edition`; this window also runs in builds without that
// class, where a bare var() resolves to NOTHING and a solid fill silently turns transparent
// (caught live 2026-08-31: the active tab label vanished). Every node var carries a fallback.
const SEGMENT_ACTIVE = 'var(--color-node-5, #db2777)';
const INK_DARK = '#1a1a1a';

const STATE_COLOR: Record<MlxEngineState, string> = {
  running: GREEN,
  mounting: AMBER,
  failed: RED,
  stopped: SLATE,
};

const GB = 1024 * 1024 * 1024;

export function formatGb(bytes: number): string {
  if (bytes <= 0) return 'unknown size';
  const gb = bytes / GB;
  if (gb >= 10) return `${gb.toFixed(0)} GB`;
  if (gb >= 0.1) return `${gb.toFixed(1)} GB`;
  return `${(bytes / (1024 * 1024)).toFixed(0)} MB`;
}

export function formatBytesShort(bytes: number): string {
  if (bytes <= 0) return '0 B';
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  if (bytes < GB) return `${(bytes / (1024 * 1024)).toFixed(0)} MB`;
  return `${(bytes / GB).toFixed(2)} GB`;
}

export function formatCount(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return `${n}`;
}

export function formatDate(iso: string): string {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return iso;
  return new Date(t).toLocaleDateString(undefined, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
  });
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
// Small solid building blocks
// ---------------------------------------------------------------------------

function Chip({
  color,
  ink = '#ffffff',
  children,
  title,
}: {
  color: string;
  ink?: string;
  children: React.ReactNode;
  title?: string;
}) {
  return (
    <span
      title={title}
      className="inline-flex shrink-0 items-center gap-1 rounded px-1.5 py-0.5 text-[10px] font-bold uppercase tracking-wide"
      style={{ backgroundColor: color, color: ink }}
    >
      {children}
    </span>
  );
}

function StateBadge({ state }: { state: MlxEngineState }) {
  return (
    <span
      data-testid="mlx-state-badge"
      className={`inline-flex items-center gap-1.5 rounded px-2.5 py-1 text-xs font-bold uppercase tracking-wider ${
        state === 'mounting' ? 'animate-pulse' : ''
      }`}
      style={{ backgroundColor: STATE_COLOR[state], color: state === 'mounting' ? INK_DARK : '#fff' }}
    >
      {state === 'mounting' && <Loader2 className="h-3 w-3 animate-spin" />}
      {state}
    </span>
  );
}

/** Solid full-width banner. Red carries backend text VERBATIM — never paraphrased. */
function SolidBanner({
  color,
  label,
  text,
  action,
}: {
  color: string;
  label: string;
  text: string;
  action?: React.ReactNode;
}) {
  const dark = color === AMBER;
  return (
    <div
      className="flex items-center gap-3 rounded px-4 py-3"
      style={{ backgroundColor: color }}
      role="alert"
    >
      <span
        className="shrink-0 text-[10px] font-black uppercase tracking-widest"
        style={{ color: dark ? INK_DARK : '#ffffff' }}
      >
        {label}
      </span>
      <span
        className="min-w-0 flex-1 break-words text-sm font-semibold"
        style={{ color: dark ? INK_DARK : '#ffffff' }}
      >
        {text}
      </span>
      {action}
    </div>
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
      <span className="shrink-0 text-xs font-bold tabular-nums" style={{ color: tight ? AMBER : AZURE }}>
        {availableGb.toFixed(1)} GB free
      </span>
      <span className="shrink-0 text-xs tabular-nums text-text-secondary">
        of {totalGb.toFixed(1)} GB
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
      <span className="text-xs font-semibold uppercase tracking-wider text-text-secondary">
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
              active ? 'text-white' : 'bg-background-secondary text-text-secondary hover:text-text-primary'
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
            className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[10px] font-bold uppercase tracking-wide text-white hover:opacity-90"
            style={{ backgroundColor: AZURE }}
            title="Clear — fall back to the engine default"
          >
            set <X className="w-2.5 h-2.5" />
          </button>
        ) : (
          <Chip color={SLATE} title="No value sent — the engine uses its own default">
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
      <span className="shrink-0 text-xs font-bold tabular-nums" style={{ color: AZURE }}>
        {formatGb(option.model.sizeBytes)}
      </span>
      {!option.model.complete && <Chip color={AMBER} ink={INK_DARK}>partial download</Chip>}
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
      placeholder={models.length === 0 ? 'No models in the models folder yet' : 'Pick a model to mount'}
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
      {option.hasProfile && <Chip color={VIOLET}>profile</Chip>}
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
      <span className="text-[10px] font-semibold uppercase tracking-wider text-text-secondary">
        {label}
      </span>
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
      {statusError && (
        <SolidBanner color={RED} label="Engine unreachable" text={statusError} />
      )}
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
            <Chip color={VIOLET} title="Tool-call parser reported by the live engine">
              {status.toolCallParser}
            </Chip>
          )}
        </CardHeader>
        <div className="flex flex-col gap-4 px-3 py-3">
          <div className="grid grid-cols-2 gap-x-6 gap-y-3 md:grid-cols-4">
            <StatusFact label="Context window">
              {status?.contextWindow != null ? (
                <span className="font-bold tabular-nums" style={{ color: AZURE }}>
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
        <div className="flex items-start gap-2 px-3 py-3">
          <div className="min-w-0 flex-1">
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
            <Button disabled className="rounded font-bold text-white" style={{ backgroundColor: GREEN }}>
              <Loader2 className="w-4 h-4 animate-spin" />
              Mounting
            </Button>
          ) : selectionIsMounted ? (
            <Button disabled className="rounded font-bold text-white" style={{ backgroundColor: GREEN }}>
              <Check className="w-4 h-4" />
              Mounted
            </Button>
          ) : state === 'running' ? (
            <Button
              onClick={onMount}
              disabled={!canSwitch}
              className="rounded font-bold text-white hover:opacity-90"
              style={{ backgroundColor: GREEN }}
            >
              <Play className="w-4 h-4" />
              Switch model
            </Button>
          ) : (
            <Button
              onClick={onMount}
              disabled={!canMount}
              className="rounded font-bold text-white hover:opacity-90"
              style={{ backgroundColor: GREEN }}
            >
              <Play className="w-4 h-4" />
              Mount
            </Button>
          )}
          <Button
            onClick={onUnmount}
            disabled={!canUnmount}
            className="rounded font-bold text-white hover:opacity-90"
            style={{ backgroundColor: SLATE }}
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
              {dirty && <Chip color={AMBER} ink={INK_DARK}>unsaved</Chip>}
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
          A blank field sends nothing — the engine keeps its own default. Profiles apply at
          mount, per model: saving never touches a live process, and the status reports restart
          required until the mounted model is remounted.
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

function DownloadProgressRow({
  repoId,
  progress,
  onCancel,
}: {
  repoId: string;
  progress: MlxDownloadProgress;
  onCancel: () => void;
}) {
  const pct =
    progress.totalBytes > 0
      ? Math.min(100, (progress.downloadedBytes / progress.totalBytes) * 100)
      : 0;
  const active = progress.state === 'queued' || progress.state === 'downloading';
  return (
    <div className="mt-2 flex flex-col gap-1.5" data-testid={`mlx-download-${repoId}`}>
      <div className="flex items-center gap-2">
        <div
          className="h-2.5 flex-1 overflow-hidden rounded border border-border-primary"
          role="progressbar"
          aria-valuenow={Math.round(pct)}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-label={`Download progress for ${repoId}`}
        >
          <div className="h-full" style={{ width: `${pct}%`, backgroundColor: AZURE }} />
        </div>
        <span className="shrink-0 text-xs font-bold tabular-nums" style={{ color: AZURE }}>
          {formatBytesShort(progress.downloadedBytes)}
          {progress.totalBytes > 0 ? ` / ${formatBytesShort(progress.totalBytes)}` : ''}
        </span>
        {active && (
          <Button
            size="xs"
            onClick={onCancel}
            className="shrink-0 rounded font-bold text-white hover:opacity-90"
            style={{ backgroundColor: SLATE }}
          >
            <X className="w-3 h-3" />
            Cancel
          </Button>
        )}
      </div>
      <div className="flex min-w-0 items-center gap-2">
        {progress.state === 'queued' && <Chip color={SLATE}>queued</Chip>}
        {progress.state === 'downloading' && <Chip color={AZURE}>downloading</Chip>}
        {progress.state === 'done' && <Chip color={GREEN}>done</Chip>}
        {progress.state === 'cancelled' && <Chip color={SLATE}>cancelled</Chip>}
        {progress.currentFile && (
          <span className="truncate font-mono text-[11px] text-text-secondary">
            {progress.currentFile}
          </span>
        )}
      </div>
    </div>
  );
}

// ------------------------- Hugging Face browser ----------------------------

const PROVIDER_CHOICES = [
  'all',
  'mlx-community',
  'lmstudio-community',
  'Qwen',
  'unsloth',
  'nightmedia',
  'other',
] as const;
type ProviderChoice = (typeof PROVIDER_CHOICES)[number];

const QUANT_CHOICES = ['all', '3-bit', '4-bit', '5-bit', '6-bit', '8-bit', 'bf16'] as const;
type QuantChoice = (typeof QUANT_CHOICES)[number];

/**
 * The architecture tags the backend measured live on MLX repos — mirrors
 * MEASURED_ARCH_TAGS in crates/goose-sidecar/src/hf.rs (2026-08-31), sorted for humans.
 */
export const ARCH_CHOICES = [
  'cohere',
  'deepseek_v2',
  'deepseek_v3',
  'ernie4_5',
  'exaone',
  'gemma3',
  'gemma4',
  'gemma4_unified',
  'glm4',
  'glm4_moe',
  'glm4v',
  'gpt_oss',
  'granite',
  'internvl',
  'kimi_k2',
  'kimi_k25',
  'lfm2',
  'lfm2_moe',
  'llama',
  'mamba',
  'minimax',
  'mistral',
  'mixtral',
  'nemotron',
  'olmo2',
  'phi',
  'phi3',
  'qwen',
  'qwen2',
  'qwen3',
  'qwen3_5',
  'qwen3_5_moe',
  'qwen3_moe',
  'qwen3_vl',
  'qwen4_exp',
  'smollm3',
  'starcoder2',
  'whisper',
] as const;

interface FilterOption {
  value: string;
  label: string;
}

const PROVIDER_OPTIONS: FilterOption[] = PROVIDER_CHOICES.map((v) => ({
  value: v,
  label: v === 'all' ? 'Provider: all' : v === 'other' ? 'Other…' : v,
}));
const QUANT_OPTIONS: FilterOption[] = QUANT_CHOICES.map((v) => ({
  value: v,
  label: v === 'all' ? 'Quant: all' : v,
}));
const ARCH_OPTIONS: FilterOption[] = [
  { value: 'all', label: 'Arch: all' },
  ...ARCH_CHOICES.map((v) => ({ value: v, label: v })),
];

// Distinct solid hues for author chips — deterministic per author, full-rainbow, no washes.
const AUTHOR_HUES = [
  'var(--color-node-1, #1d4ed8)',
  'var(--color-node-2, #0891b2)',
  'var(--color-node-3, #7c3aed)',
  'var(--color-node-4, #ea580c)',
  'var(--color-node-5, #db2777)',
  'var(--color-node-6, #16a34a)',
];

export function authorHue(author: string): string {
  let h = 0;
  for (let i = 0; i < author.length; i += 1) h = (h * 31 + author.charCodeAt(i)) >>> 0;
  return AUTHOR_HUES[h % AUTHOR_HUES.length];
}

interface BrowseHitRowProps {
  hit: MlxBrowseHit;
  sort: MlxBrowseSort;
  progress: MlxDownloadProgress | undefined;
  startError: string | undefined;
  onDownload: () => void;
  onCancel: () => void;
}

function BrowseHitRow({ hit, sort, progress, startError, onDownload, onCancel }: BrowseHitRowProps) {
  const failed = progress?.state === 'failed';
  return (
    <div className="border-t border-border-primary px-3 py-2">
      {/* flex-wrap + a real min width on the id: at narrow window widths the chip cluster
          otherwise squeezed the model id down to "lmstudi…" (caught live 2026-08-31). */}
      <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
        <span className="min-w-[220px] flex-1 truncate font-mono text-xs font-medium text-text-primary">
          {hit.id}
        </span>
        <Chip color={authorHue(hit.author)} title={`Published by ${hit.author}`}>
          {hit.author}
        </Chip>
        {hit.quant && (
          <Chip color={AZURE} title="Derived from the repo's tags or name">
            {hit.quant}
          </Chip>
        )}
        {hit.arch && (
          <Chip color={TEAL} title="Derived from the repo's tags or name">
            {hit.arch}
          </Chip>
        )}
        <span
          className="shrink-0 text-xs font-bold tabular-nums"
          style={{ color: AZURE }}
          title={`${hit.downloads.toLocaleString()} downloads`}
        >
          ↓ {formatCount(hit.downloads)}
        </span>
        <span
          className="shrink-0 text-xs font-bold tabular-nums"
          style={{ color: VIOLET }}
          title={`${hit.likes.toLocaleString()} likes`}
        >
          ♥ {formatCount(hit.likes)}
        </span>
        {hit.createdAt &&
          (sort === 'newest' ? (
            <span
              className="shrink-0 rounded px-1.5 py-0.5 text-[11px] font-bold tabular-nums"
              style={{ backgroundColor: AMBER, color: INK_DARK }}
              title={`Created ${hit.createdAt}`}
            >
              {formatDate(hit.createdAt)}
            </span>
          ) : (
            <span
              className="shrink-0 text-xs tabular-nums text-text-secondary"
              title={`Created ${hit.createdAt}`}
            >
              {formatDate(hit.createdAt)}
            </span>
          ))}
        {!progress && (
          <Button
            size="sm"
            onClick={onDownload}
            className="shrink-0 rounded font-bold text-white hover:opacity-90"
            style={{ backgroundColor: GREEN }}
            aria-label={`Download ${hit.id}`}
          >
            <Download className="w-3.5 h-3.5" />
            Download
          </Button>
        )}
        {failed && (
          <Button
            size="sm"
            onClick={onDownload}
            className="shrink-0 rounded font-bold text-white hover:opacity-90"
            style={{ backgroundColor: RED }}
          >
            <RefreshCw className="w-3.5 h-3.5" />
            Retry
          </Button>
        )}
      </div>
      {startError && (
        <div className="mt-1 break-words text-xs font-semibold" style={{ color: RED }}>
          {startError}
        </div>
      )}
      {progress && !failed && (
        <DownloadProgressRow repoId={hit.id} progress={progress} onCancel={onCancel} />
      )}
      {failed && (
        <div className="mt-1 break-words text-xs font-semibold" style={{ color: RED }}>
          {progress?.error ?? 'Download failed.'}
        </div>
      )}
    </div>
  );
}

interface HfBrowserProps {
  downloads: Record<string, MlxDownloadProgress>;
  downloadErrors: Record<string, string>;
  onDownload: (repoId: string) => void;
  onCancelDownload: (repoId: string) => void;
}

/**
 * Paginated MLX-only Hugging Face browser. Every filter is applied SERVER-side through
 * `_goose/unstable/mlxEngine/browse`; changing any filter/sort/search resets pagination
 * (an epoch guard drops stale in-flight pages), and Load more appends via `nextCursor`.
 */
function HfBrowser({ downloads, downloadErrors, onDownload, onCancelDownload }: HfBrowserProps) {
  const [queryText, setQueryText] = useState('');
  const [appliedQuery, setAppliedQuery] = useState('');
  const [provider, setProvider] = useState<ProviderChoice>('all');
  const [authorText, setAuthorText] = useState('');
  const [appliedAuthor, setAppliedAuthor] = useState('');
  const [quant, setQuant] = useState<QuantChoice>('all');
  const [arch, setArch] = useState<string>('all');
  const [sort, setSort] = useState<MlxBrowseSort>('downloads');

  const [hits, setHits] = useState<MlxBrowseHit[] | null>(null);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const epoch = useRef(0);

  const author =
    provider === 'all' ? undefined : provider === 'other' ? appliedAuthor || undefined : provider;

  const baseParams = useMemo(
    () => ({
      sort,
      query: appliedQuery || undefined,
      author,
      quant: quant === 'all' ? undefined : quant,
      arch: arch === 'all' ? undefined : arch,
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
        setError(errorMessage(e, 'Hugging Face browse failed.'));
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
        setError(errorMessage(e, 'Loading the next page failed.'));
      } finally {
        if (epoch.current === id) setLoadingMore(false);
      }
    })();
  }, [baseParams, nextCursor]);

  const commitQuery = useCallback(() => setAppliedQuery(queryText.trim()), [queryText]);
  const commitAuthor = useCallback(() => setAppliedAuthor(authorText.trim()), [authorText]);

  const providerOption = PROVIDER_OPTIONS.find((o) => o.value === provider) ?? PROVIDER_OPTIONS[0];
  const quantOption = QUANT_OPTIONS.find((o) => o.value === quant) ?? QUANT_OPTIONS[0];
  const archOption = ARCH_OPTIONS.find((o) => o.value === arch) ?? ARCH_OPTIONS[0];

  return (
    <Card>
      <CardHeader
        label="Hugging Face — MLX models"
        right={
          <>
            {loading && (
              <Chip color={AMBER} ink={INK_DARK}>
                <Loader2 className="h-2.5 w-2.5 animate-spin" />
                loading
              </Chip>
            )}
            <Chip color={AZURE}>{hits?.length ?? 0} loaded</Chip>
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
          <div className="w-52">
            <Select
              aria-label="Provider filter"
              options={PROVIDER_OPTIONS}
              value={providerOption}
              onChange={(o) => setProvider(((o as FilterOption)?.value ?? 'all') as ProviderChoice)}
            />
          </div>
          {provider === 'other' && (
            <div className="w-56">
              <Input
                value={authorText}
                onChange={(e) => setAuthorText(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') commitAuthor();
                }}
                onBlur={commitAuthor}
                placeholder="author (e.g. inferencerlabs)"
                className="rounded font-mono text-sm"
                aria-label="Author filter"
              />
            </div>
          )}
          <div className="w-40">
            <Select
              aria-label="Quant filter"
              options={QUANT_OPTIONS}
              value={quantOption}
              onChange={(o) => setQuant(((o as FilterOption)?.value ?? 'all') as QuantChoice)}
            />
          </div>
          <div className="w-48">
            <Select
              aria-label="Architecture filter"
              options={ARCH_OPTIONS}
              value={archOption}
              onChange={(o) => setArch((o as FilterOption)?.value ?? 'all')}
            />
          </div>
        </div>
        {error && <SolidBanner color={RED} label="Browse failed" text={error} />}
      </div>

      {hits != null && hits.length === 0 && !loading && !error && (
        <div className="border-t border-border-primary px-3 py-3 text-sm text-text-secondary">
          No MLX models match these filters.
        </div>
      )}
      {hits != null && hits.length > 0 && (
        <div>
          {hits.map((hit) => (
            <BrowseHitRow
              key={hit.id}
              hit={hit}
              sort={sort}
              progress={downloads[hit.id]}
              startError={downloadErrors[hit.id]}
              onDownload={() => onDownload(hit.id)}
              onCancel={() => onCancelDownload(hit.id)}
            />
          ))}
        </div>
      )}
      {nextCursor != null && !loading && (
        <button
          type="button"
          onClick={loadMore}
          disabled={loadingMore}
          className="flex w-full items-center justify-center gap-2 border-t border-border-primary py-2.5 text-sm font-semibold text-white transition-opacity hover:opacity-90 disabled:opacity-60"
          style={{ backgroundColor: TEAL }}
        >
          {loadingMore ? <Loader2 className="h-4 w-4 animate-spin" /> : null}
          Load more
        </button>
      )}
      <CardFooter>
        Quant and Arch filters match Hugging Face tags — a model whose quant appears only in its
        name is excluded by those filters but still findable via search.
      </CardFooter>
    </Card>
  );
}

interface ModelsSectionProps {
  settings: MlxEngineSettings | null;
  models: MlxLocalModel[];
  mountedModelId: string | null;
  refreshModels: () => void;
  saveSettings: (next: MlxEngineSettings) => Promise<void>;
  onOpenSampling: (modelId: string) => void;
}

function ModelsSection({
  settings,
  models,
  mountedModelId,
  refreshModels,
  saveSettings,
  onOpenSampling,
}: ModelsSectionProps) {
  const [dirDialogOpen, setDirDialogOpen] = useState(false);
  const [dirSaving, setDirSaving] = useState(false);
  const [dirError, setDirError] = useState<string | null>(null);

  const [downloads, setDownloads] = useState<Record<string, MlxDownloadProgress>>({});
  const [downloadErrors, setDownloadErrors] = useState<Record<string, string>>({});

  const [pendingDelete, setPendingDelete] = useState<MlxLocalModel | null>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [deleting, setDeleting] = useState(false);

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

  const startDownload = useCallback(async (repoId: string) => {
    setDownloadErrors((prev) => {
      const next = { ...prev };
      delete next[repoId];
      return next;
    });
    setDownloads((prev) => ({
      ...prev,
      [repoId]: { state: 'queued', totalBytes: 0, downloadedBytes: 0 },
    }));
    try {
      await mlxEngineDownload(repoId);
    } catch (error) {
      setDownloads((prev) => {
        const next = { ...prev };
        delete next[repoId];
        return next;
      });
      setDownloadErrors((prev) => ({
        ...prev,
        [repoId]: errorMessage(error, 'Download failed to start.'),
      }));
    }
  }, []);

  const cancelDownload = useCallback(async (repoId: string) => {
    try {
      await mlxEngineDownloadCancel(repoId);
    } catch (error) {
      setDownloadErrors((prev) => ({
        ...prev,
        [repoId]: errorMessage(error, 'Cancel failed.'),
      }));
    }
  }, []);

  // Poll live downloads every second — the bar shows real bytes, never a fake animation.
  const activeKey = useMemo(
    () =>
      Object.entries(downloads)
        .filter(([, p]) => p.state === 'queued' || p.state === 'downloading')
        .map(([id]) => id)
        .sort()
        .join('\n'),
    [downloads]
  );
  useEffect(() => {
    if (activeKey === '') return undefined;
    const repoIds = activeKey.split('\n');
    const timer = setInterval(() => {
      for (const repoId of repoIds) {
        void (async () => {
          try {
            const progress = await mlxEngineDownloadProgress(repoId);
            if (!progress) return;
            setDownloads((prev) => ({ ...prev, [repoId]: progress }));
            if (progress.state === 'done') refreshModels();
          } catch {
            // transient poll failure — keep the last real numbers rather than inventing any
          }
        })();
      }
    }, 1000);
    return () => clearInterval(timer);
  }, [activeKey, refreshModels]);

  const confirmDelete = useCallback(async () => {
    if (!pendingDelete) return;
    setDeleting(true);
    setDeleteError(null);
    try {
      await mlxEngineModelDelete(pendingDelete.id);
      setPendingDelete(null);
      refreshModels();
    } catch (error) {
      setDeleteError(errorMessage(error, `Could not delete ${pendingDelete.id}.`));
    } finally {
      setDeleting(false);
    }
  }, [pendingDelete, refreshModels]);

  return (
    <div className="flex flex-col gap-4 pb-8">
      {/* Models folder */}
      <Card>
        <CardHeader label="Models folder" />
        <div className="flex flex-col gap-2 px-3 py-3">
          <div className="flex items-center gap-2">
            <Folder className="w-4 h-4 shrink-0" style={{ color: AZURE }} />
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
        </div>
        <CardFooter>One directory used by downloads and mounts alike.</CardFooter>
      </Card>

      {/* Hugging Face browser */}
      <HfBrowser
        downloads={downloads}
        downloadErrors={downloadErrors}
        onDownload={(repoId) => void startDownload(repoId)}
        onCancelDownload={(repoId) => void cancelDownload(repoId)}
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
          <Chip color={AZURE}>{models.length}</Chip>
        </CardHeader>
        {deleteError && (
          <div className="px-3 pt-3">
            <SolidBanner color={RED} label="Delete failed" text={deleteError} />
          </div>
        )}
        {models.length === 0 ? (
          <div className="px-3 py-3 text-sm text-text-secondary">
            Nothing downloaded yet — browse Hugging Face above.
          </div>
        ) : (
          <div>
            {models.map((model) => (
              <div
                key={model.id}
                className="flex min-w-0 items-center gap-3 border-t border-border-primary px-3 py-2.5 first:border-t-0"
              >
                <HardDrive
                  className="w-4 h-4 shrink-0"
                  style={{ color: model.complete ? AZURE : AMBER }}
                />
                <span className="min-w-0 flex-1 truncate font-mono text-sm text-text-primary">
                  {model.id}
                </span>
                {model.id === mountedModelId && <Chip color={GREEN}>mounted</Chip>}
                {!model.complete && <Chip color={AMBER} ink={INK_DARK}>partial download</Chip>}
                <span className="shrink-0 text-xs font-bold tabular-nums" style={{ color: AZURE }}>
                  {formatGb(model.sizeBytes)}
                </span>
                <Button
                  size="xs"
                  onClick={() => onOpenSampling(model.id)}
                  className="shrink-0 rounded font-bold text-white hover:opacity-90"
                  style={{ backgroundColor: VIOLET }}
                  aria-label={`Sampling for ${model.id}`}
                  title="This model's sampling profile — opens the Sampling tab"
                >
                  <SlidersHorizontal className="w-3 h-3" />
                  Sampling
                </Button>
                <Button
                  size="xs"
                  onClick={() => {
                    setDeleteError(null);
                    setPendingDelete(model);
                  }}
                  className="shrink-0 rounded font-bold text-white hover:opacity-90"
                  style={{ backgroundColor: RED }}
                  aria-label={`Delete ${model.id}`}
                >
                  <Trash2 className="w-3 h-3" />
                </Button>
              </div>
            ))}
          </div>
        )}
      </Card>

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
            ? `Delete ${pendingDelete.id} (${formatGb(pendingDelete.sizeBytes)}) from the models folder? This removes the files from disk.`
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
// The view
// ---------------------------------------------------------------------------

type MlxTab = 'engine' | 'models' | 'sampling';

const STATUS_POLL_MS = 2000;

const MlxEngineView: React.FC = () => {
  const [tab, setTab] = useState<MlxTab>('engine');

  const [status, setStatus] = useState<MlxEngineStatus | null>(null);
  const [statusError, setStatusError] = useState<string | null>(null);
  const [settings, setSettings] = useState<MlxEngineSettings | null>(null);
  const [models, setModels] = useState<MlxLocalModel[]>([]);

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
      const next = await mlxEngineStatus();
      setStatus(next);
      setStatusError(null);
    } catch (error) {
      setStatusError(errorMessage(error, 'Could not read the engine status.'));
    }
  }, []);

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
        setModels(await mlxEngineModelsList());
      } catch (error) {
        // The models list failing is a real fact; show it where models are picked.
        setMountError(errorMessage(error, 'Could not list local models.'));
      }
    })();
  }, []);

  useEffect(() => {
    refreshModels();
    void (async () => {
      try {
        setSettings(await mlxEngineSettingsRead());
      } catch (error) {
        setSaveError(errorMessage(error, 'Could not read the engine settings.'));
      }
    })();
  }, [refreshModels]);

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
        await mlxEngineMount(mountModelId);
      } catch (error) {
        setMountError(errorMessage(error, 'Mount failed.'));
      } finally {
        setEngineBusy(false);
        void refreshStatus();
      }
    })();
  }, [mountModelId, refreshStatus]);

  const onUnmount = useCallback(() => {
    void (async () => {
      setEngineBusy(true);
      setMountError(null);
      try {
        await mlxEngineUnmount();
      } catch (error) {
        setMountError(errorMessage(error, 'Unmount failed.'));
      } finally {
        setEngineBusy(false);
        void refreshStatus();
      }
    })();
  }, [refreshStatus]);

  const onRemount = useCallback(() => {
    const modelId = status?.modelId ?? settings?.modelId;
    if (!modelId) return;
    void (async () => {
      setEngineBusy(true);
      setMountError(null);
      try {
        await mlxEngineUnmount();
        await mlxEngineMount(modelId);
      } catch (error) {
        setMountError(errorMessage(error, 'Remount failed.'));
      } finally {
        setEngineBusy(false);
        void refreshStatus();
      }
    })();
  }, [status?.modelId, settings?.modelId, refreshStatus]);

  const savedDraftsForSelected = useMemo(
    () =>
      settings && samplingModelId
        ? draftsFromProfile(settings.modelProfiles?.[samplingModelId])
        : null,
    [settings, samplingModelId]
  );
  const draftsForSelected =
    samplingModelId != null
      ? (profileDrafts[samplingModelId] ?? savedDraftsForSelected)
      : null;

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
      const saved = await mlxEngineSettingsUpdate(next);
      setSettings(saved);
      void refreshStatus();
    },
    [refreshStatus]
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
          active ? 'text-white' : 'bg-background-secondary text-text-secondary hover:text-text-primary'
        }`}
        style={active ? { backgroundColor: SEGMENT_ACTIVE } : undefined}
        aria-pressed={active}
      >
        {label}
        {extra}
      </button>
    );
  };

  return (
    <MainPanelLayout>
      <div className="flex-1 flex flex-col min-h-0">
        <div className="bg-background-primary px-8 pb-5 pt-16">
          <header className="flex flex-col page-transition border-b border-border-primary pb-5">
            <div className="flex items-center gap-3">
              <span
                className="inline-flex h-9 w-9 shrink-0 items-center justify-center rounded"
                style={{ backgroundColor: AZURE }}
              >
                <Cpu className="w-5 h-5 text-white" />
              </span>
              <h1 className="text-2xl font-bold text-text-primary">Leanzero MLX</h1>
              {status && <StateBadge state={status.state} />}
            </div>
            <p className="mt-1 text-sm font-bold" style={{ color: AZURE }}>
              Powered by Rapid-MLX
            </p>
            <p className="mt-1 max-w-[70ch] text-sm text-text-secondary">
              The in-house supervised MLX engine: mount and unmount models, tune per-model
              sampling profiles, and pull models straight from Hugging Face.
            </p>
            <div className="mt-3 flex self-start overflow-hidden rounded border border-border-primary">
              {tabBtn('engine', 'Engine')}
              {tabBtn(
                'models',
                'Models',
                <span
                  className="rounded px-1 py-px text-[10px] font-bold tabular-nums text-white"
                  style={{ backgroundColor: tab === 'models' ? INK_DARK : SLATE }}
                >
                  {models.length}
                </span>
              )}
              {tabBtn('sampling', 'Sampling')}
            </div>
          </header>
        </div>

        <div className="flex-1 min-h-0 relative px-8 pt-4">
          <ScrollArea className="h-full">
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
                mountedModelId={status?.modelId ?? null}
                refreshModels={refreshModels}
                saveSettings={saveSettings}
                onOpenSampling={openSamplingFor}
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
          </ScrollArea>
        </div>
      </div>
    </MainPanelLayout>
  );
};

export default MlxEngineView;
