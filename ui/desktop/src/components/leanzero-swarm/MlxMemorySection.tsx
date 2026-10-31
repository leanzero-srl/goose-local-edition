  const target = isSelf ? undefined : nodeId;
  const reachable = isSelf || nodeId != null;
  const readMachine = useCallback(() => {
    if (!reachable) return;
    mlxEngineMemoryMachine(target)
      .then((m) => {
        setMachine(m);
        setMachineError(null);
      })
      .catch((e) => setMachineError(mlxErrorMessage(e, intl.formatMessage(i18n.readFailed))));
  }, [intl, reachable, target]);

  useEffect(() => {
    setMachine(null);
    if (!reachable) return;
    readMachine();
    const timer = window.setInterval(readMachine, target ? 15000 : 5000);
    return () => window.clearInterval(timer);
  }, [reachable, target, readMachine]);

// The Memory tab: every memory number the engine and macOS use on the picked Mac, editable, with
// its unit, valid range, engine flag and live effect. Presets only fill the fields; nothing is
// saved until Save, and the engine restarts only on Apply (never while it serves requests).
import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react';
import { Loader2, Minus, Plus, RefreshCw, X } from 'lucide-react';
import {
  Button,
  Chip,
  Disclosure,
  KeyValue,
  Panel,
  FOCUS,
  MOTION,
  RADIUS,
  SURFACE,
  TNUM,
  TONE_TEXT,
  TYPE,
  WEIGHT,
  cx,
  type KeyValueItem,
} from '../lz';
import { INPUT, StudioSwitch, ToneBanner } from './studio';
import {
  mlxEngineMemoryMachine,
  mlxEngineWiredLimitApply,
  mlxEngineWiredLimitDisable,
  mlxEngineWiredLimitRemoveDaemon,
  type MlxEngineMemoryBudget,
  type MlxEngineSettings,
  type MlxEngineStatus,
  type MlxMemoryMachine,
} from '../../acp/mlx-engine';
import { mlxErrorMessage } from './mlxErrorMessage';
import type { IntlShape } from 'react-intl';
import { defineMessages, useIntl } from '../../i18n';
import {
  ENGINE_AUTO_CEILING,
  ENGINE_AUTO_FLOOR,
  ENGINE_AUTO_HEADROOM,
  ENGINE_PREFILL_STEP,
  ENGINE_SYSTEM_RESERVE,
  GB,
  GOOSE_MAX_CONCURRENT,
  MEMORY_FIELDS,
  defaultFloorGb,
  draftsFromMemory,
  memoryConsequences,
  memoryDraftsEqual,
  parseMemoryDrafts,
  WIRED_DEFAULT_MIN_RESERVE_GIB,
  physicalMb,
  presetDrafts,
  suggestedWiredMb,
  wiredFallbackCommand,
  wiredLimitCheck,
  wiredLimitCommand,
  wiredMaxMb,
  type MemoryDrafts,
  type MemoryFieldSpec,
  type MemoryKey,
  type MemoryProblem,
  type PresetId,
} from './memoryTunables';

