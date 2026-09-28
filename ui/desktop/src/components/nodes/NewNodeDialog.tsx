import { useEffect, useId, useMemo, useState, type ReactNode } from 'react';
import { ChevronRight, Cloud, Cpu, ExternalLink, Loader2, Plug, RefreshCw, X } from 'lucide-react';
import type { NodesServingWayDto } from '@aaif/goose-sdk';
import { defineMessages, useIntl } from '../../i18n';
import {
  Button,
  Checkbox,
  Chip,
  Combobox,
  FOCUS,
  MOTION,
  RADIUS,
  SURFACE,
  Segmented,
  TNUM,
  TONE_TEXT,
  TYPE,
  WEIGHT,
  cx,
} from '../lz';
import { OverlayDialog, OverlayDialogTitle } from '../ui/OverlayDialog';
import { INPUT } from '../leanzero-swarm/studio';
import { mlxErrorMessage } from '../leanzero-swarm/mlxErrorMessage';
import { useMacs } from '../leanzero-swarm/useMacs';
import { usePlacementPlans } from '../leanzero-swarm/usePlacementPlans';
import { PlacementBadge, pickerBadgeOf } from '../leanzero-swarm/PlacementCard';
import { PlacementCandidates, waysOf, type Way } from '../leanzero-swarm/PlacementCandidates';
import { formatGb } from '../leanzero-swarm/primitives';
import {
  providerListKind,
  providerRowState,
  type ProviderListKind,
} from '../leanzero-swarm/cloudProviderState';
import { acpListProviderDetails, acpListProviderLiveModels } from '../../acp/providers';
import type { PlacementGoal } from '../../acp/mlx-placement';
import type { ProviderDetails } from '../../types/providers';
import {
  defaultNodeName,
  mlxDef,
  modelShortName,
  nodeIdFor,
  placementOfCandidate,
  providerDef,
  putNode,
  samePlacement,
  uniqueName,
  whereWords,
  type PinnedPlacement,
} from './nodeDraft';
import type { NodeDef, ResolvedNodeDef } from './model';

/**
 * NEW NODE (DESIGN-NODES-AND-STRATEGIES.md §8.3): a stepper that makes one node definition and
 * writes it through `nodes/write` — the only door to the `nodes` key (it never writes `swarm`: the
 * pool keeps its own editor). Two paths:
 *
 * - On your Macs: Kind › Model (the models on your Macs, each with the planner's badge) › Way (the
 *   planner's candidates for a goal — Run it's own ways list, picked) › Name.
 * - A cloud model or your endpoint: Kind › Provider (configured ones only) › Model (the provider's
 *   own list) › Name.
 *
 * Nothing is guessed: a plan that failed is its words with Retry, a plan that measured nothing shows
 * its notes, a provider list that could not be read says so. The same dialog edits a node (it opens
 * on the Name step, keeps the id) and pins a way for a pool node's model.
 */

