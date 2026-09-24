import { useEffect, useMemo, useState, type ReactNode } from 'react';
import { Loader2, Radar, Save } from 'lucide-react';
import { defineMessages, useIntl } from '../../i18n';
import { Button, Chip, Disclosure, SURFACE, TNUM, TONE_TEXT, TYPE, WEIGHT, cx } from '../lz';
import {
  mlxDistributedConfigUpdate,
  mlxDistributedDiscover,
  mlxDistributedPeerCandidates,
  mlxDistributedProvision,
  type MlxDistributedConfig,
  type MlxDistributedLinkDiscovery,
  type MlxDistributedDiscoveredModel,
  type MlxDistributedDiscoveredNode,
  type MlxDistributedDiscovery,
  type MlxDistributedGap,
  type MlxDistributedPeerCandidate,
} from '../../acp/mlx-distributed';
import { backendName, cleanConfig, gb1, gib, missingFields } from './mlxDistributed';
import { mlxErrorMessage } from './mlxErrorMessage';
import { INPUT, StudioSelect, ToneBanner, type StudioSelectOption } from './studio';
import { touchLocalNetwork } from './LocalNetworkNotice';
import { DistributedLinkPeers } from './DistributedLinkPeers';

/** A LeanZero Link Mac is named `link:<node>` wherever an ssh alias would be. */
export function linkNode(host: string | null | undefined): string | null {
  return host?.startsWith('link:') ? host.slice('link:'.length) || null : null;
}

/**
 * "Set up" for the distributed engine: the person names the other Mac ONCE and goose probes both
 * (`distributedDiscover`, read-only) and pre-fills the whole configuration. Every value is shown
 * with the evidence it came from; a value goose could not find is shown as the backend's named
 * gap, in red, and the field stays empty — nothing is guessed here. Save persists the config and
 * starts provisioning each node's goose-managed Python (progress follows in the section). The raw
 * fields stay reachable under Advanced.
 */