const i18n = defineMessages({
  intro: {
    id: 'mlxMemory.intro',
    defaultMessage:
      'Every memory number the engine plans under on this Mac. A blank field keeps the default shown beside it. These settings live on the Mac they belong to.',
  },
  presetsLabel: { id: 'mlxMemory.presetsLabel', defaultMessage: 'Fill in starting values' },
  presetWorkstation: { id: 'mlxMemory.presetWorkstation', defaultMessage: 'Workstation' },
  presetInferenceBox: { id: 'mlxMemory.presetInferenceBox', defaultMessage: 'Inference box' },
  presetsNote: {
    id: 'mlxMemory.presetsNote',
    defaultMessage:
      'Workstation: automatic cap, floor 8% of RAM. Inference box: cap = RAM in GB minus 16 (112 GB on 128 GB), floor 4 GB. A preset only fills the fields; nothing changes until you save.',
  },
  livePanel: { id: 'mlxMemory.livePanel', defaultMessage: 'Memory now' },
  livePhysical: { id: 'mlxMemory.livePhysical', defaultMessage: 'Physical RAM' },
  liveWorkingSet: { id: 'mlxMemory.liveWorkingSet', defaultMessage: 'macOS GPU working set' },
  liveWired: { id: 'mlxMemory.liveWired', defaultMessage: 'macOS GPU wired limit' },
  liveCap: { id: 'mlxMemory.liveCap', defaultMessage: 'Engine cap' },
  liveCapValue: {
    id: 'mlxMemory.liveCapValue',
    defaultMessage: '{gb} · {source} · bound by {binding}',
  },
  liveActive: { id: 'mlxMemory.liveActive', defaultMessage: 'Engine active / peak' },
  liveOthers: {
    id: 'mlxMemory.liveOthers',
    defaultMessage: 'Other apps + macOS + the engine outside the GPU',
  },
  liveOthersValue: {
    id: 'mlxMemory.liveOthersValue',
    defaultMessage: '{gb} (engine outside the GPU: {engine})',
  },
  liveFileCache: {
    id: 'mlxMemory.liveFileCache',
    defaultMessage: 'File cache (macOS frees it on demand; not counted)',
  },
  liveFree: { id: 'mlxMemory.liveFree', defaultMessage: 'Free' },
  liveKv: { id: 'mlxMemory.liveKv', defaultMessage: 'KV cache per token' },
  liveKvValue: {
    id: 'mlxMemory.liveKvValue',
    defaultMessage: '{bytes} bytes · {gbPer100k} GB per 100k tokens',
  },
  liveMaxContext: { id: 'mlxMemory.liveMaxContext', defaultMessage: 'Max context at the ceiling' },
  liveUnknown: { id: 'mlxMemory.liveUnknown', defaultMessage: 'not reported' },
  liveEngineStopped: {
    id: 'mlxMemory.liveEngineStopped',
    defaultMessage: 'engine not running',
  },
  wiredDefault: { id: 'mlxMemory.wiredDefault', defaultMessage: '0 (macOS default)' },
  bindingEngine: { id: 'mlxMemory.bindingEngine', defaultMessage: 'the engine cap' },
  bindingMacos: { id: 'mlxMemory.bindingMacos', defaultMessage: 'the macOS working set' },
  bindingFloor: { id: 'mlxMemory.bindingFloor', defaultMessage: 'the free-memory floor' },
  tunablesPanel: { id: 'mlxMemory.tunablesPanel', defaultMessage: 'Engine memory' },
  advanced: {
    id: 'mlxMemory.advanced',
    defaultMessage: 'Advanced: the automatic cap and the soft reserve',
  },
  unsaved: { id: 'mlxMemory.unsaved', defaultMessage: 'unsaved' },
  save: { id: 'mlxMemory.save', defaultMessage: 'Save' },
  apply: { id: 'mlxMemory.apply', defaultMessage: 'Apply: restart the engine' },
  saveFailed: { id: 'mlxMemory.saveFailed', defaultMessage: 'Save failed' },
  restartNeeded: { id: 'mlxMemory.restartNeeded', defaultMessage: 'Restart needed' },
  restartNeededText: {
    id: 'mlxMemory.restartNeededText',
    defaultMessage:
      'The running engine still uses its old memory settings. Apply restarts it with these; nothing restarts on its own.',
  },
  savedStopped: {
    id: 'mlxMemory.savedStopped',
    defaultMessage:
      'Every change here needs an engine restart; the engine is not running, so they apply at its next start.',
  },
  busy: {
    id: 'mlxMemory.busy',
    defaultMessage:
      'The engine is serving {count, plural, one {# request} other {# requests}}: Apply waits until it is idle.',
  },
  invalid: { id: 'mlxMemory.invalid', defaultMessage: 'Not possible' },
  needsEngine: {
    id: 'mlxMemory.needsEngine',
    defaultMessage:
      'This Mac launches {launcher}. The fields marked lz.20 need Rapid-MLX v0.14.3-lz.20 or newer: a start with any of them set is refused by name.',
  },
  needsEngineChip: { id: 'mlxMemory.needsEngineChip', defaultMessage: 'lz.20' },
  consequences: {
    id: 'mlxMemory.consequences',
    defaultMessage:
      'At this cap: max context ~{tokens} tokens, {free} free for macOS and other apps.',
  },
  consequencesNoKv: {
    id: 'mlxMemory.consequencesNoKv',
    defaultMessage:
      'At this cap: {free} free for macOS and other apps. The max context shows once the engine runs and reports its KV per token.',
  },
  consequencesAuto: {
    id: 'mlxMemory.consequencesAuto',
    defaultMessage:
      'Automatic cap: the engine computes it at start from the model and the context.',
  },
  capAboveWorkingSet: {
    id: 'mlxMemory.capAboveWorkingSet',
    defaultMessage:
      'The cap {cap} is above the macOS GPU working set ({ws}): macOS binds first unless the wired limit is raised (suggested {mb} MB, below).',
  },
  // field labels
  'label.metalLimitGb': { id: 'mlxMemory.label.metalLimitGb', defaultMessage: 'Engine Metal cap' },
  'label.memoryFloorGb': {
    id: 'mlxMemory.label.memoryFloorGb',
    defaultMessage: 'Free-memory floor',
  },
  'label.prefixCacheGb': {
    id: 'mlxMemory.label.prefixCacheGb',
    defaultMessage: 'Prefix-cache budget',
  },
  'label.prefillStepSize': {
    id: 'mlxMemory.label.prefillStepSize',
    defaultMessage: 'Max prefill chunk',
  },
  'label.maxContextTokens': {
    id: 'mlxMemory.label.maxContextTokens',
    defaultMessage: 'Max context (admitted window)',
  },
  'label.guardMarginGb': {
    id: 'mlxMemory.label.guardMarginGb',
    defaultMessage: 'Guard safety margin',
  },
  'label.maxConcurrentRequests': {
    id: 'mlxMemory.label.maxConcurrentRequests',
    defaultMessage: 'Max concurrent requests',
  },
  'label.autoUtilizationFloor': {
    id: 'mlxMemory.label.autoUtilizationFloor',
    defaultMessage: 'Auto cap floor',
  },
  'label.autoUtilizationCeiling': {
    id: 'mlxMemory.label.autoUtilizationCeiling',
    defaultMessage: 'Auto cap ceiling',
  },
  'label.autoHeadroomFraction': {
    id: 'mlxMemory.label.autoHeadroomFraction',
    defaultMessage: 'Auto headroom',
  },
  'label.systemReserveFraction': {
    id: 'mlxMemory.label.systemReserveFraction',
    defaultMessage: 'Soft reserve',
  },
  unitGb: { id: 'mlxMemory.unitGb', defaultMessage: 'GB' },
  unitTokens: { id: 'mlxMemory.unitTokens', defaultMessage: 'tokens' },
  unitRequests: { id: 'mlxMemory.unitRequests', defaultMessage: 'requests' },
  unitFraction: { id: 'mlxMemory.unitFraction', defaultMessage: 'share' },
  rangeOpen: { id: 'mlxMemory.rangeOpen', defaultMessage: 'valid: {min} or more' },
  rangeBetween: { id: 'mlxMemory.rangeBetween', defaultMessage: 'valid: {min} to {max}' },
  rangeAboveBelow: {
    id: 'mlxMemory.rangeAboveBelow',
    defaultMessage: 'valid: above {min}, up to {max}',
  },
  rangeBelow: { id: 'mlxMemory.rangeBelow', defaultMessage: 'valid: {min} to under {max}' },
  clear: { id: 'mlxMemory.clear', defaultMessage: 'Clear' },
  decrease: { id: 'mlxMemory.decrease', defaultMessage: 'Decrease {label}' },
  increase: { id: 'mlxMemory.increase', defaultMessage: 'Increase {label}' },
  defaultAuto: {
    id: 'mlxMemory.defaultAuto',
    defaultMessage: 'auto: the engine computes it at start',
  },
  defaultAutoLive: { id: 'mlxMemory.defaultAutoLive', defaultMessage: 'auto: {gb} now' },
  defaultFloor: {
    id: 'mlxMemory.defaultFloor',
    defaultMessage: 'goose default {gb} GB (5% of RAM)',
  },
  defaultEngineAuto: { id: 'mlxMemory.defaultEngineAuto', defaultMessage: 'engine auto' },
  defaultEngineAutoLive: {
    id: 'mlxMemory.defaultEngineAutoLive',
    defaultMessage: 'engine auto: {gb} now',
  },
  defaultValue: { id: 'mlxMemory.defaultValue', defaultMessage: 'engine default {value}' },
  defaultGoose: { id: 'mlxMemory.defaultGoose', defaultMessage: 'goose default {value}' },
  defaultContext: {
    id: 'mlxMemory.defaultContext',
    defaultMessage: "the model's configured context, else its maximum",
  },
  defaultContextLive: {
    id: 'mlxMemory.defaultContextLive',
    defaultMessage: 'engine plans {tokens} tokens now',
  },
  defaultMargin: { id: 'mlxMemory.defaultMargin', defaultMessage: 'auto: 2% of the ceiling' },
  // problems
  problemNotNumber: { id: 'mlxMemory.problemNotNumber', defaultMessage: '{label}: not a number' },
  problemNotInteger: {
    id: 'mlxMemory.problemNotInteger',
    defaultMessage: '{label}: must be a whole number',
  },
  problemRange: { id: 'mlxMemory.problemRange', defaultMessage: '{label}: {range}' },
  problemCapPlusFloor: {
    id: 'mlxMemory.problemCapPlusFloor',
    defaultMessage: 'Cap {cap} GB + floor {floor} GB is more than this Mac’s {physical} GB of RAM.',
  },
  problemFloorAboveCeiling: {
    id: 'mlxMemory.problemFloorAboveCeiling',
    defaultMessage: 'Auto cap floor {floor} is above the auto cap ceiling {ceiling}.',
  },
  problemKvOverCap: {
    id: 'mlxMemory.problemKvOverCap',
    defaultMessage:
      'A {tokens}-token window’s KV cache alone is {kv} GB, more than the {cap} GB cap.',
  },
  // wired limit
  wiredPanel: { id: 'mlxMemory.wiredPanel', defaultMessage: 'macOS GPU wired limit' },
  wiredIntro: {
    id: 'mlxMemory.wiredIntro',
    defaultMessage:
      'How much RAM macOS lets the GPU wire, which bounds every engine on {mac}. Enable and Disable each run one command as administrator: macOS asks for the password on {mac} and goose never sees or stores it. The limit resets at restart unless you keep it. Metal reads it when a process starts, so an engine sees a new value at its next start.',
  },
  wiredStateRaised: { id: 'mlxMemory.wiredStateRaised', defaultMessage: 'Raised to {mb} MB' },
  wiredStateDefault: { id: 'mlxMemory.wiredStateDefault', defaultMessage: 'macOS default' },
  wiredStateKept: { id: 'mlxMemory.wiredStateKept', defaultMessage: 'Kept after restart' },
  wiredCurrent: {
    id: 'mlxMemory.wiredCurrent',
    defaultMessage: 'Now (sysctl iogpu.wired_limit_mb)',
  },
  wiredBoot: { id: 'mlxMemory.wiredBoot', defaultMessage: 'Boot setting (LaunchDaemon)' },
  wiredBootNone: { id: 'mlxMemory.wiredBootNone', defaultMessage: 'not installed' },
  wiredOthers: { id: 'mlxMemory.wiredOthers', defaultMessage: 'In use outside the GPU now' },
  wiredReserve: { id: 'mlxMemory.wiredReserve', defaultMessage: 'Minimum reserve for macOS' },
  wiredReserveValue: {
    id: 'mlxMemory.wiredReserveValue',
    defaultMessage: '{gib} GiB (GOOSE_WIRED_LIMIT_MIN_RESERVE_GIB)',
  },
  wiredValue: { id: 'mlxMemory.wiredValue', defaultMessage: 'New limit' },
  wiredRangeReserve: {
    id: 'mlxMemory.wiredRangeReserve',
    defaultMessage: 'MB ({gib} GiB) · 8192 to {max}',
  },
  wiredKeep: {
    id: 'mlxMemory.wiredKeep',
    defaultMessage: 'macOS and other apps keep {keep} GiB of {physical} GiB',
  },
  wiredBelowReserve: {
    id: 'mlxMemory.wiredBelowReserve',
    defaultMessage:
      'Refused: that leaves {keep} GiB, under the {reserve} GiB minimum. The MLX engine also needs RAM outside the GPU (about 12 GB measured under load) and the kernel and system about 10 GB more. The highest allowed here is {max} MB.',
  },
  wiredBelowOthers: {
    id: 'mlxMemory.wiredBelowOthers',
    defaultMessage:
      'That leaves {keep} GiB, less than {mac} uses outside the GPU right now ({others} GiB) plus the {reserve} GiB reserve: expect memory pressure and swapping.',
  },
  wiredInvalidValue: {
    id: 'mlxMemory.wiredInvalidValue',
    defaultMessage: 'Use a whole number from 8192 to {max} MB, or Disable for the macOS default.',
  },
  wiredSuggested: {
    id: 'mlxMemory.wiredSuggested',
    defaultMessage:
      'Suggested from measurement: {mb} MB (keeps {others} GiB in use now + the {reserve} GiB reserve)',
  },
  wiredSuggestedLow: {
    id: 'mlxMemory.wiredSuggestedLow',
    defaultMessage:
      'The macOS default already gives the GPU {ws}: on what {mac} uses now, a raise above {mb} MB would cut into the reserve.',
  },
  wiredNoSuggestion: {
    id: 'mlxMemory.wiredNoSuggestion',
    defaultMessage:
      'No raise suggested: what {mac} uses now plus the {reserve} GiB reserve leaves too little to wire.',
  },
  wiredUseSuggestion: { id: 'mlxMemory.wiredUseSuggestion', defaultMessage: 'Use' },
  wiredPersistLabel: { id: 'mlxMemory.wiredPersistLabel', defaultMessage: 'Keep after restart' },
  wiredPersistNote: {
    id: 'mlxMemory.wiredPersistNote',
    defaultMessage: 'Installs a LaunchDaemon that sets the value at every boot; Disable removes it.',
  },
  wiredCommand: { id: 'mlxMemory.wiredCommand', defaultMessage: 'Runs as administrator' },
  wiredEnable: { id: 'mlxMemory.wiredEnable', defaultMessage: 'Enable' },
  wiredDisable: { id: 'mlxMemory.wiredDisable', defaultMessage: 'Disable (macOS default)' },
  wiredRemove: { id: 'mlxMemory.wiredRemove', defaultMessage: 'Remove boot setting' },
  wiredRemoveNote: {
    id: 'mlxMemory.wiredRemoveNote',
    defaultMessage: 'Removing the boot setting leaves the current value until the next reboot.',
  },
  wiredWaiting: {
    id: 'mlxMemory.wiredWaiting',
    defaultMessage:
      'Waiting for the administrator prompt on {mac}: whoever is at it approves or cancels. Nothing changes until then.',
  },
  wiredDone: { id: 'mlxMemory.wiredDone', defaultMessage: 'Done' },
  wiredDoneText: {
    id: 'mlxMemory.wiredDoneText',
    defaultMessage: 'The wired limit on {mac} reads {mb} MB now.',
  },
  wiredDoneDefault: {
    id: 'mlxMemory.wiredDoneDefault',
    defaultMessage: 'The wired limit on {mac} is back to the macOS default.',
  },
  wiredFailed: { id: 'mlxMemory.wiredFailed', defaultMessage: 'Not changed' },
  wiredFallback: {
    id: 'mlxMemory.wiredFallback',
    defaultMessage: 'Or run this in Terminal on {mac}:',
  },
  readFailed: { id: 'mlxMemory.readFailed', defaultMessage: 'Could not read this Mac’s memory' },
});

