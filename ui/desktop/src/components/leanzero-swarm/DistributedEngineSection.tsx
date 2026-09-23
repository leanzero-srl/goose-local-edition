import { useEffect, useMemo, useState, type ReactNode } from 'react';
import {
  Loader2,
  Network,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  Square,
  Stethoscope,
  Trash2,
} from 'lucide-react';
import type { IntlShape, MessageDescriptor } from 'react-intl';
import { defineMessages, useIntl } from '../../i18n';
import {
  Button,
  Chip,
  EmptyState,
  KeyValue,
  Panel,
  RADIUS,
  StatusDot,
  SURFACE,
  TNUM,
  TONE_DOT,
  TONE_FILL,
  TONE_TEXT,
  TYPE,
  WEIGHT,
  cx,
  type KeyValueItem,
  type Tone,
} from '../lz';
import { ConfirmationModal } from '../ui/ConfirmationModal';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '../ui/dialog';
import {
  mlxDistributedConfigUpdate,
  mlxDistributedPreflight,
  mlxDistributedStart,
  mlxDistributedStop,
  type MlxDistributedCheck,
  type MlxDistributedConfig,
  type MlxDistributedEvent,
  type MlxDistributedNodeConfig,
  type MlxDistributedNodePreflight,
  type MlxDistributedNodeStatus,
  type MlxDistributedPreflight,
  type MlxDistributedStartResponse,
  type MlxDistributedStatus,
  type MlxDistributedStopResponse,
} from '../../acp/mlx-distributed';
import {
  mlxEngineUnmount,
  type MlxEngineStatus,
  type MlxLocalModel,
} from '../../acp/mlx-engine';
import {
  backendName,
  cleanConfig,
  emptyNode,
  eventKeys,
  eventTone,
  gb1,
  gib,
  layerSpan,
  missingFields,
  modeSummary,
  nodeStateTone,
  ownsTheMac,
  planForRank,
  pressureTone,
  runStateInFlight,
  runStateTone,
  sameConfig,
  verdictTone,
  withModel,
  type LayerSpan,
  type MissingField,
  type NodeTextField,
} from './mlxDistributed';
import { distributedStateWord, formatMlxMode } from './mlxModeLabel';
import { formatElapsed } from './mlxLiveStats';
import { mlxErrorMessage } from './mlxErrorMessage';
import { INPUT, StudioSelect, StudioSwitch, ToneBanner, type StudioSelectOption } from './studio';

/**
 * Providers › LeanZero MLX › Engine: the DISTRIBUTED engine — one model split across several Macs,
 * supervised by this Mac (rank 0). Everything here is driven by goose's `distributedStatus` (polled
 * by the view shell) and the answers of the actions; the mode, the nodes and every figure are what
 * the backend reported. Nothing is started, stopped or unmounted without the person asking: a start
 * refused because the single engine is mounted OFFERS "Unmount and continue" in a dialog.
 */

