// The engine memory tunables' pure half: the field table, drafts, the validation the Memory tab
// shows live (the backend refuses the same cases: goose-sidecar memory_tunables::validate), the
// derived consequences, the two starting-value presets and the macOS wired-limit command.
import type { MlxEngineMemoryBudget, MlxEngineMemoryTunables } from '../../acp/mlx-engine';

export type MemoryKey = keyof MlxEngineMemoryTunables;
export type MemoryUnit = 'gb' | 'tokens' | 'requests' | 'fraction';

export const GB = 1e9;
const MIB = 1024 * 1024;
const GIB = 1024 * MIB;

/** The engine's defaults for the auto pair (Rapid-MLX memory_budget.py, #2858; INTERFACE.md). */
export const ENGINE_AUTO_FLOOR = 0.9;
export const ENGINE_AUTO_CEILING = 0.97;
export const ENGINE_AUTO_HEADROOM = 0.08;
export const ENGINE_SYSTEM_RESERVE = 0.08;
/** The guard's auto safety margin: 2 % of its ceiling (engine MARGIN_CEILING_RATIO). */
export const ENGINE_MARGIN_SHARE = 0.02;
/** The engine's prefill chunk without a flag (scheduler.py prefill_step_size). */
export const ENGINE_PREFILL_STEP = 2048;
/** goose's admission cap without a flag (goose-sidecar engine::MAX_CONCURRENT_REQUESTS). */
export const GOOSE_MAX_CONCURRENT = 8;

export interface MemoryFieldSpec {
  key: MemoryKey;
  unit: MemoryUnit;
  integer: boolean;
  step: number;
  /** The engine flag it becomes (INTERFACE.md). */
  flag: string;
  /** Read only by Rapid-MLX lz.20+. */
  needsNewEngine: boolean;
  advanced: boolean;
  /** Lowest valid value; `exclusiveMin` when the bound itself is invalid. */
  min: number;
  exclusiveMin?: boolean;
  /** Highest valid value on this Mac; `null` = no upper bound. `exclusiveMax` when the bound is invalid. */
  max: (physicalGb: number | null) => number | null;
  exclusiveMax?: boolean;
}

const belowRam = (p: number | null) => p;

export const MEMORY_FIELDS: readonly MemoryFieldSpec[] = [
  {
    key: 'metalLimitGb',
    unit: 'gb',
    integer: false,
    step: 1,
    flag: '--metal-limit-gb',
    needsNewEngine: true,
    advanced: false,
    min: 0,
    exclusiveMin: true,
    max: belowRam,
  },
  {
    key: 'memoryFloorGb',
    unit: 'gb',
    integer: false,
    step: 1,
    flag: '--memory-floor-gb',
    needsNewEngine: true,
    advanced: false,
    min: 0,
    max: belowRam,
    exclusiveMax: true,
  },
  {
    key: 'prefixCacheGb',
    unit: 'gb',
    integer: false,
    step: 1,
    flag: '--cache-memory-mb',
    needsNewEngine: false,
    advanced: false,
    min: 0,
    max: belowRam,
    exclusiveMax: true,
  },
  {
    key: 'prefillStepSize',
    unit: 'tokens',
    integer: true,
    step: 256,
    flag: '--prefill-step-size',
    needsNewEngine: false,
    advanced: false,
    min: 1,
    max: () => null,
  },
  {
    key: 'maxContextTokens',
    unit: 'tokens',
    integer: true,
    step: 8192,
    flag: '--max-model-len',
    needsNewEngine: true,
    advanced: false,
    min: 1,
    max: () => null,
  },
  {
    key: 'guardMarginGb',
    unit: 'gb',
    integer: false,
    step: 0.5,
    flag: '--memory-guard-margin-gb',
    needsNewEngine: true,
    advanced: false,
    min: 0,
    max: belowRam,
    exclusiveMax: true,
  },
  {
    key: 'maxConcurrentRequests',
    unit: 'requests',
    integer: true,
    step: 1,
    flag: '--max-concurrent-requests',
    needsNewEngine: false,
    advanced: false,
    min: 1,
    max: () => null,
  },
  {
    key: 'autoUtilizationFloor',
    unit: 'fraction',
    integer: false,
    step: 0.01,
    flag: '--auto-utilization-floor',
    needsNewEngine: true,
    advanced: true,
    min: 0.1,
    max: () => 1,
  },
  {
    key: 'autoUtilizationCeiling',
    unit: 'fraction',
    integer: false,
    step: 0.01,
    flag: '--auto-utilization-ceiling',
    needsNewEngine: true,
    advanced: true,
    min: 0.1,
    max: () => 1,
  },
  {
    key: 'autoHeadroomFraction',
    unit: 'fraction',
    integer: false,
    step: 0.01,
    flag: '--auto-headroom-fraction',
    needsNewEngine: true,
    advanced: true,
    min: 0,
    max: () => 1,
  },
  {
    key: 'systemReserveFraction',
    unit: 'fraction',
    integer: false,
    step: 0.01,
    flag: '--system-reserve-fraction',
    needsNewEngine: true,
    advanced: true,
    min: 0,
    max: () => 0.5,
  },
];