const i18n = defineMessages({
  title: { id: 'mlxDistributedSetup.title', defaultMessage: 'Set up the distributed engine' },
  intro: {
    id: 'mlxDistributedSetup.intro',
    defaultMessage:
      'Name the other Mac once. goose probes this Mac and it over ssh and fills in the link, RDMA, backend, models, ports and Python.',
  },
  peer: { id: 'mlxDistributedSetup.peer', defaultMessage: 'Other Mac (ssh alias or host)' },
  peerPlaceholder: { id: 'mlxDistributedSetup.peerPlaceholder', defaultMessage: 'workhorse' },
  detect: { id: 'mlxDistributedSetup.detect', defaultMessage: 'Detect' },
  detecting: {
    id: 'mlxDistributedSetup.detecting',
    defaultMessage: 'Probing this Mac and {peer} over ssh…',
  },
  detectingLink: {
    id: 'mlxDistributedSetup.detectingLink',
    defaultMessage: 'Probing this Mac and {peer} over LeanZero Link…',
  },
  headless: { id: 'mlxDistributedSetup.headless', defaultMessage: 'Headless (ssh)' },
  viaLink: { id: 'mlxDistributedSetup.viaLink', defaultMessage: 'LeanZero Link · {node}' },
  answering: {
    id: 'mlxDistributedSetup.answering',
    defaultMessage: 'Answering from ~/.ssh/config',
  },
  noneAnswering: {
    id: 'mlxDistributedSetup.noneAnswering',
    defaultMessage: 'No alias in ~/.ssh/config answered a non-interactive ssh.',
  },
  probed: {
    id: 'mlxDistributedSetup.probed',
    defaultMessage: 'Probed {count, plural, one {# node} other {# nodes}} in {ms} ms',
  },
  gapsTitle: {
    id: 'mlxDistributedSetup.gapsTitle',
    defaultMessage: '{count, plural, one {# value not found} other {# values not found}}',
  },
  backend: { id: 'mlxDistributedSetup.backend', defaultMessage: 'Backend' },
  model: { id: 'mlxDistributedSetup.model', defaultMessage: 'Model' },
  modelPick: { id: 'mlxDistributedSetup.modelPick', defaultMessage: 'Pick a model' },
  everyNode: { id: 'mlxDistributedSetup.everyNode', defaultMessage: 'on every node' },
  missingOn: { id: 'mlxDistributedSetup.missingOn', defaultMessage: 'missing on {nodes}' },
  differsOn: { id: 'mlxDistributedSetup.differsOn', defaultMessage: 'differs on {nodes}' },
  manifestAgree: { id: 'mlxDistributedSetup.manifestAgree', defaultMessage: 'SHA256SUMS agree' },
  manifestDiffer: {
    id: 'mlxDistributedSetup.manifestDiffer',
    defaultMessage: 'SHA256SUMS differ',
  },
  apiPort: { id: 'mlxDistributedSetup.apiPort', defaultMessage: 'API port' },
  coordinatorPort: {
    id: 'mlxDistributedSetup.coordinatorPort',
    defaultMessage: 'Coordinator port',
  },
  thisMac: { id: 'mlxDistributedSetup.thisMac', defaultMessage: 'this Mac' },
  unreachable: { id: 'mlxDistributedSetup.unreachable', defaultMessage: 'not reachable' },
  memory: {
    id: 'mlxDistributedSetup.memory',
    defaultMessage: '{available} of {total} GiB available',
  },
  tbInterface: { id: 'mlxDistributedSetup.field.tbInterface', defaultMessage: 'Interface' },
  tbIp: { id: 'mlxDistributedSetup.field.tbIp', defaultMessage: 'Link IPv4' },
  tbNetmask: { id: 'mlxDistributedSetup.field.tbNetmask', defaultMessage: 'Netmask' },
  tbService: { id: 'mlxDistributedSetup.field.tbService', defaultMessage: 'Network service' },
  rdmaDevice: { id: 'mlxDistributedSetup.field.rdmaDevice', defaultMessage: 'RDMA device' },
  python: { id: 'mlxDistributedSetup.field.python', defaultMessage: 'Python' },
  pipelinePython: {
    id: 'mlxDistributedSetup.field.pipelinePython',
    defaultMessage: 'Pipeline Python',
  },
  modelDir: { id: 'mlxDistributedSetup.field.modelDir', defaultMessage: 'Model folder' },
  envReady: { id: 'mlxDistributedSetup.env.ready', defaultMessage: 'ready' },
  envAbsent: { id: 'mlxDistributedSetup.env.absent', defaultMessage: 'built at Save' },
  envBroken: { id: 'mlxDistributedSetup.env.broken', defaultMessage: 'rebuilt at Save' },
  envNoUv: { id: 'mlxDistributedSetup.env.noUv', defaultMessage: 'no uv' },
  advanced: { id: 'mlxDistributedSetup.advanced', defaultMessage: 'Advanced' },
  advancedMeta: {
    id: 'mlxDistributedSetup.advancedMeta',
    defaultMessage: 'every field, editable — a Python of your own, another model folder, ports',
  },
  save: { id: 'mlxDistributedSetup.save', defaultMessage: 'Save and provision' },
  cancel: { id: 'mlxDistributedSetup.cancel', defaultMessage: 'Cancel' },
  stillEmpty: {
    id: 'mlxDistributedSetup.stillEmpty',
    defaultMessage:
      'Save needs every value above; resolve the red ones or set them under Advanced.',
  },
  detectError: { id: 'mlxDistributedSetup.error.detect', defaultMessage: 'Detect failed' },
  saveError: { id: 'mlxDistributedSetup.error.save', defaultMessage: 'Save failed' },
  candidatesError: {
    id: 'mlxDistributedSetup.error.candidates',
    defaultMessage: 'ssh aliases unreadable',
  },
});

type NodeField =
  | 'tbInterface'
  | 'tbIp'
  | 'tbNetmask'
  | 'tbService'
  | 'rdmaDevice'
  | 'python'
  | 'pipelinePython'
  | 'modelDir';

const NODE_FIELDS: readonly NodeField[] = [
  'tbInterface',
  'tbIp',
  'tbNetmask',
  'tbService',
  'rdmaDevice',
  'python',
  'pipelinePython',
  'modelDir',
];

function evidenceFor(d: MlxDistributedDiscovery, node: number | null, field: string) {
  return d.evidence.find((e) => (e.node ?? null) === node && e.field === field) ?? null;
}

function gapsFor(d: MlxDistributedDiscovery, node: number | null, field: string) {
  return d.gaps.filter((g) => (g.node ?? null) === node && g.field === field);
}

