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
  GOLDEN,
  RESEARCH_MODES,
  type ResearchMode,
  PRESETS,
  detectPreset,
  presetPatch,
  type PresetId,
} from './golden';

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
function PresetBar({
  active,
  onApply,
}: {
  active: PresetId;
  onApply: (id: 'golden' | 'default') => void;
}) {
  return (
    <div
      className="flex items-center justify-between gap-3 border border-border-primary px-3 py-2"
      style={{ borderRadius: 3 }}
    >
      <div className="min-w-0">
        <div className="text-sm font-semibold text-text-primary">Preset</div>
        <div className="text-xs text-text-secondary truncate">
          {active === 'golden'
            ? 'Golden formula — the tested tuning that builds passing apps'
            : active === 'default'
              ? 'Defaults — faithful to goose’s built-in values'
              : 'Custom — your own tuning'}
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
        {PRESETS.map((p) => {
          const on = active === p.id;
          return (
            <button
              key={p.id}
              type="button"
              onClick={() => onApply(p.id)}
              className={`px-3 py-1 text-xs font-semibold border border-border-primary ${
                on ? 'text-background-primary' : 'text-text-primary hover:bg-background-secondary'
              }`}
              style={{
                borderRadius: 3,
                backgroundColor: on ? (p.id === 'golden' ? '#2e8bff' : '#5b6472') : 'transparent',
              }}
            >
              {p.label}
            </button>
          );
        })}
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

  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const raw = (await read('swarm', false)) as SwarmConfig | null;
        if (alive) setCfg({ ...DEFAULTS, ...(raw ?? {}) });
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

  const applyPreset = useCallback(
    (id: 'golden' | 'default') => set(presetPatch(id === 'golden' ? GOLDEN : DEFAULTS)),
    [set]
  );
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
              hint="When the planner isn't confident in how to break the app down, pause and ask YOU a few clarifying questions in the run panel instead of guessing. Best for a weak local planner."
            >
              <SwarmSwitch
                checked={(cfg.ask_floor ?? 0) > 0}
                onChange={(v) => set({ ask_floor: v ? 70 : 0 })}
              />
            </Row>
            {(cfg.ask_floor ?? 0) > 0 ? (
              <Row
                label="How many questions it may ask"
                hint="goose asks at most this many at once — and anything it does not ask, it decides for you. One build turned up five choices only you could make (equal or uneven splits, fewest transfers or pay-who-you-owe, cents or decimals, and two more); it asked about three and quietly picked the rest itself. Raise this and it asks instead of picking."
              >
                <NumberField
                  value={cfg.ask_max_q}
                  onCommit={(v) => set({ ask_max_q: v ?? 3 })}
                />
              </Row>
            ) : null}
            {(cfg.ask_floor ?? 0) > 0 ? (
              <Row
                label="Only offer answers your spec allows"
                hint="Stops goose offering you choices your own spec already ruled out. One build asked “SQLite, JSON, or CSV?” for a spec that said storage MUST be tab-separated — every option broke the spec, so there was no right answer to click. It had read the spec; the question prompt just talked it out of it, with a worked example that answered storage questions with SQLite/JSON no matter what, and two nudges toward “the most common choice”. With this on, anything your spec fixes is not up for question, and goose asks about what you actually left open."
              >
                <SwarmSwitch
                  checked={!!cfg.clarify_spec_bound}
                  onChange={(v) => set({ clarify_spec_bound: v })}
                />
              </Row>
            ) : null}
            <Row
              label="Your spec beats anything goose researched"
              hint="When goose researches a decision you left open, it writes the answer into your spec labelled “settled — do not re-ask”, with nothing saying your own words come first. So a guess it made can quietly outrank what you actually wrote — and a note you type mid-build loses to it, because notes are told the spec wins. With this on, researched answers are DEFAULTS: where one contradicts something you fixed, your spec is right and goose ignores it. Each one also says which decision it was answering, and the run records exactly what goose added to your spec."
            >
              <SwarmSwitch checked={!!cfg.spec_wins} onChange={(v) => set({ spec_wins: v })} />
            </Row>
            <Row
              label="Seconds goose may spend working out if your spec is clear"
              hint="Before building, goose checks whether your spec actually pins down what to build — that check is what makes it ask you questions instead of guessing. It runs alongside the planning drafts on the same nodes, and each node handles one request at a time, so on a busy fleet it can wait in the queue until it gives up. When it gives up, goose does not tell you it gave up: it proceeds on how much the drafts agreed with EACH OTHER, which says nothing about whether YOUR spec was clear, and reports high confidence with no questions. Two of fourteen builds did exactly that and invented the whole product. Give it room."
            >
              <NumberField
                value={cfg.clarity_probe_secs}
                onCommit={(v) => set({ clarity_probe_secs: v ?? 120 })}
              />
            </Row>
            <Row
              label="Give every worker the app's non-negotiables"
              hint="Before building, goose distils your spec into a short list of app-wide acceptance criteria and puts them in front of every worker as non-negotiable — so a worker writing one file still knows what the whole app must do. It has never actually run: its switch was wired to a mode nobody turns on, so in every build ever made, no worker has seen one. This makes it reachable. Costs one planning pass; unproven, because it has never run once."
            >
              <SwarmSwitch checked={!!cfg.goals} onChange={(v) => set({ goals: v })} />
            </Row>
            <Row
              label="How many steps the final whole-app check may take"
              hint="At the end, one worker builds the app, runs every command your spec advertises, checks the output is right, and fixes what is broken. It gets the same step budget as a worker that owns a single file — and it runs out: in 5 of 9 builds it never reached a verdict, 3 of them stopping mid-check with “I've reached the maximum number of actions I can do without user input”. The build still reported a result. Raise this so the check can finish; each step costs about a minute on local models, so this is the quality-versus-speed knob that matters most."
            >
              <NumberField
                value={cfg.sink_max_turns}
                onCommit={(v) => set({ sink_max_turns: v ?? 40 })}
              />
            </Row>
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

              <Group title="Reliability">
                <Row label="Worker max turns" hint="cap per worker before it must finish">
                  <NumberField value={cfg.worker_max_turns} onCommit={(v) => set({ worker_max_turns: v ?? 40 })} />
                </Row>
                <Row label="Max attempts" hint="retries per subtask">
                  <NumberField value={cfg.max_attempts} onCommit={(v) => set({ max_attempts: v ?? 3 })} />
                </Row>
                <Row label="Worker timeout (s)" hint="no-progress re-route failsafe per worker (0 = off)">
                  <NumberField value={cfg.worker_timeout_secs} onCommit={(v) => set({ worker_timeout_secs: v ?? 900 })} />
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

              <Group title="Pipeline (parallelism)">
                <Row label="Research planning" hint="off / on / auto (auto = only with source files)">
                  <Segmented options={RESEARCH_MODES} value={(cfg.research_planning ?? 'on') as ResearchMode} onChange={(v) => set({ research_planning: v })} />
                </Row>
                <Row label="Parallel planning" hint="write subtask specs across the fleet at once">
                  <SwarmSwitch checked={!!cfg.parallel_planning} onChange={(v) => set({ parallel_planning: v })} />
                </Row>
                <Row label="Dynamic replan" hint="re-plan mid-run when the tree drifts">
                  <SwarmSwitch checked={!!cfg.dynamic_replan} onChange={(v) => set({ dynamic_replan: v })} />
                </Row>
                <Row label="Max replans" hint="cap on dynamic-replan rounds">
                  <NumberField value={cfg.max_replans} onCommit={(v) => set({ max_replans: v ?? 2 })} />
                </Row>
                <Row label="Research scouts" hint="parallel fixed-lens scouts vs serial scoping">
                  <SwarmSwitch checked={cfg.research_scouts !== false} onChange={(v) => set({ research_scouts: v })} />
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

              <Group title="Plan confidence" cost="Buys: a plan the fleet agrees on, and goose asking you instead of guessing. Costs: planning minutes — each extra draft round is another pass over the whole fleet.">

                <Row
                  label="Convergence molding"
                  hint="steer the weak planner to one canonical decomposition + measure agreement role-normalized — the proven way to raise plan confidence. ON by default."
                >
                  <SwarmSwitch checked={cfg.converge !== false} onChange={(v) => set({ converge: v })} />
                </Row>
                <Row
                  label="Dynamic retarget"
                  hint="when confidence is below the ask floor, re-draft toward consensus or research the open decisions BEFORE asking you. Needs 'Ask when uncertain' on. Experimental — adds planning time."
                >
                  <SwarmSwitch checked={!!cfg.retarget} onChange={(v) => set({ retarget: v })} />
                </Row>
                <Row
                  label="Backbone-lock"
                  hint="extract the module set a majority of drafts agree on, lock it, and re-draft so the fleet's plans genuinely converge. Experimental — adds a second draft round."
                >
                  <SwarmSwitch checked={!!cfg.backbone} onChange={(v) => set({ backbone: v })} />
                </Row>
                <Row label="Draft temperature" hint="temperature for skeleton drafting only (blank = model default)">
                  <NumberField value={cfg.draft_temp ?? null} placeholder="default" onCommit={(v) => set({ draft_temp: v })} />
                </Row>
                <Row
                  label="Stop re-planning when it stops improving"
                  hint="when goose is unsure about its plan it re-drafts it again and again, hoping the drafts agree better. Measured on a real build, they got worse — 84 to 70 to 52 over three rounds, an hour of every machine, and it shipped the first plan anyway. This stops after a round that fails to beat the best so far. It cannot cost you quality: the best plan is kept either way."
                >
                  <SwarmSwitch
                    checked={!!cfg.retarget_stall_guard}
                    onChange={(v) => set({ retarget_stall_guard: v })}
                  />
                </Row>
              </Group>

              <Group title="Research" cost="Buys: goose stops inventing YOUR product decisions. Costs: it stops and waits for you. Measured: it spent 65 minutes failing to answer what you answered in under two.">
                <Row
                  label="Ask me when it has nothing to search with"
                  hint="if no search tools are set up, goose currently still sends open decisions off to 'research' — which can only invent an answer and then treat it as settled. With this on it asks you instead. Measured: it spent 65 minutes failing to answer what you answered in under two."
                >
                  <SwarmSwitch
                    checked={!!cfg.no_tools_means_ask}
                    onChange={(v) => set({ no_tools_means_ask: v })}
                  />
                </Row>
                <Row
                  label="Only trust research it actually looked up"
                  hint="when goose researches an open decision, a finding only counts as settled if it truly searched the web or docs. A pure guess still informs the plan but no longer silences the question — so a product choice goose merely assumed still gets asked."
                >
                  <SwarmSwitch
                    checked={!!cfg.grounded_research_only}
                    onChange={(v) => set({ grounded_research_only: v })}
                  />
                </Row>
              </Group>

              <Group title="While it builds" cost="Buys: the worker is told the convention BEFORE it writes, not corrected after. Costs: almost nothing — these add prompt text, not extra model calls.">
                <Row
                  label="Tell the author the domain rules up front"
                  hint="gives the worker the known-correct conventions its task touches (cron day-of-week, timezones, money precision, off-by-one) BEFORE it writes the code, instead of only catching the mistake in review. Only the rules that match the task are sent."
                >
                  <SwarmSwitch
                    checked={!!cfg.author_pitfalls}
                    onChange={(v) => set({ author_pitfalls: v })}
                  />
                </Row>
                <Row
                  label="Check the agreed interfaces are real"
                  hint="before building, goose freezes an interface contract so parallel workers agree. It is never checked — a worker can be handed prose instead of an interface and nobody knows. This records what was actually frozen and whether it parses. It only reports; it changes nothing about the build."
                >
                  <SwarmSwitch
                    checked={!!cfg.contract_validate}
                    onChange={(v) => set({ contract_validate: v })}
                  />
                </Row>
                <Row
                  label="Check the modules agree about each other"
                  hint="parallel workers write different files. One can read a field off another's class that the class doesn't have — and the app builds, imports, and passes its tests, then crashes on the first real request. This reads the finished code and finds those disagreements. Model-free, so it can't be argued with."
                >
                  <SwarmSwitch
                    checked={!!cfg.cross_module_check}
                    onChange={(v) => set({ cross_module_check: v })}
                  />
                </Row>
                <Row
                  label="Let me add notes while it builds"
                  hint="a build runs for hours and today you can only speak to it once, at the start. With this on, notes you add are picked up by the next task goose starts — never interrupting work already in flight. They are background, not orders: the spec still wins."
                >
                  <SwarmSwitch checked={!!cfg.user_notes} onChange={(v) => set({ user_notes: v })} />
                </Row>
              </Group>

              <Group title="Before it says done" cost="Buys: goose stops calling a broken app verified — 7 runs have. Costs: real minutes. Every check here RUNS something, and a failed check triggers a fix round.">
                <Row
                  label="Run the app's own tests before calling it verified"
                  hint="for TypeScript/JavaScript projects, runs the test script the project itself declares. Python already does this; TS was only being compiled, so an app whose tests fail could still be reported as verified."
                >
                  <SwarmSwitch
                    checked={!!cfg.ts_smoke_tests}
                    onChange={(v) => set({ ts_smoke_tests: v })}
                  />
                </Row>
                <Row
                  label="Show the checker the app's real interface first"
                  hint="before goose verifies the finished app, it runs the entry point and reads its actual --help, so its checks target the interface the app really has. Without this it guesses from the spec — and a wrong guess has made it 'fix' a working app."
                >
                  <SwarmSwitch
                    checked={!!cfg.sink_prebuild}
                    onChange={(v) => set({ sink_prebuild: v })}
                  />
                </Row>
                <Row
                  label="Check for dead code after the build"
                  hint="reads the finished code and finds modules that were built but nothing imports — they can never run. Model-free, so it can't be argued with. Also finds unimplemented stubs."
                >
                  <SwarmSwitch checked={!!cfg.review} onChange={(v) => set({ review: v })} />
                </Row>
                <Row
                  label="Dead code un-verifies the run"
                  hint="if a module is built but imported by nothing and goose can't wire it up, the run stops claiming it was verified. Standalone scripts are never counted — they're meant to be run directly. Needs the dead-code check on."
                >
                  <SwarmSwitch
                    checked={!!cfg.unwired_demotes_verified}
                    onChange={(v) => set({ unwired_demotes_verified: v })}
                  />
                </Row>
                <Row
                  label="Try to reproduce a crash before believing it"
                  hint="when a review reports a crash, goose copies the app to a clean directory and runs it twice. Only a crash that reproduces both times counts — a one-off or an environment quirk is dropped. This is what makes the next setting mean anything: without it there is never a proven crash to act on."
                >
                  <SwarmSwitch
                    checked={!!cfg.review_repro}
                    onChange={(v) => set({ review_repro: v })}
                  />
                </Row>
                <Row
                  label="A proven crash un-verifies the run"
                  hint="if goose reproduces a crash twice in a clean copy and can't repair it, the run stops claiming it was verified and prints the command to reproduce. Never fails a passing app — it only drops the 'verified' claim. Needs the repro oracle on."
                >
                  <SwarmSwitch
                    checked={!!cfg.repro_demotes_verified}
                    onChange={(v) => set({ repro_demotes_verified: v })}
                  />
                </Row>
                <Row
                  label="Keep working if a task failed"
                  hint="if one of the build's own tasks fails outright, goose keeps trying to finish it instead of declaring the app done. Today it only checks the smoke gate, so a failed task can still be reported as verified. Bonus tasks are never counted."
                >
                  <SwarmSwitch
                    checked={!!cfg.failed_tasks_block_green}
                    onChange={(v) => set({ failed_tasks_block_green: v })}
                  />
                </Row>
              </Group>

              <Group title="Learning" cost="Buys: goose gets faster at a stack the more you build it. Costs: one reflection pass at the end of a build that worked.">
                <Row
                  label="Learn from builds that worked"
                  hint="after an app builds and verifies, goose reflects on what worked and writes a reusable skill for that stack — then starts from it next time instead of re-deriving it. It records the structure, never your app's features, and the skill is a plain file you can read, edit, or delete."
                >
                  <SwarmSwitch checked={!!cfg.persona} onChange={(v) => set({ persona: v })} />
                </Row>
              </Group>

              <Group title="Sampling">
                <Row label="Temperature" hint="blank = model default">
                  <NumberField value={cfg.temperature ?? null} placeholder="default" onCommit={(v) => set({ temperature: v })} />
                </Row>
                <Row label="Top P">
                  <NumberField value={cfg.top_p ?? null} placeholder="default" onCommit={(v) => set({ top_p: v })} />
                </Row>
                <Row label="Top K">
                  <NumberField value={cfg.top_k ?? null} placeholder="default" onCommit={(v) => set({ top_k: v })} />
                </Row>
                <Row label="Min P">
                  <NumberField value={cfg.min_p ?? null} placeholder="default" onCommit={(v) => set({ min_p: v })} />
                </Row>
                <Row label="Repeat penalty">
                  <NumberField value={cfg.repeat_penalty ?? null} placeholder="default" onCommit={(v) => set({ repeat_penalty: v })} />
                </Row>
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
