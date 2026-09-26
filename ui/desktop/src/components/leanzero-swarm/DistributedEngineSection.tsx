import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';
import {
  Loader2,
  MemoryStick,
  Network,
  Pencil,
  Play,
  Plus,
  Radar,
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
  Disclosure,
  EmptyState,
  KeyValue,
  PHASE_DOT,
  PHASE_FILL,
  Panel,
  RADIUS,
  StatusDot,
  SURFACE,
  TNUM,
  TONE_DOT,
  TONE_TEXT,
  TYPE,
  WEIGHT,
  cx,
  type EnginePhase,
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
  foreignOwner,
  mlxDistributedConfigUpdate,
  mlxDistributedMakeRoom,
  mlxDistributedPreflight,
  mlxDistributedStart,
  mlxDistributedStop,
  type MlxDistributedCheck,
  type MlxDistributedCompaction,
  type MlxDistributedConfig,
  type MlxDistributedEvent,
  type MlxDistributedNodeConfig,
  type MlxDistributedNodePreflight,
  type MlxDistributedNodeStatus,
  type MlxDistributedOwner,
  type MlxDistributedPreflight,
  type MlxDistributedProvision,
  type MlxDistributedRunnerEnv,
  type MlxDistributedStartResponse,
  type MlxDistributedStatus,
  type MlxDistributedStopResponse,
} from '../../acp/mlx-distributed';
import { mlxEngineUnmount, type MlxEngineStatus, type MlxLocalModel } from '../../acp/mlx-engine';
import {
  backendName,
  cleanConfig,
  compactionFor,
  emptyNode,
  eventKeys,
  eventTone,
  freeMemoryOn,
  gb1,
  gib,
  isAlarmOrNotice,
  layerSpan,
  configuredModeSummary,
  missingFields,
  nodeLoadProgress,
  nodeStartWord,
  ownsTheMac,
  planForRank,
  pressureTone,
  runStateInFlight,
  sameConfig,
  verdictTone,
  withFreeMemory,
  withGooseRunner,
  withModel,
  type LayerSpan,
  type MissingField,
  type NodeTextField,
} from './mlxDistributed';
import { distributedStateWord, formatMlxMode } from './mlxModeLabel';
import { nodePhase, runPhase } from './mlxPhase';
import { DistributedSetup, linkNode } from './DistributedSetup';
import { formatElapsed } from './mlxLiveStats';
import { mlxErrorMessage } from './mlxErrorMessage';
import { INPUT, StudioSelect, StudioSwitch, ToneBanner, type StudioSelectOption } from './studio';
import {
  LOCAL_NETWORK_CHECK,
  LOCAL_NETWORK_EVENT,
  LocalNetworkNotice,
  touchLocalNetwork,
} from './LocalNetworkNotice';

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
  slotsInUse: { id: 'mlxDistributed.slotsInUse', defaultMessage: 'Slots {used} / {slots}' },
  sequencesInFlight: {
    id: 'mlxDistributed.sequencesInFlight',
    defaultMessage: '{count, plural, one {# sequence} other {# sequences}} in the batch',
  },
  waiting: { id: 'mlxDistributed.waiting', defaultMessage: 'Waiting {count}' },
  serverStatusError: {
    id: 'mlxDistributed.serverStatusError',
    defaultMessage: 'Server status unreadable: {error}',
  },
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
  loadBytes: { id: 'mlxDistributed.load.bytes', defaultMessage: 'Loaded {done} of {total} GB' },
  loadBar: { id: 'mlxDistributed.load.bar', defaultMessage: 'Weights loaded' },
  makingRoom: { id: 'mlxDistributed.node.makingRoom', defaultMessage: 'Making room' },
  warming: { id: 'mlxDistributed.node.warming', defaultMessage: 'Warming up' },
  peakBar: { id: 'mlxDistributed.peakBar', defaultMessage: 'Peak memory against the budget' },
  active: { id: 'mlxDistributed.active', defaultMessage: 'active {gb} GiB' },
  planned: { id: 'mlxDistributed.planned', defaultMessage: 'planned {gb} GiB with overhead' },
  available: {
    id: 'mlxDistributed.available',
    defaultMessage: '{available} of {total} GiB available',
  },
  limits: {
    id: 'mlxDistributed.limits',
    defaultMessage: 'Caps: memory {memory} · wired {wired} · cache {cache} GiB',
  },
  limitsNone: { id: 'mlxDistributed.limitsNone', defaultMessage: 'Caps not reported yet' },
  kv: { id: 'mlxDistributed.kv', defaultMessage: 'KV {reserved} of {budget} GiB' },
  kvBar: { id: 'mlxDistributed.kvBar', defaultMessage: 'KV reserved against the budget' },
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
  checksPassed: {
    id: 'mlxDistributed.checksPassed',
    defaultMessage: '{count, plural, one {# check passed} other {# checks passed}}',
  },
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
  notConfigured: { id: 'mlxDistributed.notConfigured', defaultMessage: 'Not configured' },
  detectAgain: { id: 'mlxDistributed.detectAgain', defaultMessage: 'Detect again' },
  advanced: { id: 'mlxDistributed.advanced', defaultMessage: 'Advanced' },
  advancedMeta: {
    id: 'mlxDistributed.advancedMeta',
    defaultMessage: 'every field of the saved configuration',
  },
  summaryNode: {
    id: 'mlxDistributed.summaryNode',
    defaultMessage: 'rank {rank} · {name} · {where}',
  },
  provisionTitle: { id: 'mlxDistributed.provisionTitle', defaultMessage: 'Python on every node' },
  provisionRunning: { id: 'mlxDistributed.provision.running', defaultMessage: 'Provisioning' },
  provisionDone: { id: 'mlxDistributed.provision.done', defaultMessage: 'Ready' },
  provisionFailed: { id: 'mlxDistributed.provision.failed', defaultMessage: 'Failed' },
  provisionSkipped: {
    id: 'mlxDistributed.provision.skipped',
    defaultMessage: 'Your own Python',
  },
  provisionElapsed: {
    id: 'mlxDistributed.provision.elapsed',
    defaultMessage: '{seconds} s',
  },
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
  slots: { id: 'mlxDistributed.field.slots', defaultMessage: 'Slots (pipeline runner)' },
  slotsDefault: { id: 'mlxDistributed.slotsDefault', defaultMessage: 'runner default' },
  slotsHint: {
    id: 'mlxDistributed.slotsHint',
    defaultMessage:
      'Full-context sequences each rank is planned for; a request that would overrun waits. The tensor runner has none.',
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
  otherWindow: {
    id: 'mlxDistributed.otherWindow.label',
    defaultMessage: 'Running in another window',
  },
  otherWindowRun: {
    id: 'mlxDistributed.otherWindow.run',
    defaultMessage: '{model} on {nodes} · {backend} · {state}',
  },
  otherWindowAnswering: { id: 'mlxDistributed.otherWindow.answering', defaultMessage: 'answering' },
  otherWindowNotAnswering: {
    id: 'mlxDistributed.otherWindow.notAnswering',
    defaultMessage: 'not answering',
  },
  otherWindowReadOnly: {
    id: 'mlxDistributed.otherWindow.readOnly',
    defaultMessage:
      'Read-only here: Start and Stop belong to the window that started it (goosed pid {pid}).',
  },
  preflightError: { id: 'mlxDistributed.error.preflight', defaultMessage: 'Preflight error' },
  startError: { id: 'mlxDistributed.error.start', defaultMessage: 'Start error' },
  stopError: { id: 'mlxDistributed.error.stop', defaultMessage: 'Stop error' },
  saveError: { id: 'mlxDistributed.error.save', defaultMessage: 'Save error' },
  runnerUpdateTitle: {
    id: 'mlxDistributed.runnerUpdate.title',
    defaultMessage: 'Updating the split’s runner',
  },
  checkDetails: { id: 'mlxDistributed.check.details', defaultMessage: 'Details' },
  useGooseRunner: {
    id: 'mlxDistributed.check.useGooseRunner',
    defaultMessage: 'Use goose’s runner',
  },
  useGooseRunnerError: {
    id: 'mlxDistributed.error.useGooseRunner',
    defaultMessage: 'Could not hand this runner to goose',
  },
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

const ROOM = defineMessages({
  budgetLine: {
    id: 'mlxDistributed.room.budgetLine',
    defaultMessage: 'Budget {budget} GiB (GPU limit {ceiling} · available {available})',
  },
  budgetLineCompacted: {
    id: 'mlxDistributed.room.budgetLineCompacted',
    defaultMessage:
      'Budget {budget} GiB (GPU limit {ceiling} · available after compaction {available})',
  },
  freeMemory: { id: 'mlxDistributed.room.freeMemory', defaultMessage: 'Free memory automatically' },
  makeRoom: { id: 'mlxDistributed.room.makeRoom', defaultMessage: 'Make room' },
  hint: {
    id: 'mlxDistributed.room.hint',
    defaultMessage:
      'Asks macOS to reclaim memory: idle apps are compressed and caches dropped. Nothing is quit.',
  },
  freed: { id: 'mlxDistributed.room.freed', defaultMessage: 'Freed {gib} GiB' },
  freedNothing: {
    id: 'mlxDistributed.room.freedNothing',
    defaultMessage: 'Nothing freed ({gib} GiB less)',
  },
  refused: { id: 'mlxDistributed.room.refused', defaultMessage: 'Make room did not run' },
  failed: { id: 'mlxDistributed.room.failed', defaultMessage: 'Make room failed' },
  shortBy: {
    id: 'mlxDistributed.room.shortBy',
    defaultMessage: 'Short by {gib} GiB — Make room, or close apps: {apps}',
  },
  shortByNoApps: {
    id: 'mlxDistributed.room.shortByNoApps',
    defaultMessage: 'Short by {gib} GiB — Make room, or close apps',
  },
  memoryCompacted: { id: 'mlxDistributed.event.memoryCompacted', defaultMessage: 'Memory freed' },
  compactionSkipped: {
    id: 'mlxDistributed.event.compactionSkipped',
    defaultMessage: 'Make room skipped',
  },
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
  localNetworkBlocked: {
    id: 'mlxDistributed.event.localNetworkBlocked',
    defaultMessage: 'Local network blocked',
  },
  runnerUpdating: {
    id: 'mlxDistributed.event.runnerUpdating',
    defaultMessage: 'Updating the runner',
  },
  runnerUpdated: { id: 'mlxDistributed.event.runnerUpdated', defaultMessage: 'Runner updated' },
  runnerUpdateFailed: {
    id: 'mlxDistributed.event.runnerUpdateFailed',
    defaultMessage: 'Runner update failed',
  },
  // formatjs reads the literal entries above; these two borrow ROOM's and must stay last.
  memoryCompacted: ROOM.memoryCompacted,
  compactionSkipped: ROOM.compactionSkipped,
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
/** distributedStart's refusal code while another window's goosed supervises the run. */
const OWNED_BY_ANOTHER_WINDOW = 'ownedByAnotherWindow';
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
function Track({
  fraction,
  tone,
  phase,
  label,
}: {
  fraction: number;
  tone?: Tone;
  /** An engine phase's fill instead of a tone (a rank's load progress). */
  phase?: EnginePhase;
  label: string;
}) {
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
      <div
        className={cx('h-full', phase ? PHASE_DOT[phase] : TONE_DOT[tone ?? 'accent'])}
        style={{ width: `${pct}%` }}
      />
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

/**
 * Hands an interpreter the operator chose over to goose ("Use goose's runner" on a `runnerEnv`
 * WARN): provided by the section, which owns the config. goose never rebuilds such an interpreter
 * on its own (Q-116).
 */
const AdoptRunnerContext = createContext<{
  adopt: (env: MlxDistributedRunnerEnv) => void;
  disabled: boolean;
} | null>(null);

function CheckRow({ check, node }: { check: MlxDistributedCheck; node?: string }) {
  const intl = useIntl();
  const adopter = useContext(AdoptRunnerContext);
  const env = check.env;
  const offerAdopt = adopter != null && env != null && !env.managed && env.target != null;
  return (
    <li
      data-testid="mlx-dist-check"
      data-verdict={check.verdict}
      data-check={check.id}
      className="flex min-w-0 flex-col gap-1"
    >
      <span className="flex min-w-0 items-start gap-2">
        <Chip tone={verdictTone(check.verdict)}>{verdictWord(intl, check.verdict)}</Chip>
        <span className={cx('shrink-0', TYPE.mono)}>
          {node ? `${node} · ${check.id}` : check.id}
        </span>
        <span className={cx('min-w-0 break-words', TYPE.body, TNUM)}>{check.message}</span>
      </span>
      {(check.detail || offerAdopt) && (
        <span className="flex min-w-0 flex-col gap-1 pl-2">
          {offerAdopt && (
            <span>
              <Button
                size="sm"
                variant="secondary"
                onClick={() => adopter.adopt(env)}
                disabled={adopter.disabled}
                data-testid="mlx-dist-use-goose-runner"
              >
                {intl.formatMessage(i18n.useGooseRunner)}
              </Button>
            </span>
          )}
          {check.detail && (
            <Disclosure
              variant="plain"
              title={intl.formatMessage(i18n.checkDetails)}
              testId="mlx-dist-check-detail"
            >
              <p className={cx('whitespace-pre-wrap break-all', TYPE.mono)}>{check.detail}</p>
            </Disclosure>
          )}
        </span>
      )}
    </li>
  );
}

function PlanBlock({ node }: { node: MlxDistributedNodePreflight }) {
  const intl = useIntl();
  const plan = node.plan;
  if (!plan) {
    return (
      <p className={cx(TYPE.body, WEIGHT.semibold, TONE_TEXT.err)}>
        {intl.formatMessage(i18n.noPlan)}
      </p>
    );
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

/** What a node card needs to show and drive "Make room" (absent in read-only renders). */
interface RoomControls {
  freeMemory: (node: string) => boolean | null;
  compaction: (node: string) => MlxDistributedCompaction | null;
  onToggleFree: (node: string, on: boolean) => void;
  onMakeRoom: (node: string) => void;
  making: string | null;
  locked: boolean;
}

function CompactionLine({ compaction }: { compaction: MlxDistributedCompaction }) {
  const intl = useIntl();
  if (compaction.outcome === 'compacted' && compaction.gainedBytes != null) {
    const gained = compaction.gainedBytes;
    return (
      <div
        data-testid="mlx-dist-compaction"
        data-outcome="compacted"
        className="flex flex-col gap-1"
      >
        <Chip tone={gained > 0 ? 'ok' : 'warn'}>
          {gained >= 0
            ? intl.formatMessage(ROOM.freed, { gib: gb1(gib(gained)) })
            : intl.formatMessage(ROOM.freedNothing, { gib: gb1(gib(-gained)) })}
        </Chip>
        <span className={cx('break-words', META)}>{compaction.message}</span>
      </div>
    );
  }
  return (
    <div
      data-testid="mlx-dist-compaction"
      data-outcome={compaction.outcome}
      className="flex flex-col gap-1"
    >
      <Chip tone={compaction.outcome === 'refused' ? 'warn' : 'err'}>
        {intl.formatMessage(compaction.outcome === 'refused' ? ROOM.refused : ROOM.failed)}
      </Chip>
      <span className={cx('break-words', TYPE.body)}>{compaction.message}</span>
    </div>
  );
}

function RoomBlock({ node, room }: { node: MlxDistributedNodePreflight; room: RoomControls }) {
  const intl = useIntl();
  const free = room.freeMemory(node.name);
  const compaction = room.compaction(node.name);
  const making = room.making === node.name;
  const apps = (node.topApps ?? []).map((a) => `${a.name} (${gb1(gib(a.rssBytes))} GiB)`);
  return (
    <div data-testid="mlx-dist-room" className="flex flex-col gap-2">
      {node.shortBytes != null && (
        <p
          data-testid="mlx-dist-short"
          className={cx('break-words', TYPE.body, WEIGHT.semibold, TONE_TEXT.err)}
        >
          {apps.length > 0
            ? intl.formatMessage(ROOM.shortBy, {
                gib: gb1(gib(node.shortBytes)),
                apps: apps.join(', '),
              })
            : intl.formatMessage(ROOM.shortByNoApps, { gib: gb1(gib(node.shortBytes)) })}
        </p>
      )}
      <div className="flex flex-wrap items-center gap-3">
        {free != null && (
          <span className="flex items-center gap-2">
            <StudioSwitch
              checked={free}
              onChange={(on) => room.onToggleFree(node.name, on)}
              aria-label={`${intl.formatMessage(ROOM.freeMemory)} · ${node.name}`}
              disabled={room.locked}
            />
            <span className={TYPE.meta}>{intl.formatMessage(ROOM.freeMemory)}</span>
          </span>
        )}
        <Button
          size="sm"
          variant="secondary"
          icon={making ? <Loader2 className="animate-spin" /> : <MemoryStick />}
          onClick={() => room.onMakeRoom(node.name)}
          disabled={room.locked || room.making != null}
        >
          {intl.formatMessage(ROOM.makeRoom)}
        </Button>
      </div>
      <span className={META}>{intl.formatMessage(ROOM.hint)}</span>
      {compaction && <CompactionLine compaction={compaction} />}
    </div>
  );
}

/** "Budget X GiB (GPU limit Y · available Z)" — the figures the node's budget was built from. */
function BudgetLine({
  node,
  compaction,
  ranAtMs,
}: {
  node: MlxDistributedNodePreflight;
  compaction: MlxDistributedCompaction | null;
  ranAtMs: number;
}) {
  const intl = useIntl();
  if (!node.plan || node.availableBytes == null) return null;
  const compacted =
    compaction?.outcome === 'compacted' && compaction.atMs <= ranAtMs ? compaction : null;
  return (
    <span data-testid="mlx-dist-budget-line" className={cx(TYPE.body, TNUM)}>
      {intl.formatMessage(compacted ? ROOM.budgetLineCompacted : ROOM.budgetLine, {
        budget: gb1(gib(node.plan.budgetBytes)),
        ceiling: node.ceilingBytes != null ? gb1(gib(node.ceilingBytes)) : '—',
        available: gb1(gib(node.availableBytes)),
      })}
    </span>
  );
}

function PreflightNodeCard({
  node,
  ranAtMs,
  room,
}: {
  node: MlxDistributedNodePreflight;
  ranAtMs: number;
  room?: RoomControls;
}) {
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
        {node.pressure && <Chip tone={tone ?? undefined}>{pressureWord(intl, node.pressure)}</Chip>}
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
      <BudgetLine node={node} compaction={room?.compaction(node.name) ?? null} ranAtMs={ranAtMs} />
      {room && <RoomBlock node={node} room={room} />}
      <CheckList checks={node.checks} />
    </div>
  );
}

/** A check list: every failing or warning check in full, the passing ones folded under a count. */
function CheckList({ checks }: { checks: readonly MlxDistributedCheck[] }) {
  const intl = useIntl();
  const loud = checks.filter((c) => c.verdict !== 'pass');
  const passed = checks.filter((c) => c.verdict === 'pass');
  if (checks.length === 0) return null;
  return (
    <div className="flex flex-col gap-1.5">
      {loud.length > 0 && (
        <ul className="flex flex-col gap-1.5">
          {loud.map((c) => (
            <CheckRow key={`${c.id}|${c.message}`} check={c} />
          ))}
        </ul>
      )}
      {passed.length > 0 && (
        <Disclosure
          variant="plain"
          title={intl.formatMessage(i18n.checksPassed, { count: passed.length })}
          testId="mlx-dist-checks-passed"
        >
          <ul className="flex flex-col gap-1.5">
            {passed.map((c) => (
              <CheckRow key={`${c.id}|${c.message}`} check={c} />
            ))}
          </ul>
        </Disclosure>
      )}
    </div>
  );
}

function PreflightReportView({
  report,
  room,
  nodes = true,
}: {
  report: MlxDistributedPreflight;
  room?: RoomControls;
  /** false while the run lives: each Mac is its node card then, never twice. */
  nodes?: boolean;
}) {
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
      {failing.some(({ c }) => c.id === LOCAL_NETWORK_CHECK) && <LocalNetworkNotice />}
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
              max: report.maxContextFits != null ? intl.formatNumber(report.maxContextFits) : '—',
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
      {nodes && (
        <div className="grid grid-cols-1 gap-3 xl:grid-cols-2">
          {report.nodes.map((n) => (
            <PreflightNodeCard
              key={`${n.rank}|${n.name}`}
              node={n}
              ranAtMs={report.ranAtMs}
              room={room}
            />
          ))}
        </div>
      )}
      {report.checks.length > 0 && (
        <div className="flex flex-col gap-2">
          <span className={TYPE.meta}>{intl.formatMessage(i18n.clusterChecks)}</span>
          <CheckList checks={report.checks} />
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
  startWord,
}: {
  node: MlxDistributedNodeStatus;
  budgetGb: number | null;
  /** `nodeStartWord`: makingRoom / warming while it starts, else the node's state. */
  startWord: string;
}) {
  const intl = useIntl();
  const phase = nodePhase(startWord);
  const load =
    node.state === 'loading' && startWord !== 'makingRoom' ? nodeLoadProgress(node) : null;
  const peak = node.peakMemoryGb ?? null;
  const fraction = peak != null && budgetGb != null && budgetGb > 0 ? peak / budgetGb : null;
  const pTone = pressureTone(node.pressure);
  const kvFraction =
    node.kvReservedGb != null && node.kvBudgetGb != null && node.kvBudgetGb > 0
      ? node.kvReservedGb / node.kvBudgetGb
      : null;
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
      data-phase={phase}
      className={cx('flex min-w-0 flex-col gap-3 p-4', SURFACE.card)}
    >
      <div className="flex flex-wrap items-center gap-2">
        <StatusDot
          phase={phase}
          live={runStateInFlight(node.state) || node.state === 'loading'}
          label={node.state}
        />
        <span className={TYPE.h2}>{node.name}</span>
        <Chip phase={phase}>
          {startWord === 'makingRoom'
            ? intl.formatMessage(i18n.makingRoom)
            : startWord === 'warming'
              ? intl.formatMessage(i18n.warming)
              : distributedStateWord(intl, node.state)}
        </Chip>
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
      {load && (
        <div data-testid="mlx-dist-node-load" className="flex flex-col gap-1">
          <span className={cx(TYPE.body, TNUM)}>
            {intl.formatMessage(i18n.loadBytes, {
              done: gb1(gib(load.done)),
              total: gb1(gib(load.total)),
            })}
          </span>
          <Track
            fraction={load.done / load.total}
            phase="loading"
            label={intl.formatMessage(i18n.loadBar)}
          />
        </div>
      )}
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
      {kvFraction != null && node.kvReservedGb != null && node.kvBudgetGb != null && (
        <div data-testid="mlx-dist-node-kv" className="flex flex-col gap-1.5">
          <span className={cx(TYPE.body, TNUM)}>
            {intl.formatMessage(i18n.kv, {
              reserved: gb1(node.kvReservedGb),
              budget: gb1(node.kvBudgetGb),
            })}
          </span>
          <Track
            fraction={kvFraction}
            tone={budgetTone(kvFraction)}
            label={intl.formatMessage(i18n.kvBar)}
          />
        </div>
      )}
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

/**
 * Rank 0's /v1/status: the pipeline's slots (absent for the tensor runner — nothing is drawn for
 * them), the requests queued behind the batch, and the named reason when the poll broke.
 */
function ServerLoad({ status }: { status: MlxDistributedStatus }) {
  const intl = useIntl();
  const { slots, slotsInUse, sequencesInFlight, waiting, serverStatusError } = status;
  const slotLoad = slots != null && slotsInUse != null ? { used: slotsInUse, slots } : null;
  if (!slotLoad && sequencesInFlight == null && waiting == null && !serverStatusError) return null;
  return (
    <div className="flex flex-col gap-1.5 pt-1">
      <div className={cx('flex flex-wrap gap-x-3 gap-y-1', TYPE.body, TNUM)}>
        {slotLoad && (
          <span
            data-testid="mlx-dist-slots"
            className={cx(slotLoad.used >= slotLoad.slots && cx(WEIGHT.semibold, TONE_TEXT.warn))}
          >
            {intl.formatMessage(i18n.slotsInUse, slotLoad)}
          </span>
        )}
        {sequencesInFlight != null && (
          <span data-testid="mlx-dist-sequences">
            {intl.formatMessage(i18n.sequencesInFlight, { count: sequencesInFlight })}
          </span>
        )}
        {waiting != null && (
          <span
            data-testid="mlx-dist-waiting"
            className={cx(waiting > 0 && cx(WEIGHT.semibold, TONE_TEXT.warn))}
          >
            {intl.formatMessage(i18n.waiting, { count: waiting })}
          </span>
        )}
      </div>
      {serverStatusError && (
        <p
          data-testid="mlx-dist-server-status-error"
          className={cx('break-words', TYPE.body, WEIGHT.semibold, TONE_TEXT.err)}
        >
          {intl.formatMessage(i18n.serverStatusError, { error: serverStatusError })}
        </p>
      )}
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
          status.admissionOpen ? SURFACE.card : PHASE_FILL.held
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
        <ServerLoad status={status} />
      </div>
      <div
        data-testid="mlx-dist-liveness"
        className={cx('flex flex-col gap-1.5 p-4', SURFACE.card)}
      >
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
// A run another window supervises (each window runs its own goosed)
// ---------------------------------------------------------------------------

function OtherWindowRun({ owner }: { owner: MlxDistributedOwner }) {
  const intl = useIntl();
  const answering = owner.state === 'answering';
  const run = intl.formatMessage(i18n.otherWindowRun, {
    model: owner.servedModelId ?? owner.modelId ?? '—',
    nodes: owner.nodeNames?.length ? owner.nodeNames.join(' · ') : '—',
    backend: backendName(owner.backend) ?? '—',
    state: intl.formatMessage(answering ? i18n.otherWindowAnswering : i18n.otherWindowNotAnswering),
  });
  return (
    <div
      data-testid="mlx-dist-other-window"
      data-state={owner.state}
      className="flex flex-col gap-1"
    >
      <ToneBanner
        tone={answering ? 'accent' : 'warn'}
        label={intl.formatMessage(i18n.otherWindow)}
        text={owner.detail ? `${run} — ${owner.detail}` : run}
      />
      <p className={TYPE.meta}>
        {intl.formatMessage(i18n.otherWindowReadOnly, { pid: owner.pid ?? '—' })}
      </p>
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
    <Disclosure
      title={intl.formatMessage(i18n.eventsTitle)}
      meta={<span className={cx(TYPE.meta, TNUM)}>{events.length}</span>}
      testId="mlx-dist-events-disclosure"
    >
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
    </Disclosure>
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
    const local = models
      .filter((m) => m.complete)
      .map((m) => ({ value: m.id, label: m.id, local: true }));
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
        <div className="grid grid-cols-2 gap-3">
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
          <label className="flex min-w-0 flex-col gap-1">
            <span className={TYPE.meta}>{intl.formatMessage(i18n.slots)}</span>
            <NumberInput
              value={config.slots}
              onChange={(n) => onChange({ ...config, slots: n || null })}
              label={intl.formatMessage(i18n.slots)}
              placeholder={intl.formatMessage(i18n.slotsDefault)}
              disabled={locked}
            />
          </label>
        </div>
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
      <p className={TYPE.meta}>{intl.formatMessage(i18n.slotsHint)}</p>
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

function provisionTone(state: string): Tone {
  if (state === 'done') return 'ok';
  if (state === 'running') return 'accent';
  if (state === 'skipped') return 'stopped';
  return 'err';
}

/** The nodes' goose-managed Python: one row per env, its state, the step and the last line. */
function ProvisionPanel({
  provision,
  runnerUpdate = false,
}: {
  provision: MlxDistributedProvision;
  /** The rebuild a start ran by itself (Q-116), not the owner's "Save and provision". */
  runnerUpdate?: boolean;
}) {
  const intl = useIntl();
  const word: Record<string, MessageDescriptor> = {
    running: i18n.provisionRunning,
    done: i18n.provisionDone,
    failed: i18n.provisionFailed,
    skipped: i18n.provisionSkipped,
  };
  return (
    <div
      data-testid={runnerUpdate ? 'mlx-dist-runner-update' : 'mlx-dist-provision'}
      data-state={provision.state}
      className="flex flex-col gap-2"
    >
      <span className={TYPE.zone}>
        {runnerUpdate
          ? intl.formatMessage(i18n.runnerUpdateTitle)
          : intl.formatMessage(i18n.provisionTitle)}
      </span>
      <ul className="flex flex-col gap-2">
        {provision.nodes.map((n) => {
          const last = n.lines.length ? n.lines[n.lines.length - 1] : null;
          const elapsed =
            n.finishedMs != null ? ((n.finishedMs - n.startedMs) / 1000).toFixed(1) : null;
          return (
            <li
              key={`${n.rank}|${n.python}`}
              data-testid="mlx-dist-provision-node"
              data-state={n.state}
              className={cx('flex min-w-0 flex-col gap-1 px-3 py-2', SURFACE.card)}
            >
              <span className="flex flex-wrap items-center gap-2">
                <Chip
                  tone={provisionTone(n.state)}
                  icon={n.state === 'running' ? <Loader2 className="animate-spin" /> : undefined}
                >
                  {word[n.state] ? intl.formatMessage(word[n.state]) : n.state}
                </Chip>
                <span className={cx(TYPE.body, WEIGHT.semibold)}>{n.name}</span>
                {n.step && <span className={TYPE.mono}>{n.step}</span>}
                {elapsed && (
                  <span className={META}>
                    {intl.formatMessage(i18n.provisionElapsed, { seconds: elapsed })}
                  </span>
                )}
              </span>
              <span className={cx('break-all', TYPE.mono)}>{n.python}</span>
              <span
                className={cx(
                  'break-words',
                  n.state === 'failed' ? cx(TYPE.body, WEIGHT.semibold, TONE_TEXT.err) : TYPE.meta
                )}
              >
                {n.detail}
              </span>
              {n.state === 'running' && last && last !== n.detail && (
                <span className={cx('break-all', TYPE.meta)}>{last}</span>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}

/** The saved configuration at a glance: backend, model, and each node where it runs. */
function ConfigSummary({ config }: { config: MlxDistributedConfig }) {
  const intl = useIntl();
  return (
    <div data-testid="mlx-dist-config-summary" className="flex flex-col gap-1.5">
      <span className="flex flex-wrap items-center gap-2">
        {backendName(config.backend) && <Chip tone="accent">{backendName(config.backend)}</Chip>}
        <span className={cx('min-w-0 break-all', TYPE.mono)}>{config.modelId || '—'}</span>
      </span>
      <ul className="flex flex-col gap-1">
        {config.nodes.map((n, rank) => (
          <li key={rank} className={cx('break-all', TYPE.meta)}>
            {intl.formatMessage(i18n.summaryNode, {
              rank,
              name: n.name || '—',
              where: [
                rank === 0 ? intl.formatMessage(i18n.thisMac) : n.ssh || '—',
                n.tbIp,
                n.python,
              ]
                .filter(Boolean)
                .join(' · '),
            })}
          </li>
        ))}
      </ul>
    </div>
  );
}

// ---------------------------------------------------------------------------
// The section
// ---------------------------------------------------------------------------

export interface DistributedEngineSectionProps {
  /** goose offers the distributed engine (the `mlxDistributed` capability). */
  capability: boolean;
  /**
   * Folded under Run it's split row: the row carries the state, Run and Stop, so the section is
   * its details only — no title, no second Start.
   */
  embedded?: boolean;
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

type Busy = 'preflight' | 'start' | 'stop' | 'save' | 'unmount' | 'room' | null;

interface ActionError {
  label: MessageDescriptor;
  text: string;
}

function Section({ children, embedded }: { children: ReactNode; embedded?: boolean }) {
  const intl = useIntl();
  if (embedded) {
    return (
      <div data-testid="mlx-distributed" className="flex flex-col gap-4">
        {children}
      </div>
    );
  }
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
  const {
    capability,
    embedded = false,
    status,
    statusError,
    onRefresh,
    models,
    singleStatus,
  } = props;

  const [draft, setDraft] = useState<MlxDistributedConfig | null>(null);
  const [busy, setBusy] = useState<Busy>(null);
  const [actionError, setActionError] = useState<ActionError | null>(null);
  const [refusal, setRefusal] = useState<MlxDistributedStartResponse['refusal']>(null);
  const [freshPreflight, setFreshPreflight] = useState<MlxDistributedPreflight | null>(null);
  const [repairLink, setRepairLink] = useState(false);
  const [confirmUnmount, setConfirmUnmount] = useState<string | null>(null);
  const [confirmStop, setConfirmStop] = useState(false);
  const [stopReport, setStopReport] = useState<MlxDistributedStopResponse['stop'] | null>(null);
  const [setupOpen, setSetupOpen] = useState(false);
  const [makingRoom, setMakingRoom] = useState<string | null>(null);

  const owning = ownsTheMac(status);
  const otherWindow = foreignOwner(status);
  // A refusal is the answer to ONE start; once the run owns the Mac it no longer describes it.
  useEffect(() => {
    if (owning) setRefusal(null);
  }, [owning]);

  if (!capability) {
    return (
      <Section embedded={embedded}>
        <EmptyState
          icon={<Network />}
          title={intl.formatMessage(i18n.unavailableTitle)}
          body={intl.formatMessage(i18n.unavailableBody)}
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

  const lastAlarmKind =
    [...(status?.events ?? [])].reverse().find((e) => isAlarmOrNotice(e.kind))?.kind ?? null;

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
      await touchLocalNetwork();
      setFreshPreflight(await mlxDistributedPreflight(payload, repairLink));
    });

  /** One start; a refusal because the single engine is mounted OFFERS the unmount, never does it. */
  const startOnce = async (offerUnmount: boolean) => {
    setRefusal(null);
    setStopReport(null);
    await touchLocalNetwork();
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

  const onMakeRoom = (node: string) => {
    setMakingRoom(node);
    void run('room', ROOM.failed, async () => {
      try {
        await mlxDistributedMakeRoom(node, payload);
      } finally {
        setMakingRoom(null);
      }
    });
  };

  /** The switch edits the draft when one is open; otherwise it is saved at once. */
  const onToggleFree = (node: string, on: boolean) => {
    if (!config) return;
    const next = withFreeMemory(config, node, on);
    if (dirty) {
      setDraft(next);
      return;
    }
    void run('save', i18n.saveError, async () => {
      await mlxDistributedConfigUpdate(cleanConfig(next));
    });
  };

  const room: RoomControls = {
    freeMemory: (node) => {
      const configured = config?.nodes.find((n) => n.name === node);
      return configured ? freeMemoryOn(configured) : null;
    },
    compaction: (node) => compactionFor(status, node),
    onToggleFree,
    onMakeRoom,
    making: makingRoom,
    locked: busy != null || owning || otherWindow != null || config == null,
  };

  const onSave = () =>
    void run('save', i18n.saveError, async () => {
      if (!config) return;
      await mlxDistributedConfigUpdate(cleanConfig(config));
      setDraft(null);
    });

  /**
   * "Use goose's runner": the interpreter the operator chose is replaced by goose's own path on
   * that Mac — saved at once (or into the open draft), then checked again, so the row turns into
   * "goose builds it when you press Run". The one way goose ever touches an interpreter it did not
   * choose: the owner pressed this.
   */
  const onAdoptRunner = (env: MlxDistributedRunnerEnv) =>
    void run('save', i18n.useGooseRunnerError, async () => {
      if (!config) return;
      const next = withGooseRunner(config, env);
      if (!next) throw new Error(`no Mac in this setup runs ${env.python} any more`);
      if (dirty) {
        setDraft(next);
        return;
      }
      await mlxDistributedConfigUpdate(cleanConfig(next));
      await touchLocalNetwork();
      setFreshPreflight(await mlxDistributedPreflight(null, repairLink));
    });
  const adoptRunner = { adopt: onAdoptRunner, disabled: busy != null || owning };

  // The section says what IT is: the distributed engine as configured (or running), never the
  // single engine's "Single · this Mac" — that chip lives on the tab row and the tile.
  const configured = configuredModeSummary(status);
  const modeText = configured
    ? formatMlxMode(intl, configured, null)
    : intl.formatMessage(i18n.notConfigured);
  const state = status?.state ?? null;
  const statePhase: EnginePhase = state
    ? runPhase(state, status?.admissionOpen ?? true)
    : 'unloaded';
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
          value: status.contextLimit != null ? intl.formatNumber(status.contextLimit) : <Absent />,
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

  // The run is live from its start to its stop: only then do its admission, pids and peaks mean
  // anything — a stopped run has none, and its Macs are described once, by the preflight.
  const liveRun =
    status != null && status.state !== 'stopped' && (owning || status.state === 'failed');

  return (
    <Section embedded={embedded}>
      {!embedded && <p className={TYPE.bodyMuted}>{intl.formatMessage(i18n.intro)}</p>}
      {statusError && (
        <ToneBanner
          tone="err"
          label={intl.formatMessage(i18n.unreadable)}
          text={statusError}
          action={
            <Button
              size="sm"
              variant="secondary"
              icon={<RefreshCw />}
              onClick={() => void onRefresh()}
            >
              {intl.formatMessage(i18n.retry)}
            </Button>
          }
        />
      )}
      {!status && !statusError && (
        <p className={TYPE.bodyMuted}>{intl.formatMessage(i18n.reading)}</p>
      )}
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
          tone={refusal.code === OWNED_BY_ANOTHER_WINDOW ? 'warn' : 'err'}
          label={intl.formatMessage(
            refusal.code === OWNED_BY_ANOTHER_WINDOW ? i18n.otherWindow : i18n.startRefused
          )}
          text={refusal.message}
          testId="mlx-dist-refusal"
        />
      )}
      {refusal?.detail && (
        <Disclosure
          variant="plain"
          title={intl.formatMessage(i18n.checkDetails)}
          testId="mlx-dist-refusal-detail"
        >
          <p className={cx('whitespace-pre-wrap break-all', TYPE.mono)}>{refusal.detail}</p>
        </Disclosure>
      )}
      {stopReport && (
        <div data-testid="mlx-dist-stop-report" className="flex flex-col gap-1">
          <ToneBanner
            tone={stopReport.verified ? 'ok' : 'err'}
            label={intl.formatMessage(
              stopReport.verified ? i18n.stopVerified : i18n.stopUnverified
            )}
            text={stopReport.steps.join(' · ')}
          />
        </div>
      )}

      {status && !embedded && (
        <div
          data-testid="mlx-dist-mode"
          data-mode={status.mode}
          className="flex flex-wrap items-center gap-3"
        >
          <StatusDot
            phase={statePhase}
            live={runStateInFlight(status.state)}
            label={distributedStateWord(intl, status.state)}
            size={10}
          />
          <span data-testid="mlx-dist-mode-text" className={cx('text-lz-h1', TNUM)}>
            {modeText}
          </span>
          {configured && (
            <Chip
              phase={statePhase}
              icon={
                runStateInFlight(status.state) ? <Loader2 className="animate-spin" /> : undefined
              }
            >
              {distributedStateWord(intl, status.state)}
            </Chip>
          )}
        </div>
      )}
      {status?.lastError && (
        <ToneBanner tone="err" label={intl.formatMessage(i18n.lastError)} text={status.lastError} />
      )}
      {lastAlarmKind === LOCAL_NETWORK_EVENT && <LocalNetworkNotice />}
      {otherWindow && <OtherWindowRun owner={otherWindow} />}

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
            {!embedded && (owning || status.state === 'failed') ? (
              <Button
                variant="destructive"
                icon={busy === 'stop' ? <Loader2 className="animate-spin" /> : <Square />}
                onClick={() => setConfirmStop(true)}
                disabled={busy != null || otherWindow != null}
              >
                {intl.formatMessage(i18n.stop)}
              </Button>
            ) : null}
            {!embedded && !owning && (
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
                disabled={!canAct || otherWindow != null || status.hosting != null}
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
          {!embedded && <p className={TYPE.meta}>{intl.formatMessage(i18n.startHint)}</p>}
        </div>
      )}

      {status && liveRun && status.nodes.length > 0 && (
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
                    startWord={nodeStartWord(status, n)}
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
            <AdoptRunnerContext.Provider value={adoptRunner}>
              <PreflightReportView report={preflight} room={room} nodes={!liveRun} />
            </AdoptRunnerContext.Provider>
          ) : (
            <p className={TYPE.bodyMuted}>{intl.formatMessage(i18n.preflightNone)}</p>
          )}
        </div>
      )}

      {status?.runnerUpdate && <ProvisionPanel provision={status.runnerUpdate} runnerUpdate />}
      {status?.provision && <ProvisionPanel provision={status.provision} />}

      {status && (
        <div className="flex flex-col gap-2">
          <div className="flex flex-wrap items-center gap-2">
            <span className={TYPE.zone}>{intl.formatMessage(i18n.configTitle)}</span>
            {base && !setupOpen && (
              <Button
                size="sm"
                variant="secondary"
                icon={<Radar />}
                onClick={() => setSetupOpen(true)}
                disabled={owning || busy != null}
              >
                {intl.formatMessage(i18n.detectAgain)}
              </Button>
            )}
          </div>
          {setupOpen ? (
            <DistributedSetup
              initialPeer={base?.nodes.map((n) => n.ssh).find((h) => linkNode(h) != null) ?? ''}
              preferredModel={base?.modelId || null}
              renderAdvanced={(d, onChange) => (
                <ConfigEditor config={d} models={models} locked={false} onChange={onChange} />
              )}
              onCancel={() => setSetupOpen(false)}
              onSaved={() => {
                setSetupOpen(false);
                setDraft(null);
                void onRefresh();
              }}
            />
          ) : config ? (
            <>
              <ConfigSummary config={config} />
              <Disclosure
                testId="mlx-dist-advanced"
                title={intl.formatMessage(i18n.advanced)}
                meta={
                  dirty ? (
                    <span className="flex items-center gap-2">
                      <Button
                        size="sm"
                        variant="secondary"
                        onClick={() => setDraft(null)}
                        disabled={busy != null}
                      >
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
                  ) : (
                    <span className={TYPE.meta}>{intl.formatMessage(i18n.advancedMeta)}</span>
                  )
                }
                defaultOpen={base == null}
              >
                <div className="p-4">
                  <ConfigEditor
                    config={config}
                    models={models}
                    locked={owning || busy != null}
                    onChange={setDraft}
                  />
                </div>
              </Disclosure>
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
                variant="primary"
                icon={<Plus />}
                onClick={() => setSetupOpen(true)}
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