type Messages = typeof i18n;
const label = (intl: IntlShape, key: MemoryKey) =>
  intl.formatMessage(i18n[`label.${key}` as keyof Messages]);

const gbText = (intl: IntlShape, bytes: number | null | undefined) =>
  bytes == null
    ? intl.formatMessage(i18n.liveUnknown)
    : `${intl.formatNumber(bytes / GB, { maximumFractionDigits: 1 })} GB`;

function unitText(intl: IntlShape, spec: MemoryFieldSpec): string {
  switch (spec.unit) {
    case 'gb':
      return intl.formatMessage(i18n.unitGb);
    case 'tokens':
      return intl.formatMessage(i18n.unitTokens);
    case 'requests':
      return intl.formatMessage(i18n.unitRequests);
    case 'fraction':
      return intl.formatMessage(i18n.unitFraction);
  }
}

function rangeText(intl: IntlShape, spec: MemoryFieldSpec, physicalGb: number | null): string {
  const max = spec.max(physicalGb);
  const fmt = (n: number) => intl.formatNumber(n, { maximumFractionDigits: 2 });
  if (max == null) return intl.formatMessage(i18n.rangeOpen, { min: fmt(spec.min) });
  if (spec.exclusiveMin) {
    return intl.formatMessage(i18n.rangeAboveBelow, { min: fmt(spec.min), max: fmt(max) });
  }
  if (spec.exclusiveMax) {
    return intl.formatMessage(i18n.rangeBelow, { min: fmt(spec.min), max: fmt(max) });
  }
  return intl.formatMessage(i18n.rangeBetween, { min: fmt(spec.min), max: fmt(max) });
}

