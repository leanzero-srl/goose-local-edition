import { useCallback, useEffect, useState } from 'react';
import { Loader2 } from 'lucide-react';
import type { SwarmDeviceRow } from '../settings/swarm/golden';
import {
  Button,
  Chip,
  DataTable,
  KeyValue,
  Panel,
  StatusDot,
  SURFACE,
  TYPE,
  cx,
  type DataTableColumn,
  type KeyValueItem,
} from '../lz';
import { INPUT, ToneBanner } from './studio';

/** The last CLI error line, human-readable — the engine prints one-line `Error: …` messages. */
export function cloudCliErr(r: { stdout: string; stderr: string; error: string | null }): string {
  const m = (r.stderr || '').match(/Error:\s*([\s\S]+)/);
  if (m) return m[1].trim();
  return (r.stderr || r.error || 'the goose engine call failed').trim();
}

/** The cloud providers the panel can add nodes from — THE single mirror of the engine's CLOUD_DEFS
 *  (crates/goose-cli/src/commands/swarm.rs). `cli` = the `goose swarm cloud <cli>` name and the
 *  SwarmDevice.provider value; `registry` = the goose provider-registry id (CloudDef.registry),
 *  which is how configured-ness joins acpListProviderDetails; `seg` = the short label every
 *  surface shows for what serves a node. Every surface derives from this table — when the engine
 *  grows a cloud family, it is added HERE and nowhere else. No colour lives here: provider is
 *  text (a quiet Chip), node identity is the nodeHue ramp. */
export const CLOUD_PROVIDERS = [
  {
    seg: 'Bedrock',
    cli: 'bedrock',
    registry: 'aws_bedrock',
    label: 'Amazon Bedrock',
    keyPlaceholder: 'Bedrock API key (ABSK…)',
    region: true,
  },
  {
    seg: 'Z.ai',
    cli: 'zai',
    registry: 'zai',
    label: 'Z.ai',
    keyPlaceholder: 'Z.ai API key',
    region: false,
  },
  {
    seg: 'Gemini',
    cli: 'google',
    registry: 'google',
    label: 'Google Gemini',
    keyPlaceholder: 'Gemini API key (AIza…)',
    region: false,
  },
  {
    seg: 'DeepSeek',
    cli: 'deepseek',
    registry: 'custom_deepseek',
    label: 'DeepSeek',
    keyPlaceholder: 'DeepSeek API key (sk-…)',
    region: false,
  },
] as const;
export type CloudProviderDef = (typeof CLOUD_PROVIDERS)[number];

export const chipFor = (provider: string | null | undefined): CloudProviderDef | null =>
  CLOUD_PROVIDERS.find((c) => c.cli === provider) ?? null;

/** A node is local unless a cloud provider claims it: the LM Studio fleet, or the LeanZero MLX
 *  engine (SwarmDevice.engine === 'mlx-sidecar') — every row in the Nodes list is labelled by
 *  what serves it. */
export const LOCAL_CHIP = { seg: 'LM Studio' } as const;
export const MLX_CHIP = { seg: 'LeanZero MLX' } as const;

/**
 * A cloud provider's key + roster + node add/remove (Bedrock, Z.ai, Gemini, DeepSeek). The whole
 * contract runs through the engine CLI over IPC (`goose swarm cloud <provider> … --json`) — the
 * same code path the terminal uses, so desktop and CLI can never disagree: key validation happens
 * ENGINE-side (stored only when the provider accepts it), the model roster AUTO-POPULATES from
 * what the key can actually invoke, and add/rm write the device list through the engine. After any
 * device mutation the parent re-reads the swarm config (onChanged) so the panel's in-memory copy
 * never clobbers CLI-written devices on a later save. THE INVARIANT: the desktop never upserts a
 * cloud device row itself.
 */