export const MEMORY_KEYS: readonly MemoryKey[] = MEMORY_FIELDS.map((f) => f.key);

/** Text drafts per field; '' = unset (the engine's default, or goose's for the floor). */
export type MemoryDrafts = Record<MemoryKey, string>;

export function draftsFromMemory(memory: MlxEngineMemoryTunables | undefined): MemoryDrafts {
  const drafts = {} as MemoryDrafts;
  for (const key of MEMORY_KEYS) {
    const v = memory?.[key];
    drafts[key] = v == null ? '' : String(v);
  }
  return drafts;
}

export function memoryDraftsEqual(a: MemoryDrafts, b: MemoryDrafts): boolean {
  return MEMORY_KEYS.every((k) => a[k].trim() === b[k].trim());
}

/** A problem with the drafts, said by i18n id with its values (rendered by the tab). */
export type MemoryProblem =
  | { kind: 'notNumber'; key: MemoryKey }
  | { kind: 'notInteger'; key: MemoryKey }
  | { kind: 'range'; key: MemoryKey; min: number; max: number | null }
  | { kind: 'capPlusFloor'; capGb: number; floorGb: number; physicalGb: number }
  | { kind: 'floorAboveCeiling'; floor: number; ceiling: number }
  | { kind: 'kvOverCap'; tokens: number; kvGb: number; capGb: number };

export interface MemoryParse {
  memory: MlxEngineMemoryTunables;
  problems: MemoryProblem[];
}

/** The drafts as the payload, every impossible value or combination listed. */
export function parseMemoryDrafts(
  drafts: MemoryDrafts,
  facts: { physicalBytes: number | null; kvBytesPerToken: number | null }
): MemoryParse {
  const memory: MlxEngineMemoryTunables = {};
  const problems: MemoryProblem[] = [];
  const physicalGb = facts.physicalBytes != null ? facts.physicalBytes / GB : null;
  for (const spec of MEMORY_FIELDS) {
    const text = drafts[spec.key].trim();
    if (text === '') continue;
    const n = Number(text);
    if (!Number.isFinite(n)) {
      problems.push({ kind: 'notNumber', key: spec.key });
      continue;
    }
    if (spec.integer && !Number.isInteger(n)) {
      problems.push({ kind: 'notInteger', key: spec.key });
      continue;
    }
    const max = spec.max(physicalGb);
    const lowBad = spec.exclusiveMin ? n <= spec.min : n < spec.min;
    const highBad = max != null && (spec.exclusiveMax ? n >= max : n > max);
    if (lowBad || highBad) {
      problems.push({ kind: 'range', key: spec.key, min: spec.min, max });
      continue;
    }
    memory[spec.key] = n;
  }
  const cap = memory.metalLimitGb;
  if (cap != null && physicalGb != null) {
    // A blank floor is not 0: goose sends its default (5% of RAM), so check that one.
    const floor = memory.memoryFloorGb ?? defaultFloorGb(physicalGb * GB);
    if (cap + floor > physicalGb) {
      problems.push({ kind: 'capPlusFloor', capGb: cap, floorGb: floor, physicalGb });
    }
  }
  if (memory.autoUtilizationFloor != null || memory.autoUtilizationCeiling != null) {
    const floor = memory.autoUtilizationFloor ?? ENGINE_AUTO_FLOOR;
    const ceiling = memory.autoUtilizationCeiling ?? ENGINE_AUTO_CEILING;
    if (floor > ceiling) problems.push({ kind: 'floorAboveCeiling', floor, ceiling });
  }
  if (cap != null && memory.maxContextTokens != null && facts.kvBytesPerToken != null) {
    const kvGb = (memory.maxContextTokens * facts.kvBytesPerToken) / GB;
    if (kvGb > cap) {
      problems.push({ kind: 'kvOverCap', tokens: memory.maxContextTokens, kvGb, capGb: cap });
    }
  }
  return { memory, problems };
}

/**
 * goose's floor when the field is unset: round(5 % x physical GB), the backend's rule
 * (goose-sidecar memory_tunables::DEFAULT_FLOOR_SHARE, receipt there: the engine counts the
 * memory the GPU driver wires for it as "others", so 8 % would cap a 128 GiB Mac below lz.18).
 */
export const DEFAULT_FLOOR_SHARE = 0.05;
/** The Workstation preset's floor: the engine's own 8 % soft reserve, the owner's choice. */
export const WORKSTATION_FLOOR_SHARE = ENGINE_SYSTEM_RESERVE;