export function problemText(
  intl: IntlShape,
  problem: MemoryProblem,
  physicalGb: number | null
): string {
  const fmt = (n: number) => intl.formatNumber(n, { maximumFractionDigits: 1 });
  switch (problem.kind) {
    case 'notNumber':
      return intl.formatMessage(i18n.problemNotNumber, { label: label(intl, problem.key) });
    case 'notInteger':
      return intl.formatMessage(i18n.problemNotInteger, { label: label(intl, problem.key) });
    case 'range': {
      const spec = MEMORY_FIELDS.find((f) => f.key === problem.key)!;
      return intl.formatMessage(i18n.problemRange, {
        label: label(intl, problem.key),
        range: rangeText(intl, spec, physicalGb),
      });
    }
    case 'capPlusFloor':
      return intl.formatMessage(i18n.problemCapPlusFloor, {
        cap: fmt(problem.capGb),
        floor: fmt(problem.floorGb),
        physical: fmt(problem.physicalGb),
      });
    case 'floorAboveCeiling':
      return intl.formatMessage(i18n.problemFloorAboveCeiling, {
        floor: problem.floor,
        ceiling: problem.ceiling,
      });
    case 'kvOverCap':
      return intl.formatMessage(i18n.problemKvOverCap, {
        tokens: intl.formatNumber(problem.tokens),
        kv: fmt(problem.kvGb),
        cap: fmt(problem.capGb),
      });
  }
}

function defaultText(
  intl: IntlShape,
  key: MemoryKey,
  budget: MlxEngineMemoryBudget | null,
  physicalBytes: number | null
): string {
  switch (key) {
    case 'metalLimitGb':
      return budget?.limitBytes != null
        ? intl.formatMessage(i18n.defaultAutoLive, { gb: gbText(intl, budget.limitBytes) })
        : intl.formatMessage(i18n.defaultAuto);
    case 'memoryFloorGb':
      return physicalBytes != null
        ? intl.formatMessage(i18n.defaultFloor, { gb: defaultFloorGb(physicalBytes) })
        : intl.formatMessage(i18n.defaultEngineAuto);
    case 'prefixCacheGb':
      return budget?.prefixBudgetBytes != null
        ? intl.formatMessage(i18n.defaultEngineAutoLive, {
            gb: gbText(intl, budget.prefixBudgetBytes),
          })
        : intl.formatMessage(i18n.defaultEngineAuto);
    case 'prefillStepSize':
      return intl.formatMessage(i18n.defaultValue, { value: ENGINE_PREFILL_STEP });
    case 'maxContextTokens':
      return budget?.planContextTokens != null
        ? intl.formatMessage(i18n.defaultContextLive, {
            tokens: intl.formatNumber(budget.planContextTokens),
          })
        : intl.formatMessage(i18n.defaultContext);
    case 'guardMarginGb':
      return intl.formatMessage(i18n.defaultMargin);
    case 'maxConcurrentRequests':
      return intl.formatMessage(i18n.defaultGoose, { value: GOOSE_MAX_CONCURRENT });
    case 'autoUtilizationFloor':
      return intl.formatMessage(i18n.defaultValue, { value: ENGINE_AUTO_FLOOR });
    case 'autoUtilizationCeiling':
      return intl.formatMessage(i18n.defaultValue, { value: ENGINE_AUTO_CEILING });
    case 'autoHeadroomFraction':
      return intl.formatMessage(i18n.defaultValue, { value: ENGINE_AUTO_HEADROOM });
    case 'systemReserveFraction':
      return intl.formatMessage(i18n.defaultValue, { value: ENGINE_SYSTEM_RESERVE });
  }
}

const STEP_BUTTON = cx(
  'flex size-7 shrink-0 items-center justify-center bg-lz-surface text-lz-ink-2 hover:bg-lz-surface-2 hover:text-lz-ink [&_svg]:size-3.5',
  SURFACE.outline,
  RADIUS.control,
  FOCUS,
  MOTION
);

function MemoryField({
  spec,
  text,
  onText,
  defaultLine,
  physicalGb,
  unsupported,
  problem,
}: {
  spec: MemoryFieldSpec;
  text: string;
  onText: (v: string) => void;
  defaultLine: string;
  physicalGb: number | null;
  unsupported: boolean;
  problem: string | null;
}) {
  const intl = useIntl();
  const name = label(intl, spec.key);
  const isSet = text.trim() !== '';
  const bump = (dir: 1 | -1) => {
    const base = isSet && Number.isFinite(Number(text)) ? Number(text) : 0;
    const next = Math.max(spec.min, base + dir * spec.step);
    onText(String(spec.integer ? Math.round(next) : Number(next.toFixed(4))));
  };
  return (
    <div
      data-testid={`mlx-memory-field-${spec.key}`}
      className={cx(
        'grid grid-cols-[minmax(160px,240px)_1fr] items-start gap-4 border-t py-2 first:border-t-0',
        SURFACE.hairline
      )}
    >
      <div className="flex flex-col gap-0.5 pt-1">
        <span className={cx('flex items-center gap-2', TYPE.body)}>
          {name}
          {unsupported && <Chip tone="warn">{intl.formatMessage(i18n.needsEngineChip)}</Chip>}
        </span>
        <span className={cx('font-mono', TYPE.meta)}>{spec.flag}</span>
      </div>
      <div className="flex min-w-0 flex-col gap-1">
        <div className="flex min-w-0 flex-wrap items-center gap-2">
          <button
            type="button"
            className={STEP_BUTTON}
            aria-label={intl.formatMessage(i18n.decrease, { label: name })}
            onClick={() => bump(-1)}
          >
            <Minus />
          </button>
          <input
            type="number"
            step={spec.step}
            value={text}
            onChange={(e) => onText(e.target.value)}
            className={cx(INPUT, 'w-36 text-right', TNUM, problem && 'border-lz-err')}
            aria-label={name}
            aria-invalid={problem != null}
          />
          <button
            type="button"
            className={STEP_BUTTON}
            aria-label={intl.formatMessage(i18n.increase, { label: name })}
            onClick={() => bump(1)}
          >
            <Plus />
          </button>
          <span className={cx(TYPE.meta, WEIGHT.semibold)}>{unitText(intl, spec)}</span>
          {isSet ? (
            <Button size="sm" variant="ghost" icon={<X />} onClick={() => onText('')}>
              {intl.formatMessage(i18n.clear)}
            </Button>
          ) : (
            <span className={TYPE.meta} data-testid={`mlx-memory-default-${spec.key}`}>
              {defaultLine}
            </span>
          )}
        </div>
        <span className={cx(TYPE.meta, TNUM)}>{rangeText(intl, spec, physicalGb)}</span>
        {problem && (
          <span className={cx('text-lz-meta', WEIGHT.semibold, TONE_TEXT.err)} role="alert">
            {problem}
          </span>
        )}
      </div>
    </div>
  );
}