const i18n = defineMessages({
  title: { id: 'nodes.newTitle', defaultMessage: 'New node' },
  editTitle: { id: 'nodes.newEditTitle', defaultMessage: 'Edit {name}' },
  close: { id: 'nodes.newClose', defaultMessage: 'Close' },
  stepKind: { id: 'nodes.newStepKind', defaultMessage: 'Kind' },
  stepModel: { id: 'nodes.newStepModel', defaultMessage: 'Model' },
  stepWay: { id: 'nodes.newStepWay', defaultMessage: 'Way' },
  stepProvider: { id: 'nodes.newStepProvider', defaultMessage: 'Provider' },
  stepName: { id: 'nodes.newStepName', defaultMessage: 'Name' },
  stepOf: { id: 'nodes.newStepOf', defaultMessage: 'Step {n} of {total} · {label}' },
  whereRun: { id: 'nodes.newWhere', defaultMessage: 'Where should it run?' },
  onYourMacs: { id: 'nodes.newOnYourMacs', defaultMessage: 'On your Macs' },
  onYourMacsBody: {
    id: 'nodes.newOnYourMacsBody',
    defaultMessage: 'LeanZero MLX engine, one Mac or split',
  },
  cloud: { id: 'nodes.newCloud', defaultMessage: 'A cloud model' },
  cloudBody: {
    id: 'nodes.newCloudBody',
    defaultMessage: '{count, plural, =0 {None set up} one {# set up} other {# set up}}',
  },
  endpoint: { id: 'nodes.newEndpoint', defaultMessage: 'An endpoint' },
  endpointBody: {
    id: 'nodes.newEndpointBody',
    defaultMessage:
      'Any OpenAI-compatible server you added · {count, plural, =0 {none yet} one {# added} other {# added}}',
  },
  providersReading: { id: 'nodes.newProvidersReading', defaultMessage: 'Reading your providers…' },
  providersFailed: {
    id: 'nodes.newProvidersFailed',
    defaultMessage: 'Your providers could not be read: {error}',
  },
  noneSetUp: { id: 'nodes.newNoneSetUp', defaultMessage: 'None set up' },
  setUpAnother: { id: 'nodes.newSetUpAnother', defaultMessage: 'Set up another provider' },
  whichModel: { id: 'nodes.newWhichModel', defaultMessage: 'Which model?' },
  filterModels: { id: 'nodes.newFilterModels', defaultMessage: 'Filter models' },
  modelsOn: { id: 'nodes.newModelsOn', defaultMessage: 'on {macs}' },
  noModels: { id: 'nodes.newNoModels', defaultMessage: 'No models on your Macs yet.' },
  getModels: { id: 'nodes.newGetModels', defaultMessage: 'Get one in Models' },
  howRun: { id: 'nodes.newHowRun', defaultMessage: 'How should it run?' },
  goal: { id: 'nodes.newGoal', defaultMessage: 'Goal' },
  goalChat: { id: 'placementCard.goalChat', defaultMessage: 'Chat' },
  goalLong: { id: 'placementCard.goalLong', defaultMessage: 'Long documents' },
  goalMany: { id: 'placementCard.goalMany', defaultMessage: 'Many requests' },
  planning: { id: 'placementCard.planning', defaultMessage: 'Measuring your Macs and the model…' },
  planFailed: { id: 'placementCard.planFailed', defaultMessage: 'Could not plan' },
  retry: { id: 'nodes.newRetry', defaultMessage: 'Plan again' },
  ways: { id: 'nodes.newWays', defaultMessage: 'Ways to run {model}' },
  noWays: {
    id: 'nodes.newNoWays',
    defaultMessage: 'goose found no way to run this model on your Macs.',
  },
  fitOn: { id: 'nodes.newFitOn', defaultMessage: '{mac}: needs {need} GB, {budget} GB free' },
  whichProvider: { id: 'nodes.newWhichProvider', defaultMessage: 'Which provider?' },
  providerModels: { id: 'nodes.newProviderModels', defaultMessage: 'Model' },
  modelsReading: {
    id: 'nodes.newModelsReading',
    defaultMessage: 'Asking {provider} for its models…',
  },
  modelsFailed: {
    id: 'nodes.newModelsFailed',
    defaultMessage:
      '{provider} did not list its models ({error}); these are the models goose knows for it.',
  },
  modelsUnlisted: {
    id: 'nodes.newModelsUnlisted',
    defaultMessage: '{provider} has no model listing; these are the models goose knows for it.',
  },
  pickModel: { id: 'nodes.newPickModel', defaultMessage: 'Pick a model' },
  providerDefault: { id: 'nodes.newProviderDefault', defaultMessage: 'Default' },
  nameIt: { id: 'nodes.newNameIt', defaultMessage: 'Name it' },
  name: { id: 'nodes.newName', defaultMessage: 'Name' },
  keepLoaded: {
    id: 'nodes.newKeepLoaded',
    defaultMessage: 'Keep loaded: never stop it for another node',
  },
  back: { id: 'nodes.newBack', defaultMessage: 'Back' },
  next: { id: 'nodes.newNext', defaultMessage: 'Next' },
  create: { id: 'nodes.newCreate', defaultMessage: 'Create node' },
  createStart: { id: 'nodes.newCreateStart', defaultMessage: 'Create and start' },
  save: { id: 'nodes.newSave', defaultMessage: 'Save node' },
  refused: { id: 'nodes.newRefused', defaultMessage: 'Not saved' },
  writeFailed: { id: 'nodes.newWriteFailed', defaultMessage: 'The node could not be saved' },
  needModel: { id: 'nodes.newNeedModel', defaultMessage: 'Pick a model to go on' },
  needWay: { id: 'nodes.newNeedWay', defaultMessage: 'Pick a way to run it to go on' },
  needProvider: { id: 'nodes.newNeedProvider', defaultMessage: 'Pick a provider to go on' },
  needName: { id: 'nodes.newNeedName', defaultMessage: 'Give it a name to save it' },
  nameTaken: {
    id: 'nodes.newNameTaken',
    defaultMessage: 'Another node is already named “{name}” — give this one another name',
  },
  startStopsOn: {
    id: 'nodes.newStartStopsOn',
    defaultMessage: 'Starting it stops {model} on {macs}.',
  },
  startStops: { id: 'nodes.newStartStops', defaultMessage: 'Starting it stops {model}.' },
});

type Kind = 'mlx' | 'cloud' | 'endpoint';
type Step = 'kind' | 'model' | 'way' | 'provider' | 'providerModel' | 'name';

const MLX_STEPS: Step[] = ['kind', 'model', 'way', 'name'];
const PROVIDER_STEPS: Step[] = ['kind', 'provider', 'providerModel', 'name'];

export type NewNodeStart =
  | { kind: 'new' }
  /** Pin a way for a model (a pool node's): straight to Way, or to Model when none is known. */
  | { kind: 'pin'; model: string | null }
  | { kind: 'edit'; node: ResolvedNodeDef };

