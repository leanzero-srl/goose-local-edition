import React, { useCallback, useEffect, useState } from 'react';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '../../ui/card';
import { Switch } from '../../ui/switch';
import { Input } from '../../ui/input';
import { useConfig } from '../../ConfigContext';
import FanInCard from '../../swarm/FanInCard';
import { useFleet } from '../../swarm/useFleet';

/**
 * Goose Local Edition — Swarm settings. Surfaces the `swarm:` config (previously CLI-only, editable only
 * through `goose swarm pool`) as a real desktop panel, plus a LIVE fleet view (LM Link). Reads/writes the
 * `swarm` key via the existing config API — no new backend. Honors the hard UI rules: sharp/flat, full
 * borders (never a left rail), solid saturated color + bold numbers, custom controls (no native <select>).
 */

const RESEARCH_MODES = ['off', 'on', 'auto'] as const;
type ResearchMode = (typeof RESEARCH_MODES)[number];

interface SwarmConfig {
  endpoint?: string;
  planner_model?: string;
  worker_max_turns?: number;
  max_attempts?: number;
  worker_timeout_secs?: number;
  context_cap?: number | null;
  research_planning?: ResearchMode;
  parallel_planning?: boolean;
  dynamic_replan?: boolean;
  max_research_questions?: number;
  best_of_n_skeletons?: number;
  planner_also_works?: boolean;
  allow_model_load?: boolean;
  temperature?: number | null;
  top_p?: number | null;
  top_k?: number | null;
  repeat_penalty?: number | null;
  [k: string]: unknown; // preserve fields we don't edit (devices, etc.)
}

const DEFAULTS: SwarmConfig = {
  endpoint: 'http://localhost:1234',
  planner_model: 'qwen/qwen3.6-27b',
  worker_max_turns: 40,
  max_attempts: 3,
  worker_timeout_secs: 420,
  research_planning: 'on',
  parallel_planning: true,
  dynamic_replan: true,
  max_research_questions: 4,
  best_of_n_skeletons: 1,
  planner_also_works: true,
  allow_model_load: false,
};

function Row({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-4 py-1.5">
      <div className="min-w-0">
        <div className="text-sm text-text-primary truncate">{label}</div>
        {hint && <div className="text-xs text-text-secondary truncate">{hint}</div>}
      </div>
      <div className="shrink-0">{children}</div>
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

function Group({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="border border-border-primary" style={{ borderRadius: 3 }}>
      <div className="px-3 py-1.5 text-xs font-semibold text-text-primary bg-background-secondary border-b border-border-primary">
        {title}
      </div>
      <div className="px-3 py-1 divide-y divide-border-primary">{children}</div>
    </div>
  );
}

export default function SwarmSettingsSection() {
  const { read, upsert } = useConfig();
  const fleet = useFleet();
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

  return (
    <section id="swarm" className="space-y-4 pr-4 pb-8">
      <Card className="rounded-lg">
        <CardHeader className="pb-0">
          <CardTitle className="mb-1">Swarm — fleet</CardTitle>
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
          <div className="text-xs text-text-secondary">
            Download and manage the models each node runs in the <span className="text-text-primary font-medium">Local Inference</span> tab.
          </div>
        </CardContent>
      </Card>

      <Card className="rounded-lg">
        <CardHeader className="pb-0">
          <CardTitle className="mb-1">Swarm — tunables</CardTitle>
          <CardDescription>
            The knobs that were CLI-only (`goose swarm pool`). Changes save to your goose config immediately.
          </CardDescription>
        </CardHeader>
        <CardContent className="pt-4 px-4 space-y-3">
          {!loaded ? (
            <div className="text-sm text-text-secondary">Loading swarm config…</div>
          ) : (
            <>
              <Group title="Reliability">
                <Row label="Worker max turns" hint="cap per worker before it must finish">
                  <NumberField value={cfg.worker_max_turns} onCommit={(v) => set({ worker_max_turns: v ?? 40 })} />
                </Row>
                <Row label="Max attempts" hint="retries per subtask">
                  <NumberField value={cfg.max_attempts} onCommit={(v) => set({ max_attempts: v ?? 3 })} />
                </Row>
                <Row label="Worker timeout (s)" hint="hang failsafe per worker call">
                  <NumberField value={cfg.worker_timeout_secs} onCommit={(v) => set({ worker_timeout_secs: v ?? 420 })} />
                </Row>
                <Row label="Context cap (tokens)" hint="blank = off">
                  <NumberField value={cfg.context_cap ?? null} placeholder="off" onCommit={(v) => set({ context_cap: v })} />
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
                <Row label="Max research questions" hint="scoping questions before planning">
                  <NumberField value={cfg.max_research_questions} onCommit={(v) => set({ max_research_questions: v ?? 4 })} />
                </Row>
                <Row label="Best-of-N skeletons" hint="candidate plans; pick the structurally-best">
                  <NumberField value={cfg.best_of_n_skeletons} onCommit={(v) => set({ best_of_n_skeletons: v ?? 1 })} />
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
                <Row label="Repeat penalty">
                  <NumberField value={cfg.repeat_penalty ?? null} placeholder="default" onCommit={(v) => set({ repeat_penalty: v })} />
                </Row>
              </Group>

              <Group title="Pool & planner">
                <Row label="Planner model" hint="model id for planning/architecting">
                  <Input
                    className="w-56 text-right"
                    style={{ borderRadius: 3 }}
                    defaultValue={cfg.planner_model}
                    onBlur={(e) => set({ planner_model: e.target.value })}
                  />
                </Row>
                <Row label="Planner also works" hint="planner node also runs worker tasks">
                  <SwarmSwitch checked={!!cfg.planner_also_works} onChange={(v) => set({ planner_also_works: v })} />
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