const i18n = defineMessages({
  title: { id: 'mlxDistributed.title', defaultMessage: 'Distributed engine' },
  intro: {
    id: 'mlxDistributed.intro',
    defaultMessage:
      'One model split across several Macs over Thunderbolt. This Mac is rank 0: it serves the API and supervises every rank.',
  },
  unavailableTitle: {
    id: 'mlxDistributed.unavailableTitle',
    defaultMessage: 'Distributed inference is unavailable',
  },
  unavailableBody: {
    id: 'mlxDistributed.unavailableBody',
    defaultMessage:
      'This goose backend does not offer the distributed engine: the mlxDistributed capability is missing, so it predates the build that added it. Update goose to split one model across several Macs.',
  },
  peerTitle: {
    id: 'mlxDistributed.peerTitle',
    defaultMessage: 'The distributed engine is supervised from this Mac',
  },
  peerBody: {
    id: 'mlxDistributed.peerBody',
    defaultMessage:
      'You are managing {host}. Switch “Manage on” back to This device to configure, preflight, start or stop it.',
  },
  reading: { id: 'mlxDistributed.reading', defaultMessage: 'Reading the distributed engine…' },
  unreadable: { id: 'mlxDistributed.unreadable', defaultMessage: 'Status unreadable' },
  retry: { id: 'mlxDistributed.retry', defaultMessage: 'Retry' },
  model: { id: 'mlxDistributed.fact.model', defaultMessage: 'Model' },
  baseUrl: { id: 'mlxDistributed.fact.baseUrl', defaultMessage: 'Base URL' },
  contextLimit: { id: 'mlxDistributed.fact.contextLimit', defaultMessage: 'Context limit' },
  runner: { id: 'mlxDistributed.fact.runner', defaultMessage: 'Runner' },
  restarts: { id: 'mlxDistributed.fact.restarts', defaultMessage: 'Restarts' },
  lastError: { id: 'mlxDistributed.lastError', defaultMessage: 'Last error' },
  preflight: { id: 'mlxDistributed.action.preflight', defaultMessage: 'Preflight (dry run)' },
  start: { id: 'mlxDistributed.action.start', defaultMessage: 'Start' },
  stop: { id: 'mlxDistributed.action.stop', defaultMessage: 'Stop' },
  repairLink: {
    id: 'mlxDistributed.repairLink',
    defaultMessage: 'Repair the Thunderbolt link if a JACCL check fails',
  },
  startHint: {
    id: 'mlxDistributed.startHint',
    defaultMessage:
      'Start runs the preflight, repairs the Thunderbolt link when JACCL needs it, then launches every rank under supervision.',
  },
  locked: {
    id: 'mlxDistributed.locked',
    defaultMessage:
      'The configuration is locked while the distributed engine owns this Mac. Stop it to change nodes or the model.',
  },
  admissionOpen: { id: 'mlxDistributed.admissionOpen', defaultMessage: 'Admitting requests' },
  admissionClosed: { id: 'mlxDistributed.admissionClosed', defaultMessage: 'Admission closed' },
  admissionClosedWhy: {
    id: 'mlxDistributed.admissionClosedWhy',
    defaultMessage: "A node's memory is low: new requests wait until it recovers.",
  },
  inflight: { id: 'mlxDistributed.inflight', defaultMessage: 'In flight' },
  notMeasured: { id: 'mlxDistributed.notMeasured', defaultMessage: 'not measured' },
  liveness: { id: 'mlxDistributed.liveness', defaultMessage: 'Liveness' },
  livenessLine: {
    id: 'mlxDistributed.livenessLine',
    defaultMessage:
      '{samples, plural, one {# sample} other {# samples}} · median {median} · hang bound {bound} · silent {silent}',
  },
  livenessNoBound: {
    id: 'mlxDistributed.livenessNoBound',
    defaultMessage:
      '{samples, plural, one {# sample} other {# samples}} · silent {silent} · no hang bound yet',
  },
  livenessNone: {
    id: 'mlxDistributed.livenessNone',
    defaultMessage: 'No progress measured yet',
  },
  livenessBar: {
    id: 'mlxDistributed.livenessBar',
    defaultMessage: 'Silence against the hang bound',
  },
  nodes: { id: 'mlxDistributed.nodes', defaultMessage: 'Nodes' },
  coordinator: { id: 'mlxDistributed.role.coordinator', defaultMessage: 'coordinator' },
  worker: { id: 'mlxDistributed.role.worker', defaultMessage: 'worker' },
  rank: { id: 'mlxDistributed.rank', defaultMessage: 'rank {rank}' },
  pid: { id: 'mlxDistributed.pid', defaultMessage: 'pid {pid}' },
  layers: {
    id: 'mlxDistributed.layers',
    defaultMessage: 'Layers {first}–{last} · {count, plural, one {# layer} other {# layers}}',
  },
  shard: { id: 'mlxDistributed.shard', defaultMessage: 'Shard {index} of {count}' },
  noLayers: { id: 'mlxDistributed.noLayers', defaultMessage: 'Layers not reported' },
  peakOf: { id: 'mlxDistributed.peakOf', defaultMessage: 'GiB peak of {budget} GiB budget' },
  peakNoBudget: {
    id: 'mlxDistributed.peakNoBudget',
    defaultMessage: 'GiB peak · no budget reported',
  },
  noPeak: { id: 'mlxDistributed.noPeak', defaultMessage: 'No peak reported yet' },
  peakBar: { id: 'mlxDistributed.peakBar', defaultMessage: 'Peak memory against the budget' },
  active: { id: 'mlxDistributed.active', defaultMessage: 'active {gb} GiB' },
  planned: { id: 'mlxDistributed.planned', defaultMessage: 'planned {gb} GiB' },
  available: {
    id: 'mlxDistributed.available',
    defaultMessage: '{available} of {total} GiB available',
  },
  limits: {
    id: 'mlxDistributed.limits',
    defaultMessage: 'Caps: memory {memory} · wired {wired} · cache {cache} GiB',
  },
  limitsNone: { id: 'mlxDistributed.limitsNone', defaultMessage: 'Caps not reported yet' },
  pressureNormal: { id: 'mlxDistributed.pressure.normal', defaultMessage: 'Pressure normal' },
  pressureWarn: { id: 'mlxDistributed.pressure.warn', defaultMessage: 'Pressure warn' },
  pressureCritical: {
    id: 'mlxDistributed.pressure.critical',
    defaultMessage: 'Pressure critical',
  },
  memoryUnread: { id: 'mlxDistributed.memoryUnread', defaultMessage: 'Memory unread: {error}' },
  speedUnknown: { id: 'mlxDistributed.speedUnknown', defaultMessage: 'speed not reported' },
  preflightTitle: { id: 'mlxDistributed.preflightTitle', defaultMessage: 'Preflight' },
  preflightNone: {
    id: 'mlxDistributed.preflightNone',
    defaultMessage:
      'No preflight has run yet. A dry run checks every node (reachability, memory, the model, Python, the Thunderbolt link, ports) and plans the layers per node without launching anything.',
  },
  passed: { id: 'mlxDistributed.passed', defaultMessage: 'Passed' },
  failed: { id: 'mlxDistributed.failed', defaultMessage: 'Failed' },
  ranAt: { id: 'mlxDistributed.ranAt', defaultMessage: 'ran {time}' },
  contextLine: {
    id: 'mlxDistributed.contextLine',
    defaultMessage: 'context {limit} ({source}) · largest that fits {max}',
  },
  sourceRequested: { id: 'mlxDistributed.source.requested', defaultMessage: 'requested' },
  sourceDerived: { id: 'mlxDistributed.source.derived', defaultMessage: 'derived' },
  clusterChecks: { id: 'mlxDistributed.clusterChecks', defaultMessage: 'Cluster checks' },
  failing: { id: 'mlxDistributed.failing', defaultMessage: 'Failing checks' },
  pass: { id: 'mlxDistributed.verdict.pass', defaultMessage: 'pass' },
  warn: { id: 'mlxDistributed.verdict.warn', defaultMessage: 'warn' },
  fail: { id: 'mlxDistributed.verdict.fail', defaultMessage: 'fail' },
  fits: { id: 'mlxDistributed.fits', defaultMessage: 'fits' },
  noFit: { id: 'mlxDistributed.noFit', defaultMessage: 'does not fit' },
  planLine: {
    id: 'mlxDistributed.planLine',
    defaultMessage: 'GiB planned with overhead, of {budget} GiB budget',
  },
  planBreakdown: {
    id: 'mlxDistributed.planBreakdown',
    defaultMessage:
      'weights {weights} · state {state} · workspace {workspace} · prompt cache {cache} GiB',
  },
  planBar: { id: 'mlxDistributed.planBar', defaultMessage: 'Planned memory against the budget' },
  noPlan: { id: 'mlxDistributed.noPlan', defaultMessage: 'No plan for this rank' },
  repairs: { id: 'mlxDistributed.repairs', defaultMessage: 'Link repairs' },
  configTitle: { id: 'mlxDistributed.configTitle', defaultMessage: 'Configuration' },
  configNone: {
    id: 'mlxDistributed.configNone',
    defaultMessage: 'No distributed configuration is saved yet.',
  },
  setUp: { id: 'mlxDistributed.setUp', defaultMessage: 'Set up' },
  backend: { id: 'mlxDistributed.field.backend', defaultMessage: 'Backend' },
  backendJaccl: {
    id: 'mlxDistributed.backend.jaccl',
    defaultMessage: 'JACCL · RDMA over Thunderbolt 5',
  },
  backendRing: {
    id: 'mlxDistributed.backend.ring',
    defaultMessage: 'ring · TCP over the Thunderbolt link',
  },
  backendPick: { id: 'mlxDistributed.backendPick', defaultMessage: 'Pick a backend' },
  modelId: { id: 'mlxDistributed.field.modelId', defaultMessage: 'Model' },
  modelPick: { id: 'mlxDistributed.modelPick', defaultMessage: 'Pick a model' },
  modelNotLocal: {
    id: 'mlxDistributed.modelNotLocal',
    defaultMessage: 'not in this Mac’s models folder',
  },
  context: { id: 'mlxDistributed.field.context', defaultMessage: 'Context (tokens)' },
  contextDerived: {
    id: 'mlxDistributed.contextDerived',
    defaultMessage: 'derived from memory',
  },
  port: { id: 'mlxDistributed.field.port', defaultMessage: 'API port' },
  coordinatorPort: {
    id: 'mlxDistributed.field.coordinatorPort',
    defaultMessage: 'Coordinator port',
  },
  restart: {
    id: 'mlxDistributed.field.restart',
    defaultMessage: 'Restart after a rank dies or hangs',
  },
  nodesField: { id: 'mlxDistributed.field.nodes', defaultMessage: 'at least 2 nodes' },
  thisMac: { id: 'mlxDistributed.thisMac', defaultMessage: 'this Mac' },
  addNode: { id: 'mlxDistributed.addNode', defaultMessage: 'Add node' },
  editNode: { id: 'mlxDistributed.editNode', defaultMessage: 'Edit {name}' },
  removeNode: { id: 'mlxDistributed.removeNode', defaultMessage: 'Remove {name}' },
  save: { id: 'mlxDistributed.save', defaultMessage: 'Save configuration' },
  revert: { id: 'mlxDistributed.revert', defaultMessage: 'Revert' },
  missing: { id: 'mlxDistributed.missing', defaultMessage: 'Still empty: {fields}' },
  missingNode: { id: 'mlxDistributed.missingNode', defaultMessage: '{node}: {field}' },
  unnamedNode: { id: 'mlxDistributed.unnamedNode', defaultMessage: 'rank {rank}' },
  nodeDialogAdd: { id: 'mlxDistributed.nodeDialog.add', defaultMessage: 'Add node' },
  nodeDialogEdit: { id: 'mlxDistributed.nodeDialog.edit', defaultMessage: 'Edit node' },
  nodeDialogBody: {
    id: 'mlxDistributed.nodeDialog.body',
    defaultMessage:
      'Rank {rank}. Rank 0 is this Mac; every other node is reached over its ssh alias.',
  },
  apply: { id: 'mlxDistributed.apply', defaultMessage: 'Apply' },
  cancel: { id: 'mlxDistributed.cancel', defaultMessage: 'Cancel' },
  name: { id: 'mlxDistributed.field.name', defaultMessage: 'Name' },
  ssh: { id: 'mlxDistributed.field.ssh', defaultMessage: 'ssh alias' },
  tbIp: { id: 'mlxDistributed.field.tbIp', defaultMessage: 'Thunderbolt IPv4' },
  tbNetmask: { id: 'mlxDistributed.field.tbNetmask', defaultMessage: 'Netmask' },
  tbInterface: {
    id: 'mlxDistributed.field.tbInterface',
    defaultMessage: 'Thunderbolt interface',
  },
  tbService: { id: 'mlxDistributed.field.tbService', defaultMessage: 'Network service' },
  rdmaDevice: { id: 'mlxDistributed.field.rdmaDevice', defaultMessage: 'RDMA device' },
  python: { id: 'mlxDistributed.field.python', defaultMessage: 'Python (mlx + mlx_lm)' },
  pipelinePython: {
    id: 'mlxDistributed.field.pipelinePython',
    defaultMessage: 'Pipeline Python (optional)',
  },
  modelDir: { id: 'mlxDistributed.field.modelDir', defaultMessage: 'Model folder on this node' },
  startRefused: { id: 'mlxDistributed.startRefused', defaultMessage: 'Start refused' },
  preflightError: { id: 'mlxDistributed.error.preflight', defaultMessage: 'Preflight error' },
  startError: { id: 'mlxDistributed.error.start', defaultMessage: 'Start error' },
  stopError: { id: 'mlxDistributed.error.stop', defaultMessage: 'Stop error' },
  saveError: { id: 'mlxDistributed.error.save', defaultMessage: 'Save error' },
  unmountError: { id: 'mlxDistributed.error.unmount', defaultMessage: 'Unmount error' },
  unmountTitle: {
    id: 'mlxDistributed.unmountTitle',
    defaultMessage: 'Unmount the single engine?',
  },
  unmountMessage: {
    id: 'mlxDistributed.unmountMessage',
    defaultMessage:
      'The single MLX engine is mounted on this Mac ({detail}). One engine owns a Mac at a time: unmounting stops that model so the distributed engine can start.',
  },
  unmountConfirm: {
    id: 'mlxDistributed.unmountConfirm',
    defaultMessage: 'Unmount and continue',
  },
  unmountCancel: {
    id: 'mlxDistributed.unmountCancel',
    defaultMessage: 'Keep the single engine',
  },
  stopTitle: { id: 'mlxDistributed.stopTitle', defaultMessage: 'Stop the distributed engine?' },
  stopMessage: {
    id: 'mlxDistributed.stopMessage',
    defaultMessage:
      'Every rank on {nodes} is stopped and verified gone, pid by pid. Requests in flight are cut off.',
  },
  stopCancel: { id: 'mlxDistributed.stopCancel', defaultMessage: 'Keep running' },
  stopVerified: { id: 'mlxDistributed.stopVerified', defaultMessage: 'Stopped, verified' },
  stopUnverified: { id: 'mlxDistributed.stopUnverified', defaultMessage: 'Stop not verified' },
  eventsTitle: { id: 'mlxDistributed.eventsTitle', defaultMessage: 'Supervisor events' },
  eventsNone: { id: 'mlxDistributed.eventsNone', defaultMessage: 'No events yet.' },
});