function bindingText(intl: IntlShape, binding: string | undefined): string {
  switch (binding) {
    case 'engine':
      return intl.formatMessage(i18n.bindingEngine);
    case 'macos_working_set':
      return intl.formatMessage(i18n.bindingMacos);
    case 'memory_floor':
      return intl.formatMessage(i18n.bindingFloor);
    default:
      return binding ?? intl.formatMessage(i18n.liveUnknown);
  }
}

function LivePanel({
  machine,
  status,
}: {
  machine: {
    physicalBytes: number | null;
    availableBytes: number | null;
    workingSetBytes: number | null;
    wiredLimitMb: number | null;
  };
  status: MlxEngineStatus | null;
}) {
  const intl = useIntl();
  const budget = status?.memoryBudget ?? null;
  const running = status?.state === 'running';
  const stopped = intl.formatMessage(i18n.liveEngineStopped);
  const physical = budget?.physicalBytes ?? machine.physicalBytes;
  const others =
    budget?.othersBytes ??
    (!running && machine.physicalBytes != null && machine.availableBytes != null
      ? machine.physicalBytes - machine.availableBytes
      : null);
  const free = budget?.freeBytes ?? (running ? null : machine.availableBytes);
  const wired = budget?.wiredLimitMb ?? machine.wiredLimitMb;
  const items: KeyValueItem[] = [
    {
      key: 'physical',
      label: intl.formatMessage(i18n.livePhysical),
      value: gbText(intl, physical),
    },
    {
      key: 'ws',
      label: intl.formatMessage(i18n.liveWorkingSet),
      value: gbText(intl, budget?.deviceWorkingSetBytes ?? machine.workingSetBytes),
    },
    {
      key: 'wired',
      label: intl.formatMessage(i18n.liveWired),
      value:
        wired == null
          ? intl.formatMessage(i18n.liveUnknown)
          : wired === 0
            ? intl.formatMessage(i18n.wiredDefault)
            : `${intl.formatNumber(wired)} MB`,
    },
    {
      key: 'cap',
      label: intl.formatMessage(i18n.liveCap),
      tone: budget?.binding === 'engine' ? undefined : budget?.binding ? 'warn' : undefined,
      value: !running
        ? stopped
        : budget?.limitBytes == null
          ? intl.formatMessage(i18n.liveUnknown)
          : intl.formatMessage(i18n.liveCapValue, {
              gb: gbText(intl, budget.limitBytes),
              source: budget.source ?? '—',
              binding: bindingText(intl, budget.binding),
            }),
    },
    {
      key: 'active',
      label: intl.formatMessage(i18n.liveActive),
      value: !running
        ? stopped
        : `${gbText(intl, budget?.activeBytes)} / ${gbText(intl, budget?.peakBytes)}`,
    },
    {
      key: 'others',
      label: intl.formatMessage(i18n.liveOthers),
      value:
        budget?.othersEngineBytes == null
          ? gbText(intl, others)
          : intl.formatMessage(i18n.liveOthersValue, {
              gb: gbText(intl, others),
              engine: gbText(intl, budget.othersEngineBytes),
            }),
    },
    ...(budget?.fileCacheExcludedBytes == null
      ? []
      : [
          {
            key: 'filecache',
            label: intl.formatMessage(i18n.liveFileCache),
            value: gbText(intl, budget.fileCacheExcludedBytes),
          },
        ]),
    {
      key: 'free',
      label: intl.formatMessage(i18n.liveFree),
      tone: 'ok',
      value: gbText(intl, free),
    },
    {
      key: 'kv',
      label: intl.formatMessage(i18n.liveKv),
      value:
        budget?.kvBytesPerToken == null
          ? running
            ? intl.formatMessage(i18n.liveUnknown)
            : stopped
          : intl.formatMessage(i18n.liveKvValue, {
              bytes: intl.formatNumber(budget.kvBytesPerToken),
              gbPer100k: intl.formatNumber((budget.kvBytesPerToken * 100_000) / GB, {
                maximumFractionDigits: 2,
              }),
            }),
    },
    {
      key: 'maxctx',
      label: intl.formatMessage(i18n.liveMaxContext),
      value:
        budget?.maxContextAtCeiling == null
          ? running
            ? intl.formatMessage(i18n.liveUnknown)
            : stopped
          : intl.formatNumber(budget.maxContextAtCeiling),
    },
  ];
  return (
    <Panel title={intl.formatMessage(i18n.livePanel)}>
      <div data-testid="mlx-memory-live">
        <KeyValue items={items} dense />
      </div>
    </Panel>
  );
}

const gibText = (intl: IntlShape, mib: number) =>
  intl.formatNumber(mib / 1024, { maximumFractionDigits: 1 });

/** The picked Mac's wired limit: Enable (a value the person picks) or Disable (the macOS default
 * and no boot setting), each behind the macOS administrator prompt on THAT Mac. */