export interface NewNodeDialogProps {
  open: boolean;
  onClose: () => void;
  start: NewNodeStart;
  /** Every node now, for unique names and ids. */
  nodes: readonly ResolvedNodeDef[];
  /**
   * The way serving this Mac's goose now (null = nothing serves): one MLX way serves at a time, so
   * "Create and start" says beside it what starting stops (Q-299).
   */
  serving: NodesServingWayDto | null;
  /** The def was written; `startIt` when the person chose "Create and start". */
  onSaved: (def: NodeDef, startIt: boolean) => void;
  /** Where "Set up another provider" and "Get one in Models" go; the host owns navigation. */
  onOpenCloudProviders: () => void;
  onOpenModels: () => void;
}

type ProvidersRead =
  | { kind: 'reading' }
  | { kind: 'read'; providers: ProviderDetails[] }
  | { kind: 'failed'; error: string };

type ModelsRead =
  | { kind: 'reading' }
  | { kind: 'read'; models: string[] }
  | { kind: 'unlisted'; catalog: string[] }
  | { kind: 'failed'; error: string; catalog: string[] };

function Tile({
  icon,
  title,
  body,
  selected,
  onClick,
  testId,
}: {
  icon: ReactNode;
  title: string;
  body: string;
  selected: boolean;
  onClick: () => void;
  testId: string;
}) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={selected}
      onClick={onClick}
      data-testid={testId}
      className={cx(
        'flex min-w-0 flex-1 basis-40 flex-col items-start gap-1 p-3 text-left',
        SURFACE.card,
        SURFACE.hover,
        selected && SURFACE.selectedRing,
        FOCUS,
        MOTION
      )}
    >
      <span
        className={cx(
          'inline-flex h-5 items-center gap-1 px-1.5 text-lz-meta font-lz-semibold [&_svg]:size-3',
          RADIUS.control,
          'bg-[#111827] text-[#FFFFFF] dark:bg-[#F3F4F6] dark:text-[#111827]'
        )}
      >
        {icon}
      </span>
      <span className={cx(TYPE.body, WEIGHT.semibold)}>{title}</span>
      <span className={TYPE.meta}>{body}</span>
    </button>
  );
}

function PickRow({
  selected,
  onClick,
  children,
  testId,
}: {
  selected: boolean;
  onClick: () => void;
  children: ReactNode;
  testId: string;
}) {
  return (
    <li>
      <button
        type="button"
        role="radio"
        aria-checked={selected}
        onClick={onClick}
        data-testid={testId}
        className={cx(
          'flex w-full min-w-0 flex-wrap items-center gap-2 px-3 py-2 text-left',
          SURFACE.inset,
          RADIUS.card,
          selected && SURFACE.selectedRing,
          FOCUS,
          MOTION
        )}
      >
        {children}
      </button>
    </li>
  );
}

/**
 * The way "Create and start" stops, or null: one MLX way serves this Mac's goose at a time, so
 * starting a node stops whatever serves — unless it serves this very model on this very way.
 * Where the record cannot tell the ways apart (no Macs in it) and the model is the same, nothing
 * is claimed.
 */
export function stopsOnStart(
  serving: NodesServingWayDto | null,
  model: string | null,
  placement: PinnedPlacement | null
): { modelId: string; macs: string[] } | null {
  if (!serving) return null;
  const stops = { modelId: serving.modelId, macs: serving.macNames };
  if (!model || serving.modelId !== model) return stops;
  if (!placement || !serving.macs) return null;
  const servingSplit = serving.kind === 'split';
  const sameWay =
    servingSplit === (placement.kind !== 'single') &&
    serving.macs.length === placement.macs.length &&
    serving.macs.every((mac, i) => mac === placement.macs[i]);
  return sameWay ? null : stops;
}

export function NewNodeDialog(props: NewNodeDialogProps) {
  // Mounted fresh per open, so every open starts from its own `start`.
  return props.open ? <NewNodeDialogBody {...props} /> : null;
}