const EVENT_WORDS = defineMessages({
  preflight: { id: 'mlxDistributed.event.preflight', defaultMessage: 'Preflight' },
  linkRepaired: { id: 'mlxDistributed.event.linkRepaired', defaultMessage: 'Link repaired' },
  launched: { id: 'mlxDistributed.event.launched', defaultMessage: 'Launched' },
  ready: { id: 'mlxDistributed.event.ready', defaultMessage: 'Ready' },
  startFailed: { id: 'mlxDistributed.event.startFailed', defaultMessage: 'Start failed' },
  rankDied: { id: 'mlxDistributed.event.rankDied', defaultMessage: 'Rank died' },
  rankFrozen: { id: 'mlxDistributed.event.rankFrozen', defaultMessage: 'Rank frozen' },
  hang: { id: 'mlxDistributed.event.hang', defaultMessage: 'Hang detected' },
  streamWithoutDone: {
    id: 'mlxDistributed.event.streamWithoutDone',
    defaultMessage: 'Stream without [DONE]',
  },
  restart: { id: 'mlxDistributed.event.restart', defaultMessage: 'Restart' },
  breakerOpen: { id: 'mlxDistributed.event.breakerOpen', defaultMessage: 'Restart breaker open' },
  watchdogWarn: { id: 'mlxDistributed.event.watchdogWarn', defaultMessage: 'Memory warning' },
  watchdogCritical: {
    id: 'mlxDistributed.event.watchdogCritical',
    defaultMessage: 'Memory critical',
  },
  watchdogBlind: {
    id: 'mlxDistributed.event.watchdogBlind',
    defaultMessage: 'Memory unreadable',
  },
  admissionClosed: {
    id: 'mlxDistributed.event.admissionClosed',
    defaultMessage: 'Admission closed',
  },
  admissionOpened: {
    id: 'mlxDistributed.event.admissionOpened',
    defaultMessage: 'Admission opened',
  },
  stopRequested: { id: 'mlxDistributed.event.stopRequested', defaultMessage: 'Stop requested' },
  stopped: { id: 'mlxDistributed.event.stopped', defaultMessage: 'Stopped' },
  orphanReclaimed: {
    id: 'mlxDistributed.event.orphanReclaimed',
    defaultMessage: 'Orphan reclaimed',
  },
});

const FIELD_LABEL: Record<MissingField['field'], MessageDescriptor> = {
  modelId: i18n.modelId,
  backend: i18n.backend,
  port: i18n.port,
  coordinatorPort: i18n.coordinatorPort,
  nodes: i18n.nodesField,
  name: i18n.name,
  ssh: i18n.ssh,
  tbIp: i18n.tbIp,
  tbNetmask: i18n.tbNetmask,
  tbInterface: i18n.tbInterface,
  tbService: i18n.tbService,
  rdmaDevice: i18n.rdmaDevice,
  python: i18n.python,
  pipelinePython: i18n.pipelinePython,
  modelDir: i18n.modelDir,
};

const NODE_FIELDS: readonly NodeTextField[] = [
  'name',
  'ssh',
  'tbIp',
  'tbNetmask',
  'tbInterface',
  'tbService',
  'rdmaDevice',
  'python',
  'pipelinePython',
  'modelDir',
];

const META = cx(TYPE.meta, TNUM);
const BIG = cx('text-[28px] leading-none tracking-tight', WEIGHT.semibold, TNUM);

function Absent() {
  return <span className="text-lz-ink-4">—</span>;
}

function eventWord(intl: IntlShape, kind: string): string {
  const message = (EVENT_WORDS as Record<string, MessageDescriptor | undefined>)[kind];
  return message ? intl.formatMessage(message) : kind;
}

function verdictWord(intl: IntlShape, verdict: string): string {
  if (verdict === 'pass') return intl.formatMessage(i18n.pass);
  if (verdict === 'warn') return intl.formatMessage(i18n.warn);
  if (verdict === 'fail') return intl.formatMessage(i18n.fail);
  return verdict;
}

function pressureWord(intl: IntlShape, pressure: string): string {
  if (pressure === 'normal') return intl.formatMessage(i18n.pressureNormal);
  if (pressure === 'warn') return intl.formatMessage(i18n.pressureWarn);
  if (pressure === 'critical') return intl.formatMessage(i18n.pressureCritical);
  return pressure;
}

function spanText(intl: IntlShape, span: LayerSpan | null): string {
  if (!span) return intl.formatMessage(i18n.noLayers);
  return span.kind === 'layers'
    ? intl.formatMessage(i18n.layers, { first: span.first, last: span.last, count: span.count })
    : intl.formatMessage(i18n.shard, { index: span.index, count: span.count });
}

function durationText(ms: number): string {
  return ms < 1000 ? `${ms} ms` : formatElapsed(ms / 1000);
}

/** A solid fill on a surface-2 track — the page's usage-bar register. `over` turns it red. */
function Track({ fraction, tone, label }: { fraction: number; tone: Tone; label: string }) {
  const pct = Math.round(Math.min(1, Math.max(0, fraction)) * 100);
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={pct}
      className={cx('h-2 w-full overflow-hidden', RADIUS.pill, SURFACE.inset)}
    >
      <div className={cx('h-full', TONE_DOT[tone])} style={{ width: `${pct}%` }} />
    </div>
  );
}

/** Peak (or planned) memory against the budget: under 85% is the accent, above warn, over err. */
function budgetTone(fraction: number): Tone {
  if (fraction > 1) return 'err';
  if (fraction > 0.85) return 'warn';
  return 'accent';
}

// ---------------------------------------------------------------------------
// Checks and the preflight report
// ---------------------------------------------------------------------------