/** One value: its label, the value (or the gap in red), and the evidence under it. */
function Field({
  discovery,
  node,
  field,
  label,
  value,
  extra,
}: {
  discovery: MlxDistributedDiscovery;
  node: number | null;
  field: string;
  label: string;
  value: string;
  extra?: ReactNode;
}) {
  const ev = evidenceFor(discovery, node, field);
  const gaps = gapsFor(discovery, node, field);
  return (
    <div
      data-testid="mlx-dist-setup-field"
      data-field={field}
      data-node={node ?? 'cluster'}
      data-found={gaps.length === 0 && value !== ''}
      className="flex min-w-0 flex-col gap-0.5"
    >
      <span className="flex items-center gap-2">
        <span className={TYPE.meta}>{label}</span>
        {extra}
      </span>
      {value !== '' && <span className={cx('break-all', TYPE.mono)}>{value}</span>}
      {gaps.map((g) => (
        <span
          key={g.reason}
          className={cx('break-words', TYPE.body, WEIGHT.semibold, TONE_TEXT.err)}
        >
          {g.reason}
        </span>
      ))}
      {value === '' && gaps.length === 0 && <span className={TYPE.mono}>—</span>}
      {ev && <span className={cx('break-words', TYPE.meta)}>{ev.evidence}</span>}
    </div>
  );
}

function envTone(state: string | undefined) {
  if (state === 'ready') return 'ok' as const;
  if (state === 'absent') return 'accent' as const;
  return 'err' as const;
}

function NodeBlock({
  discovery,
  draft,
  node,
}: {
  discovery: MlxDistributedDiscovery;
  draft: MlxDistributedConfig;
  node: MlxDistributedDiscoveredNode;
}) {
  const intl = useIntl();
  const rank = node.rank;
  const config = draft.nodes[rank];
  const envWord: Record<string, string> = {
    ready: intl.formatMessage(i18n.envReady),
    absent: intl.formatMessage(i18n.envAbsent),
    broken: intl.formatMessage(i18n.envBroken),
    noUv: intl.formatMessage(i18n.envNoUv),
  };
  const fields = NODE_FIELDS.filter((f) => {
    if (f === 'rdmaDevice') return draft.backend === 'jaccl' || gapsFor(discovery, rank, f).length;
    if (f === 'pipelinePython') return config?.pipelinePython != null;
    return true;
  });
  return (
    <div
      data-testid="mlx-dist-setup-node"
      data-node={node.name}
      className={cx('flex min-w-0 flex-col gap-3 p-4', SURFACE.card)}
    >
      <div className="flex flex-wrap items-center gap-2">
        <span className={TYPE.h2}>{node.name}</span>
        <Chip tone={node.reachable ? 'accent' : 'err'}>
          {linkNode(node.host)
            ? intl.formatMessage(i18n.viaLink, { node: linkNode(node.host) })
            : (node.host ?? intl.formatMessage(i18n.thisMac))}
        </Chip>
        {!node.reachable && <Chip tone="err">{intl.formatMessage(i18n.unreachable)}</Chip>}
        {node.linkSpeed && <Chip tone="ok">{node.linkSpeed}</Chip>}
      </div>
      {node.availableBytes != null && node.totalBytes != null && (
        <span className={cx(TYPE.body, TNUM)}>
          {intl.formatMessage(i18n.memory, {
            available: gb1(gib(node.availableBytes)),
            total: gb1(gib(node.totalBytes)),
          })}
        </span>
      )}
      <div className="grid grid-cols-1 gap-3 lg:grid-cols-2">
        {fields.map((field) => (
          <Field
            key={field}
            discovery={discovery}
            node={rank}
            field={field}
            label={intl.formatMessage(i18n[field])}
            value={(config?.[field] as string | undefined) ?? ''}
            extra={
              field === 'python' && node.env ? (
                <Chip tone={envTone(node.env.state)}>
                  {envWord[node.env.state] ?? node.env.state}
                </Chip>
              ) : undefined
            }
          />
        ))}
      </div>
      {node.uv && <span className={cx('break-all', TYPE.meta)}>uv · {node.uv}</span>}
    </div>
  );
}

interface ModelOption extends StudioSelectOption {
  model: MlxDistributedDiscoveredModel;
}

