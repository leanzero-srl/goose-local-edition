import React, { useCallback, useEffect, useState } from 'react';
import { ChevronDown, Check, Plus, Radar } from 'lucide-react';
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
  type SwarmDeviceRow,
  nodeRows,
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
import {
  Button,
  Chip,
  DataTable,
  EmptyState,
  Panel,
  SectionHeader,
  Segmented as LzSegmented,
  StatusDot,
  FOCUS,
  MOTION,
  RADIUS,
  SURFACE,
  TNUM,
  TONE_FILL,
  TYPE,
  WEIGHT,
  cx,
  type DataTableColumn,
  type NodeIndex,
} from '../../lz';
import {
  INPUT,
  StudioSwitch,
  ToneBanner,
  WeightStepper,
  nodeHue,
} from '../../leanzero-swarm/studio';
import { LOCAL_CHIP, chipFor } from '../../leanzero-swarm/cloud';

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
 * `swarm` key via the existing config API — no new backend. LeanZero Studio register: Panels on the
 * surface, 1px hairlines (never a left rail), solid colour with meaning, custom controls only.
 */

function Row({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    // items-START, not items-center: a wrapped hint makes the row tall, and a centered control then floats
    // in the middle of a paragraph instead of sitting beside the thing it toggles.
    <div className="flex items-start justify-between gap-4 py-2.5">
      <div className="min-w-0">
        <div className={TYPE.body}>{label}</div>
        {/* The hint is the WHOLE POINT of this panel — it is what each setting costs you and what it
            bought, measured. `truncate` killed every one of them at the viewport edge mid-sentence:
            "…it shipped the first plan anyway. This stops after a round that fails to beat the be…" and
            "…Model-free, so it can't be argue…". A one-line rule cannot explain a quality/speed trade-off,
            and a reader who cannot finish the sentence cannot make the choice. Let it wrap; cap the
            measure so it stays readable next to the control. */}
        {hint && <p className={cx(TYPE.bodyMuted, 'mt-0.5 max-w-[92ch]')}>{hint}</p>}
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
    <input
      type="number"
      className={cx(INPUT, width, 'text-right', TNUM)}
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

/** The Studio Segmented over a plain string vocabulary — never a native <select>. */
function Segmented<T extends string>({
  options,
  value,
  onChange,
  label,
}: {
  options: readonly T[];
  value: T;
  onChange: (v: T) => void;
  label: string;
}) {
  return (
    <LzSegmented
      aria-label={label}
      size="sm"
      options={options.map((o) => ({ value: o, label: o }))}
      value={value}
      onChange={onChange}
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
    <Panel title={title}>
      {cost ? <p className={cx(TYPE.bodyMuted, 'mb-2 max-w-[92ch]')}>{cost}</p> : null}
      <div className="divide-y divide-lz-border">{children}</div>
    </Panel>
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
          className={cx(
            'flex h-8 w-56 items-center justify-between gap-2 bg-lz-surface px-2.5 text-left text-lz-body text-lz-ink [&>svg]:size-3.5 [&>svg]:shrink-0 [&>svg]:text-lz-ink-3',
            SURFACE.outline,
            RADIUS.control,
            FOCUS,
            MOTION
          )}
        >
          <span className="truncate">{value || 'auto (best resident)'}</span>
          <ChevronDown />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align="end"
        className="max-h-64 w-56 overflow-y-auto rounded-lz-card border-lz-border bg-lz-surface p-1"
      >
        {options.length > 0 ? (
          options.map((opt) => (
            <DropdownMenuItem
              key={opt}
              onClick={() => onChange(opt)}
              className="rounded-lz-control focus:bg-lz-surface-2 focus:text-lz-ink"
            >
              <span className={cx('truncate', TYPE.body)}>{opt}</span>
              {opt === value && <Check className="ml-auto size-3.5 shrink-0 text-lz-accent" />}
            </DropdownMenuItem>
          ))
        ) : (
          <div className={cx('px-2 py-1.5', TYPE.bodyMuted)}>
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
 * (devices/speed_weights/endpoint). A Chip tone=warn flags a diverged "Custom" config; the preset buttons
 * are secondary Buttons, solid-disabled while the golden formula is already running.
 */
function PresetBar({ active, onApply }: { active: PresetId; onApply: () => void }) {
  return (
    <div className={cx('flex items-center justify-between gap-3 px-4 py-3', SURFACE.card)}>
      <div className="min-w-0">
        <div className={TYPE.h2}>Golden formula</div>
        <div className={cx(TYPE.bodyMuted, 'truncate')}>
          {active === 'golden'
            ? 'Running the tested tuning that builds passing apps — everything below is at its golden value.'
            : 'You’ve changed a setting below. The golden formula is one click away.'}
        </div>
      </div>
      <div className="flex shrink-0 items-center gap-2">
        {active === 'custom' && <Chip tone="warn">Custom</Chip>}
        {PRESETS.map((p) => (
          <Button
            key={p.id}
            variant="secondary"
            size="sm"
            onClick={onApply}
            disabled={active === 'golden'}
          >
            {p.label}
          </Button>
        ))}
      </div>
    </div>
  );
}

/** Free-form JSON object editor for the LM Studio extra-body passthrough. Commits on blur only
 *  when the text parses as a JSON OBJECT; a bad parse shows a solid err ring and commits
 *  nothing. Empty = clears the field. */
function ExtraBodyField({
  value,
  onCommit,
}: {
  value: Record<string, unknown> | undefined;
  onCommit: (v: Record<string, unknown> | undefined) => void;
}) {
  const [text, setText] = useState(value ? JSON.stringify(value) : '');
  const [bad, setBad] = useState(false);
  useEffect(() => {
    setText(value ? JSON.stringify(value) : '');
    setBad(false);
  }, [value]);
  return (
    <input
      className={cx(INPUT, 'w-64 font-mono', bad && 'ring-2 ring-inset ring-lz-err')}
      aria-invalid={bad || undefined}
      placeholder="{}"
      value={text}
      onChange={(e) => setText(e.target.value)}
      onBlur={() => {
        const t = text.trim();
        if (!t) {
          setBad(false);
          onCommit(undefined);
          return;
        }
        try {
          const v = JSON.parse(t) as unknown;
          if (v && typeof v === 'object' && !Array.isArray(v)) {
            setBad(false);
            onCommit(v as Record<string, unknown>);
          } else {
            setBad(true);
          }
        } catch {
          setBad(true);
        }
      }}
    />
  );
}

/** The last CLI error line, human-readable — the engine prints one-line `Error: …` messages. */
function bedrockErr(r: { stdout: string; stderr: string; error: string | null }): string {
  const m = (r.stderr || '').match(/Error:\s*([\s\S]+)/);
  if (m) return m[1].trim();
  return (r.stderr || r.error || 'the goose engine call failed').trim();
}

/** The cloud providers the panel can add nodes from — mirrors the engine's CLOUD_DEFS (cli =
 *  the `goose swarm cloud <cli>` name and the SwarmDevice.provider value). */
const CLOUD_PROVIDERS = [
  { seg: 'Bedrock', cli: 'bedrock', label: 'Amazon Bedrock', keyPlaceholder: 'Bedrock API key (ABSK…)', region: true },
  { seg: 'Z.ai', cli: 'zai', label: 'Z.ai', keyPlaceholder: 'Z.ai API key', region: false },
  { seg: 'Gemini', cli: 'google', label: 'Google Gemini', keyPlaceholder: 'Gemini API key (AIza…)', region: false },
  { seg: 'DeepSeek', cli: 'deepseek', label: 'DeepSeek', keyPlaceholder: 'DeepSeek API key (sk-…)', region: false },
] as const;
type CloudProviderDef = (typeof CLOUD_PROVIDERS)[number];

const NODE_PROVIDERS = ['LM Studio', ...CLOUD_PROVIDERS.map((c) => c.seg)] as [string, ...string[]];
type NodeProvider = string;

/**
 * A cloud provider's nodes (Bedrock, Z.ai, Gemini, DeepSeek). The panel's whole contract runs
 * through the engine CLI over IPC (`goose swarm cloud <provider> … --json`) — the same code path
 * the terminal uses, so desktop and CLI can never disagree: key validation happens ENGINE-side
 * (stored only when the provider accepts it), the model roster AUTO-POPULATES from what the key
 * can actually invoke, and add/rm write the device list through the engine. After any device
 * mutation the parent re-reads the swarm config (onChanged) so the panel's in-memory copy never
 * clobbers CLI-written devices on a later save.
 */
function CloudPane({
  def,
  devices,
  onChanged,
}: {
  def: CloudProviderDef;
  devices: SwarmDeviceRow[];
  onChanged: () => Promise<void>;
}) {
  const [phase, setPhase] = useState<'checking' | 'no-key' | 'ready'>('checking');
  const [error, setError] = useState<string | null>(null);
  const [region, setRegion] = useState('us-east-1');
  const [keyText, setKeyText] = useState('');
  const [roster, setRoster] = useState<string[]>([]);
  const [filter, setFilter] = useState('');
  const [busy, setBusy] = useState<string | null>(null); // 'validate' | model_id being added/removed
  const [editKey, setEditKey] = useState(false);

  // Every bridge call below is guarded: a REJECTED invoke (goosed gone, IPC torn down) used to leave
  // `phase` on 'checking' or `busy` pinned on a model id forever — a spinner claiming work nobody
  // was doing. A rejection is an error the user reads, and busy always clears.
  const refresh = useCallback(async () => {
    let r: Awaited<ReturnType<typeof window.electron.swarmCloud>>;
    try {
      r = await window.electron.swarmCloud(def.cli, ['models', '--json']);
    } catch (e) {
      setError(`engine bridge failed: ${e instanceof Error ? e.message : String(e)}`);
      setPhase('no-key');
      return;
    }
    if (r.ok) {
      try {
        const v = JSON.parse(r.stdout) as { region?: string; models?: string[] };
        setRoster(Array.isArray(v.models) ? v.models : []);
        if (v.region) setRegion(v.region);
        setPhase('ready');
        setError(null);
        return;
      } catch {
        setError('unreadable roster answer from the engine');
      }
    } else if (/no .* API key stored/i.test(`${r.stderr} ${r.error ?? ''}`)) {
      setError(null);
    } else {
      setError(bedrockErr(r));
    }
    setPhase('no-key');
  }, [def.cli]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const validateKey = useCallback(async () => {
    const key = keyText.trim();
    if (!key) return;
    setBusy('validate');
    setError(null);
    const args = ['key', key, '--json'];
    const reg = region.trim();
    if (def.region && reg) args.push('--region', reg);
    try {
      const r = await window.electron.swarmCloud(def.cli, args);
      if (r.ok) {
        try {
          const v = JSON.parse(r.stdout) as { region?: string; models?: string[] };
          setRoster(Array.isArray(v.models) ? v.models : []);
          if (v.region) setRegion(v.region);
          setKeyText('');
          setEditKey(false);
          setPhase('ready');
        } catch {
          setError('unreadable roster answer from the engine');
        }
      } else {
        setError(bedrockErr(r));
      }
    } catch (e) {
      setError(`engine bridge failed: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setBusy(null);
    }
  }, [keyText, region, def.cli, def.region]);

  // `def.cli` is a real input of both callbacks — it was missing from their deps, so a pane whose
  // provider changed under it kept calling the OLD provider's CLI.
  const addNode = useCallback(
    async (modelId: string) => {
      setBusy(modelId);
      setError(null);
      try {
        const r = await window.electron.swarmCloud(def.cli, ['add', modelId, '--weight', '2']);
        if (!r.ok) setError(bedrockErr(r));
        await onChanged();
      } catch (e) {
        setError(`engine bridge failed: ${e instanceof Error ? e.message : String(e)}`);
      } finally {
        setBusy(null);
      }
    },
    [def.cli, onChanged]
  );

  const rmNode = useCallback(
    async (modelId: string) => {
      setBusy(modelId);
      setError(null);
      try {
        const r = await window.electron.swarmCloud(def.cli, ['rm', modelId]);
        if (!r.ok) setError(bedrockErr(r));
        await onChanged();
      } catch (e) {
        setError(`engine bridge failed: ${e instanceof Error ? e.message : String(e)}`);
      } finally {
        setBusy(null);
      }
    },
    [def.cli, onChanged]
  );

  const configured = new Set(devices.map((d) => d.model_id));
  const shown = roster.filter(
    (m) => !filter.trim() || m.toLowerCase().includes(filter.trim().toLowerCase())
  );
  const keyEntry = (
    <div className="flex flex-col gap-2">
      <div className={cx(TYPE.bodyMuted, 'max-w-[92ch]')}>
        Paste a {def.label} API key. goose validates it live first — the key is stored (encrypted,
        in your goose secret store) only when {def.label} accepts it, and the models it can run
        auto-populate below.
      </div>
      <div className="flex items-center gap-2">
        <input
          type="password"
          className={cx(INPUT, 'flex-1')}
          placeholder={def.keyPlaceholder}
          value={keyText}
          onChange={(e) => setKeyText(e.target.value)}
        />
        {def.region && (
          <input
            className={cx(INPUT, 'w-28')}
            placeholder="region"
            value={region}
            onChange={(e) => setRegion(e.target.value)}
          />
        )}
        <Button
          variant="primary"
          size="sm"
          disabled={busy === 'validate' || !keyText.trim()}
          onClick={() => void validateKey()}
        >
          {busy === 'validate' ? 'Validating…' : 'Validate & save'}
        </Button>
      </div>
    </div>
  );

  const modelColumn: DataTableColumn<{ id: string }>[] = [
    {
      key: 'model',
      header: 'Model',
      cell: (r) => (
        <span className="block truncate font-mono text-lz-mono" title={r.id}>
          {r.id}
        </span>
      ),
    },
  ];

  return (
    <div className="flex flex-col gap-3">
      {phase === 'checking' ? (
        <div className={TYPE.bodyMuted}>Checking for a stored {def.label} key…</div>
      ) : phase === 'no-key' || editKey ? (
        keyEntry
      ) : (
        <div className="flex items-center justify-between gap-3">
          <span className="inline-flex items-center gap-2">
            <Chip tone="ok">key valid</Chip>
            <span className={cx(TYPE.meta, TNUM)}>
              {region} · {roster.length} model{roster.length === 1 ? '' : 's'} available
            </span>
          </span>
          <Button variant="secondary" size="sm" onClick={() => setEditKey(true)}>
            Replace key
          </Button>
        </div>
      )}

      {error && <ToneBanner tone="err" label={def.seg} text={error} />}

      {devices.length > 0 && (
        <div className="flex flex-col gap-1">
          <SectionHeader as="h3" title="Cloud nodes in your swarm pool" count={devices.length} />
          <DataTable
            dense
            aria-label={`${def.label} nodes in the pool`}
            columns={modelColumn}
            rows={devices.map((d) => ({ id: d.model_id }))}
            rowKey={(r) => r.id}
            rowAction={(r) => (
              <Button
                variant="ghost"
                size="sm"
                disabled={busy === r.id}
                onClick={() => void rmNode(r.id)}
              >
                {busy === r.id ? 'Removing…' : 'Remove'}
              </Button>
            )}
          />
        </div>
      )}

      {phase === 'ready' && (
        <div className="flex flex-col gap-1.5">
          <div className="flex items-center justify-between gap-3">
            <SectionHeader as="h3" title="Available models" count={shown.length} />
            <input
              className={cx(INPUT, 'w-44')}
              placeholder="filter…"
              aria-label="Filter models"
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
            />
          </div>
          <div className={cx('max-h-64 overflow-y-auto', SURFACE.card)}>
            <DataTable
              dense
              aria-label={`${def.label} models — add one as a swarm node`}
              columns={modelColumn}
              rows={shown.map((m) => ({ id: m }))}
              rowKey={(r) => r.id}
              rowAction={(r) =>
                configured.has(r.id) ? (
                  <Chip tone="ok">in pool</Chip>
                ) : (
                  <Button
                    variant="secondary"
                    size="sm"
                    icon={<Plus />}
                    disabled={busy === r.id}
                    onClick={() => void addNode(r.id)}
                  >
                    {busy === r.id ? 'Adding…' : 'Add'}
                  </Button>
                )
              }
              empty={<EmptyState title="No match" body="No model matches the filter." />}
            />
          </div>
        </div>
      )}
    </div>
  );
}

/** A weight row with its identity hue by list position. */
interface WeightRow {
  id: string;
  name: string;
  provider: string;
  modelId: string;
  supervises: boolean;
  hue: NodeIndex;
}

export default function SwarmSettingsSection() {
  const { read, upsert } = useConfig();
  const [logMode, setLogMode] = useSwarmLogMode();
  const [cfg, setCfg] = useState<SwarmConfig>(DEFAULTS);
  // Probe the host the ENGINE uses — the same `cfg.endpoint` the card prints in its offline message.
  const fleet = useFleet(5000, cfg.endpoint);
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

  /// EXACTLY ONE SUPERVISOR. `supervision` keeps a node OUT of the build pool so it is free to take the
  /// judge, review and synthesis calls — which is what "smartest" means operationally: put the strongest
  /// model where the thinking happens and let the others build under it. The engine refuses a pool with no
  /// non-supervision device, so this can never empty the build fleet; clicking the current supervisor
  /// again clears it.
  const setSupervisor = useCallback(
    (deviceId: string, modelId: string, on: boolean) => {
      setCfg((prev) => {
        const rows: SwarmDeviceRow[] = Array.isArray(prev.devices) ? [...prev.devices] : [];
        for (const r of rows) r.supervision = false;
        if (on) {
          const found = rows.find((r) => r.id === deviceId || r.model_id === modelId);
          if (found) found.supervision = true;
          // A live LM Studio node the user has never configured has no row yet; give it one so the
          // setting has somewhere to live, matching the shape discovery would have produced.
          else rows.push({ id: deviceId, model_id: modelId, weight: 1, enabled: true, supervision: true });
        }
        const next = { ...prev, devices: rows };
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

  // Which node source the fleet card is showing. Pure view state — the run pool always uses BOTH
  // (local residents + configured cloud devices merge engine-side).
  const [nodeProvider, setNodeProvider] = useState<NodeProvider>('LM Studio');

  // Re-read the swarm config after an ENGINE-side device mutation (`goose swarm bedrock add/rm`).
  // Mandatory: every UI edit upserts the WHOLE swarm object from this state, so a stale in-memory
  // copy would silently clobber the CLI-written device list on the next toggle.
  const reloadSwarm = useCallback(async () => {
    try {
      const raw = (await read('swarm', false)) as SwarmConfig | null;
      setCfg({ ...DEFAULTS, ...(raw ?? {}) });
    } catch {
      // keep the current state; the next mount re-reads
    }
  }, [read]);

  // Node weight rows: the configured pool (its ids are what speed_weights keys match against), or the live
  // fleet models when the pool is empty. weightFor mirrors the scheduler's speed_weight_for (first key the
  // device id contains, else 1) so the UI shows the ACTUAL effective weight — including pool-set keys.
  const configuredDevices: SwarmDeviceRow[] = Array.isArray(cfg.devices) ? cfg.devices : [];
  const activeCloud = CLOUD_PROVIDERS.find((c) => c.seg === nodeProvider) ?? null;
  const activeCloudDevices = activeCloud
    ? configuredDevices.filter((d) => d.provider === activeCloud.cli)
    : [];
  // EVERY node the swarm would actually run, in one list. `nodeRows` (golden.ts) owns the union so the
  // test exercises the shipped rule rather than a copy of it.
  const weightRows: WeightRow[] = nodeRows(configuredDevices, fleet.models).map((r, i) => {
    const chip = chipFor(r.provider);
    return {
      id: r.id,
      name: chip ? r.modelId : deviceFromModelId(r.modelId) || r.id,
      provider: chip ? chip.seg : LOCAL_CHIP.seg,
      modelId: r.modelId,
      supervises: r.supervises,
      hue: nodeHue(i),
    };
  });
  const weightFor = (id: string): number => {
    const sw = cfg.speed_weights ?? {};
    if (id in sw) return sw[id] ?? 1; // exact device-id key wins (avoids substring collisions)
    const key = Object.keys(sw).find((k) => id.includes(k));
    return key ? (sw[key] ?? 1) : 1;
  };

  const weightColumns: DataTableColumn<WeightRow>[] = [
    {
      key: 'node',
      header: 'Node',
      cell: (row) => (
        <span className="flex items-center gap-2">
          <StatusDot node={row.hue} label={`node ${row.name}`} />
          <span className={cx('truncate', WEIGHT.semibold)} title={row.id}>
            {row.name}
          </span>
        </span>
      ),
    },
    { key: 'provider', header: 'Provider', cell: (row) => <Chip>{row.provider}</Chip> },
    {
      key: 'supervisor',
      header: 'Supervisor',
      cell: (row) => (
        <button
          type="button"
          onClick={() => setSupervisor(row.id, row.modelId, !row.supervises)}
          aria-pressed={row.supervises}
          title={
            row.supervises
              ? 'Supervisor: takes the judge, review and synthesis calls and does not build'
              : 'Make this the supervisor — the strongest model, kept out of the build pool'
          }
          className={cx(
            'inline-flex h-7 items-center px-2.5 text-[12px] font-lz-medium',
            RADIUS.control,
            row.supervises
              ? TONE_FILL.secondary
              : cx(SURFACE.outline, 'bg-lz-surface text-lz-ink-2 hover:bg-lz-surface-2 hover:text-lz-ink'),
            FOCUS,
            MOTION
          )}
        >
          Smartest
        </button>
      ),
    },
    {
      key: 'weight',
      header: 'Weight',
      numeric: true,
      cell: (row) => (
        <WeightStepper value={weightFor(row.id)} onChange={(v) => setWeight(row.id, v)} label={row.id} />
      ),
    },
  ];

  return (
    <section id="swarm" className="flex flex-col gap-lz-section pb-8 pr-4">
      <Panel
        title="Goose Swarm — fleet"
        headerRight={
          <Segmented
            options={NODE_PROVIDERS}
            value={nodeProvider}
            onChange={setNodeProvider}
            label="Add a node from"
          />
        }
      >
        <div className="flex flex-col gap-4">
          <p className={TYPE.bodyMuted}>
            Your local model fleet (LM Studio / LM Link), live. The swarm auto-pools whatever is resident.
          </p>
          {/* ADD A NODE, by choosing what will serve it. The list below is the nodes you HAVE; the
              Segmented in the header is how a new one is made, which is the order the user thinks in —
              a node first, then what runs it. A provider only appears here once it has been configured
              with a working key (the panes validate engine-side), so the list can never offer a node
              that cannot run. */}
          <div className="flex items-center justify-between gap-3">
            {nodeProvider === 'LM Studio' ? (
              <span className="flex min-w-0 items-center gap-2">
                <span className="truncate font-mono text-lz-mono text-lz-ink-3">{cfg.endpoint}</span>
                <span className={cx('inline-flex shrink-0 items-center gap-1.5', TYPE.meta, TNUM)}>
                  <StatusDot
                    tone={fleet.online ? 'ok' : 'stopped'}
                    label={fleet.online ? 'fleet live' : 'fleet offline'}
                    live={fleet.online}
                  />
                  {fleet.online
                    ? `${fleet.lanes.length} node${fleet.lanes.length === 1 ? '' : 's'} live`
                    : 'offline'}
                </span>
              </span>
            ) : (
              <span className={cx(TYPE.meta, TNUM)}>
                {activeCloudDevices.length} {activeCloud?.label} node
                {activeCloudDevices.length === 1 ? '' : 's'} in the pool
              </span>
            )}
          </div>
          {nodeProvider === 'LM Studio' ? (
            fleet.online && fleet.lanes.length > 0 ? (
              <FanInCard dispatch="fleet · live" lanes={fleet.lanes} />
            ) : (
              <EmptyState
                icon={<Radar />}
                title="No fleet detected"
                body={`Start LM Studio (LM Link) at ${cfg.endpoint} to see your nodes.`}
              />
            )
          ) : activeCloud ? (
            <CloudPane
              key={activeCloud.cli}
              def={activeCloud}
              devices={activeCloudDevices}
              onChanged={reloadSwarm}
            />
          ) : null}

          {weightRows.length > 0 && (
            <div className={cx('flex flex-col gap-1 border-t pt-3', SURFACE.hairline)}>
              <SectionHeader as="h3" title="Node weights" count={weightRows.length} />
              <p className={cx(TYPE.bodyMuted, 'mb-1')}>
                Higher = a bigger share of the tasks. Turn a slower machine down so it does less.
              </p>
              <DataTable
                dense
                aria-label="Node weights"
                columns={weightColumns}
                rows={weightRows}
                rowKey={(r) => r.id}
              />
            </div>
          )}

          <div className={cx('divide-y divide-lz-border border-t', SURFACE.hairline)}>
            <Row
              label="Run panel detail"
              hint="Compact = headline phases · Verbose = full timeline + reasoning + tool calls · Developer = everything expanded & raw"
            >
              <Segmented options={SWARM_LOG_MODES} value={logMode} onChange={setLogMode} label="Run panel detail" />
            </Row>
            <Row
              label="Ask when uncertain"
              hint="When the planner isn't confident how to break the app down, it pauses and asks YOU a few clarifying questions instead of guessing. On by default — the single most useful choice for a weak local planner. Turn it off (0) for an unattended/CI build that must never block on a human."
            >
              <StudioSwitch
                aria-label="Ask when uncertain"
                checked={(cfg.ask_floor ?? 80) > 0}
                onChange={(v) => set({ ask_floor: v ? 80 : 0 })}
              />
            </Row>
            {/* ask_max_q is GONE: the engine asks EVERY open decision since the truncation kill
                (cfcd32908) and reads the key nowhere — the control was inert. The measured harm the
                old hint described (3 of 5 asked, 2 guessed silently) is exactly what the kill fixed. */}
          </div>

          <p className={TYPE.bodyMuted}>
            Download and manage the models each node runs in the{' '}
            <span className={cx('text-lz-ink', WEIGHT.medium)}>LeanZero MLX</span> tab.
          </p>
        </div>
      </Panel>

      <Panel title="Goose Swarm — tunables">
        <p className={TYPE.bodyMuted}>
          The knobs that were CLI-only (`goose swarm pool`). Changes save to your goose config immediately.
        </p>
      </Panel>

      {!loaded ? (
        <div className={TYPE.bodyMuted}>Loading swarm config…</div>
      ) : (
        <>
          <PresetBar active={activePreset} onApply={applyPreset} />

          <Group title="Advanced — timeouts & budgets" cost="Per-hardware tuning. The defaults are the golden formula; change these only for an unusually fast or slow fleet.">
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

          <Group title="Research & planning">
            <Row label="Research planning" hint="off / on / auto (auto = only with source files). Grounds the plan in looked-up fact; costs a research phase.">
              <Segmented options={RESEARCH_MODES} value={(cfg.research_planning ?? 'on') as ResearchMode} onChange={(v) => set({ research_planning: v })} label="Research planning" />
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
              <StudioSwitch aria-label="Also flag dead code after the build" checked={!!cfg.review} onChange={(v) => set({ review: v })} />
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
            <Row
              label="Extra LM Studio request fields (JSON)"
              hint='Merged verbatim into every LM Studio request body — the passthrough for fields LM Studio honors per request. Note: per-model CUSTOM fields (like this model&apos;s thinking effort) are applied by LM Studio itself, not per request — set those in LM Studio&apos;s model settings on each host. Example: {"seed": 7}'
            >
              <ExtraBodyField
                value={cfg.lm_extra_body}
                onCommit={(v) => set({ lm_extra_body: v })}
              />
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
              <StudioSwitch aria-label="Planner also works" checked={!!cfg.planner_also_works} onChange={(v) => set({ planner_also_works: v })} />
            </Row>
            <Row label="Planner weight" hint="worker weight for the planner when it pitches in">
              <NumberField value={cfg.planner_weight} onCommit={(v) => set({ planner_weight: v ?? 1 })} />
            </Row>
            <Row label="Homogeneous models" hint="all workers same weights/tokenizer → planner splits more aggressively">
              <StudioSwitch aria-label="Homogeneous models" checked={!!cfg.homogeneous_models} onChange={(v) => set({ homogeneous_models: v })} />
            </Row>
            <Row label="Allow model load" hint="let the swarm spin up non-resident models (off = warm fleet only)">
              <StudioSwitch aria-label="Allow model load" checked={!!cfg.allow_model_load} onChange={(v) => set({ allow_model_load: v })} />
            </Row>
          </Group>
        </>
      )}
    </section>
  );
}
