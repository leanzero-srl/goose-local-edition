import React, { useCallback, useEffect, useState } from 'react';
import { ChevronDown, Check } from 'lucide-react';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '../../ui/card';
import { Switch } from '../../ui/switch';
import { Input } from '../../ui/input';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '../../ui/dropdown-menu';
import { useConfig } from '../../ConfigContext';
import FanInCard from '../../swarm/FanInCard';
import { useFleet, deviceFromModelId } from '../../swarm/useFleet';
import { useSwarmLogMode, SWARM_LOG_MODES } from '../../swarm/useVerboseSwarm';
import {
  type SwarmConfig,
  DEFAULTS,
  RESEARCH_MODES,
  type ResearchMode,
  PRESETS,
  detectPreset,
  presetPatch,
  type PresetId,
} from './golden';
import {
  SAMPLING_KNOBS,
  clampKnob,
  hasAnySampling,
  loadSamplingDefaults,
  sanitizeSampling,
  saveSamplingDefaults,
  type SamplingKnobId,
  type SamplingSettings,
} from '../../swarm/sampling';

// The engine-config (config.yaml) field each UI knob writes through to — snake_case, the same
// fields `goose swarm run` resolves when no per-run env override is present.
const SAMPLING_CFG_KEY: Record<SamplingKnobId, keyof SwarmConfig> = {
  temperature: 'temperature',
  topP: 'top_p',
  topK: 'top_k',
  minP: 'min_p',
  repeatPenalty: 'repeat_penalty',
};

const SAMPLING_ROW_LABEL: Record<SamplingKnobId, string> = {
  temperature: 'Temperature',
  topP: 'Top P',
  topK: 'Top K',
  minP: 'Min P',
  repeatPenalty: 'Repeat penalty',
};

/**
 * Goose Local Edition — Swarm settings. Surfaces the `swarm:` config (previously CLI-only, editable only
 * through `goose swarm pool`) as a real desktop panel, plus a LIVE fleet view (LM Link). Reads/writes the
 * `swarm` key via the existing config API — no new backend. Honors the hard UI rules: sharp/flat, full
 * borders (never a left rail), solid saturated color + bold numbers, custom controls (no native <select>).
 */

function Row({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    // items-START, not items-center: a wrapped hint makes the row tall, and a centered control then floats
    // in the middle of a paragraph instead of sitting beside the thing it toggles.
    <div className="flex items-start justify-between gap-4 py-2">
      <div className="min-w-0">
        <div className="text-sm text-text-primary">{label}</div>
        {/* The hint is the WHOLE POINT of this panel — it is what each setting costs you and what it
            bought, measured. `truncate` killed every one of them at the viewport edge mid-sentence:
            "…it shipped the first plan anyway. This stops after a round that fails to beat the be…" and
            "…Model-free, so it can't be argue…". A one-line rule cannot explain a quality/speed trade-off,
            and a reader who cannot finish the sentence cannot make the choice. Let it wrap; cap the
            measure so it stays readable next to the control. */}
        {hint && <p className="text-xs text-text-secondary max-w-[92ch] mt-0.5">{hint}</p>}
      </div>
      <div className="shrink-0 pt-0.5">{children}</div>
    </div>
  );
}

function NumberField({
  value,
  placeholder,
  onCommit,
  width = 'w-20',
}: {
  value: number | null | undefined;
  placeholder?: string;
  onCommit: (v: number | null) => void;
  width?: string;
}) {
  const [text, setText] = useState(value == null ? '' : String(value));
  useEffect(() => setText(value == null ? '' : String(value)), [value]);
  return (
    <Input
      type="number"
      className={`${width} text-right`}
      style={{ borderRadius: 3 }}
      value={text}
      placeholder={placeholder}
      onChange={(e) => setText(e.target.value)}
      onBlur={() => {
        if (text.trim() === '') return onCommit(null);
        const n = Number(text);
        if (!Number.isNaN(n)) onCommit(n);
      }}
    />
  );
}