function modelChips(
  intl: ReturnType<typeof useIntl>,
  model: MlxDistributedDiscoveredModel,
  names: (rank: number) => string
) {
  const absent = model.nodes.filter((n) => n.state === 'absent').map((n) => names(n.rank));
  const differs = model.nodes.filter((n) => n.state === 'differs').map((n) => names(n.rank));
  return (
    <>
      {model.onEveryNode && <Chip tone="ok">{intl.formatMessage(i18n.everyNode)}</Chip>}
      {absent.length > 0 && (
        <Chip tone="err">{intl.formatMessage(i18n.missingOn, { nodes: absent.join(', ') })}</Chip>
      )}
      {differs.length > 0 && (
        <Chip tone="err">{intl.formatMessage(i18n.differsOn, { nodes: differs.join(', ') })}</Chip>
      )}
      {model.manifest === 'agree' && <Chip>{intl.formatMessage(i18n.manifestAgree)}</Chip>}
      {model.manifest === 'differ' && (
        <Chip tone="err">{intl.formatMessage(i18n.manifestDiffer)}</Chip>
      )}
      <Chip>{`${gb1(gib(model.weightsBytes))} GiB · ${model.runner}`}</Chip>
    </>
  );
}

export interface DistributedSetupProps {
  /** The peer to probe first (a saved config's alias), or '' for a fresh setup. */
  initialPeer: string;
  /** Prefer this model when it is on every node (a saved config's). */
  preferredModel: string | null;
  /** The raw editor for the draft (the section's own), shown under Advanced. */
  renderAdvanced: (
    draft: MlxDistributedConfig,
    onChange: (next: MlxDistributedConfig) => void
  ) => ReactNode;
  onCancel: () => void;
  /** Saved and provisioning started — the section re-reads its status. */
  onSaved: () => void;
}