export function WiredLimitPanel({
  machine,
  macName,
  nodeId,
  othersBytes,
  onChanged,
}: {
  machine: MlxMemoryMachine;
  macName: string;
  /** A Link peer: every call is relayed and its app shows the prompt. */
  nodeId?: string;
  /** What the Mac uses outside the GPU: the running engine's own `othersBytes`, else physical − available. */
  othersBytes: number;
  onChanged: () => void;
}) {
  const intl = useIntl();
  const physical = machine.physicalBytes;
  const reserve = machine.minReserveGib ?? WIRED_DEFAULT_MIN_RESERVE_GIB;
  const max = wiredMaxMb(physical, reserve);
  const suggestion = suggestedWiredMb(physical, othersBytes, reserve);
  const current = machine.wiredLimitMb ?? null;
  const daemon = machine.daemonWiredLimitMb ?? null;
  const [text, setText] = useState(String(current || suggestion || max));
  const [persist, setPersist] = useState(daemon != null);
  const [running, setRunning] = useState(false);
  const [result, setResult] = useState<{ ok: boolean; text: string; mb?: number } | null>(null);
  const mb = Number(text.trim());
  const check =
    text.trim() === '' ? ({ kind: 'invalid' } as const) : wiredLimitCheck(mb, physical, reserve, othersBytes);
  const canEnable = check.kind === 'ok' || check.kind === 'belowOthers';
  const canDisable = (current != null && current !== 0) || daemon != null;
  const physicalGib = gibText(intl, physicalMb(physical));
  const othersGib = intl.formatNumber(othersBytes / 1024 ** 3, { maximumFractionDigits: 1 });
  const remote = nodeId != null;

  const run = (action: () => Promise<{ wiredLimitMb?: number }>, fallbackMb: number) => {
    setRunning(true);
    setResult(null);
    void action()
      .then((r) =>
        setResult({
          ok: true,
          text: r.wiredLimitMb
            ? intl.formatMessage(i18n.wiredDoneText, {
                mac: macName,
                mb: intl.formatNumber(r.wiredLimitMb),
              })
            : intl.formatMessage(i18n.wiredDoneDefault, { mac: macName }),
        })
      )
      .catch((e) => setResult({ ok: false, text: mlxErrorMessage(e, ''), mb: fallbackMb }))
      .finally(() => {
        setRunning(false);
        onChanged();
      });
  };

  return (
    <Panel
      title={intl.formatMessage(i18n.wiredPanel)}
      headerRight={
        <>
          <span data-testid="mlx-memory-wired-state" data-raised={current ? 'true' : 'false'}>
            <Chip tone={current ? 'ok' : undefined}>
              {current
                ? intl.formatMessage(i18n.wiredStateRaised, { mb: intl.formatNumber(current) })
                : intl.formatMessage(i18n.wiredStateDefault)}
            </Chip>
          </span>
          {daemon != null && <Chip tone="ok">{intl.formatMessage(i18n.wiredStateKept)}</Chip>}
        </>
      }
    >
      <div
        className="flex flex-col gap-3"
        data-testid="mlx-memory-wired"
        data-remote={remote ? 'true' : 'false'}
      >
        <p className={TYPE.bodyMuted}>{intl.formatMessage(i18n.wiredIntro, { mac: macName })}</p>
        <KeyValue
          dense
          items={[
            {
              key: 'now',
              label: intl.formatMessage(i18n.wiredCurrent),
              value:
                current == null
                  ? (machine.wiredLimitError ?? intl.formatMessage(i18n.liveUnknown))
                  : current === 0
                    ? intl.formatMessage(i18n.wiredDefault)
                    : `${intl.formatNumber(current)} MB`,
            },
            {
              key: 'ws',
              label: intl.formatMessage(i18n.liveWorkingSet),
              value: gbText(intl, machine.workingSetBytes),
            },
            {
              key: 'boot',
              label: intl.formatMessage(i18n.wiredBoot),
              value:
                daemon == null
                  ? (machine.daemonError ?? intl.formatMessage(i18n.wiredBootNone))
                  : `${intl.formatNumber(daemon)} MB`,
            },
            {
              key: 'others',
              label: intl.formatMessage(i18n.wiredOthers),
              value: `${othersGib} GiB`,
            },
            {
              key: 'reserve',
              label: intl.formatMessage(i18n.wiredReserve),
              value: intl.formatMessage(i18n.wiredReserveValue, { gib: reserve }),
            },
          ]}
        />
        <div className="flex flex-wrap items-center gap-2">
          <span className={TYPE.body}>{intl.formatMessage(i18n.wiredValue)}</span>
          <input
            type="number"
            step={1000}
            value={text}
            onChange={(e) => setText(e.target.value)}
            className={cx(INPUT, 'w-36 text-right', TNUM)}
            aria-label={intl.formatMessage(i18n.wiredValue)}
            aria-invalid={!canEnable}
            data-testid="mlx-memory-wired-input"
          />
          <span className={cx(TYPE.meta, TNUM)}>
            {intl.formatMessage(i18n.wiredRangeReserve, {
              gib: Number.isFinite(mb) ? gibText(intl, mb) : '—',
              max: intl.formatNumber(max),
            })}
          </span>
        </div>
        {'keepMb' in check && (
          <p
            className={cx(
              TYPE.body,
              WEIGHT.semibold,
              TNUM,
              check.kind === 'belowReserve'
                ? TONE_TEXT.err
                : check.kind === 'belowOthers'
                  ? TONE_TEXT.warn
                  : TONE_TEXT.ok
            )}
            data-testid="mlx-memory-wired-keep"
            data-kind={check.kind}
          >
            {intl.formatMessage(i18n.wiredKeep, {
              keep: gibText(intl, check.keepMb),
              physical: physicalGib,
            })}
          </p>
        )}
        {check.kind === 'belowReserve' && (
          <ToneBanner
            tone="err"
            label={intl.formatMessage(i18n.wiredFailed)}
            text={intl.formatMessage(i18n.wiredBelowReserve, {
              keep: gibText(intl, check.keepMb),
              reserve,
              max: intl.formatNumber(max),
            })}
            testId="mlx-memory-wired-refused"
          />
        )}
        {check.kind === 'belowOthers' && (
          <ToneBanner
            tone="warn"
            label={intl.formatMessage(i18n.wiredReserve)}
            text={intl.formatMessage(i18n.wiredBelowOthers, {
              keep: gibText(intl, check.keepMb),
              mac: macName,
              others: othersGib,
              reserve,
            })}
            testId="mlx-memory-wired-warn"
          />
        )}
        {(check.kind === 'invalid' || check.kind === 'default') && (
          <span className={cx('text-lz-meta', WEIGHT.semibold, TONE_TEXT.err)} role="alert">
            {intl.formatMessage(i18n.wiredInvalidValue, { max: intl.formatNumber(max) })}
          </span>
        )}
        <div className="flex flex-wrap items-center gap-2" data-testid="mlx-memory-wired-suggestion">
          <span className={cx(TYPE.meta, TNUM)}>
            {suggestion == null
              ? intl.formatMessage(i18n.wiredNoSuggestion, { mac: macName, reserve })
              : machine.workingSetBytes != null && suggestion * 1024 * 1024 <= machine.workingSetBytes
                ? intl.formatMessage(i18n.wiredSuggestedLow, {
                    ws: gbText(intl, machine.workingSetBytes),
                    mac: macName,
                    mb: intl.formatNumber(suggestion),
                  })
                : intl.formatMessage(i18n.wiredSuggested, {
                    mb: intl.formatNumber(suggestion),
                    others: othersGib,
                    reserve,
                  })}
          </span>
          {suggestion != null && String(suggestion) !== text.trim() && (
            <Button size="sm" variant="secondary" onClick={() => setText(String(suggestion))}>
              {intl.formatMessage(i18n.wiredUseSuggestion)}
            </Button>
          )}
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <StudioSwitch
            checked={persist}
            onChange={setPersist}
            aria-label={intl.formatMessage(i18n.wiredPersistLabel)}
            disabled={running}
          />
          <span className={TYPE.body}>{intl.formatMessage(i18n.wiredPersistLabel)}</span>
          <span className={TYPE.meta}>{intl.formatMessage(i18n.wiredPersistNote)}</span>
        </div>
        {canEnable && (
          <div className="flex flex-wrap items-center gap-2">
            <span className={TYPE.meta}>{intl.formatMessage(i18n.wiredCommand)}</span>
            <code className={TYPE.mono} data-testid="mlx-memory-wired-command">
              {wiredLimitCommand(mb)}
            </code>
          </div>
        )}
        <div className="flex flex-wrap items-center gap-2">
          <Button
            variant="primary"
            size="sm"
            disabled={!canEnable || running}
            icon={running ? <Loader2 className="animate-spin" /> : undefined}
            onClick={() => run(() => mlxEngineWiredLimitApply(mb, persist, nodeId), mb)}
            data-testid="mlx-memory-wired-enable"
          >
            {intl.formatMessage(i18n.wiredEnable)}
          </Button>
          <Button
            variant="destructive"
            size="sm"
            disabled={!canDisable || running}
            onClick={() => run(() => mlxEngineWiredLimitDisable(nodeId), 0)}
            data-testid="mlx-memory-wired-disable"
          >
            {intl.formatMessage(i18n.wiredDisable)}
          </Button>
          {daemon != null && (
            <Button
              variant="secondary"
              size="sm"
              disabled={running}
              onClick={() => run(() => mlxEngineWiredLimitRemoveDaemon(nodeId), current ?? 0)}
            >
              {intl.formatMessage(i18n.wiredRemove)}
            </Button>
          )}
        </div>
        {daemon != null && (
          <span className={TYPE.meta}>{intl.formatMessage(i18n.wiredRemoveNote)}</span>
        )}
        {running && (
          <ToneBanner
            tone="warn"
            live
            label={intl.formatMessage(i18n.wiredCommand)}
            text={intl.formatMessage(i18n.wiredWaiting, { mac: macName })}
            testId="mlx-memory-wired-waiting"
          />
        )}
        {result && (
          <ToneBanner
            tone={result.ok ? 'ok' : 'err'}
            label={intl.formatMessage(result.ok ? i18n.wiredDone : i18n.wiredFailed)}
            text={result.text}
            testId="mlx-memory-wired-result"
          />
        )}
        {result && !result.ok && result.mb != null && (
          <div className="flex flex-wrap items-center gap-2" data-testid="mlx-memory-wired-fallback">
            <span className={TYPE.meta}>
              {intl.formatMessage(i18n.wiredFallback, { mac: macName })}
            </span>
            <code className={TYPE.mono}>{wiredFallbackCommand(result.mb)}</code>
          </div>
        )}
      </div>
    </Panel>
  );
}