// Custom −/number/+ stepper for a node's relative task-share weight (no native slider/select, per UI rules).
function WeightStepper({ value, onChange }: { value: number; onChange: (v: number) => void }) {
  const clamp = (v: number) => Math.max(1, Math.min(9, v));
  const btn =
    'h-6 w-6 flex items-center justify-center border border-border-primary text-text-secondary hover:text-text-primary hover:border-text-secondary transition-colors leading-none';
  return (
    <div className="flex items-center gap-1.5">
      <button onClick={() => onChange(clamp(value - 1))} className={btn} style={{ borderRadius: 3 }} aria-label="Less work">
        −
      </button>
      <span className="w-4 text-center font-bold tabular-nums" style={{ color: '#2e8bff' }}>
        {value}
      </span>
      <button onClick={() => onChange(clamp(value + 1))} className={btn} style={{ borderRadius: 3 }} aria-label="More work">
        +
      </button>
    </div>
  );
}

/** Custom segmented control — never a native <select>. Solid inverse fill on the active option. */
function Segmented<T extends string>({
  options,
  value,
  onChange,
}: {
  options: readonly T[];
  value: T;
  onChange: (v: T) => void;
}) {
  return (
    <div className="inline-flex border border-border-primary" style={{ borderRadius: 3 }}>
      {options.map((opt, i) => {
        const active = opt === value;
        return (
          <button
            key={opt}
            type="button"
            onClick={() => onChange(opt)}
            className={`px-2.5 py-1 text-xs ${
              active ? 'text-background-primary font-semibold' : 'text-text-secondary'
            } ${i > 0 ? 'border-l border-border-primary' : ''}`}
            style={{ backgroundColor: active ? '#2e8bff' : 'transparent' }}
          >
            {opt}
          </button>
        );
      })}
    </div>
  );
}

/**
 * Local-Edition switch. The shared Switch `default` variant makes the thumb `bg-background-primary`, which
 * equals the dark track in dark mode = invisible. Use the `mono` variant (visible thumb) with a solid azure
 * ON state (per the hard UI rule: solid saturated color, never faded).
 */
function SwarmSwitch({ checked, onChange }: { checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <Switch
      variant="mono"
      checked={checked}
      onCheckedChange={onChange}
      className="data-[state=checked]:!bg-[#2e8bff] dark:data-[state=checked]:!bg-[#2e8bff]"
    />
  );
}

/**
 * A group of settings, with what the group COSTS you.
 *
 * Mihai's read, and it is the right one: "The text for each of these features is like QUALITY VERSUS
 * SPEED. Some users may choose not to wait so long, so it may make sense to untick some — but they're all
 * quality-oriented, aside from the queued messages which is a genuine feature." The panel never said that
 * anywhere, so every toggle looked like an equal coin-flip. `cost` states the trade so a reader can decide
 * what to switch off, instead of guessing which ones are safe.
 */
function Group({
  title,
  cost,
  children,
}: {
  title: string;
  cost?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="border border-border-primary" style={{ borderRadius: 3 }}>
      <div className="px-3 py-1.5 bg-background-secondary border-b border-border-primary">
        <div className="flex items-baseline justify-between gap-3 flex-wrap">
          <span className="text-xs font-semibold text-text-primary">{title}</span>
          {cost ? <span className="text-[11px] text-text-secondary">{cost}</span> : null}
        </div>
      </div>
      <div className="px-3 py-1 divide-y divide-border-primary">{children}</div>
    </div>
  );
}

/**
 * Custom model dropdown (never a native <select>) listing the LIVE LM Studio fleet ids. The swarm matches
 * planner_model by EXACT equality against a resident model identifier, so the options are the raw ids
 * verbatim; a value that is not currently resident still shows (the swarm then auto-picks the best resident).
 */