function CheckRow({ check, node }: { check: MlxDistributedCheck; node?: string }) {
  const intl = useIntl();
  return (
    <li
      data-testid="mlx-dist-check"
      data-verdict={check.verdict}
      data-check={check.id}
      className="flex min-w-0 items-start gap-2"
    >
      <Chip tone={verdictTone(check.verdict)}>{verdictWord(intl, check.verdict)}</Chip>
      <span className={cx('shrink-0', TYPE.mono)}>
        {node ? `${node} · ${check.id}` : check.id}
      </span>
      <span className={cx('min-w-0 break-words', TYPE.body, TNUM)}>{check.message}</span>
    </li>
  );
}

function PlanBlock({ node }: { node: MlxDistributedNodePreflight }) {
  const intl = useIntl();
  const plan = node.plan;
  if (!plan) {
    return <p className={cx(TYPE.body, WEIGHT.semibold, TONE_TEXT.err)}>{intl.formatMessage(i18n.noPlan)}</p>;
  }
  const fraction = plan.budgetBytes > 0 ? plan.withOverheadBytes / plan.budgetBytes : 1;
  return (
    <div data-testid="mlx-dist-plan" className="flex flex-col gap-1.5">
      <div className="flex flex-wrap items-baseline gap-2">
        <span className={cx(TYPE.body, WEIGHT.semibold)}>{spanText(intl, layerSpan(plan))}</span>
        <Chip tone={plan.fits ? 'ok' : 'err'}>
          {intl.formatMessage(plan.fits ? i18n.fits : i18n.noFit)}
        </Chip>
      </div>
      <div className="flex items-baseline gap-2">
        <span data-testid="mlx-dist-plan-planned" className={BIG}>
          {gb1(gib(plan.withOverheadBytes))}
        </span>
        <span className={cx(TYPE.body, TNUM)}>
          {intl.formatMessage(i18n.planLine, { budget: gb1(gib(plan.budgetBytes)) })}
        </span>
      </div>
      <Track
        fraction={fraction}
        tone={plan.fits ? budgetTone(fraction) : 'err'}
        label={intl.formatMessage(i18n.planBar)}
      />
      <span className={META}>
        {intl.formatMessage(i18n.planBreakdown, {
          weights: gb1(gib(plan.weightsBytes)),
          state: gb1(gib(plan.stateBytes)),
          workspace: gb1(gib(plan.workspaceBytes)),
          cache: gb1(gib(plan.promptCacheBytes)),
        })}
      </span>
    </div>
  );
}

function PreflightNodeCard({ node }: { node: MlxDistributedNodePreflight }) {
  const intl = useIntl();
  const tone = pressureTone(node.pressure);
  return (
    <div
      data-testid="mlx-dist-preflight-node"
      data-node={node.name}
      className={cx('flex min-w-0 flex-col gap-3 p-4', SURFACE.card)}
    >
      <div className="flex flex-wrap items-center gap-2">
        <span className={cx(TYPE.h2)}>{node.name}</span>
        <Chip>{intl.formatMessage(i18n.rank, { rank: node.rank })}</Chip>
        {node.host && <span className={TYPE.mono}>{node.host}</span>}
        {node.pressure && (
          <Chip tone={tone ?? undefined}>{pressureWord(intl, node.pressure)}</Chip>
        )}
      </div>
      <div className={cx('flex flex-wrap gap-x-3 gap-y-1', META)}>
        {node.availableBytes != null && node.totalBytes != null && (
          <span>
            {intl.formatMessage(i18n.available, {
              available: gb1(gib(node.availableBytes)),
              total: gb1(gib(node.totalBytes)),
            })}
          </span>
        )}
        <span>{node.linkSpeed ?? intl.formatMessage(i18n.speedUnknown)}</span>
        {node.mlxVersion && <span>{node.mlxVersion}</span>}
      </div>
      <PlanBlock node={node} />
      {node.checks.length > 0 && (
        <ul className="flex flex-col gap-1.5">
          {node.checks.map((c) => (
            <CheckRow key={`${c.id}|${c.message}`} check={c} />
          ))}
        </ul>
      )}
    </div>
  );
}

