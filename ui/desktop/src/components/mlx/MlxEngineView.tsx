import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Cpu,
  Download,
  Folder,
  HardDrive,
  Loader2,
  Pencil,
  Play,
  RefreshCw,
  Search,
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
  mlxEngineDownload,
  mlxEngineDownloadCancel,
  mlxEngineDownloadProgress,
  mlxEngineHfSearch,
  mlxEngineModelDelete,
  mlxEngineModelsList,
  mlxEngineMount,
  mlxEngineSettingsRead,
  mlxEngineSettingsUpdate,
  mlxEngineStatus,
  mlxEngineUnmount,
  type MlxDownloadProgress,
  type MlxEngineSettings,
  type MlxEngineState,
  type MlxEngineStatus,
  type MlxHfModelHit,
  type MlxLocalModel,
} from '../../acp/mlx-engine';

// Solid saturated palette — the same language as the swarm surfaces. Never faded tints,
// never a left accent rail.
const AZURE = '#2e8bff';
const GREEN = '#2ecc71';
const AMBER = '#f5a623';
const RED = '#e5484d';
const SLATE = '#64748b';
const VIOLET = '#7c3aed';

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
// Sampling / context settings: text drafts, where '' means "engine default".
// A cleared field OMITS the key from the payload; an explicit 0 sends 0.
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

export function draftsFromSettings(settings: MlxEngineSettings): NumericDrafts {
  const drafts = {} as NumericDrafts;
  for (const key of NUMERIC_KEYS) {
    const value = settings[key];
    drafts[key] = value == null ? '' : String(value);
  }
  return drafts;
}

/**
 * Rebuild a full settings payload from the persisted settings plus the numeric drafts.
 * Non-numeric fields (modelsDir, port, spawnCommand, modelId) pass through untouched;
 * a blank draft leaves its key ABSENT (engine default), a "0" draft sends the number 0.
 */
export function settingsWithDrafts(
  settings: MlxEngineSettings,
  drafts: NumericDrafts
): MlxEngineSettings {
  const next: MlxEngineSettings = {
    modelsDir: settings.modelsDir,
    port: settings.port,
    spawnCommand: settings.spawnCommand,
  };
  if (settings.modelId != null) next.modelId = settings.modelId;
  for (const key of NUMERIC_KEYS) {
    const text = drafts[key].trim();
    if (text === '') continue;
    const n = Number(text);
    if (Number.isNaN(n)) continue;
    next[key] = n;
  }
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
  children,
  title,
}: {
  color: string;
  children: React.ReactNode;
  title?: string;
}) {
  return (
    <span
      title={title}
      className="inline-flex items-center gap-1 px-1.5 py-0.5 text-[10px] font-bold uppercase tracking-wide text-white shrink-0"
      style={{ backgroundColor: color, borderRadius: 3 }}
    >
      {children}
    </span>
  );
}

function StateBadge({ state }: { state: MlxEngineState }) {
  return (
    <span
      data-testid="mlx-state-badge"
      className={`inline-flex items-center gap-1.5 px-2.5 py-1 text-xs font-bold uppercase tracking-wider text-white ${
        state === 'mounting' ? 'animate-pulse' : ''
      }`}
      style={{ backgroundColor: STATE_COLOR[state], borderRadius: 3 }}
    >
      {state === 'mounting' && <Loader2 className="w-3 h-3 animate-spin" />}
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
      className="flex items-center gap-3 px-4 py-3"
      style={{ backgroundColor: color, borderRadius: 3 }}
      role="alert"
    >
      <span
        className="text-[10px] font-black uppercase tracking-widest shrink-0"
        style={{ color: dark ? '#1a1a1a' : '#ffffff' }}
      >
        {label}
      </span>
      <span
        className="text-sm font-semibold flex-1 min-w-0 break-words"
        style={{ color: dark ? '#1a1a1a' : '#ffffff' }}
      >
        {text}
      </span>
      {action}
    </div>
  );
}