export function CloudPane({
  def,
  devices,
  onChanged,
  onAdded,
  addWeight = 2,
}: {
  def: CloudProviderDef;
  devices: SwarmDeviceRow[];
  /** Fired after a SUCCESSFUL engine-side device mutation (add or rm) — re-read the swarm config.
   *  A refused CLI call changed nothing, so it must not fire (the reassign flow keys off this). */
  onChanged: () => Promise<void>;
  /** Fired only after a successful ADD — the reassign flow removes the old row here. */
  onAdded?: (modelId: string) => Promise<void>;
  /** Weight passed to `goose swarm cloud <p> add <model> --weight N` (the add-node dialog's stepper). */
  addWeight?: number;
}) {
  const [phase, setPhase] = useState<'checking' | 'no-key' | 'ready'>('checking');
  const [error, setError] = useState<string | null>(null);
  const [region, setRegion] = useState('us-east-1');
  const [keyText, setKeyText] = useState('');
  const [roster, setRoster] = useState<string[]>([]);
  const [filter, setFilter] = useState('');
  const [busy, setBusy] = useState<string | null>(null); // 'validate' | model_id being added/removed
  const [editKey, setEditKey] = useState(false);

  const refresh = useCallback(async () => {
    const r = await window.electron.swarmCloud(def.cli, ['models', '--json']);
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
      setError(cloudCliErr(r));
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
    const r = await window.electron.swarmCloud(def.cli, args);
    setBusy(null);
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
      setError(cloudCliErr(r));
    }
  }, [keyText, region, def.cli, def.region]);

  const addNode = useCallback(
    async (modelId: string) => {
      setBusy(modelId);
      setError(null);
      const r = await window.electron.swarmCloud(def.cli, [
        'add',
        modelId,
        '--weight',
        String(addWeight),
      ]);
      setBusy(null);
      if (!r.ok) {
        setError(cloudCliErr(r));
        return;
      }
      await onChanged();
      if (onAdded) await onAdded(modelId);
    },
    [def.cli, addWeight, onChanged, onAdded]
  );

  const rmNode = useCallback(
    async (modelId: string) => {
      setBusy(modelId);
      setError(null);
      const r = await window.electron.swarmCloud(def.cli, ['rm', modelId]);
      setBusy(null);
      if (!r.ok) {
        setError(cloudCliErr(r));
        return;
      }
      await onChanged();
    },
    [def.cli, onChanged]
  );

  const configured = new Set(devices.map((d) => d.model_id));
  const shown = roster.filter(
    (m) => !filter.trim() || m.toLowerCase().includes(filter.trim().toLowerCase())
  );
  const keyEntry = (
    <div className="flex flex-col gap-2">
      <p className={TYPE.bodyMuted}>
        Paste a {def.label} API key. goose validates it live first — the key is stored (encrypted,
        in your goose secret store) only when {def.label} accepts it, and the models it can run
        auto-populate below.
      </p>
      <div className="flex flex-wrap items-center gap-2">
        <input
          type="password"
          className={cx(INPUT, 'min-w-[220px] flex-1')}
          placeholder={def.keyPlaceholder}
          value={keyText}
          onChange={(e) => setKeyText(e.target.value)}
          aria-label={`${def.label} API key`}
          autoComplete="off"
        />
        {def.region && (
          <input
            className={cx(INPUT, 'w-28')}
            placeholder="region"
            value={region}
            onChange={(e) => setRegion(e.target.value)}
            aria-label="Region"
            autoComplete="off"
          />
        )}
        <Button
          variant="secondary"
          disabled={busy === 'validate' || !keyText.trim()}
          onClick={() => void validateKey()}
          icon={busy === 'validate' ? <Loader2 className="animate-spin" /> : undefined}
        >
          {busy === 'validate' ? 'Validating…' : 'Validate & save'}
        </Button>
      </div>
    </div>
  );

  const keyFacts: KeyValueItem[] = [
    {
      key: 'key',
      label: 'API key',
      value: (
        <span className="inline-flex items-center gap-2">
          <StatusDot tone="ok" label="key valid" />
          key valid
        </span>
      ),
      tone: 'ok',
    },
    { key: 'region', label: 'Region', value: region, mono: true },
    {
      key: 'models',
      label: 'Models available',
      value: `${roster.length} model${roster.length === 1 ? '' : 's'}`,
    },
  ];

  const deviceColumns: DataTableColumn<SwarmDeviceRow>[] = [
    {
      key: 'model',
      header: 'Model',
      cell: (d) => (
        <span className="flex min-w-0 items-center gap-2">
          <Chip>{def.seg}</Chip>
          <span className="truncate font-mono text-lz-mono text-lz-ink" title={d.model_id}>
            {d.model_id}
          </span>
        </span>
      ),
    },
  ];

  const rosterColumns: DataTableColumn<string>[] = [
    {
      key: 'model',
      header: 'Model',
      cell: (m) => (
        <span className="truncate font-mono text-lz-mono text-lz-ink" title={m}>
          {m}
        </span>
      ),
    },
  ];

  return (
    <div className="flex flex-col gap-3">
      {phase === 'checking' ? (
        <p className={cx('flex items-center gap-2', TYPE.meta)}>
          <Loader2 className="size-3 animate-spin" />
          Checking for a stored {def.label} key…
        </p>
      ) : phase === 'no-key' || editKey ? (
        keyEntry
      ) : (
        <Panel
          title={`${def.label} key`}
          headerRight={
            <Button size="sm" variant="ghost" onClick={() => setEditKey(true)}>
              Replace key
            </Button>
          }
          padded={false}
        >
          <div className="px-4">
            <KeyValue dense items={keyFacts} aria-label={`${def.label} key status`} />
          </div>
        </Panel>
      )}

      {error && <ToneBanner tone="err" label={def.label} text={error} />}

      {devices.length > 0 && (
        <Panel title="Cloud nodes in your flock pool" count={devices.length} padded={false}>
          <DataTable
            dense
            aria-label={`${def.label} nodes in the pool`}
            columns={deviceColumns}
            rows={devices}
            rowKey={(d) => d.id}
            rowAction={(d) => (
              <Button
                size="sm"
                variant="ghost"
                disabled={busy === d.model_id}
                onClick={() => void rmNode(d.model_id)}
              >
                {busy === d.model_id ? 'Removing…' : 'Remove'}
              </Button>
            )}
          />
        </Panel>
      )}

      {phase === 'ready' && (
        <Panel
          title="Available models"
          count={shown.length}
          headerRight={
            <input
              className={cx(INPUT, 'w-44')}
              placeholder="filter…"
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
              aria-label={`Filter ${def.label} models`}
              autoComplete="off"
            />
          }
          padded={false}
        >
          <p className={cx('border-b px-4 py-2', TYPE.meta, SURFACE.hairline)}>
            What this key can actually invoke — add one as a swarm node.
          </p>
          <div className="max-h-52 overflow-y-auto">
            <DataTable
              dense
              aria-label={`${def.label} models`}
              columns={rosterColumns}
              rows={shown}
              rowKey={(m) => m}
              rowAction={(m) =>
                configured.has(m) ? (
                  <Chip tone="ok">in pool</Chip>
                ) : (
                  <Button
                    size="sm"
                    variant="secondary"
                    disabled={busy === m}
                    onClick={() => void addNode(m)}
                  >
                    {busy === m ? 'Adding…' : '+ Add'}
                  </Button>
                )
              }
              empty={<p className={TYPE.meta}>no model matches the filter</p>}
            />
          </div>
        </Panel>
      )}
    </div>
  );
}