function NewNodeDialogBody({
  open,
  onClose,
  start,
  nodes,
  serving,
  onSaved,
  onOpenCloudProviders,
  onOpenModels,
}: NewNodeDialogProps) {
  const intl = useIntl();
  const macs = useMacs();
  const editing = start.kind === 'edit' ? start.node : null;
  const editDef = editing?.def ?? null;

  const [kind, setKind] = useState<Kind>(editDef ? editDef.kind : 'mlx');
  const [step, setStep] = useState<Step>(
    editDef ? 'name' : start.kind === 'pin' ? (start.model ? 'way' : 'model') : 'kind'
  );
  const [model, setModel] = useState<string | null>(
    start.kind === 'pin' ? start.model : (editing?.model ?? null)
  );
  const [goal, setGoal] = useState<PlacementGoal>(editDef?.goal ?? 'chat');
  const [placement, setPlacement] = useState<PinnedPlacement | null>(
    editDef?.placement && editDef.placement.kind !== 'follows' ? editDef.placement : null
  );
  const [provider, setProvider] = useState<string | null>(editing?.provider ?? null);
  const [providerModel, setProviderModel] = useState<string>(
    editDef && editDef.kind !== 'mlx' ? (editing?.model ?? '') : ''
  );
  const [name, setName] = useState<string>(editDef?.name ?? '');
  const [nameTouched, setNameTouched] = useState(editDef != null);
  const [keepLoaded, setKeepLoaded] = useState(editDef?.keepLoaded ?? false);
  const [filter, setFilter] = useState('');
  const [planAgain, setPlanAgain] = useState(0);
  const [saving, setSaving] = useState(false);
  const [refusals, setRefusals] = useState<string[]>([]);
  const blockedId = useId();
  const stopsId = useId();

  const [providers, setProviders] = useState<ProvidersRead>({ kind: 'reading' });
  useEffect(() => {
    let alive = true;
    acpListProviderDetails()
      .then((list) => alive && setProviders({ kind: 'read', providers: list }))
      .catch(
        (e: unknown) =>
          alive && setProviders({ kind: 'failed', error: mlxErrorMessage(e, String(e)) })
      );
    return () => {
      alive = false;
    };
  }, []);
  // The providers the Cloud Providers tab lists as configured, of one kind — never a local engine
  // (Q-429): the tile's count and the Provider step read this one list.
  const configured = (want: ProviderListKind) =>
    providers.kind === 'read'
      ? providers.providers.filter(
          (p) => providerRowState(p) !== 'not-set-up' && providerListKind(p) === want
        )
      : [];

  // Every model on your Macs, once, with the Macs that hold a complete copy.
  const localModels = useMemo(() => {
    const byId = new Map<string, { id: string; sizeBytes: number; macs: string[] }>();
    for (const mac of macs.macs) {
      for (const m of macs.factsOf(mac.key).models ?? []) {
        if (!m.complete) continue;
        const row = byId.get(m.id) ?? { id: m.id, sizeBytes: m.sizeBytes, macs: [] };
        row.macs.push(mac.name);
        byId.set(m.id, row);
      }
    }
    return [...byId.values()];
  }, [macs]);
  const modelsKey = localModels.map((m) => m.id).join('\n');
  const badgePlans = usePlacementPlans(
    kind === 'mlx' && step === 'model' ? 'chat' : null,
    modelsKey
  );
  const wayPlans = usePlacementPlans(
    kind === 'mlx' && step === 'way' && model ? goal : null,
    `${planAgain}`,
    model ?? undefined
  );

  const [models, setModels] = useState<ModelsRead>({ kind: 'reading' });
  const providerDetails =
    providers.kind === 'read'
      ? (providers.providers.find((p) => p.name === provider) ?? null)
      : null;
  useEffect(() => {
    if (!provider || kind === 'mlx') return;
    let alive = true;
    const catalog = providerDetails?.metadata.known_models.map((m) => m.name) ?? [];
    setModels({ kind: 'reading' });
    acpListProviderLiveModels(provider)
      .then((live) => {
        if (!alive) return;
        // An empty list means the provider has no listing endpoint (acp/providers.ts), never a
        // bad key: goose's catalog for it is offered, and said to be the catalog.
        setModels(live.length > 0 ? { kind: 'read', models: live } : { kind: 'unlisted', catalog });
      })
      .catch(
        (e: unknown) =>
          alive && setModels({ kind: 'failed', error: mlxErrorMessage(e, String(e)), catalog })
      );
    return () => {
      alive = false;
    };
  }, [provider, kind, providerDetails]);

  const steps = kind === 'mlx' ? MLX_STEPS : PROVIDER_STEPS;
  const at = steps.indexOf(step);
  const stepLabel = (s: Step) =>
    intl.formatMessage(
      s === 'kind'
        ? i18n.stepKind
        : s === 'model' || s === 'providerModel'
          ? i18n.stepModel
          : s === 'way'
            ? i18n.stepWay
            : s === 'provider'
              ? i18n.stepProvider
              : i18n.stepName
    );

  const takenNames = nodes.filter((n) => n.def.id !== editDef?.id).map((n) => n.def.name);
  const providerName = providerDetails?.metadata.display_name ?? provider ?? '';
  const suggestedName = (): string => {
    if (kind === 'mlx' && model && placement) {
      return defaultNodeName(intl, model, whereWords(intl, placement, macs.macs), takenNames);
    }
    // A provider's node is named by the model id as the provider lists it (Q-439: two
    // providers' "deepseek-v4.1-flash" are different models); the card's model line under the
    // name carries the short name.
    if (kind !== 'mlx' && providerModel) {
      return uniqueName(`${providerModel.trim()} · ${providerName}`, takenNames);
    }
    return '';
  };
  const goTo = (next: Step) => {
    if (next === 'name' && !nameTouched) setName(suggestedName());
    setRefusals([]);
    setStep(next);
  };

  const canNext =
    step === 'kind'
      ? kind === 'mlx' || configured(kind).length > 0
      : step === 'model'
        ? model != null
        : step === 'way'
          ? placement != null
          : step === 'provider'
            ? provider != null
            : step === 'providerModel'
              ? providerModel.trim() !== ''
              : false;

  const save = async (startIt: boolean) => {
    setSaving(true);
    setRefusals([]);
    const id =
      editDef?.id ??
      nodeIdFor(
        name,
        nodes.map((n) => n.def.id)
      );
    const def: NodeDef =
      kind === 'mlx'
        ? {
            ...mlxDef(id, {
              name: name.trim(),
              model: model ?? '',
              placement: placement!,
              goal,
              keepLoaded,
              origin: editDef?.origin === 'runIt' ? 'runIt' : 'user',
            }),
          }
        : providerDef(id, {
            kind,
            name: name.trim(),
            provider: provider ?? '',
            model: providerModel.trim(),
          });
    try {
      const response = await putNode(def);
      if (response.written) {
        onSaved(def, startIt);
        onClose();
        return;
      }
      setRefusals((response.refusals ?? []).map((r) => r.message));
    } catch (e) {
      setRefusals([mlxErrorMessage(e, intl.formatMessage(i18n.writeFailed))]);
    } finally {
      setSaving(false);
    }
  };

  const kindStep = (
    <div className="flex flex-col gap-3" data-testid="new-node-kind">
      <p className={cx(TYPE.body, WEIGHT.semibold)}>{intl.formatMessage(i18n.whereRun)}</p>
      <div
        className="flex flex-wrap gap-2"
        role="radiogroup"
        aria-label={intl.formatMessage(i18n.whereRun)}
      >
        <Tile
          icon={<Cpu />}
          title={intl.formatMessage(i18n.onYourMacs)}
          body={intl.formatMessage(i18n.onYourMacsBody)}
          selected={kind === 'mlx'}
          onClick={() => setKind('mlx')}
          testId="new-node-kind-mlx"
        />
        <Tile
          icon={<Cloud />}
          title={intl.formatMessage(i18n.cloud)}
          body={
            providers.kind === 'read'
              ? intl.formatMessage(i18n.cloudBody, { count: configured('cloud').length })
              : intl.formatMessage(i18n.providersReading)
          }
          selected={kind === 'cloud'}
          onClick={() => setKind('cloud')}
          testId="new-node-kind-cloud"
        />
        <Tile
          icon={<Plug />}
          title={intl.formatMessage(i18n.endpoint)}
          body={
            providers.kind === 'read'
              ? intl.formatMessage(i18n.endpointBody, { count: configured('endpoint').length })
              : intl.formatMessage(i18n.providersReading)
          }
          selected={kind === 'endpoint'}
          onClick={() => setKind('endpoint')}
          testId="new-node-kind-endpoint"
        />
      </div>
      {providers.kind === 'failed' && (
        <p className={cx('break-words', TYPE.meta, WEIGHT.semibold, TONE_TEXT.err)}>
          {intl.formatMessage(i18n.providersFailed, { error: providers.error })}
        </p>
      )}
      {kind !== 'mlx' && providers.kind === 'read' && configured(kind).length === 0 && (
        <div className="flex flex-wrap items-center gap-2" data-testid="new-node-none-set-up">
          <span className={cx(TYPE.body, WEIGHT.semibold)}>
            {intl.formatMessage(i18n.noneSetUp)}
          </span>
          <Button
            variant="primary"
            size="sm"
            icon={<ExternalLink />}
            onClick={onOpenCloudProviders}
          >
            {intl.formatMessage(i18n.setUpAnother)}
          </Button>
        </div>
      )}
    </div>
  );

  const shownModels = localModels.filter((m) =>
    m.id.toLowerCase().includes(filter.trim().toLowerCase())
  );
  const modelStep = (
    <div className="flex flex-col gap-3" data-testid="new-node-model">
      <p className={cx(TYPE.body, WEIGHT.semibold)}>{intl.formatMessage(i18n.whichModel)}</p>
      {localModels.length === 0 ? (
        <div className="flex flex-wrap items-center gap-2" data-testid="new-node-no-models">
          <span className={TYPE.body}>{intl.formatMessage(i18n.noModels)}</span>
          <Button variant="primary" size="sm" icon={<ExternalLink />} onClick={onOpenModels}>
            {intl.formatMessage(i18n.getModels)}
          </Button>
        </div>
      ) : (
        <>
          <input
            className={cx(INPUT, 'w-full')}
            placeholder={intl.formatMessage(i18n.filterModels)}
            aria-label={intl.formatMessage(i18n.filterModels)}
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
          />
          <ul
            className="flex max-h-72 flex-col gap-1.5 overflow-y-auto"
            role="radiogroup"
            aria-label={intl.formatMessage(i18n.whichModel)}
          >
            {shownModels.map((m) => {
              const plan = badgePlans.kind === 'read' ? badgePlans.plans.get(m.id) : undefined;
              const badge = plan ? pickerBadgeOf(plan) : null;
              return (
                <PickRow
                  key={m.id}
                  selected={model === m.id}
                  onClick={() => {
                    setModel(m.id);
                    setPlacement(null);
                  }}
                  testId="new-node-model-row"
                >
                  <span className={cx('min-w-0 flex-1 break-all', TYPE.mono)}>{m.id}</span>
                  <span className={cx(TYPE.meta, TNUM)}>{formatGb(m.sizeBytes)}</span>
                  {badge && <PlacementBadge badge={badge} />}
                  <span className={cx('basis-full', TYPE.meta)}>
                    {intl.formatMessage(i18n.modelsOn, {
                      macs: intl.formatList(m.macs, { type: 'conjunction' }),
                    })}
                  </span>
                </PickRow>
              );
            })}
          </ul>
          {badgePlans.kind === 'failed' && (
            <p className={cx('break-words', TYPE.meta)}>
              {intl.formatMessage(i18n.planFailed)}: {badgePlans.error}
            </p>
          )}
        </>
      )}
    </div>
  );

  const plan = wayPlans.kind === 'read' && model ? (wayPlans.plans.get(model) ?? null) : null;
  const { ways } = waysOf(plan, macs.macs, false);
  // Only the rows goose planned: with no candidate there is no guessed row (§8.3).
  const plannedWays: Way[] = plan && !plan.error ? ways.filter((w) => w.candidate != null) : [];
  const wayStep = (
    <div className="flex flex-col gap-3" data-testid="new-node-way">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className={cx(TYPE.body, WEIGHT.semibold)}>{intl.formatMessage(i18n.howRun)}</p>
        <Segmented<PlacementGoal>
          size="sm"
          aria-label={intl.formatMessage(i18n.goal)}
          options={[
            { value: 'chat', label: intl.formatMessage(i18n.goalChat) },
            { value: 'longDocuments', label: intl.formatMessage(i18n.goalLong) },
            { value: 'manyRequests', label: intl.formatMessage(i18n.goalMany) },
          ]}
          value={goal}
          onChange={(g) => {
            setGoal(g);
            setPlacement(null);
          }}
        />
      </div>
      {model && <p className={cx('break-all', TYPE.mono)}>{model}</p>}
      {wayPlans.kind === 'reading' && (
        <p className={TYPE.bodyMuted} data-testid="new-node-planning">
          {intl.formatMessage(i18n.planning)}
        </p>
      )}
      {(wayPlans.kind === 'failed' || plan?.error) && (
        <div className="flex flex-wrap items-center gap-2" data-testid="new-node-plan-failed">
          <span
            className={cx('min-w-0 flex-1 break-words', TYPE.body, TONE_TEXT.err, WEIGHT.semibold)}
          >
            {intl.formatMessage(i18n.planFailed)}:{' '}
            {wayPlans.kind === 'failed' ? wayPlans.error : plan?.error}
          </span>
          <Button
            variant="secondary"
            size="sm"
            icon={<RefreshCw />}
            onClick={() => setPlanAgain((n) => n + 1)}
          >
            {intl.formatMessage(i18n.retry)}
          </Button>
        </div>
      )}
      {wayPlans.kind === 'read' && plan && !plan.error && plannedWays.length === 0 && (
        <p className={TYPE.body}>{intl.formatMessage(i18n.noWays)}</p>
      )}
      {plannedWays.length > 0 && (
        <PlacementCandidates
          plan={plan}
          ways={plannedWays}
          goal={goal}
          pick={{
            label: intl.formatMessage(i18n.ways, { model: modelShortName(model ?? '') }),
            selected:
              plannedWays.find(
                (w) => w.candidate && samePlacement(placementOfCandidate(w.candidate), placement)
              )?.key ?? null,
            onSelect: (way) => way.candidate && setPlacement(placementOfCandidate(way.candidate)),
          }}
          renderBelow={(way) => {
            const c = way.candidate;
            if (!c) return null;
            return (
              <>
                {(c.fit.nodes ?? []).length > 0 && (
                  <p className={cx('break-words', TYPE.meta, TNUM)}>
                    {(c.fit.nodes ?? [])
                      .map((n) =>
                        intl.formatMessage(i18n.fitOn, {
                          mac: n.name,
                          need: intl.formatNumber(n.needBytes / 1024 ** 3, {
                            minimumFractionDigits: 1,
                            maximumFractionDigits: 1,
                          }),
                          budget: intl.formatNumber(n.budgetBytes / 1024 ** 3, {
                            minimumFractionDigits: 1,
                            maximumFractionDigits: 1,
                          }),
                        })
                      )
                      .join(' · ')}
                  </p>
                )}
                {c.action.kind === 'unavailable' && (
                  <p className={cx('break-words', TYPE.meta, WEIGHT.semibold, TONE_TEXT.warn)}>
                    {c.action.reason}
                  </p>
                )}
              </>
            );
          }}
        />
      )}
      {(plan?.notes ?? []).map((note) => (
        <p key={note} className={cx('break-words', TYPE.meta)} data-testid="new-node-plan-note">
          {note}
        </p>
      ))}
    </div>
  );

  const providerChoices = kind === 'mlx' ? [] : configured(kind);
  const providerStep = (
    <div className="flex flex-col gap-3" data-testid="new-node-provider">
      <p className={cx(TYPE.body, WEIGHT.semibold)}>{intl.formatMessage(i18n.whichProvider)}</p>
      <ul
        className="flex flex-col gap-1.5"
        role="radiogroup"
        aria-label={intl.formatMessage(i18n.whichProvider)}
      >
        {providerChoices.map((p) => (
          <PickRow
            key={p.name}
            selected={provider === p.name}
            onClick={() => {
              setProvider(p.name);
              // The default chosen in Cloud Providers is the first pick (Q-437).
              setProviderModel(p.default_model ?? '');
            }}
            testId="new-node-provider-row"
          >
            <span className={cx(TYPE.body, WEIGHT.semibold)}>{p.metadata.display_name}</span>
            {p.connection_error && (
              <Chip tone="err" title={p.connection_error}>
                {p.connection_error}
              </Chip>
            )}
          </PickRow>
        ))}
      </ul>
      <div>
        <Button variant="ghost" size="sm" icon={<ExternalLink />} onClick={onOpenCloudProviders}>
          {intl.formatMessage(i18n.setUpAnother)}
        </Button>
      </div>
    </div>
  );

  const listed =
    models.kind === 'read'
      ? models.models
      : models.kind === 'unlisted' || models.kind === 'failed'
        ? models.catalog
        : [];
  // The provider's default model (chosen and proven in Cloud Providers, whose dialog promises it
  // "leads the list whenever you add a node") leads, marked — then the provider's own order (Q-437).
  const providerDefault = providerDetails?.default_model || null;
  const modelOptions = providerDefault
    ? [providerDefault, ...listed.filter((m) => m !== providerDefault)]
    : listed;
  const providerModelStep = (
    <div className="flex flex-col gap-3" data-testid="new-node-provider-model">
      <p className={cx(TYPE.body, WEIGHT.semibold)}>{intl.formatMessage(i18n.whichModel)}</p>
      {models.kind === 'reading' && (
        <p className={TYPE.bodyMuted}>
          {intl.formatMessage(i18n.modelsReading, { provider: providerName })}
        </p>
      )}
      {models.kind === 'failed' && (
        <p className={cx('break-words', TYPE.meta, WEIGHT.semibold, TONE_TEXT.warn)}>
          {intl.formatMessage(i18n.modelsFailed, { provider: providerName, error: models.error })}
        </p>
      )}
      {models.kind === 'unlisted' && (
        <p className={cx('break-words', TYPE.meta)}>
          {intl.formatMessage(i18n.modelsUnlisted, { provider: providerName })}
        </p>
      )}
      {models.kind !== 'reading' && (
        <Combobox
          aria-label={intl.formatMessage(i18n.providerModels)}
          placeholder={intl.formatMessage(i18n.pickModel)}
          options={modelOptions.map((m) => ({
            value: m,
            hint: m === providerDefault ? intl.formatMessage(i18n.providerDefault) : undefined,
          }))}
          value={providerModel}
          onChange={setProviderModel}
        />
      )}
    </div>
  );

  const nameStep = (
    <div className="flex flex-col gap-3" data-testid="new-node-name">
      <p className={cx(TYPE.body, WEIGHT.semibold)}>{intl.formatMessage(i18n.nameIt)}</p>
      <label className="flex flex-col gap-1">
        <span className={TYPE.meta}>{intl.formatMessage(i18n.name)}</span>
        <input
          className={cx(INPUT, 'w-full')}
          value={name}
          onChange={(e) => {
            setNameTouched(true);
            setName(e.target.value);
          }}
          data-testid="new-node-name-input"
        />
      </label>
      {kind === 'mlx' && (
        <Checkbox
          checked={keepLoaded}
          onChange={setKeepLoaded}
          label={intl.formatMessage(i18n.keepLoaded)}
          testId="new-node-keep-loaded"
        />
      )}
    </div>
  );

  // The server refuses two nodes with one name after the click (Q-309); the same exact-name rule
  // is checked here, so Create is disabled with the reason before anything is sent.
  const nameTaken = takenNames.includes(name.trim());
  const canSave = !saving && name.trim() !== '' && !nameTaken;
  // What "Create and start" stops (Q-299): the way serving now, unless it IS this node's way.
  const startStops =
    kind === 'mlx' && !editDef && step === 'name' ? stopsOnStart(serving, model, placement) : null;
  const stopsText = startStops
    ? startStops.macs.length > 0
      ? intl.formatMessage(i18n.startStopsOn, {
          model: modelShortName(startStops.modelId),
          macs: intl.formatList(startStops.macs, { type: 'conjunction' }),
        })
      : intl.formatMessage(i18n.startStops, { model: modelShortName(startStops.modelId) })
    : null;

  // A disabled Next/Create says why beside it (Q-259: never a button that silently does nothing).
  // Steps whose own body already says why (no models, no ways, reading, a failed read) add nothing.
  const blocked: string | null =
    step === 'model' && model == null && localModels.length > 0
      ? intl.formatMessage(i18n.needModel)
      : step === 'way' && placement == null && plannedWays.length > 0
        ? intl.formatMessage(i18n.needWay)
        : step === 'provider' && provider == null && providerChoices.length > 0
          ? intl.formatMessage(i18n.needProvider)
          : step === 'providerModel' && providerModel.trim() === '' && models.kind !== 'reading'
            ? intl.formatMessage(i18n.needModel)
            : step === 'name' && name.trim() === ''
              ? intl.formatMessage(i18n.needName)
              : step === 'name' && nameTaken
                ? intl.formatMessage(i18n.nameTaken, { name: name.trim() })
                : null;

  const body =
    step === 'kind'
      ? kindStep
      : step === 'model'
        ? modelStep
        : step === 'way'
          ? wayStep
          : step === 'provider'
            ? providerStep
            : step === 'providerModel'
              ? providerModelStep
              : nameStep;

  return (
    <OverlayDialog
      open={open}
      onClose={onClose}
      panelClassName={cx(
        'flex max-h-[calc(100vh-2rem)] w-[44rem] flex-col gap-4 overflow-y-auto p-5',
        SURFACE.overlay
      )}
    >
      <div className="flex items-start justify-between gap-3" data-testid="new-node-dialog">
        <div className="flex min-w-0 flex-col gap-1">
          <OverlayDialogTitle asChild>
            <h2 className={TYPE.h2}>
              {editDef
                ? intl.formatMessage(i18n.editTitle, { name: editDef.name })
                : intl.formatMessage(i18n.title)}
            </h2>
          </OverlayDialogTitle>
          <ol className="hidden flex-wrap items-center gap-1 sm:flex" data-testid="new-node-steps">
            {steps.map((s, i) => (
              <li key={s} className="flex items-center gap-1">
                {i > 0 && <ChevronRight aria-hidden className="size-3.5 text-lz-ink-3" />}
                <span
                  className={cx(
                    'text-lz-meta',
                    TNUM,
                    i === at ? cx(WEIGHT.semibold, 'text-lz-ink') : 'text-lz-ink-3'
                  )}
                  aria-current={i === at ? 'step' : undefined}
                >
                  {i + 1} {stepLabel(s)}
                </span>
              </li>
            ))}
          </ol>
          <p className={cx('sm:hidden', TYPE.meta)} data-testid="new-node-step-compact">
            {intl.formatMessage(i18n.stepOf, {
              n: at + 1,
              total: steps.length,
              label: stepLabel(step),
            })}
          </p>
        </div>
        <Button
          variant="ghost"
          size="sm"
          iconOnly
          icon={<X />}
          aria-label={intl.formatMessage(i18n.close)}
          onClick={onClose}
        />
      </div>

      {body}

      {refusals.length > 0 && (
        <div role="alert" className="flex flex-col gap-1" data-testid="new-node-refusals">
          <span className={cx('text-lz-meta', WEIGHT.semibold, TONE_TEXT.err)}>
            {intl.formatMessage(i18n.refused)}
          </span>
          {refusals.map((r) => (
            <p key={r} className={cx('break-words', TYPE.body)}>
              {r}
            </p>
          ))}
        </div>
      )}

      <div className="flex flex-wrap items-center justify-end gap-2">
        {blocked && (
          <span
            id={blockedId}
            className={cx('mr-auto', TYPE.meta, WEIGHT.semibold)}
            data-testid="new-node-blocked"
          >
            {blocked}
          </span>
        )}
        {!blocked && stopsText && (
          <span
            id={stopsId}
            className={cx('mr-auto break-words text-lz-meta', WEIGHT.semibold, TONE_TEXT.warn)}
            data-testid="new-node-start-stops"
          >
            {stopsText}
          </span>
        )}
        {at > 0 && !(editDef && step === 'name' && kind !== 'mlx') && (
          <Button variant="ghost" onClick={() => goTo(steps[at - 1])} data-testid="new-node-back">
            {intl.formatMessage(i18n.back)}
          </Button>
        )}
        {step !== 'name' ? (
          <Button
            variant="primary"
            disabled={!canNext}
            aria-describedby={blocked ? blockedId : undefined}
            onClick={() => goTo(steps[at + 1])}
            data-testid="new-node-next"
          >
            {intl.formatMessage(i18n.next)}
          </Button>
        ) : (
          <>
            <Button
              variant={kind === 'mlx' && !editDef && !stopsText ? 'secondary' : 'primary'}
              disabled={!canSave}
              aria-describedby={blocked ? blockedId : undefined}
              icon={saving ? <Loader2 className="animate-spin" /> : undefined}
              onClick={() => void save(false)}
              data-testid="new-node-create"
            >
              {intl.formatMessage(editDef ? i18n.save : i18n.create)}
            </Button>
            {kind === 'mlx' && !editDef && (
              // When starting stops what serves, the safe door is the primary one and this one
              // carries the words beside it (Q-299).
              <Button
                variant={stopsText ? 'secondary' : 'primary'}
                disabled={!canSave}
                aria-describedby={blocked ? blockedId : stopsText ? stopsId : undefined}
                onClick={() => void save(true)}
                data-testid="new-node-create-start"
              >
                {intl.formatMessage(i18n.createStart)}
              </Button>
            )}
          </>
        )}
      </div>
    </OverlayDialog>
  );
}