function ModelPicker({
  value,
  options,
  online,
  onChange,
}: {
  value: string;
  options: string[];
  online: boolean;
  onChange: (v: string) => void;
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          className="flex items-center justify-between gap-2 w-56 border border-border-primary px-2.5 py-1 text-xs text-text-primary hover:border-text-secondary transition-colors"
          style={{ borderRadius: 3 }}
        >
          <span className="truncate">{value || 'auto (best resident)'}</span>
          <ChevronDown className="h-3.5 w-3.5 shrink-0 text-text-secondary" />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="max-h-64 overflow-y-auto w-56">
        {options.length > 0 ? (
          options.map((opt) => (
            <DropdownMenuItem key={opt} onClick={() => onChange(opt)} className="text-xs">
              <span className="truncate">{opt}</span>
              {opt === value && <Check className="ml-auto h-3.5 w-3.5 shrink-0" />}
            </DropdownMenuItem>
          ))
        ) : (
          <div className="px-2 py-1.5 text-xs text-text-secondary">
            {online ? 'No models loaded' : 'Start LM Studio to list models'}
          </div>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/**
 * Preset bar — one click to apply the GOLDEN formula (the tested tuning that builds passing apps) or the
 * faithful Defaults. Applying only touches the portable tuning keys (PRESET_KEYS), never the fleet identity
 * (devices/speed_weights/endpoint). A solid azure fill marks the active preset; a solid amber chip flags a
 * diverged "Custom" config. Custom control per the hard UI rules — never a native <select>.
 */
function PresetBar({ active, onApply }: { active: PresetId; onApply: () => void }) {
  return (
    <div
      className="flex items-center justify-between gap-3 border border-border-primary px-3 py-2"
      style={{ borderRadius: 3 }}
    >
      <div className="min-w-0">
        <div className="text-sm font-semibold text-text-primary">Golden formula</div>
        <div className="text-xs text-text-secondary truncate">
          {active === 'golden'
            ? 'Running the tested tuning that builds passing apps — everything below is at its golden value.'
            : 'You’ve changed a setting below. The golden formula is one click away.'}
        </div>
      </div>
      <div className="flex items-center gap-2 shrink-0">
        {active === 'custom' && (
          <span
            className="text-xs font-semibold px-2 py-0.5 text-background-primary"
            style={{ backgroundColor: '#f5a623', borderRadius: 3 }}
          >
            Custom
          </span>
        )}
        {PRESETS.map((p) => (
          <button
            key={p.id}
            type="button"
            onClick={onApply}
            disabled={active === 'golden'}
            className={`px-3 py-1 text-xs font-semibold border border-border-primary ${
              active === 'golden'
                ? 'text-text-secondary opacity-50 cursor-default'
                : 'text-text-primary hover:bg-background-secondary'
            }`}
            style={{ borderRadius: 3, backgroundColor: 'transparent' }}
          >
            {p.label}
          </button>
        ))}
      </div>
    </div>
  );
}

export default function SwarmSettingsSection() {
  const { read, upsert } = useConfig();
  const fleet = useFleet();
  const [logMode, setLogMode] = useSwarmLogMode();
  const [cfg, setCfg] = useState<SwarmConfig>(DEFAULTS);
  const [loaded, setLoaded] = useState(false);
  // Sampling DEFAULTS: canonical in localStorage (`swarmSamplingDefaults` — what every run
  // window's strip prefills from), written through to the swarm config so a headless/CLI run
  // shares the same defaults. Each run window can still override these per run (env beats config).
  const [samplingDefaults, setSamplingDefaults] = useState<SamplingSettings>(() =>
    loadSamplingDefaults()
  );

  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const raw = (await read('swarm', false)) as SwarmConfig | null;
        if (alive) {
          setCfg({ ...DEFAULTS, ...(raw ?? {}) });
          // One-time seed for pre-existing setups: config.yaml sampling values predating the
          // defaults store become the defaults, so the panel and the run strips agree.
          const fromCfg = sanitizeSampling({
            temperature: raw?.temperature,
            topP: raw?.top_p,
            topK: raw?.top_k,
            minP: raw?.min_p,
            repeatPenalty: raw?.repeat_penalty,
          });
          if (!hasAnySampling(loadSamplingDefaults()) && hasAnySampling(fromCfg)) {
            saveSamplingDefaults(fromCfg);
            setSamplingDefaults(fromCfg);
          }
        }
      } catch {
        if (alive) setCfg(DEFAULTS);
      } finally {
        if (alive) setLoaded(true);
      }
    })();
    return () => {
      alive = false;
    };
  }, [read]);

  const set = useCallback(
    (patch: Partial<SwarmConfig>) => {
      setCfg((prev) => {
        const next = { ...prev, ...patch };
        void upsert('swarm', next, false).catch(() => {});
        return next;
      });
    },
    [upsert]
  );

  const setWeight = useCallback(
    (deviceId: string, w: number) => {
      setCfg((prev) => {
        const sw = { ...(prev.speed_weights ?? {}) };
        // Update the key that already matches this device — an exact device-id key first, else an existing
        // substring key like "gabee" (the pool's scheme) — in place; otherwise write the FULL device id as
        // the key. Both weightFor and the Rust scheduler match a key when the device id CONTAINS it, and a
        // full id contains itself, so a newly written key always matches its device. (Keying by the model-
        // derived node name did NOT match the device id and silently lost the setting.)
        const existing = deviceId in sw ? deviceId : Object.keys(sw).find((k) => deviceId.includes(k));
        sw[existing ?? deviceId] = w;
        const next = { ...prev, speed_weights: sw };
        void upsert('swarm', next, false).catch(() => {});
        return next;
      });
    },
    [upsert]
  );

  const setSamplingDefault = useCallback(
    (id: SamplingKnobId, v: number | null) => {
      const clamped = v == null ? null : (clampKnob(id, v) ?? null);
      setSamplingDefaults((prev) => {
        const next: SamplingSettings = { ...prev };
        if (clamped == null) delete next[id];
        else next[id] = clamped;
        saveSamplingDefaults(next);
        return next;
      });
      set({ [SAMPLING_CFG_KEY[id]]: clamped } as Partial<SwarmConfig>);
    },
    [set]
  );

  const applyPreset = useCallback(() => set(presetPatch(DEFAULTS)), [set]);
  const activePreset = detectPreset(cfg);

  // Node weight rows: the configured pool (its ids are what speed_weights keys match against), or the live
  // fleet models when the pool is empty. weightFor mirrors the scheduler's speed_weight_for (first key the
  // device id contains, else 1) so the UI shows the ACTUAL effective weight — including pool-set keys.
  const configuredDevices = (Array.isArray(cfg.devices) ? cfg.devices : []) as Array<{
    id: string;
    model_id: string;
  }>;
  const weightRows =
    configuredDevices.length > 0
      ? configuredDevices.map((d) => ({ id: d.id, name: deviceFromModelId(d.model_id) || d.id }))
      : fleet.models.map((m) => ({ id: m, name: deviceFromModelId(m) }));
  const weightFor = (id: string): number => {
    const sw = cfg.speed_weights ?? {};
    if (id in sw) return sw[id] ?? 1; // exact device-id key wins (avoids substring collisions)
    const key = Object.keys(sw).find((k) => id.includes(k));
    return key ? (sw[key] ?? 1) : 1;
  };

  return (
    <section id="swarm" className="space-y-4 pr-4 pb-8">
      <Card className="rounded-lg">
        <CardHeader className="pb-0">
          <CardTitle className="mb-1">Swarm LeanZero — fleet</CardTitle>
          <CardDescription>
            Your local model fleet (LM Studio / LM Link), live. The swarm auto-pools whatever is resident.
          </CardDescription>
        </CardHeader>
        <CardContent className="pt-4 px-4 space-y-2">
          <div className="text-xs flex items-center justify-between">
            <span className="text-text-secondary">{cfg.endpoint}</span>
            <span style={{ color: fleet.online ? '#2ecc71' : '#878787' }}>
              {fleet.online
                ? `${fleet.lanes.length} node${fleet.lanes.length === 1 ? '' : 's'} live`
                : 'offline'}
            </span>
          </div>
          {fleet.online && fleet.lanes.length > 0 ? (
            <FanInCard dispatch="fleet · live" lanes={fleet.lanes} />
          ) : (
            <div className="text-sm text-text-secondary border border-border-primary px-3 py-4 text-center" style={{ borderRadius: 3 }}>
              No fleet detected. Start LM Studio (LM Link) at {cfg.endpoint} to see your nodes.
            </div>
          )}

          {weightRows.length > 0 && (
            <div className="pt-2 mt-1 border-t border-border-primary space-y-1.5">
              <div className="text-xs text-text-secondary">
                Node weights — <span className="text-text-primary font-medium">higher = a bigger share of the tasks</span>.
                Turn a slower machine down so it does less.
              </div>
              {weightRows.map((row) => (
                <div key={row.id} className="flex items-center justify-between gap-3 py-0.5">
                  <span className="text-sm font-mono text-text-primary truncate" title={row.id}>
                    {row.name}
                  </span>
                  <WeightStepper value={weightFor(row.id)} onChange={(v) => setWeight(row.id, v)} />
                </div>
              ))}
            </div>
          )}

          <div className="pt-2 mt-1 border-t border-border-primary">
            <Row
              label="Run panel detail"
              hint="Compact = headline phases · Verbose = full timeline + reasoning + tool calls · Developer = everything expanded & raw"
            >
              <Segmented options={SWARM_LOG_MODES} value={logMode} onChange={setLogMode} />
            </Row>
            <Row
              label="Ask when uncertain"
              hint="When the planner isn't confident how to break the app down, it pauses and asks YOU a few clarifying questions instead of guessing. On by default — the single most useful choice for a weak local planner. Turn it off (0) for an unattended/CI build that must never block on a human."
            >
              <SwarmSwitch
                checked={(cfg.ask_floor ?? 80) > 0}
                onChange={(v) => set({ ask_floor: v ? 80 : 0 })}
              />
            </Row>
            {(cfg.ask_floor ?? 80) > 0 ? (
              <Row
                label="How many questions it may ask"
                hint="goose asks at most this many at once — and anything it does not ask, it decides for you. One build turned up five choices only you could make; it asked about three and quietly picked the rest. Raise this and it asks instead of picking."
              >
                <NumberField
                  value={cfg.ask_max_q}
                  onCommit={(v) => set({ ask_max_q: v ?? 3 })}
                />
              </Row>
            ) : null}
          </div>

          <div className="text-xs text-text-secondary pt-1">
            Download and manage the models each node runs in the <span className="text-text-primary font-medium">Local Inference</span> tab.
          </div>
        </CardContent>
      </Card>

      <Card className="rounded-lg">
        <CardHeader className="pb-0">
          <CardTitle className="mb-1">Swarm LeanZero — tunables</CardTitle>
          <CardDescription>
            The knobs that were CLI-only (`goose swarm pool`). Changes save to your goose config immediately.
          </CardDescription>
        </CardHeader>
        <CardContent className="pt-4 px-4 space-y-3">
          {!loaded ? (
            <div className="text-sm text-text-secondary">Loading swarm config…</div>
          ) : (
            <>
              <PresetBar active={activePreset} onApply={applyPreset} />

              <Group title="Advanced — timeouts &amp; budgets" cost="Per-hardware tuning. The defaults are the golden formula; change these only for an unusually fast or slow fleet.">
                <Row label="Worker max turns" hint="cap per worker before it must finish">
                  <NumberField value={cfg.worker_max_turns} onCommit={(v) => set({ worker_max_turns: v ?? 40 })} />
                </Row>
                <Row label="Max attempts" hint="retries per subtask">
                  <NumberField value={cfg.max_attempts} onCommit={(v) => set({ max_attempts: v ?? 3 })} />
                </Row>
                <Row label="Worker timeout (s)" hint="no-progress re-route failsafe per worker (0 = off)">
                  <NumberField value={cfg.worker_timeout_secs} onCommit={(v) => set({ worker_timeout_secs: v ?? 900 })} />
                </Row>
                <Row
                  label="Progress watchdog (s)"
                  hint="cut a worker that streams reasoning tokens forever without a real tool call, output, or code — measured live, tasks ran 15-26 min emitting only thinking and were never stopped (the worker timeout is idle-based, and thinking resets it). This bounds the max time WITHOUT productive progress; a slow-but-working model keeps resetting it, so only a thinking-only spiral is cut. Never touches the final integrate-verify step (that has its own cap). Try ~720. 0 = off."
                >
                  <NumberField
                    value={cfg.progress_watchdog_secs}
                    onCommit={(v) => set({ progress_watchdog_secs: v ?? 0 })}
                  />
                </Row>
                <Row label="Planner timeout (s)" hint="hang failsafe for planner-side calls">
                  <NumberField value={cfg.planner_timeout_secs} onCommit={(v) => set({ planner_timeout_secs: v ?? 900 })} />
                </Row>
                <Row label="Context cap (tokens)" hint="blank = off">
                  <NumberField value={cfg.context_cap ?? null} placeholder="off" onCommit={(v) => set({ context_cap: v })} />
                </Row>
                <Row label="Max tool-response chars" hint="hard cap on any tool result fed to a worker (blank = 30000)">
                  <NumberField value={cfg.max_tool_response_chars ?? null} placeholder="30000" onCommit={(v) => set({ max_tool_response_chars: v })} />
                </Row>
              </Group>

              <Group title="Research &amp; planning">
                <Row label="Research planning" hint="off / on / auto (auto = only with source files). Grounds the plan in looked-up fact; costs a research phase.">
                  <Segmented options={RESEARCH_MODES} value={(cfg.research_planning ?? 'on') as ResearchMode} onChange={(v) => set({ research_planning: v })} />
                </Row>
                <Row label="Max replans" hint="cap on dynamic-replan rounds">
                  <NumberField value={cfg.max_replans} onCommit={(v) => set({ max_replans: v ?? 2 })} />
                </Row>
                <Row label="Max research questions" hint="scoping questions before planning">
                  <NumberField value={cfg.max_research_questions} onCommit={(v) => set({ max_research_questions: v ?? 4 })} />
                </Row>
                <Row
                  label="Research lookups per scout"
                  hint="how many searches a scout may run before it must answer. This is the real limit on research — it stops when it has looked enough things up, not when a clock runs out."
                >
                  <NumberField
                    value={cfg.scout_max_lookups}
                    onCommit={(v) => set({ scout_max_lookups: v ?? 10 })}
                  />
                </Row>
                <Row
                  label="Scout time limit (s)"
                  hint="a backstop so a stuck model can't hang the run — not the budget. It was 120s and that was the real limit: a measured run's research ended at 120.023s, cut off mid-thought, and handed the planner an apology instead of a finding."
                >
                  <NumberField value={cfg.scout_budget_secs} onCommit={(v) => set({ scout_budget_secs: v ?? 900 })} />
                </Row>
                <Row label="Best-of-N skeletons" hint="candidate plans; pick the structurally-best">
                  <NumberField value={cfg.best_of_n_skeletons} onCommit={(v) => set({ best_of_n_skeletons: v ?? 1 })} />
                </Row>
              </Group>

              <Group title="Verification" cost="goose already runs its full verification suite on every build — real end-to-end command checks, per-module import gates, contract-stub parsing, cross-module wiring. That is baked in and always on. Below is one EXTRA, optional check on top.">
                <Row
                  label="Also flag dead code after the build"
                  hint="reads the finished code and flags modules that were built but nothing imports — they can never run. Model-free. OFF by default: it occasionally flags an intentional standalone script as dead. Turn it on for stricter builds where every module must be wired in."
                >
                  <SwarmSwitch checked={!!cfg.review} onChange={(v) => set({ review: v })} />
                </Row>
              </Group>

              <Group
                title="Sampling defaults"
                cost="The default knobs every run starts from. Each run window (benchmark or build) shows these and can override them per run — the run's own strip wins for that run only."
              >
                {SAMPLING_KNOBS.map((k) => (
                  <Row key={k.id} label={SAMPLING_ROW_LABEL[k.id]} hint={k.hint}>
                    <NumberField
                      value={samplingDefaults[k.id] ?? null}
                      placeholder="default"
                      onCommit={(v) => setSamplingDefault(k.id, v)}
                    />
                  </Row>
                ))}
              </Group>

              <Group title="Pool & planner">
                <Row label="Planner model" hint="pick a live LM Studio model for planning/architecting">
                  <ModelPicker
                    value={cfg.planner_model ?? ''}
                    options={fleet.models}
                    online={fleet.online}
                    onChange={(v) => set({ planner_model: v })}
                  />
                </Row>
                <Row label="Planner also works" hint="planner node also runs worker tasks">
                  <SwarmSwitch checked={!!cfg.planner_also_works} onChange={(v) => set({ planner_also_works: v })} />
                </Row>
                <Row label="Planner weight" hint="worker weight for the planner when it pitches in">
                  <NumberField value={cfg.planner_weight} onCommit={(v) => set({ planner_weight: v ?? 1 })} />
                </Row>
                <Row label="Homogeneous models" hint="all workers same weights/tokenizer → planner splits more aggressively">
                  <SwarmSwitch checked={!!cfg.homogeneous_models} onChange={(v) => set({ homogeneous_models: v })} />
                </Row>
                <Row label="Allow model load" hint="let the swarm spin up non-resident models (off = warm fleet only)">
                  <SwarmSwitch checked={!!cfg.allow_model_load} onChange={(v) => set({ allow_model_load: v })} />
                </Row>
              </Group>
            </>
          )}
        </CardContent>
      </Card>
    </section>
  );
}