export function DistributedSetup({
  initialPeer,
  preferredModel,
  renderAdvanced,
  onCancel,
  onSaved,
}: DistributedSetupProps) {
  const intl = useIntl();
  const [peer, setPeer] = useState(initialPeer);
  const [candidates, setCandidates] = useState<MlxDistributedPeerCandidate[] | null>(null);
  const [link, setLink] = useState<MlxDistributedLinkDiscovery | null>(null);
  const [candidatesError, setCandidatesError] = useState<string | null>(null);
  const [discovery, setDiscovery] = useState<MlxDistributedDiscovery | null>(null);
  const [draft, setDraft] = useState<MlxDistributedConfig | null>(null);
  const [busy, setBusy] = useState<'detect' | 'save' | null>(null);
  const [error, setError] = useState<{ label: string; text: string } | null>(null);

  useEffect(() => {
    let live = true;
    mlxDistributedPeerCandidates()
      .then((c) => {
        if (!live) return;
        setCandidates(c.candidates);
        setLink(c.link);
      })
      .catch((e) => live && setCandidatesError(mlxErrorMessage(e, String(e))));
    return () => {
      live = false;
    };
  }, []);

  const detect = async (modelId: string | null, pick?: string) => {
    const peers = [(pick ?? peer).trim()].filter(Boolean);
    if (peers.length === 0) return;
    setBusy('detect');
    setError(null);
    try {
      await touchLocalNetwork();
      const found = await mlxDistributedDiscover(peers, modelId);
      setDiscovery(found);
      setDraft(found.config);
    } catch (e) {
      setError({
        label: intl.formatMessage(i18n.detectError),
        text: mlxErrorMessage(e, String(e)),
      });
    } finally {
      setBusy(null);
    }
  };

  const save = async () => {
    if (!draft) return;
    setBusy('save');
    setError(null);
    try {
      const saved = await mlxDistributedConfigUpdate(cleanConfig(draft));
      await mlxDistributedProvision(saved);
      onSaved();
    } catch (e) {
      setError({ label: intl.formatMessage(i18n.saveError), text: mlxErrorMessage(e, String(e)) });
    } finally {
      setBusy(null);
    }
  };

  const names = (rank: number) =>
    discovery?.nodes.find((n) => n.rank === rank)?.name ?? String(rank);
  const modelOptions = useMemo<ModelOption[]>(
    () => (discovery?.models ?? []).map((m) => ({ value: m.id, label: m.id, model: m })),
    [discovery]
  );
  const answering = (candidates ?? []).filter((c) => c.answered);
  const missing = draft ? missingFields(draft) : [];

  return (
    <div data-testid="mlx-dist-setup" className={cx('flex flex-col gap-4 p-4', SURFACE.card)}>
      <div className="flex flex-col gap-1">
        <span className={TYPE.h2}>{intl.formatMessage(i18n.title)}</span>
        <span className={TYPE.bodyMuted}>{intl.formatMessage(i18n.intro)}</span>
      </div>
      {link != null && (
        <DistributedLinkPeers
          link={link}
          selectedHost={peer.trim()}
          disabled={busy != null}
          onPick={(host) => {
            setPeer(host);
            void detect(preferredModel, host);
          }}
        />
      )}
      <form
        className="flex flex-wrap items-end gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          void detect(preferredModel);
        }}
      >
        <label className="flex min-w-[220px] flex-1 flex-col gap-1">
          <span className={TYPE.meta}>{intl.formatMessage(i18n.peer)}</span>
          <input
            data-testid="mlx-dist-setup-peer"
            value={peer}
            onChange={(e) => setPeer(e.target.value)}
            placeholder={intl.formatMessage(i18n.peerPlaceholder)}
            className={cx(INPUT, 'w-full font-mono text-lz-mono')}
            aria-label={intl.formatMessage(i18n.peer)}
            autoComplete="off"
            spellCheck={false}
            disabled={busy != null}
          />
        </label>
        <Button
          type="submit"
          variant="primary"
          icon={busy === 'detect' ? <Loader2 className="animate-spin" /> : <Radar />}
          disabled={busy != null || peer.trim() === ''}
        >
          {intl.formatMessage(i18n.detect)}
        </Button>
        <Button type="button" variant="secondary" onClick={onCancel} disabled={busy != null}>
          {intl.formatMessage(i18n.cancel)}
        </Button>
      </form>
      {candidates != null && (
        <div data-testid="mlx-dist-setup-candidates" className="flex flex-wrap items-center gap-2">
          <span className={cx(TYPE.meta, WEIGHT.semibold)}>
            {intl.formatMessage(i18n.headless)}
          </span>
          <span className={TYPE.meta}>
            {intl.formatMessage(answering.length ? i18n.answering : i18n.noneAnswering)}
          </span>
          {answering.map((c) => (
            <button
              key={c.alias}
              type="button"
              data-testid="mlx-dist-setup-candidate"
              onClick={() => setPeer(c.alias)}
              disabled={busy != null}
              className={cx(
                'inline-flex h-6 items-center gap-1 px-2 text-lz-meta',
                WEIGHT.semibold,
                peer === c.alias
                  ? 'bg-lz-accent text-lz-accent-ink'
                  : 'border border-lz-border-strong bg-lz-surface text-lz-ink hover:bg-lz-surface-2',
                'rounded-lz-control'
              )}
            >
              <span className="font-mono">{c.alias}</span>
              <span>· {c.detail}</span>
            </button>
          ))}
        </div>
      )}
      {candidatesError && (
        <ToneBanner
          tone="warn"
          label={intl.formatMessage(i18n.candidatesError)}
          text={candidatesError}
        />
      )}
      {busy === 'detect' && (
        <ToneBanner
          tone="accent"
          live
          label={intl.formatMessage(i18n.detect)}
          text={
            linkNode(peer.trim())
              ? intl.formatMessage(i18n.detectingLink, {
                  peer: link?.peers?.find((p) => p.host === peer.trim())?.name ?? peer.trim(),
                })
              : intl.formatMessage(i18n.detecting, { peer: peer.trim() })
          }
        />
      )}
      {error && (
        <ToneBanner
          tone="err"
          label={error.label}
          text={error.text}
          testId="mlx-dist-setup-error"
        />
      )}

      {discovery && draft && (
        <div data-testid="mlx-dist-setup-result" className="flex flex-col gap-4">
          <span className={cx(TYPE.meta, TNUM)}>
            {intl.formatMessage(i18n.probed, {
              count: discovery.nodes.length,
              ms: intl.formatNumber(discovery.probeMs),
            })}
          </span>
          {discovery.gaps.length > 0 && (
            <div
              data-testid="mlx-dist-setup-gaps"
              role="alert"
              className="flex flex-col gap-1.5 rounded-lz-card bg-lz-err-solid px-4 py-3 text-white"
            >
              <span className={cx('text-lz-body', WEIGHT.semibold)}>
                {intl.formatMessage(i18n.gapsTitle, { count: discovery.gaps.length })}
              </span>
              <ul className="flex flex-col gap-1">
                {discovery.gaps.map((g: MlxDistributedGap) => (
                  <li
                    key={`${g.node ?? ''}|${g.field}|${g.reason}`}
                    className="break-words text-lz-body"
                  >
                    <span className={cx('font-mono', WEIGHT.semibold)}>
                      {g.node != null ? `${names(g.node)} · ` : ''}
                      {g.field}
                    </span>{' '}
                    {g.reason}
                  </li>
                ))}
              </ul>
            </div>
          )}
          <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
            <Field
              discovery={discovery}
              node={null}
              field="backend"
              label={intl.formatMessage(i18n.backend)}
              value={backendName(draft.backend) ?? ''}
            />
            <div
              data-testid="mlx-dist-setup-field"
              data-field="modelId"
              data-node="cluster"
              data-found={draft.modelId !== ''}
              className="flex min-w-0 flex-col gap-1"
            >
              <span className={TYPE.meta}>{intl.formatMessage(i18n.model)}</span>
              <StudioSelect<ModelOption>
                aria-label={intl.formatMessage(i18n.model)}
                options={modelOptions}
                value={modelOptions.find((o) => o.value === draft.modelId) ?? null}
                onChange={(o) => o && void detect(o.value)}
                placeholder={intl.formatMessage(i18n.modelPick)}
                disabled={busy != null}
                renderOption={(o, where) =>
                  where === 'value' ? (
                    <span className="font-mono text-lz-mono">{o.label}</span>
                  ) : (
                    <span className="flex min-w-0 items-center gap-2">
                      <span className="min-w-0 truncate font-mono text-lz-mono">{o.label}</span>
                      {modelChips(intl, o.model, names)}
                    </span>
                  )
                }
                optionTestId={(o) => `mlx-dist-setup-model-${o.value}`}
              />
              {gapsFor(discovery, null, 'modelId').map((g) => (
                <span
                  key={g.reason}
                  className={cx('break-words', TYPE.body, WEIGHT.semibold, TONE_TEXT.err)}
                >
                  {g.reason}
                </span>
              ))}
              {evidenceFor(discovery, null, 'modelId') && (
                <span className={cx('break-words', TYPE.meta)}>
                  {evidenceFor(discovery, null, 'modelId')?.evidence}
                </span>
              )}
              {(() => {
                const chosen = discovery.models.find((m) => m.id === draft.modelId);
                return chosen ? (
                  <span className="flex flex-wrap gap-1.5">{modelChips(intl, chosen, names)}</span>
                ) : null;
              })()}
            </div>
            <Field
              discovery={discovery}
              node={null}
              field="port"
              label={intl.formatMessage(i18n.apiPort)}
              value={draft.port ? String(draft.port) : ''}
            />
            <Field
              discovery={discovery}
              node={null}
              field="coordinatorPort"
              label={intl.formatMessage(i18n.coordinatorPort)}
              value={draft.coordinatorPort ? String(draft.coordinatorPort) : ''}
            />
          </div>
          <div className="grid grid-cols-1 gap-3 xl:grid-cols-2">
            {discovery.nodes.map((n) => (
              <NodeBlock key={`${n.rank}|${n.name}`} discovery={discovery} draft={draft} node={n} />
            ))}
          </div>
          <Disclosure
            testId="mlx-dist-setup-advanced"
            title={intl.formatMessage(i18n.advanced)}
            meta={<span className={TYPE.meta}>{intl.formatMessage(i18n.advancedMeta)}</span>}
          >
            <div className="p-4">{renderAdvanced(draft, setDraft)}</div>
          </Disclosure>
          {missing.length > 0 && (
            <p className={cx('break-words', TYPE.body, WEIGHT.semibold, TONE_TEXT.warn)}>
              {intl.formatMessage(i18n.stillEmpty)}
            </p>
          )}
          <div className="flex flex-wrap items-center gap-2">
            <Button
              variant="primary"
              icon={busy === 'save' ? <Loader2 className="animate-spin" /> : <Save />}
              onClick={() => void save()}
              disabled={busy != null || missing.length > 0}
            >
              {intl.formatMessage(i18n.save)}
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}
