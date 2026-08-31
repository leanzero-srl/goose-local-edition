import { useCallback, useEffect, useState } from 'react';
import { Input } from '../ui/input';
import type { SwarmDeviceRow } from '../settings/swarm/golden';

/** The last CLI error line, human-readable — the engine prints one-line `Error: …` messages. */
export function cloudCliErr(r: { stdout: string; stderr: string; error: string | null }): string {
  const m = (r.stderr || '').match(/Error:\s*([\s\S]+)/);
  if (m) return m[1].trim();
  return (r.stderr || r.error || 'the goose engine call failed').trim();
}

/** The cloud providers the panel can add nodes from — mirrors the engine's CLOUD_DEFS (cli =
 *  the `goose swarm cloud <cli>` name and the SwarmDevice.provider value). Distinct SOLID chip
 *  hues per provider, per the UI rules. */
export const CLOUD_PROVIDERS = [
  { seg: 'Bedrock', cli: 'bedrock', label: 'Amazon Bedrock', keyPlaceholder: 'Bedrock API key (ABSK…)', region: true, chip: '#8e4ec6' },
  { seg: 'Z.ai', cli: 'zai', label: 'Z.ai', keyPlaceholder: 'Z.ai API key', region: false, chip: '#f76b15' },
  { seg: 'Gemini', cli: 'google', label: 'Google Gemini', keyPlaceholder: 'Gemini API key (AIza…)', region: false, chip: '#12a594' },
  { seg: 'DeepSeek', cli: 'deepseek', label: 'DeepSeek', keyPlaceholder: 'DeepSeek API key (sk-…)', region: false, chip: '#d6409f' },
] as const;
export type CloudProviderDef = (typeof CLOUD_PROVIDERS)[number];

export const chipFor = (provider: string | null | undefined): CloudProviderDef | null =>
  CLOUD_PROVIDERS.find((c) => c.cli === provider) ?? null;

/** A node is local unless a cloud provider claims it. LM Studio gets its own solid hue, and the
 *  LeanZero MLX engine (SwarmDevice.engine === 'mlx-sidecar') its violet — every row in the Nodes
 *  list is labelled by what serves it. */
export const LOCAL_CHIP = { seg: 'LM Studio', chip: '#1d4ed8' } as const;
export const MLX_CHIP = { seg: 'LeanZero MLX', chip: '#7c3aed' } as const;

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
    <div className="space-y-2">
      <div className="text-xs text-text-secondary max-w-[92ch]">
        Paste a {def.label} API key. goose validates it live first — the key is stored (encrypted,
        in your goose secret store) only when {def.label} accepts it, and the models it can run
        auto-populate below.
      </div>
      <div className="flex items-center gap-2">
        <Input
          type="password"
          className="flex-1"
          style={{ borderRadius: 3 }}
          placeholder={def.keyPlaceholder}
          value={keyText}
          onChange={(e) => setKeyText(e.target.value)}
        />
        {def.region && (
          <Input
            className="w-28"
            style={{ borderRadius: 3 }}
            placeholder="region"
            value={region}
            onChange={(e) => setRegion(e.target.value)}
          />
        )}
        <button
          type="button"
          disabled={busy === 'validate' || !keyText.trim()}
          onClick={() => void validateKey()}
          className="px-3 py-1.5 text-xs font-semibold text-background-primary disabled:opacity-50"
          style={{ backgroundColor: '#2e8bff', borderRadius: 3 }}
        >
          {busy === 'validate' ? 'Validating…' : 'Validate & save'}
        </button>
      </div>
    </div>
  );

  return (
    <div className="space-y-2">
      {phase === 'checking' ? (
        <div className="text-sm text-text-secondary">Checking for a stored {def.label} key…</div>
      ) : phase === 'no-key' || editKey ? (
        keyEntry
      ) : (
        <div className="flex items-center justify-between gap-3">
          <span className="text-xs">
            <span style={{ color: '#2ecc71' }} className="font-semibold">
              key valid
            </span>
            <span className="text-text-secondary">
              {' '}
              · {region} · {roster.length} model{roster.length === 1 ? '' : 's'} available
            </span>
          </span>
          <button
            type="button"
            onClick={() => setEditKey(true)}
            className="px-2.5 py-1 text-xs border border-border-primary text-text-secondary hover:text-text-primary hover:border-text-secondary transition-colors"
            style={{ borderRadius: 3 }}
          >
            Replace key
          </button>
        </div>
      )}

      {error && (
        <div
          className="text-xs font-semibold px-3 py-2 text-background-primary"
          style={{ backgroundColor: '#e5484d', borderRadius: 3 }}
        >
          {error}
        </div>
      )}

      {devices.length > 0 && (
        <div className="space-y-1">
          <div className="text-xs text-text-secondary">Cloud nodes in your swarm pool:</div>
          {devices.map((d) => (
            <div
              key={d.id}
              className="flex items-center justify-between gap-3 border border-border-primary px-2.5 py-1.5"
              style={{ borderRadius: 3 }}
            >
              <span className="min-w-0 flex items-center gap-2">
                <span
                  className="text-[10px] font-bold px-1.5 py-0.5 text-background-primary shrink-0"
                  style={{ backgroundColor: def.chip, borderRadius: 3 }}
                >
                  {def.seg.toUpperCase()}
                </span>
                <span className="text-xs font-mono text-text-primary truncate" title={d.model_id}>
                  {d.model_id}
                </span>
              </span>
              <button
                type="button"
                disabled={busy === d.model_id}
                onClick={() => void rmNode(d.model_id)}
                className="px-2 py-0.5 text-xs border border-border-primary text-text-secondary hover:text-text-primary hover:border-text-secondary transition-colors shrink-0 disabled:opacity-50"
                style={{ borderRadius: 3 }}
              >
                {busy === d.model_id ? 'Removing…' : 'Remove'}
              </button>
            </div>
          ))}
        </div>
      )}

      {phase === 'ready' && (
        <div className="space-y-1.5">
          <div className="flex items-center justify-between gap-3">
            <div className="text-xs text-text-secondary">
              Available models — <span className="text-text-primary font-medium">add one as a swarm node</span>:
            </div>
            <Input
              className="w-44"
              style={{ borderRadius: 3 }}
              placeholder="filter…"
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
            />
          </div>
          <div
            className="max-h-52 overflow-y-auto border border-border-primary divide-y divide-border-primary"
            style={{ borderRadius: 3 }}
          >
            {shown.length === 0 ? (
              <div className="px-3 py-2 text-xs text-text-secondary">no model matches the filter</div>
            ) : (
              shown.map((m) => (
                <div key={m} className="flex items-center justify-between gap-3 px-2.5 py-1.5">
                  <span className="text-xs font-mono text-text-primary truncate" title={m}>
                    {m}
                  </span>
                  {configured.has(m) ? (
                    <span className="text-[10px] font-bold shrink-0" style={{ color: '#2ecc71' }}>
                      IN POOL
                    </span>
                  ) : (
                    <button
                      type="button"
                      disabled={busy === m}
                      onClick={() => void addNode(m)}
                      className="px-2 py-0.5 text-xs font-semibold text-background-primary shrink-0 disabled:opacity-50"
                      style={{ backgroundColor: '#2e8bff', borderRadius: 3 }}
                    >
                      {busy === m ? 'Adding…' : '+ Add'}
                    </button>
                  )}
                </div>
              ))
            )}
          </div>
        </div>
      )}
    </div>
  );
}