/** Strong memory bar: solid azure used-fill on a bordered track, bold numbers beside it. */
function MemoryBar({ availableGb, totalGb }: { availableGb: number; totalGb: number }) {
  const usedGb = Math.max(0, totalGb - availableGb);
  const pct = totalGb > 0 ? Math.min(100, (usedGb / totalGb) * 100) : 0;
  const tight = totalGb > 0 && availableGb / totalGb < 0.15;
  return (
    <div className="flex items-center gap-3 min-w-0">
      <div
        className="flex-1 h-2.5 border border-border-primary overflow-hidden"
        style={{ borderRadius: 3 }}
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
      <span className="text-xs font-bold tabular-nums shrink-0" style={{ color: tight ? AMBER : AZURE }}>
        {availableGb.toFixed(1)} GB free
      </span>
      <span className="text-xs text-text-secondary tabular-nums shrink-0">
        of {totalGb.toFixed(1)} GB
      </span>
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
    'h-7 w-7 flex items-center justify-center border border-border-primary text-text-secondary hover:text-text-primary hover:border-text-secondary transition-colors leading-none text-sm';
  const bump = (dir: 1 | -1) => {
    const base = isSet && !Number.isNaN(Number(text)) ? Number(text) : 0;
    let next = base + dir * spec.step;
    if (spec.integer) next = Math.round(next);
    // Float steps accumulate representation noise (0.1 + 0.05 = 0.15000000000000002).
    const rounded = spec.integer ? next : Number(next.toFixed(4));
    onText(String(rounded));
  };
  return (
    <div className="flex flex-col gap-1 min-w-0">
      <div className="flex items-center justify-between gap-2">
        <span className="text-xs font-medium text-text-primary truncate">{spec.label}</span>
        {isSet ? (
          <button
            type="button"
            onClick={() => onText('')}
            className="inline-flex items-center gap-1 px-1.5 py-0.5 text-[10px] font-bold uppercase tracking-wide text-white hover:opacity-90"
            style={{ backgroundColor: AZURE, borderRadius: 3 }}
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
          style={{ borderRadius: 3 }}
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
          className="h-7 text-right text-sm tabular-nums"
          style={{ borderRadius: 3 }}
          aria-label={spec.label}
        />
        <button
          type="button"
          className={stepBtn}
          style={{ borderRadius: 3 }}
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
// Model picker — the app's custom react-select wrapper, never a native <select>.
// ---------------------------------------------------------------------------

interface ModelOption {
  value: string;
  label: string;
  model: MlxLocalModel;
}

function ModelOptionLabel({ option }: { option: ModelOption }) {
  return (
    <span className="flex items-center gap-2 min-w-0">
      <span className="truncate font-mono text-sm">{option.model.id}</span>
      <span className="text-xs font-bold tabular-nums shrink-0" style={{ color: AZURE }}>
        {formatGb(option.model.sizeBytes)}
      </span>
      {!option.model.complete && <Chip color={AMBER}>partial download</Chip>}
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

// ---------------------------------------------------------------------------
// ENGINE tab
// ---------------------------------------------------------------------------

function StatusFact({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-0.5 min-w-0">
      <span className="text-[10px] font-semibold uppercase tracking-wider text-text-secondary">
        {label}
      </span>
      <span className="text-sm text-text-primary min-w-0 truncate">{children}</span>
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
  drafts: NumericDrafts | null;
  setDraft: (key: NumericSettingKey, text: string) => void;
  savedDrafts: NumericDrafts | null;
  onSaveSettings: () => void;
  saving: boolean;
  saveError: string | null;
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
    drafts,
    setDraft,
    savedDrafts,
    onSaveSettings,
    saving,
    saveError,
  } = props;

  const state = status?.state ?? null;
  const canMount =
    !!mountModelId && !engineBusy && (state === 'stopped' || state === 'failed' || state === null);
  const canUnmount = !engineBusy && (state === 'running' || state === 'mounting');
  const dirty = drafts != null && savedDrafts != null && !draftsEqual(drafts, savedDrafts);

  return (
    <div className="flex flex-col gap-4 pb-8">
      {statusError && (
        <SolidBanner color={RED} label="Engine unreachable" text={statusError} />
      )}
      {status?.gateMessage && (
        <SolidBanner color={RED} label="Mount blocked" text={status.gateMessage} />
      )}
      {mountError && <SolidBanner color={RED} label="Mount failed" text={mountError} />}
      {status?.state === 'failed' && status.lastError && status.lastError !== mountError && (
        <SolidBanner color={RED} label="Engine failed" text={status.lastError} />
      )}
      {status?.restartRequired && (
        <SolidBanner
          color={AMBER}
          label="Restart required"
          text="Settings changed — remount to apply."
          action={
            <Button
              size="sm"
              onClick={onRemount}
              disabled={engineBusy || !(status.modelId ?? settings?.modelId)}
              className="shrink-0 font-bold text-white hover:opacity-90"
              style={{ backgroundColor: '#1a1a1a', borderRadius: 3 }}
            >
              <RefreshCw className="w-3.5 h-3.5" />
              Remount
            </Button>
          }
        />
      )}

      {/* Status card */}
      <div
        className="border border-border-primary bg-background-primary p-4 flex flex-col gap-4"
        style={{ borderRadius: 3 }}
      >
        <div className="flex items-center gap-3 flex-wrap">
          {status ? <StateBadge state={status.state} /> : <Chip color={SLATE}>loading</Chip>}
          {status?.modelId ? (
            <span className="font-mono text-sm font-semibold text-text-primary truncate">
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
        </div>

        <div className="grid grid-cols-2 md:grid-cols-4 gap-x-6 gap-y-3">
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
          <div className="text-xs font-semibold break-words" style={{ color: RED }}>
            Probe failed: {status.probeError}
          </div>
        )}

        {status && (
          <MemoryBar availableGb={status.availableMemoryGb} totalGb={status.totalMemoryGb} />
        )}
      </div>

      {/* Mount controls */}
      <div
        className="border border-border-primary bg-background-primary p-4 flex flex-col gap-3"
        style={{ borderRadius: 3 }}
      >
        <span className="text-sm font-semibold text-text-primary">Mount a model</span>
        <div className="flex items-start gap-2">
          <div className="flex-1 min-w-0">
            <ModelPicker
              models={models}
              value={mountModelId}
              onChange={setMountModelId}
              disabled={engineBusy || state === 'mounting'}
            />
          </div>
          <Button
            onClick={onMount}
            disabled={!canMount}
            className="font-bold text-white hover:opacity-90"
            style={{ backgroundColor: GREEN, borderRadius: 3 }}
          >
            <Play className="w-4 h-4" />
            Mount
          </Button>
          <Button
            onClick={onUnmount}
            disabled={!canUnmount}
            className="font-bold text-white hover:opacity-90"
            style={{ backgroundColor: SLATE, borderRadius: 3 }}
          >
            <Square className="w-4 h-4" />
            Unmount
          </Button>
        </div>
        <span className="text-xs text-text-secondary">
          Mount returns immediately and the engine flips to mounting; the card above follows the
          live status every 2 seconds.
        </span>
      </div>

      {/* Sampling + context settings */}
      <div
        className="border border-border-primary bg-background-primary p-4 flex flex-col gap-4"
        style={{ borderRadius: 3 }}
      >
        <div className="flex items-center justify-between gap-3">
          <span className="text-sm font-semibold text-text-primary">Sampling</span>
          <div className="flex items-center gap-2">
            {dirty && <Chip color={AMBER}>unsaved</Chip>}
            <Button
              size="sm"
              onClick={onSaveSettings}
              disabled={!dirty || saving || !settings}
              className="font-bold text-white hover:opacity-90"
              style={{ backgroundColor: AZURE, borderRadius: 3 }}
            >
              {saving ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : null}
              Save
            </Button>
          </div>
        </div>
        {saveError && <SolidBanner color={RED} label="Save failed" text={saveError} />}
        {drafts ? (
          <>
            <div className="grid grid-cols-2 md:grid-cols-3 gap-x-6 gap-y-4">
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
            <span className="text-xs text-text-secondary">
              A blank field sends nothing — the engine keeps its own default. Saving while a model
              is running does not touch the live process; the status reports restart required until
              you remount.
            </span>
          </>
        ) : (
          <span className="text-sm text-text-secondary">Loading settings…</span>
        )}
      </div>

      {/* Spawn command — visible, not editable here: the owner sees exactly what would run. */}
      {settings && (
        <div
          className="border border-border-primary bg-background-primary p-4 flex flex-col gap-1"
          style={{ borderRadius: 3 }}
        >
          <span className="text-[10px] font-semibold uppercase tracking-wider text-text-secondary">
            Spawn command
          </span>
          <span className="font-mono text-xs text-text-primary break-all">
            {settings.spawnCommand.join(' ')}
          </span>
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// MODELS tab
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
          className="font-mono text-sm"
          style={{ borderRadius: 3 }}
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
            className="font-bold text-white hover:opacity-90"
            style={{ backgroundColor: AZURE, borderRadius: 3 }}
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
    <div className="flex flex-col gap-1.5 mt-2" data-testid={`mlx-download-${repoId}`}>
      <div className="flex items-center gap-2">
        <div
          className="flex-1 h-2.5 border border-border-primary overflow-hidden"
          style={{ borderRadius: 3 }}
          role="progressbar"
          aria-valuenow={Math.round(pct)}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-label={`Download progress for ${repoId}`}
        >
          <div className="h-full" style={{ width: `${pct}%`, backgroundColor: AZURE }} />
        </div>
        <span className="text-xs font-bold tabular-nums shrink-0" style={{ color: AZURE }}>
          {formatBytesShort(progress.downloadedBytes)}
          {progress.totalBytes > 0 ? ` / ${formatBytesShort(progress.totalBytes)}` : ''}
        </span>
        {active && (
          <Button
            size="xs"
            onClick={onCancel}
            className="font-bold text-white hover:opacity-90 shrink-0"
            style={{ backgroundColor: SLATE, borderRadius: 3 }}
          >
            <X className="w-3 h-3" />
            Cancel
          </Button>
        )}
      </div>
      <div className="flex items-center gap-2 min-w-0">
        {progress.state === 'queued' && <Chip color={SLATE}>queued</Chip>}
        {progress.state === 'downloading' && <Chip color={AZURE}>downloading</Chip>}
        {progress.state === 'done' && <Chip color={GREEN}>done</Chip>}
        {progress.state === 'cancelled' && <Chip color={SLATE}>cancelled</Chip>}
        {progress.currentFile && (
          <span className="font-mono text-[11px] text-text-secondary truncate">
            {progress.currentFile}
          </span>
        )}
      </div>
    </div>
  );
}

interface ModelsSectionProps {
  settings: MlxEngineSettings | null;
  models: MlxLocalModel[];
  mountedModelId: string | null;
  refreshModels: () => void;
  saveSettings: (next: MlxEngineSettings) => Promise<void>;
}

function ModelsSection({
  settings,
  models,
  mountedModelId,
  refreshModels,
  saveSettings,
}: ModelsSectionProps) {
  const [dirDialogOpen, setDirDialogOpen] = useState(false);
  const [dirSaving, setDirSaving] = useState(false);
  const [dirError, setDirError] = useState<string | null>(null);

  const [query, setQuery] = useState('');
  const [searching, setSearching] = useState(false);
  const [searchError, setSearchError] = useState<string | null>(null);
  const [hits, setHits] = useState<MlxHfModelHit[] | null>(null);

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
        await saveSettings({ ...settings, modelsDir: dir });
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

  const runSearch = useCallback(async () => {
    const q = query.trim();
    if (!q) return;
    setSearching(true);
    setSearchError(null);
    try {
      setHits(await mlxEngineHfSearch(q, 25));
    } catch (error) {
      setSearchError(errorMessage(error, 'Hugging Face search failed.'));
    } finally {
      setSearching(false);
    }
  }, [query]);

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
      <div
        className="border border-border-primary bg-background-primary p-4 flex flex-col gap-2"
        style={{ borderRadius: 3 }}
      >
        <span className="text-sm font-semibold text-text-primary">Models folder</span>
        <div className="flex items-center gap-2">
          <Folder className="w-4 h-4 shrink-0" style={{ color: AZURE }} />
          <span
            className="flex-1 min-w-0 font-mono text-sm text-text-primary truncate border border-border-primary px-2.5 py-1.5 bg-background-secondary"
            style={{ borderRadius: 3 }}
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
            style={{ borderRadius: 3 }}
          >
            <Pencil className="w-3.5 h-3.5" />
            Edit
          </Button>
        </div>
        <span className="text-xs text-text-secondary">
          One directory used by downloads and mounts alike.
        </span>
      </div>

      {/* Hugging Face search */}
      <div
        className="border border-border-primary bg-background-primary p-4 flex flex-col gap-3"
        style={{ borderRadius: 3 }}
      >
        <span className="text-sm font-semibold text-text-primary">Hugging Face</span>
        <div className="flex items-center gap-2">
          <Input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') void runSearch();
            }}
            placeholder="Search MLX models (e.g. qwen, mlx-community)…"
            className="text-sm"
            style={{ borderRadius: 3 }}
            aria-label="Search Hugging Face"
          />
          <Button
            onClick={() => void runSearch()}
            disabled={searching || query.trim() === ''}
            className="font-bold text-white hover:opacity-90 shrink-0"
            style={{ backgroundColor: AZURE, borderRadius: 3 }}
          >
            {searching ? (
              <Loader2 className="w-4 h-4 animate-spin" />
            ) : (
              <Search className="w-4 h-4" />
            )}
            Search
          </Button>
        </div>
        {searchError && <SolidBanner color={RED} label="Search failed" text={searchError} />}
        {hits != null && hits.length === 0 && !searching && (
          <span className="text-sm text-text-secondary">No results for this query.</span>
        )}
        {hits != null && hits.length > 0 && (
          <div className="flex flex-col">
            {hits.map((hit) => {
              const progress = downloads[hit.id];
              const failed = progress?.state === 'failed';
              const startError = downloadErrors[hit.id];
              return (
                <div
                  key={hit.id}
                  className="border border-border-primary px-3 py-2.5 mt-1.5 first:mt-0"
                  style={{
                    borderRadius: 3,
                    ...(failed || startError ? { borderColor: RED, borderWidth: 2 } : {}),
                  }}
                >
                  <div className="flex items-center gap-3 min-w-0">
                    <span className="font-mono text-sm font-medium text-text-primary truncate flex-1 min-w-0">
                      {hit.id}
                    </span>
                    <span
                      className="text-xs font-bold tabular-nums shrink-0"
                      style={{ color: AZURE }}
                      title={`${hit.downloads.toLocaleString()} downloads`}
                    >
                      ↓ {formatCount(hit.downloads)}
                    </span>
                    <span
                      className="text-xs font-bold tabular-nums shrink-0"
                      style={{ color: VIOLET }}
                      title={`${hit.likes.toLocaleString()} likes`}
                    >
                      ♥ {formatCount(hit.likes)}
                    </span>
                    <span className="text-xs text-text-secondary tabular-nums shrink-0">
                      {formatDate(hit.updatedAt)}
                    </span>
                    {!progress && (
                      <Button
                        size="sm"
                        onClick={() => void startDownload(hit.id)}
                        className="font-bold text-white hover:opacity-90 shrink-0"
                        style={{ backgroundColor: GREEN, borderRadius: 3 }}
                      >
                        <Download className="w-3.5 h-3.5" />
                        Download
                      </Button>
                    )}
                    {progress?.state === 'failed' && (
                      <Button
                        size="sm"
                        onClick={() => void startDownload(hit.id)}
                        className="font-bold text-white hover:opacity-90 shrink-0"
                        style={{ backgroundColor: RED, borderRadius: 3 }}
                      >
                        <RefreshCw className="w-3.5 h-3.5" />
                        Retry
                      </Button>
                    )}
                  </div>
                  {startError && (
                    <div className="text-xs font-semibold mt-1 break-words" style={{ color: RED }}>
                      {startError}
                    </div>
                  )}
                  {progress && progress.state !== 'failed' && (
                    <DownloadProgressRow
                      repoId={hit.id}
                      progress={progress}
                      onCancel={() => void cancelDownload(hit.id)}
                    />
                  )}
                  {progress?.state === 'failed' && (
                    <div className="text-xs font-semibold mt-1 break-words" style={{ color: RED }}>
                      {progress.error ?? 'Download failed.'}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </div>

      {/* Local models */}
      <div
        className="border border-border-primary bg-background-primary p-4 flex flex-col gap-3"
        style={{ borderRadius: 3 }}
      >
        <div className="flex items-center gap-2">
          <span className="text-sm font-semibold text-text-primary">Downloaded models</span>
          <Chip color={AZURE}>{models.length}</Chip>
          <Button
            size="xs"
            variant="outline"
            onClick={refreshModels}
            className="ml-auto"
            style={{ borderRadius: 3 }}
          >
            <RefreshCw className="w-3 h-3" />
            Refresh
          </Button>
        </div>
        {deleteError && <SolidBanner color={RED} label="Delete failed" text={deleteError} />}
        {models.length === 0 ? (
          <span className="text-sm text-text-secondary">
            Nothing downloaded yet — search Hugging Face above.
          </span>
        ) : (
          <div className="flex flex-col">
            {models.map((model) => (
              <div
                key={model.id}
                className="flex items-center gap-3 border border-border-primary px-3 py-2.5 mt-1.5 first:mt-0 min-w-0"
                style={{ borderRadius: 3 }}
              >
                <HardDrive className="w-4 h-4 shrink-0" style={{ color: model.complete ? AZURE : AMBER }} />
                <span className="font-mono text-sm text-text-primary truncate flex-1 min-w-0">
                  {model.id}
                </span>
                {model.id === mountedModelId && <Chip color={GREEN}>mounted</Chip>}
                {!model.complete && <Chip color={AMBER}>partial download</Chip>}
                <span className="text-xs font-bold tabular-nums shrink-0" style={{ color: AZURE }}>
                  {formatGb(model.sizeBytes)}
                </span>
                <Button
                  size="xs"
                  onClick={() => {
                    setDeleteError(null);
                    setPendingDelete(model);
                  }}
                  className="font-bold text-white hover:opacity-90 shrink-0"
                  style={{ backgroundColor: RED, borderRadius: 3 }}
                  aria-label={`Delete ${model.id}`}
                >
                  <Trash2 className="w-3 h-3" />
                </Button>
              </div>
            ))}
          </div>
        )}
      </div>

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

type MlxTab = 'engine' | 'models';

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

  const [drafts, setDrafts] = useState<NumericDrafts | null>(null);
  const [savedDrafts, setSavedDrafts] = useState<NumericDrafts | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  const defaultedPicker = useRef(false);

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
        const s = await mlxEngineSettingsRead();
        setSettings(s);
        const d = draftsFromSettings(s);
        setDrafts(d);
        setSavedDrafts(d);
      } catch (error) {
        setSaveError(errorMessage(error, 'Could not read the engine settings.'));
      }
    })();
  }, [refreshModels]);

  // Default the picker once to the mounted (or persisted) model without clobbering a user pick.
  useEffect(() => {
    if (defaultedPicker.current) return;
    const candidate = status?.modelId ?? settings?.modelId;
    if (candidate) {
      defaultedPicker.current = true;
      setMountModelId(candidate);
    }
  }, [status?.modelId, settings?.modelId]);

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

  const setDraft = useCallback((key: NumericSettingKey, text: string) => {
    setDrafts((prev) => (prev ? { ...prev, [key]: text } : prev));
  }, []);

  const saveSettings = useCallback(
    async (next: MlxEngineSettings) => {
      const saved = await mlxEngineSettingsUpdate(next);
      setSettings(saved);
      const d = draftsFromSettings(saved);
      setDrafts(d);
      setSavedDrafts(d);
      void refreshStatus();
    },
    [refreshStatus]
  );

  const onSaveSettings = useCallback(() => {
    if (!settings || !drafts) return;
    void (async () => {
      setSaving(true);
      setSaveError(null);
      try {
        await saveSettings(settingsWithDrafts(settings, drafts));
      } catch (error) {
        setSaveError(errorMessage(error, 'Could not save settings.'));
      } finally {
        setSaving(false);
      }
    })();
  }, [settings, drafts, saveSettings]);

  const tabBtn = (t: MlxTab, label: string, extra?: React.ReactNode) => {
    const active = tab === t;
    return (
      <button
        type="button"
        onClick={() => setTab(t)}
        className={`px-4 py-1.5 text-sm inline-flex items-center gap-2 ${
          active ? 'font-bold text-white' : 'text-text-secondary hover:text-text-primary'
        }`}
        style={{ backgroundColor: active ? AZURE : 'transparent', borderRadius: 3 }}
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
        <div className="bg-background-primary px-8 pb-6 pt-16">
          <div className="flex flex-col page-transition">
            <div className="flex items-center gap-3 mb-1">
              <span
                className="inline-flex items-center justify-center w-9 h-9 shrink-0"
                style={{ backgroundColor: AZURE, borderRadius: 3 }}
              >
                <Cpu className="w-5 h-5 text-white" />
              </span>
              <h1 className="text-4xl font-light">MLX Engine</h1>
              {status && <StateBadge state={status.state} />}
            </div>
            <p className="text-sm text-text-secondary mb-3">
              The in-house supervised MLX sidecar: mount and unmount models, tune sampling, and
              pull models straight from Hugging Face.
            </p>
            <div
              className="inline-flex self-start border border-border-primary p-0.5 gap-0.5"
              style={{ borderRadius: 3 }}
            >
              {tabBtn('engine', 'Engine')}
              {tabBtn(
                'models',
                'Models',
                <span
                  className="text-[10px] font-bold tabular-nums px-1 py-px text-white"
                  style={{ backgroundColor: tab === 'models' ? '#1a1a1a' : SLATE, borderRadius: 3 }}
                >
                  {models.length}
                </span>
              )}
            </div>
          </div>
        </div>

        <div className="flex-1 min-h-0 relative px-8">
          <ScrollArea className="h-full">
            {tab === 'engine' ? (
              <EngineSection
                status={status}
                statusError={statusError}
                settings={settings}
                models={models}
                mountModelId={mountModelId}
                setMountModelId={setMountModelId}
                mountError={mountError}
                engineBusy={engineBusy}
                onMount={onMount}
                onUnmount={onUnmount}
                onRemount={onRemount}
                drafts={drafts}
                setDraft={setDraft}
                savedDrafts={savedDrafts}
                onSaveSettings={onSaveSettings}
                saving={saving}
                saveError={saveError}
              />
            ) : (
              <ModelsSection
                settings={settings}
                models={models}
                mountedModelId={status?.modelId ?? null}
                refreshModels={refreshModels}
                saveSettings={saveSettings}
              />
            )}
          </ScrollArea>
        </div>
      </div>
    </MainPanelLayout>
  );
};

export default MlxEngineView;
