/**
 * Goose Local Edition — per-run sampling knobs, shared by the benchmark view, the normal swarm run
 * window and the swarm settings panel.
 *
 * The chain is the engine's own (crates/goose-cli/src/commands/swarm.rs): env beats config, run
 * beats default. A run window's set knobs ride to the engine as GOOSE_SWARM_TEMP / _TOP_P / _TOP_K /
 * _MIN_P / _REPEAT_PENALTY on the spawned `goose swarm run`; an unset knob sends nothing, so the
 * config default (the Settings panel's "Sampling defaults") and finally the model default apply.
 *
 * `swarmSamplingDefaults` (localStorage) is the default set every run window prefills from; each
 * window may then override per run without touching the defaults.
 */

export interface SamplingSettings {
  temperature?: number;
  topP?: number;
  topK?: number;
  minP?: number;
  repeatPenalty?: number;
}

export const SAMPLING_DEFAULTS_KEY = 'swarmSamplingDefaults';

export type SamplingKnobId = keyof SamplingSettings;

export interface KnobSpec {
  id: SamplingKnobId;
  label: string;
  /** Solid saturated hue for this knob's value — one distinct hue per knob, never a tint. */
  hue: string;
  min: number;
  max: number;
  step: number;
  integer?: boolean;
  hint: string;
}

// Ranges mirror the engine's own clamps (env_f32_clamped / the top_k filter in swarm.rs), so a value
// the strip accepts is exactly a value the engine will honor rather than silently re-clamp.
export const SAMPLING_KNOBS: KnobSpec[] = [
  {
    id: 'temperature',
    label: 'temp',
    hue: '#f5a623',
    min: 0,
    max: 2,
    step: 0.05,
    hint: 'Sampling temperature (0–2). Lower = more deterministic code. Unset = model default.',
  },
  {
    id: 'topP',
    label: 'top-p',
    hue: '#2e8bff',
    min: 0,
    max: 1,
    step: 0.05,
    hint: 'Nucleus sampling mass (0–1). Unset = model default.',
  },
  {
    id: 'topK',
    label: 'top-k',
    hue: '#17c4c4',
    min: 0,
    max: 1000,
    step: 1,
    integer: true,
    hint: 'Keep only the K most likely tokens (0–1000). Unset = model default.',
  },
  {
    id: 'minP',
    label: 'min-p',
    hue: '#b14cff',
    min: 0,
    max: 1,
    step: 0.05,
    hint: 'Drop tokens below this fraction of the top probability (0–1). Unset = model default.',
  },
  {
    id: 'repeatPenalty',
    label: 'repeat',
    hue: '#ff3ea5',
    min: 0.5,
    max: 2,
    step: 0.05,
    hint: 'Repetition penalty (0.5–2). Unset = model default.',
  },
];

/** Clamp one knob to its engine-honored range; null/NaN/∞ → undefined (= model default). */
export function clampKnob(id: SamplingKnobId, raw: number | null | undefined): number | undefined {
  if (raw == null || !Number.isFinite(raw)) return undefined;
  const spec = SAMPLING_KNOBS.find((k) => k.id === id);
  if (!spec) return undefined;
  const v = Math.min(spec.max, Math.max(spec.min, raw));
  return spec.integer ? Math.round(v) : v;
}

/** Keep only known knobs, each clamped; drops everything unset so {} means "all model defaults". */
export function sanitizeSampling(raw: unknown): SamplingSettings {
  const out: SamplingSettings = {};
  if (raw == null || typeof raw !== 'object') return out;
  const rec = raw as Record<string, unknown>;
  for (const spec of SAMPLING_KNOBS) {
    const v = rec[spec.id];
    const n = clampKnob(spec.id, typeof v === 'number' ? v : v == null ? null : Number(v));
    if (n !== undefined) out[spec.id] = n;
  }
  return out;
}

export function hasAnySampling(s: SamplingSettings): boolean {
  return SAMPLING_KNOBS.some((k) => s[k.id] !== undefined);
}

export function loadSamplingDefaults(): SamplingSettings {
  try {
    const raw = localStorage.getItem(SAMPLING_DEFAULTS_KEY);
    if (!raw) return {};
    return sanitizeSampling(JSON.parse(raw));
  } catch {
    return {};
  }
}

export function saveSamplingDefaults(s: SamplingSettings): void {
  try {
    localStorage.setItem(SAMPLING_DEFAULTS_KEY, JSON.stringify(sanitizeSampling(s)));
  } catch {
    // storage full/unavailable — defaults simply don't persist
  }
}