export function defaultFloorGb(physicalBytes: number): number {
  return Math.round((DEFAULT_FLOOR_SHARE * physicalBytes) / GB);
}

export function workstationFloorGb(physicalBytes: number): number {
  return Math.round((WORKSTATION_FLOOR_SHARE * physicalBytes) / GB);
}

export interface MemoryConsequences {
  /** The cap these values plan under: the typed one, else the running engine's resolved one. */
  capGb: number | null;
  capFromEngine: boolean;
  /** physical - cap: what stays for macOS and other apps when the engine fills its cap. */
  freeForMacGb: number | null;
  /** ~ (cap - weights - prefix budget - margin) / KV per token; needs the running engine's figures. */
  maxContextTokens: number | null;
  /** The cap is above the macOS GPU working set: the wired limit must be raised for it to hold. */
  capAboveWorkingSet: boolean;
}

export function memoryConsequences(
  memory: MlxEngineMemoryTunables,
  facts: {
    physicalBytes: number | null;
    workingSetBytes: number | null;
    budget: MlxEngineMemoryBudget | null;
  }
): MemoryConsequences {
  const budget = facts.budget;
  const typed = memory.metalLimitGb ?? null;
  const engineCap = budget?.limitBytes != null ? budget.limitBytes / GB : null;
  const capGb = typed ?? engineCap;
  const physicalGb = facts.physicalBytes != null ? facts.physicalBytes / GB : null;
  const freeForMacGb = capGb != null && physicalGb != null ? Math.max(0, physicalGb - capGb) : null;
  let maxContextTokens: number | null = null;
  const kv = budget?.kvBytesPerToken;
  const weights = budget?.weightsBytes;
  if (capGb != null && kv != null && kv > 0 && weights != null) {
    const capBytes = capGb * GB;
    const prefix =
      memory.prefixCacheGb != null ? memory.prefixCacheGb * GB : (budget?.prefixBudgetBytes ?? 0);
    const margin =
      memory.guardMarginGb != null ? memory.guardMarginGb * GB : capBytes * ENGINE_MARGIN_SHARE;
    const room = capBytes - weights - prefix - margin;
    maxContextTokens = room > 0 ? Math.floor(room / kv) : 0;
  }
  return {
    capGb,
    capFromEngine: typed == null && engineCap != null,
    freeForMacGb,
    maxContextTokens,
    capAboveWorkingSet:
      typed != null && facts.workingSetBytes != null && typed * GB > facts.workingSetBytes,
  };
}

// --- presets: they only FILL the drafts -------------------------------------------------------

/** The GiB macOS keeps outside an "Inference box" engine cap: 128 GiB -> a 112 GB cap. */
export const INFERENCE_BOX_RESERVE_GIB = 16;
/** The Inference box's floor: the box runs the engine and little else. */
export const INFERENCE_BOX_FLOOR_GB = 4;
/** The wired limit suggested beside an Inference box cap: the owner's 122,000 MB on 128 GiB (131,072 MB). */
export const SUGGESTED_WIRED_SHARE = 122000 / 131072;

/** cap = (physical in GiB) - 16, as GB: 128 GiB -> 112 GB, 96 GiB -> 80 GB, 64 GiB -> 48 GB. */
export function inferenceBoxCapGb(physicalBytes: number): number {
  return Math.round(physicalBytes / GIB) - INFERENCE_BOX_RESERVE_GIB;
}

export function suggestedWiredLimitMb(physicalBytes: number): number {
  return Math.round(((physicalBytes / MIB) * SUGGESTED_WIRED_SHARE) / 1000) * 1000;
}

export type PresetId = 'workstation' | 'inferenceBox';

export function presetDrafts(
  id: PresetId,
  current: MemoryDrafts,
  physicalBytes: number
): MemoryDrafts {
  if (id === 'workstation') {
    return {
      ...current,
      metalLimitGb: '',
      memoryFloorGb: String(workstationFloorGb(physicalBytes)),
    };
  }
  return {
    ...current,
    metalLimitGb: String(inferenceBoxCapGb(physicalBytes)),
    memoryFloorGb: String(INFERENCE_BOX_FLOOR_GB),
  };
}

// --- macOS GPU wired limit ----------------------------------------------------------------------

export const WIRED_MIN_NONZERO_MB = 8192;

export function wiredLimitValid(mb: number, physicalBytes: number): boolean {
  if (!Number.isInteger(mb)) return false;
  const physicalMb = Math.floor(physicalBytes / MIB);
  return mb === 0 || (mb >= WIRED_MIN_NONZERO_MB && mb <= physicalMb);
}

/** The exact root command the backend runs (goose-sidecar wired_limit::sysctl_command). */
export function wiredLimitCommand(mb: number): string {
  return `/usr/sbin/sysctl iogpu.wired_limit_mb=${mb}`;
}

export function physicalMb(physicalBytes: number): number {
  return Math.floor(physicalBytes / MIB);
}