function PreflightReportView({ report }: { report: MlxDistributedPreflight }) {
  const intl = useIntl();
  const failing = [
    ...report.checks.filter((c) => c.verdict === 'fail').map((c) => ({ c, node: undefined })),
    ...report.nodes.flatMap((n) =>
      n.checks.filter((c) => c.verdict === 'fail').map((c) => ({ c, node: n.name }))
    ),
  ];
  const source =
    report.contextSource === 'requested'
      ? intl.formatMessage(i18n.sourceRequested)
      : report.contextSource === 'derived'
        ? intl.formatMessage(i18n.sourceDerived)
        : (report.contextSource ?? '—');
  return (
    <div data-testid="mlx-dist-preflight" data-ok={report.ok} className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center gap-2">
        <Chip tone={report.ok ? 'ok' : 'err'}>
          {intl.formatMessage(report.ok ? i18n.passed : i18n.failed)}
        </Chip>
        <span className={META}>
          {intl.formatMessage(i18n.ranAt, {
            time: intl.formatTime(report.ranAtMs, {
              hour: '2-digit',
              minute: '2-digit',
              second: '2-digit',
            }),
          })}
        </span>
        {backendName(report.backend) && <Chip>{backendName(report.backend)}</Chip>}
        {report.runner && <Chip>{report.runner}</Chip>}
        {report.modelType && <Chip>{report.modelType}</Chip>}
        {report.contextLimit != null && (
          <span className={META}>
            {intl.formatMessage(i18n.contextLine, {
              limit: intl.formatNumber(report.contextLimit),
              source,
              max:
                report.maxContextFits != null ? intl.formatNumber(report.maxContextFits) : '—',
            })}
          </span>
        )}
      </div>
      {failing.length > 0 && (
        <div data-testid="mlx-dist-failing" className="flex flex-col gap-2">
          <span className={cx('text-lz-meta', WEIGHT.semibold, TONE_TEXT.err)}>
            {intl.formatMessage(i18n.failing)}
          </span>
          <ul className="flex flex-col gap-1.5">
            {failing.map(({ c, node }) => (
              <CheckRow key={`${node ?? ''}|${c.id}|${c.message}`} check={c} node={node} />
            ))}
          </ul>
        </div>
      )}
      <div className="grid grid-cols-1 gap-3 xl:grid-cols-2">
        {report.nodes.map((n) => (
          <PreflightNodeCard key={`${n.rank}|${n.name}`} node={n} />
        ))}
      </div>
      {report.checks.length > 0 && (
        <div className="flex flex-col gap-2">
          <span className={TYPE.meta}>{intl.formatMessage(i18n.clusterChecks)}</span>
          <ul className="flex flex-col gap-1.5">
            {report.checks.map((c) => (
              <CheckRow key={`${c.id}|${c.message}`} check={c} />
            ))}
          </ul>
        </div>
      )}
      {report.repairs.length > 0 && (
        <div className="flex flex-col gap-2">
          <span className={TYPE.meta}>{intl.formatMessage(i18n.repairs)}</span>
          <ul className="flex flex-col gap-1">
            {report.repairs.map((r) => (
              <li key={r} className={cx('break-words', TYPE.body)}>
                {r}
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// The running engine: admission, liveness, the per-node strip
// ---------------------------------------------------------------------------

function NodeCard({
  node,
  budgetGb,
}: {
  node: MlxDistributedNodeStatus;
  budgetGb: number | null;
}) {
  const intl = useIntl();
  const tone = nodeStateTone(node.state);
  const peak = node.peakMemoryGb ?? null;
  const fraction = peak != null && budgetGb != null && budgetGb > 0 ? peak / budgetGb : null;
  const pTone = pressureTone(node.pressure);
  const limits =
    node.memoryLimitGb != null || node.wiredLimitGb != null || node.cacheLimitGb != null
      ? intl.formatMessage(i18n.limits, {
          memory: node.memoryLimitGb != null ? gb1(node.memoryLimitGb) : '—',
          wired: node.wiredLimitGb != null ? gb1(node.wiredLimitGb) : '—',
          cache: node.cacheLimitGb != null ? gb1(node.cacheLimitGb) : '—',
        })
      : intl.formatMessage(i18n.limitsNone);
  const link = [
    backendName(node.link.backend),
    node.link.tbIp,
    node.link.interface,
    node.link.speed ?? intl.formatMessage(i18n.speedUnknown),
  ]
    .filter(Boolean)
    .join(' · ');
  return (
    <div
      data-testid="mlx-dist-node"
      data-node={node.name}
      data-state={node.state}
      className={cx('flex min-w-0 flex-col gap-3 p-4', SURFACE.card)}
    >
      <div className="flex flex-wrap items-center gap-2">
        <StatusDot tone={tone} live={runStateInFlight(node.state) || node.state === 'loading'} label={node.state} />
        <span className={TYPE.h2}>{node.name}</span>
        <Chip tone={tone}>{distributedStateWord(intl, node.state)}</Chip>
        <Chip>
          {intl.formatMessage(node.role === 'coordinator' ? i18n.coordinator : i18n.worker)} ·{' '}
          {intl.formatMessage(i18n.rank, { rank: node.rank })}
        </Chip>
        {node.pid != null && (
          <span className={TYPE.mono}>{intl.formatMessage(i18n.pid, { pid: node.pid })}</span>
        )}
      </div>
      <span data-testid="mlx-dist-node-layers" className={cx(TYPE.body, WEIGHT.semibold)}>
        {spanText(intl, layerSpan(node))}
      </span>
      {node.memoryError && (
        <p className={cx('break-words', TYPE.body, WEIGHT.semibold, TONE_TEXT.err)}>
          {intl.formatMessage(i18n.memoryUnread, { error: node.memoryError })}
        </p>
      )}
      {peak != null ? (
        <div className="flex flex-col gap-1.5">
          <div className="flex items-baseline gap-2">
            <span
              data-testid="mlx-dist-node-peak"
              className={cx(BIG, fraction != null && fraction > 1 ? TONE_TEXT.err : 'text-lz-ink')}
            >
              {gb1(peak)}
            </span>
            <span data-testid="mlx-dist-node-budget" className={cx(TYPE.body, TNUM)}>
              {budgetGb != null
                ? intl.formatMessage(i18n.peakOf, { budget: gb1(budgetGb) })
                : intl.formatMessage(i18n.peakNoBudget)}
            </span>
          </div>
          {fraction != null && (
            <Track
              fraction={fraction}
              tone={budgetTone(fraction)}
              label={intl.formatMessage(i18n.peakBar)}
            />
          )}
        </div>
      ) : (
        <span className={META}>{intl.formatMessage(i18n.noPeak)}</span>
      )}
      <div className={cx('flex flex-wrap gap-x-3 gap-y-1', META)}>
        {node.activeMemoryGb != null && (
          <span>{intl.formatMessage(i18n.active, { gb: gb1(node.activeMemoryGb) })}</span>
        )}
        {node.plannedMemoryGb != null && (
          <span>{intl.formatMessage(i18n.planned, { gb: gb1(node.plannedMemoryGb) })}</span>
        )}
        {node.availableMemoryGb != null && node.totalMemoryGb != null && (
          <span>
            {intl.formatMessage(i18n.available, {
              available: gb1(node.availableMemoryGb),
              total: gb1(node.totalMemoryGb),
            })}
          </span>
        )}
      </div>
      <div className="flex flex-wrap items-center gap-2">
        {node.pressure && (
          <Chip tone={pTone ?? undefined}>{pressureWord(intl, node.pressure)}</Chip>
        )}
        <span className={META}>{limits}</span>
      </div>
      <span data-testid="mlx-dist-node-link" className={cx('break-words', TYPE.mono)}>
        {link}
      </span>
    </div>
  );
}

function RunFacts({ status }: { status: MlxDistributedStatus }) {
  const intl = useIntl();
  const live = status.liveness ?? null;
  const hangFraction =
    live?.boundMs != null && live.boundMs > 0 ? live.silentMs / live.boundMs : null;
  return (
    <div className="grid grid-cols-1 gap-3 md:grid-cols-3">
      <div
        data-testid="mlx-dist-admission"
        data-open={status.admissionOpen}
        className={cx(
          'flex flex-col gap-1 p-4',
          RADIUS.card,
          status.admissionOpen ? SURFACE.card : TONE_FILL.warn
        )}
      >
        <span className={cx('text-lz-h2', WEIGHT.semibold)}>
          {intl.formatMessage(status.admissionOpen ? i18n.admissionOpen : i18n.admissionClosed)}
        </span>
        {!status.admissionOpen && (
          <span className="text-lz-body">{intl.formatMessage(i18n.admissionClosedWhy)}</span>
        )}
      </div>
      <div className={cx('flex flex-col gap-1 p-4', SURFACE.card)}>
        <span className={TYPE.meta}>{intl.formatMessage(i18n.inflight)}</span>
        {status.inflight != null ? (
          <span data-testid="mlx-dist-inflight" className={BIG}>
            {status.inflight}
          </span>
        ) : (
          <span data-testid="mlx-dist-inflight-unknown" className={cx(TYPE.body, TONE_TEXT.warn)}>
            {intl.formatMessage(i18n.notMeasured)}
          </span>
        )}
      </div>
      <div data-testid="mlx-dist-liveness" className={cx('flex flex-col gap-1.5 p-4', SURFACE.card)}>
        <span className={TYPE.meta}>{intl.formatMessage(i18n.liveness)}</span>
        {live == null || live.samples === 0 ? (
          <span className={TYPE.body}>{intl.formatMessage(i18n.livenessNone)}</span>
        ) : (
          <>
            <span className={cx(TYPE.body, TNUM)}>
              {live.boundMs != null && live.medianMs != null
                ? intl.formatMessage(i18n.livenessLine, {
                    samples: live.samples,
                    median: durationText(live.medianMs),
                    bound: durationText(live.boundMs),
                    silent: durationText(live.silentMs),
                  })
                : intl.formatMessage(i18n.livenessNoBound, {
                    samples: live.samples,
                    silent: durationText(live.silentMs),
                  })}
            </span>
            {hangFraction != null && (
              <Track
                fraction={hangFraction}
                tone={hangFraction >= 1 ? 'err' : hangFraction > 0.5 ? 'warn' : 'ok'}
                label={intl.formatMessage(i18n.livenessBar)}
              />
            )}
          </>
        )}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

function EventsPanel({ events }: { events: readonly MlxDistributedEvent[] }) {
  const intl = useIntl();
  const keys = eventKeys(events);
  // Newest first; the keys are computed on the backend's order so they stay stable as it grows.
  const rows = events.map((e, i) => ({ e, key: keys[i] })).reverse();
  return (
    <Panel title={intl.formatMessage(i18n.eventsTitle)} count={events.length}>
      {rows.length === 0 ? (
        <p className={TYPE.bodyMuted}>{intl.formatMessage(i18n.eventsNone)}</p>
      ) : (
        <ul data-testid="mlx-dist-events" className="flex max-h-80 flex-col gap-2 overflow-y-auto">
          {rows.map(({ e, key }) => (
            <li
              key={key}
              data-testid="mlx-dist-event"
              data-kind={e.kind}
              className="flex min-w-0 items-start gap-2"
            >
              <span className={cx('shrink-0', META)}>
                {intl.formatTime(e.atMs, { hour: '2-digit', minute: '2-digit', second: '2-digit' })}
              </span>
              <Chip tone={eventTone(e.kind)}>{eventWord(intl, e.kind)}</Chip>
              {e.node && <span className={cx('shrink-0', TYPE.mono)}>{e.node}</span>}
              <span className={cx('min-w-0 break-words', TYPE.body, TNUM)}>{e.message}</span>
            </li>
          ))}
        </ul>
      )}
    </Panel>
  );
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

function NodeDialog({
  open,
  rank,
  initial,
  onApply,
  onClose,
}: {
  open: boolean;
  rank: number;
  initial: MlxDistributedNodeConfig;
  onApply: (node: MlxDistributedNodeConfig) => void;
  onClose: () => void;
}) {
  const intl = useIntl();
  const [node, setNode] = useState(initial);
  useEffect(() => {
    if (open) setNode(initial);
  }, [open, initial]);
  const fields = NODE_FIELDS.filter((f) => !(f === 'ssh' && rank === 0));
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-[560px]">
        <DialogHeader>
          <DialogTitle>
            {intl.formatMessage(initial.name ? i18n.nodeDialogEdit : i18n.nodeDialogAdd)}
          </DialogTitle>
          <DialogDescription>{intl.formatMessage(i18n.nodeDialogBody, { rank })}</DialogDescription>
        </DialogHeader>
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          {fields.map((field) => (
            <label key={field} className="flex min-w-0 flex-col gap-1">
              <span className={TYPE.meta}>{intl.formatMessage(FIELD_LABEL[field])}</span>
              <input
                value={node[field] ?? ''}
                onChange={(e) => setNode((n) => ({ ...n, [field]: e.target.value }))}
                className={cx(INPUT, 'w-full font-mono text-lz-mono')}
                aria-label={intl.formatMessage(FIELD_LABEL[field])}
                autoComplete="off"
                spellCheck={false}
              />
            </label>
          ))}
        </div>
        <DialogFooter>
          <Button variant="secondary" onClick={onClose}>
            {intl.formatMessage(i18n.cancel)}
          </Button>
          <Button variant="primary" onClick={() => onApply(node)}>
            {intl.formatMessage(i18n.apply)}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

interface BackendOption extends StudioSelectOption {
  value: 'jaccl' | 'ring';
}

interface ModelOption extends StudioSelectOption {
  local: boolean;
}

function NumberInput({
  value,
  onChange,
  label,
  placeholder,
  disabled,
}: {
  value: number | null | undefined;
  onChange: (n: number | null) => void;
  label: string;
  placeholder?: string;
  disabled: boolean;
}) {
  return (
    <input
      inputMode="numeric"
      value={value ? String(value) : ''}
      placeholder={placeholder}
      disabled={disabled}
      onChange={(e) => {
        const text = e.target.value.replace(/[^0-9]/g, '');
        onChange(text === '' ? null : Number(text));
      }}
      className={cx(INPUT, 'w-full', TNUM)}
      aria-label={label}
      autoComplete="off"
    />
  );
}

function ConfigEditor({
  config,
  models,
  locked,
  onChange,
}: {
  config: MlxDistributedConfig;
  models: MlxLocalModel[];
  locked: boolean;
  onChange: (next: MlxDistributedConfig) => void;
}) {
  const intl = useIntl();
  const [editing, setEditing] = useState<number | null>(null);
  const backendOptions: BackendOption[] = [
    { value: 'jaccl', label: intl.formatMessage(i18n.backendJaccl) },
    { value: 'ring', label: intl.formatMessage(i18n.backendRing) },
  ];
  const modelOptions = useMemo<ModelOption[]>(() => {
    const local = models.filter((m) => m.complete).map((m) => ({ value: m.id, label: m.id, local: true }));
    if (config.modelId && !local.some((o) => o.value === config.modelId)) {
      return [{ value: config.modelId, label: config.modelId, local: false }, ...local];
    }
    return local;
  }, [models, config.modelId]);

  const editingNode =
    editing == null ? null : editing < config.nodes.length ? config.nodes[editing] : emptyNode();
  const applyNode = (node: MlxDistributedNodeConfig) => {
    if (editing == null) return;
    const nodes = [...config.nodes];
    nodes[editing] = node;
    onChange({ ...config, nodes });
    setEditing(null);
  };

  return (
    <div data-testid="mlx-dist-config" className="flex flex-col gap-4">
      {locked && <p className={TYPE.bodyMuted}>{intl.formatMessage(i18n.locked)}</p>}
      <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
        <label className="flex min-w-0 flex-col gap-1">
          <span className={TYPE.meta}>{intl.formatMessage(i18n.backend)}</span>
          <StudioSelect<BackendOption>
            aria-label={intl.formatMessage(i18n.backend)}
            options={backendOptions}
            value={backendOptions.find((o) => o.value === config.backend) ?? null}
            onChange={(o) => o && onChange({ ...config, backend: o.value })}
            placeholder={intl.formatMessage(i18n.backendPick)}
            disabled={locked}
            optionTestId={(o) => `mlx-dist-backend-${o.value}`}
          />
        </label>
        <label className="flex min-w-0 flex-col gap-1">
          <span className={TYPE.meta}>{intl.formatMessage(i18n.modelId)}</span>
          <StudioSelect<ModelOption>
            aria-label={intl.formatMessage(i18n.modelId)}
            options={modelOptions}
            value={modelOptions.find((o) => o.value === config.modelId) ?? null}
            onChange={(o) => o && onChange(withModel(config, o.value))}
            placeholder={intl.formatMessage(i18n.modelPick)}
            disabled={locked}
            renderOption={(o) => (
              <span className="flex min-w-0 items-center gap-2">
                <span className="min-w-0 truncate font-mono text-lz-mono">{o.label}</span>
                {!o.local && <Chip tone="warn">{intl.formatMessage(i18n.modelNotLocal)}</Chip>}
              </span>
            )}
            optionTestId={(o) => `mlx-dist-model-${o.value}`}
          />
        </label>
        <label className="flex min-w-0 flex-col gap-1">
          <span className={TYPE.meta}>{intl.formatMessage(i18n.context)}</span>
          <NumberInput
            value={config.context}
            onChange={(n) => onChange({ ...config, context: n })}
            label={intl.formatMessage(i18n.context)}
            placeholder={intl.formatMessage(i18n.contextDerived)}
            disabled={locked}
          />
        </label>
        <div className="grid grid-cols-2 gap-3">
          <label className="flex min-w-0 flex-col gap-1">
            <span className={TYPE.meta}>{intl.formatMessage(i18n.port)}</span>
            <NumberInput
              value={config.port}
              onChange={(n) => onChange({ ...config, port: n ?? 0 })}
              label={intl.formatMessage(i18n.port)}
              disabled={locked}
            />
          </label>
          <label className="flex min-w-0 flex-col gap-1">
            <span className={TYPE.meta}>{intl.formatMessage(i18n.coordinatorPort)}</span>
            <NumberInput
              value={config.coordinatorPort}
              onChange={(n) => onChange({ ...config, coordinatorPort: n ?? 0 })}
              label={intl.formatMessage(i18n.coordinatorPort)}
              disabled={locked}
            />
          </label>
        </div>
      </div>
      <div className="flex items-center gap-3">
        <StudioSwitch
          checked={config.restartOnFailure ?? false}
          onChange={(v) => onChange({ ...config, restartOnFailure: v })}
          aria-label={intl.formatMessage(i18n.restart)}
          disabled={locked}
        />
        <span className={TYPE.body}>{intl.formatMessage(i18n.restart)}</span>
      </div>
      <div className="flex flex-col gap-2">
        <span className={TYPE.meta}>{intl.formatMessage(i18n.nodes)}</span>
        <ul className="flex flex-col gap-2">
          {config.nodes.map((node, rank) => {
            const label = node.name || intl.formatMessage(i18n.unnamedNode, { rank });
            return (
              <li
                // Rank IS the node's identity here: the order is the rank assignment itself.
                key={rank}
                data-testid="mlx-dist-config-node"
                className={cx('flex min-w-0 flex-wrap items-center gap-2 px-3 py-2', SURFACE.card)}
              >
                <Chip>{intl.formatMessage(i18n.rank, { rank })}</Chip>
                <span className={cx(TYPE.body, WEIGHT.semibold)}>{label}</span>
                <span className={TYPE.mono}>
                  {rank === 0 ? intl.formatMessage(i18n.thisMac) : node.ssh || '—'}
                </span>
                <span className={cx('min-w-0 truncate', TYPE.mono)}>
                  {[node.tbIp, node.tbInterface, node.rdmaDevice].filter(Boolean).join(' · ') ||
                    '—'}
                </span>
                <span className={cx('min-w-0 flex-1 truncate', TYPE.mono)} title={node.modelDir}>
                  {node.modelDir || '—'}
                </span>
                <Button
                  size="sm"
                  variant="ghost"
                  iconOnly
                  icon={<Pencil />}
                  aria-label={intl.formatMessage(i18n.editNode, { name: label })}
                  onClick={() => setEditing(rank)}
                  disabled={locked}
                />
                {rank > 0 && (
                  <Button
                    size="sm"
                    variant="ghost"
                    iconOnly
                    icon={<Trash2 />}
                    aria-label={intl.formatMessage(i18n.removeNode, { name: label })}
                    onClick={() =>
                      onChange({ ...config, nodes: config.nodes.filter((_, i) => i !== rank) })
                    }
                    disabled={locked}
                  />
                )}
              </li>
            );
          })}
        </ul>
        <div>
          <Button
            size="sm"
            variant="secondary"
            icon={<Plus />}
            onClick={() => setEditing(config.nodes.length)}
            disabled={locked}
          >
            {intl.formatMessage(i18n.addNode)}
          </Button>
        </div>
      </div>
      {editingNode && editing != null && (
        <NodeDialog
          open
          rank={editing}
          initial={editingNode}
          onApply={applyNode}
          onClose={() => setEditing(null)}
        />
      )}
    </div>
  );
}

function missingText(intl: IntlShape, missing: MissingField[], config: MlxDistributedConfig) {
  return missing
    .map((m) => {
      const field = intl.formatMessage(FIELD_LABEL[m.field]);
      if (m.node == null) return field;
      const node =
        config.nodes[m.node]?.name || intl.formatMessage(i18n.unnamedNode, { rank: m.node });
      return intl.formatMessage(i18n.missingNode, { node, field });
    })
    .join(', ');
}

// ---------------------------------------------------------------------------
// The section
// ---------------------------------------------------------------------------

export interface DistributedEngineSectionProps {
  /** goose offers the distributed engine (the `mlxDistributed` capability). */
  capability: boolean;
  /** A linked device is selected in "Manage on"; the distributed engine lives on THIS Mac only. */
  peerHostname: string | null;
  status: MlxDistributedStatus | null;
  statusError: string | null;
  onRefresh: () => Promise<void>;
  /** This Mac's local models (the model picker). */
  models: MlxLocalModel[];
  /** The single engine on this Mac: what "Unmount and continue" would stop. */
  singleStatus: MlxEngineStatus | null;
  /** The single engine changed (it was unmounted here) — the view re-reads it. */
  onSingleChanged: () => void;
}

type Busy = 'preflight' | 'start' | 'stop' | 'save' | 'unmount' | null;

interface ActionError {
  label: MessageDescriptor;
  text: string;
}

function Section({ children }: { children: ReactNode }) {
  const intl = useIntl();
  return (
    <Panel title={intl.formatMessage(i18n.title)}>
      <div data-testid="mlx-distributed" className="flex flex-col gap-4">
        {children}
      </div>
    </Panel>
  );
}

export function DistributedEngineSection(props: DistributedEngineSectionProps) {
  const intl = useIntl();
  const { capability, peerHostname, status, statusError, onRefresh, models, singleStatus } = props;

  const [draft, setDraft] = useState<MlxDistributedConfig | null>(null);
  const [busy, setBusy] = useState<Busy>(null);
  const [actionError, setActionError] = useState<ActionError | null>(null);
  const [refusal, setRefusal] = useState<MlxDistributedStartResponse['refusal']>(null);
  const [freshPreflight, setFreshPreflight] = useState<MlxDistributedPreflight | null>(null);
  const [repairLink, setRepairLink] = useState(false);
  const [confirmUnmount, setConfirmUnmount] = useState<string | null>(null);
  const [confirmStop, setConfirmStop] = useState(false);
  const [stopReport, setStopReport] = useState<MlxDistributedStopResponse['stop'] | null>(null);

  const owning = ownsTheMac(status);
  // A refusal is the answer to ONE start; once the run owns the Mac it no longer describes it.
  useEffect(() => {
    if (owning) setRefusal(null);
  }, [owning]);

  if (!capability) {
    return (
      <Section>
        <EmptyState
          icon={<Network />}
          title={intl.formatMessage(i18n.unavailableTitle)}
          body={intl.formatMessage(i18n.unavailableBody)}
        />
      </Section>
    );
  }
  if (peerHostname != null) {
    return (
      <Section>
        <EmptyState
          icon={<Network />}
          title={intl.formatMessage(i18n.peerTitle)}
          body={intl.formatMessage(i18n.peerBody, { host: peerHostname })}
        />
      </Section>
    );
  }

  const base = status?.config ?? null;
  const config = draft ?? base;
  const dirty = draft != null && (base == null || !sameConfig(draft, base));
  const missing = config ? missingFields(config) : [];
  // The persisted config is what the backend uses when none is sent; an edited draft is sent whole
  // (start persists it).
  const payload = dirty && config ? cleanConfig(config) : null;
  const canAct = busy == null && status != null && config != null && missing.length === 0;

  // The newest preflight wins: this view's own dry run, or the one the supervisor ran at start.
  const lastPreflight = status?.lastPreflight ?? null;
  const preflight =
    freshPreflight && (!lastPreflight || freshPreflight.ranAtMs >= lastPreflight.ranAtMs)
      ? freshPreflight
      : lastPreflight;

  const run = async <T,>(what: Busy, label: MessageDescriptor, fn: () => Promise<T>) => {
    setBusy(what);
    setActionError(null);
    try {
      return await fn();
    } catch (error) {
      setActionError({ label, text: mlxErrorMessage(error, String(error)) });
      return undefined;
    } finally {
      setBusy(null);
      void onRefresh();
    }
  };

  const onPreflight = () =>
    void run('preflight', i18n.preflightError, async () => {
      setRefusal(null);
      setFreshPreflight(await mlxDistributedPreflight(payload, repairLink));
    });

  /** One start; a refusal because the single engine is mounted OFFERS the unmount, never does it. */
  const startOnce = async (offerUnmount: boolean) => {
    setRefusal(null);
    setStopReport(null);
    const response = await mlxDistributedStart(payload);
    if (response.preflight) setFreshPreflight(response.preflight);
    if (response.started) {
      setDraft(null);
      return;
    }
    const why = response.refusal ?? null;
    if (why?.code === 'singleEngineMounted' && offerUnmount) {
      setConfirmUnmount(singleStatus?.servedModelId ?? singleStatus?.modelId ?? why.message);
      return;
    }
    setRefusal(why);
  };

  const onStart = () => void run('start', i18n.startError, () => startOnce(true));

  const onUnmountAndContinue = () => {
    setConfirmUnmount(null);
    void run('unmount', i18n.unmountError, async () => {
      await mlxEngineUnmount();
      props.onSingleChanged();
      await startOnce(false);
    });
  };

  const onStop = () => {
    setConfirmStop(false);
    void run('stop', i18n.stopError, async () => {
      const response = await mlxDistributedStop();
      setStopReport(response.stop);
    });
  };

  const onSave = () =>
    void run('save', i18n.saveError, async () => {
      if (!config) return;
      await mlxDistributedConfigUpdate(cleanConfig(config));
      setDraft(null);
    });

  const summary = modeSummary(status);
  const modeText = formatMlxMode(intl, summary, null);
  const state = status?.state ?? null;
  const stateTone = state ? runStateTone(state) : 'stopped';
  const nodeNames = (status?.nodes.length ? status.nodes : (config?.nodes ?? []))
    .map((n) => n.name)
    .join(', ');

  const facts: KeyValueItem[] = status
    ? [
        {
          key: 'model',
          label: intl.formatMessage(i18n.model),
          value: status.modelId ?? <Absent />,
          mono: status.modelId != null,
        },
        {
          key: 'url',
          label: intl.formatMessage(i18n.baseUrl),
          value: status.baseUrl ?? <Absent />,
          mono: status.baseUrl != null,
        },
        {
          key: 'context',
          label: intl.formatMessage(i18n.contextLimit),
          value:
            status.contextLimit != null ? intl.formatNumber(status.contextLimit) : <Absent />,
        },
        {
          key: 'runner',
          label: intl.formatMessage(i18n.runner),
          value: status.runner ?? <Absent />,
          mono: status.runner != null,
        },
        {
          key: 'restarts',
          label: intl.formatMessage(i18n.restarts),
          value: <span data-testid="mlx-dist-restarts">{status.restarts}</span>,
          tone: status.restarts > 0 ? 'warn' : undefined,
        },
      ]
    : [];

  return (
    <Section>
      <p className={TYPE.bodyMuted}>{intl.formatMessage(i18n.intro)}</p>
      {statusError && (
        <ToneBanner
          tone="err"
          label={intl.formatMessage(i18n.unreadable)}
          text={statusError}
          action={
            <Button size="sm" variant="secondary" icon={<RefreshCw />} onClick={() => void onRefresh()}>
              {intl.formatMessage(i18n.retry)}
            </Button>
          }
        />
      )}
      {!status && !statusError && <p className={TYPE.bodyMuted}>{intl.formatMessage(i18n.reading)}</p>}
      {actionError && (
        <ToneBanner
          tone="err"
          label={intl.formatMessage(actionError.label)}
          text={actionError.text}
          testId="mlx-dist-action-error"
        />
      )}
      {refusal && (
        <ToneBanner
          tone="err"
          label={intl.formatMessage(i18n.startRefused)}
          text={refusal.message}
          testId="mlx-dist-refusal"
        />
      )}
      {stopReport && (
        <div data-testid="mlx-dist-stop-report" className="flex flex-col gap-1">
          <ToneBanner
            tone={stopReport.verified ? 'ok' : 'err'}
            label={intl.formatMessage(stopReport.verified ? i18n.stopVerified : i18n.stopUnverified)}
            text={stopReport.steps.join(' · ')}
          />
        </div>
      )}

      {status && (
        <div
          data-testid="mlx-dist-mode"
          data-mode={status.mode}
          className="flex flex-wrap items-center gap-3"
        >
          <StatusDot
            tone={stateTone}
            live={runStateInFlight(status.state)}
            label={distributedStateWord(intl, status.state)}
            size={10}
          />
          <span data-testid="mlx-dist-mode-text" className={cx('text-lz-h1', TNUM)}>
            {modeText}
          </span>
          <Chip
            tone={stateTone}
            icon={runStateInFlight(status.state) ? <Loader2 className="animate-spin" /> : undefined}
          >
            {distributedStateWord(intl, status.state)}
          </Chip>
        </div>
      )}
      {status?.lastError && (
        <ToneBanner tone="err" label={intl.formatMessage(i18n.lastError)} text={status.lastError} />
      )}

      {status && (
        <div className="flex flex-col gap-2">
          <div className="flex flex-wrap items-center gap-2">
            <Button
              variant="secondary"
              icon={busy === 'preflight' ? <Loader2 className="animate-spin" /> : <Stethoscope />}
              onClick={onPreflight}
              disabled={!canAct || owning}
            >
              {intl.formatMessage(i18n.preflight)}
            </Button>
            {owning || status.state === 'failed' ? (
              <Button
                variant="destructive"
                icon={busy === 'stop' ? <Loader2 className="animate-spin" /> : <Square />}
                onClick={() => setConfirmStop(true)}
                disabled={busy != null}
              >
                {intl.formatMessage(i18n.stop)}
              </Button>
            ) : null}
            {!owning && (
              <Button
                variant="primary"
                icon={
                  busy === 'start' || busy === 'unmount' ? (
                    <Loader2 className="animate-spin" />
                  ) : (
                    <Play />
                  )
                }
                onClick={onStart}
                disabled={!canAct}
              >
                {intl.formatMessage(i18n.start)}
              </Button>
            )}
            <span className="ml-2 flex items-center gap-2">
              <StudioSwitch
                checked={repairLink}
                onChange={setRepairLink}
                aria-label={intl.formatMessage(i18n.repairLink)}
                disabled={owning}
              />
              <span className={TYPE.meta}>{intl.formatMessage(i18n.repairLink)}</span>
            </span>
          </div>
          <p className={TYPE.meta}>{intl.formatMessage(i18n.startHint)}</p>
        </div>
      )}

      {status && status.nodes.length > 0 && (
        <>
          <RunFacts status={status} />
          <div className="flex flex-col gap-2">
            <span className={TYPE.meta}>{intl.formatMessage(i18n.nodes)}</span>
            <div className="grid grid-cols-1 gap-3 xl:grid-cols-2">
              {status.nodes.map((n) => {
                const plan = planForRank(status, n.rank);
                return (
                  <NodeCard
                    key={`${n.rank}|${n.name}`}
                    node={n}
                    budgetGb={plan ? gib(plan.budgetBytes) : null}
                  />
                );
              })}
            </div>
          </div>
        </>
      )}

      {status && facts.length > 0 && (owning || status.modelId) && (
        <KeyValue items={facts} aria-label={intl.formatMessage(i18n.title)} />
      )}

      {status && (
        <div className="flex flex-col gap-2">
          <span className={TYPE.zone}>{intl.formatMessage(i18n.preflightTitle)}</span>
          {preflight ? (
            <PreflightReportView report={preflight} />
          ) : (
            <p className={TYPE.bodyMuted}>{intl.formatMessage(i18n.preflightNone)}</p>
          )}
        </div>
      )}

      {status && (
        <div className="flex flex-col gap-2">
          <div className="flex flex-wrap items-center gap-2">
            <span className={TYPE.zone}>{intl.formatMessage(i18n.configTitle)}</span>
            {dirty && (
              <span className="ml-auto flex items-center gap-2">
                <Button size="sm" variant="secondary" onClick={() => setDraft(null)} disabled={busy != null}>
                  {intl.formatMessage(i18n.revert)}
                </Button>
                <Button
                  size="sm"
                  variant="primary"
                  icon={busy === 'save' ? <Loader2 className="animate-spin" /> : undefined}
                  onClick={onSave}
                  disabled={busy != null || owning || missing.length > 0}
                >
                  {intl.formatMessage(i18n.save)}
                </Button>
              </span>
            )}
          </div>
          {config ? (
            <>
              <ConfigEditor
                config={config}
                models={models}
                locked={owning || busy != null}
                onChange={setDraft}
              />
              {missing.length > 0 && (
                <p
                  data-testid="mlx-dist-missing"
                  className={cx('break-words', TYPE.body, TONE_TEXT.warn)}
                >
                  {intl.formatMessage(i18n.missing, { fields: missingText(intl, missing, config) })}
                </p>
              )}
            </>
          ) : (
            <div className="flex flex-wrap items-center gap-3">
              <span className={TYPE.bodyMuted}>{intl.formatMessage(i18n.configNone)}</span>
              <Button
                size="sm"
                variant="secondary"
                icon={<Plus />}
                onClick={() =>
                  setDraft({
                    modelId: '',
                    backend: '',
                    port: 0,
                    coordinatorPort: 0,
                    restartOnFailure: false,
                    nodes: [emptyNode(), emptyNode()],
                  })
                }
              >
                {intl.formatMessage(i18n.setUp)}
              </Button>
            </div>
          )}
        </div>
      )}

      {status && <EventsPanel events={status.events} />}

      <ConfirmationModal
        isOpen={confirmUnmount != null}
        title={intl.formatMessage(i18n.unmountTitle)}
        message={intl.formatMessage(i18n.unmountMessage, { detail: confirmUnmount ?? '' })}
        confirmLabel={intl.formatMessage(i18n.unmountConfirm)}
        cancelLabel={intl.formatMessage(i18n.unmountCancel)}
        confirmVariant="destructive"
        onConfirm={onUnmountAndContinue}
        onCancel={() => setConfirmUnmount(null)}
      />
      <ConfirmationModal
        isOpen={confirmStop}
        title={intl.formatMessage(i18n.stopTitle)}
        message={intl.formatMessage(i18n.stopMessage, { nodes: nodeNames || '—' })}
        confirmLabel={intl.formatMessage(i18n.stop)}
        cancelLabel={intl.formatMessage(i18n.stopCancel)}
        confirmVariant="destructive"
        onConfirm={onStop}
        onCancel={() => setConfirmStop(false)}
      />
    </Section>
  );
}