export interface MlxMemorySectionProps {
  macPicker: ReactNode;
  /** The picked Mac is this one: the machine facts and the wired limit are read here. */
  isSelf: boolean;
  macName: string;
  /** The picked Mac's Link node id when it is a peer: its facts and wired limit are relayed. */
  nodeId?: string;
  status: MlxEngineStatus | null;
  settings: MlxEngineSettings | null;
  engineBusy: boolean;
  saveSettings: (next: MlxEngineSettings) => Promise<MlxEngineSettings>;
  onApply: () => void;
}

export function MlxMemorySection(props: MlxMemorySectionProps) {
  const { macPicker, isSelf, macName, nodeId, status, settings, engineBusy, saveSettings, onApply } =
    props;
  const intl = useIntl();
  const [machine, setMachine] = useState<MlxMemoryMachine | null>(null);
  const [machineError, setMachineError] = useState<string | null>(null);
  const saved = useMemo(() => draftsFromMemory(settings?.memory), [settings?.memory]);
  const [drafts, setDrafts] = useState<MemoryDrafts>(saved);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  useEffect(() => setDrafts(saved), [saved]);

  const readMachine = useCallback(() => {
    if (!isSelf) return;
    mlxEngineMemoryMachine()
      .then((m) => {
        setMachine(m);
        setMachineError(null);
      })
      .catch((e) => setMachineError(mlxErrorMessage(e, intl.formatMessage(i18n.readFailed))));
  }, [intl, isSelf]);

  useEffect(() => {
    if (!isSelf) {
      setMachine(null);
      return;
    }
    readMachine();
    const timer = window.setInterval(readMachine, 5000);
    return () => window.clearInterval(timer);
  }, [isSelf, readMachine]);

  const budget = status?.memoryBudget ?? null;
  const physicalBytes =
    machine?.physicalBytes ??
    budget?.physicalBytes ??
    (status?.totalMemoryGb ? status.totalMemoryGb * 1024 ** 3 : null);
  const physicalGb = physicalBytes != null ? physicalBytes / GB : null;
  const workingSetBytes =
    budget?.deviceWorkingSetBytes ?? machine?.workingSetBytes ?? status?.gpuCeilingBytes ?? null;
  const running = status?.state === 'running';
  const kvBytesPerToken =
    running &&
    budget?.kvBytesPerToken != null &&
    status?.modelId === (settings?.modelId ?? status?.modelId)
      ? budget.kvBytesPerToken
      : null;

  const parsed = parseMemoryDrafts(drafts, { physicalBytes, kvBytesPerToken });
  const consequences = memoryConsequences(parsed.memory, {
    physicalBytes,
    workingSetBytes,
    budget: running ? budget : null,
  });
  const dirty = !memoryDraftsEqual(drafts, saved);
  const fieldProblem = (key: MemoryKey) => {
    const p = parsed.problems.find((x) => 'key' in x && x.key === key);
    return p ? problemText(intl, p, physicalGb) : null;
  };
  const comboProblems = parsed.problems.filter((p) => !('key' in p));
  const unsupported = machine != null && !machine.tunablesSupported;
  const othersBytes =
    running && budget?.othersBytes != null
      ? budget.othersBytes
      : machine
        ? (machine.othersBytes ?? machine.physicalBytes - machine.availableBytes)
        : null;
  const wiredSuggestion =
    physicalBytes != null && othersBytes != null
      ? suggestedWiredMb(
          physicalBytes,
          othersBytes,
          machine?.minReserveGib ?? WIRED_DEFAULT_MIN_RESERVE_GIB
        )
      : null;
  const active = status?.activeRequests ?? 0;

  const save = async (): Promise<boolean> => {
    if (!settings || parsed.problems.length > 0) return false;
    setSaving(true);
    setSaveError(null);
    try {
      await saveSettings({ ...settings, memory: parsed.memory });
      return true;
    } catch (e) {
      setSaveError(mlxErrorMessage(e, intl.formatMessage(i18n.saveFailed)));
      return false;
    } finally {
      setSaving(false);
    }
  };
  const apply = async () => {
    if (dirty && !(await save())) return;
    onApply();
  };

  const fill = (id: PresetId) => {
    if (physicalBytes != null) setDrafts((d) => presetDrafts(id, d, physicalBytes));
  };

  const fieldRow = (spec: MemoryFieldSpec) => (
    <MemoryField
      key={spec.key}
      spec={spec}
      text={drafts[spec.key]}
      onText={(v) => setDrafts((d) => ({ ...d, [spec.key]: v }))}
      defaultLine={defaultText(intl, spec.key, running ? budget : null, physicalBytes)}
      physicalGb={physicalGb}
      unsupported={unsupported && spec.needsNewEngine}
      problem={fieldProblem(spec.key)}
    />
  );

  const restartNeeded = running && (status?.restartRequired ?? false);
  const applyDisabled =
    !running ||
    engineBusy ||
    saving ||
    parsed.problems.length > 0 ||
    active > 0 ||
    !(dirty || restartNeeded);

  return (
    <div className="flex flex-col gap-4 pb-8" data-testid="mlx-memory-section">
      {macPicker}
      <p className={TYPE.bodyMuted}>{intl.formatMessage(i18n.intro)}</p>
      {machineError && (
        <ToneBanner tone="err" label={intl.formatMessage(i18n.readFailed)} text={machineError} />
      )}
      {unsupported && machine && (
        <ToneBanner
          tone="warn"
          label={intl.formatMessage(i18n.needsEngineChip)}
          text={intl.formatMessage(i18n.needsEngine, { launcher: machine.launcher })}
          testId="mlx-memory-needs-engine"
        />
      )}
      <LivePanel
        machine={{
          physicalBytes,
          availableBytes: machine?.availableBytes ?? null,
          workingSetBytes,
          wiredLimitMb: machine?.wiredLimitMb ?? null,
        }}
        status={status}
      />
      <Panel
        title={intl.formatMessage(i18n.tunablesPanel)}
        headerRight={
          <>
            {dirty && <Chip tone="warn">{intl.formatMessage(i18n.unsaved)}</Chip>}
            <Button
              size="sm"
              variant="secondary"
              onClick={() => void save()}
              disabled={!dirty || saving || !settings || parsed.problems.length > 0}
              icon={saving ? <Loader2 className="animate-spin" /> : undefined}
              data-testid="mlx-memory-save"
            >
              {intl.formatMessage(i18n.save)}
            </Button>
            <Button
              size="sm"
              variant="primary"
              icon={<RefreshCw />}
              onClick={() => void apply()}
              disabled={applyDisabled}
              data-testid="mlx-memory-apply"
            >
              {intl.formatMessage(i18n.apply)}
            </Button>
          </>
        }
      >
        <div className="flex flex-col gap-3">
          <div className="flex flex-wrap items-center gap-2">
            <span className={cx(TYPE.meta, WEIGHT.semibold)}>
              {intl.formatMessage(i18n.presetsLabel)}
            </span>
            <Button
              size="sm"
              variant="secondary"
              onClick={() => fill('workstation')}
              disabled={physicalBytes == null}
            >
              {intl.formatMessage(i18n.presetWorkstation)}
            </Button>
            <Button
              size="sm"
              variant="secondary"
              onClick={() => fill('inferenceBox')}
              disabled={physicalBytes == null}
            >
              {intl.formatMessage(i18n.presetInferenceBox)}
            </Button>
          </div>
          <span className={TYPE.meta}>{intl.formatMessage(i18n.presetsNote)}</span>
          {saveError && (
            <ToneBanner tone="err" label={intl.formatMessage(i18n.saveFailed)} text={saveError} />
          )}
          {restartNeeded && (
            <ToneBanner
              tone="warn"
              label={intl.formatMessage(i18n.restartNeeded)}
              text={intl.formatMessage(i18n.restartNeededText)}
              testId="mlx-memory-restart-needed"
            />
          )}
          {running && active > 0 && (dirty || restartNeeded) && (
            <ToneBanner
              tone="warn"
              label={intl.formatMessage(i18n.restartNeeded)}
              text={intl.formatMessage(i18n.busy, { count: active })}
              testId="mlx-memory-busy"
            />
          )}
          {!running && <span className={TYPE.meta}>{intl.formatMessage(i18n.savedStopped)}</span>}
          {comboProblems.map((p) => (
            <ToneBanner
              key={p.kind}
              tone="err"
              label={intl.formatMessage(i18n.invalid)}
              text={problemText(intl, p, physicalGb)}
              testId={`mlx-memory-problem-${p.kind}`}
            />
          ))}
          <p className={cx(TYPE.body, WEIGHT.semibold)} data-testid="mlx-memory-consequences">
            {consequences.capGb == null
              ? intl.formatMessage(i18n.consequencesAuto)
              : consequences.maxContextTokens != null
                ? intl.formatMessage(i18n.consequences, {
                    tokens: intl.formatNumber(consequences.maxContextTokens),
                    free: gbText(intl, (consequences.freeForMacGb ?? 0) * GB),
                  })
                : intl.formatMessage(i18n.consequencesNoKv, {
                    free: gbText(intl, (consequences.freeForMacGb ?? 0) * GB),
                  })}
          </p>
          {consequences.capAboveWorkingSet &&
            consequences.capGb != null &&
            workingSetBytes != null &&
            physicalBytes != null && (
              <ToneBanner
                tone="warn"
                label={intl.formatMessage(i18n.liveWired)}
                text={intl.formatMessage(i18n.capAboveWorkingSet, {
                  cap: gbText(intl, consequences.capGb * GB),
                  ws: gbText(intl, workingSetBytes),
                  mb: wiredSuggestion != null ? intl.formatNumber(wiredSuggestion) : '—',
                })}
                testId="mlx-memory-cap-above-ws"
              />
            )}
          <div className="flex flex-col">
            {MEMORY_FIELDS.filter((f) => !f.advanced).map(fieldRow)}
          </div>
          <Disclosure title={intl.formatMessage(i18n.advanced)} testId="mlx-memory-advanced">
            <div className="flex flex-col">
              {MEMORY_FIELDS.filter((f) => f.advanced).map(fieldRow)}
            </div>
          </Disclosure>
        </div>
      </Panel>
      {machine && othersBytes != null && (
        <WiredLimitPanel
          key={target ?? 'self'}
          machine={machine}
          macName={macName}
          nodeId={target}
          othersBytes={othersBytes}
          onChanged={readMachine}
        />
      )}
    </div>
  );
}
