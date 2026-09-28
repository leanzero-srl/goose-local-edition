import { useCallback, useEffect, useState, type ReactNode } from 'react';
import { Loader2, Repeat, RotateCcw, X } from 'lucide-react';
import type { IntlShape, MessageDescriptor } from 'react-intl';
import type { LoopTemplateDto } from '@aaif/goose-sdk';
import { useIntl } from '../../i18n';
import { loopsStart, loopsTemplates, loopsUpdate } from '../../acp/loops';
import type { KeepAwakeState } from '../../keepAwake';
import { SWARM_PROVIDER_ID } from '../../branding';
import { errorMessage } from '../../utils/conversionUtils';
import {
  Button,
  Checkbox,
  RADIUS,
  SURFACE,
  Segmented,
  TONE_FILL,
  TONE_TEXT,
  TYPE,
  WEIGHT,
  cx,
} from '../lz';
import { INPUT, TEXTAREA } from '../leanzero-swarm/studio';
import { OverlayDialog, OverlayDialogTitle } from '../ui/OverlayDialog';
import { useGlanceNodes, type GlanceNodesState } from '../engineGlance/glanceStore';
import { parseRouteModel } from '../nodes/model';
import { lastTick } from './loopView';
import { loopWords } from './loopWords';
import {
  SLOTS,
  stepSlots,
  validateLoop,
  type LoopRecord,
  type LoopRefusal,
  type LoopTemplateId,
} from './model';
import { onStartLoopRequest, type StartLoopRequest } from './startLoopRequest';
import {
  EVERY_PRESETS,
  TEMPLATE_IDS,
  formEdit,
  formFromLoop,
  formFromTemplate,
  nextTickLastStep,
  parseStopAfter,
  refusalField,
  stepSegments,
  withGoal,
  withTemplate,
  type EveryPreset,
  type FormField,
  type LoopForm,
} from './startLoopForm';
import { composerWords, startWords as w } from './startLoopWords';
import type { SessionLoop } from './useSessionLoop';

const TEMPLATE_NAME: Record<LoopTemplateId, MessageDescriptor> = {
  quality: w.templateQuality,
  until_check: w.templateUntilCheck,
  watch: w.templateWatch,
  blank: w.templateBlank,
};

const TEMPLATE_DESCRIPTION: Record<LoopTemplateId, MessageDescriptor> = {
  quality: w.descQuality,
  until_check: w.descUntilCheck,
  watch: w.descWatch,
  blank: w.descBlank,
};

/** A body-text field on the Studio outline (the goal is words, not code). */
const FIELD = cx(
  'w-full resize-y bg-lz-surface px-3 py-2 text-lz-body text-lz-ink placeholder:text-lz-ink-4 outline-none focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-ring',
  SURFACE.outline,
  RADIUS.control
);

// The meta SIZE with the err ink: TYPE.meta carries its own ink-3, and in the compiled CSS an ink
// utility beats text-lz-err (measured: text-lz-err and text-lz-accent lose to every text-lz-ink*).
const ERROR_TEXT = cx('break-words text-lz-meta', WEIGHT.semibold, TONE_TEXT.err);

export interface StartLoopFacts {
  /** The chat's working dir: where the check runs and the state file lives. */
  workingDir: string;
  /** The chat builds with the swarm: a start refuses (§4.1). */
  swarmBuild: boolean;
  /** The model chip's own words for what serves this chat — one derivation (§8.2 cost line); null
   *  when the chat names no model yet. */
  servedLabel: string | null;
  chatProvider: string | null;
  chatModel: string | null;
  /** The chat's loop as read now: a start over a live one asks first (Replace?). */
  current: SessionLoop;
  /** The loop changed: read it again. */
  onChanged: () => void;
}

/**
 * The Start dialog (§8.2): one per composer, for its own chat. It opens from the composer's Loop
 * button and from every other door (`startLoopRequest`: the rail's Start / Edit / Start a new loop,
 * a message's "Loop this"). Everything a tick will read is shown here before any model sees it:
 * the template's steps with their slots rendered as this loop's facts, the check, the cadence and
 * the state file. What goosed answers is shown as it is — a refused start keeps the dialog open
 * with goosed's words ("The loop runner is not in this build"), never a pretended success.
 */
export function StartLoopDialog({ sessionId, ...facts }: StartLoopFacts & { sessionId: string }) {
  const [request, setRequest] = useState<{ n: number; request: StartLoopRequest } | null>(null);

  useEffect(
    () =>
      onStartLoopRequest((incoming) => {
        if (incoming.sessionId !== sessionId) return false;
        setRequest((prev) => ({ n: (prev?.n ?? 0) + 1, request: incoming }));
        return true;
      }),
    [sessionId]
  );

  const close = useCallback(() => setRequest(null), []);

  return (
    <OverlayDialog
      open={request !== null}
      onClose={close}
      panelClassName={cx(
        'flex max-h-[calc(100vh-2rem)] w-[40rem] flex-col overflow-hidden',
        SURFACE.overlay
      )}
    >
      {request && (
        <StartLoopBody
          key={request.n}
          sessionId={sessionId}
          request={request.request}
          onClose={close}
          {...facts}
        />
      )}
    </OverlayDialog>
  );
}

type Read<T> = { kind: 'loading' } | { kind: 'read'; value: T } | { kind: 'failed'; error: string };

type Said = { kind: 'refused'; reason: string } | { kind: 'failed'; error: string };

/** "Each tick may load {node} and stop {way}" — only when the nodes say so (§5.5); never guessed. */
export function swapFacts(
  nodes: GlanceNodesState,
  provider: string | null,
  model: string | null
): { node: string; way: string } | null {
  if (provider !== SWARM_PROVIDER_ID || !model || nodes.kind !== 'read') return null;
  const route = parseRouteModel(model);
  if (route?.kind !== 'node') return null;
  const residency = nodes.residency.nodes.find((r) => r.node === route.id)?.residency;
  if (residency?.kind !== 'notRunning' || !residency.otherWay) return null;
  const def = nodes.read.nodes.find((n) => n.def.id === route.id)?.def;
  return { node: def?.name ?? route.id, way: residency.otherWay };
}

function refusalWords(
  intl: IntlShape,
  refusal: LoopRefusal,
  form: LoopForm,
  workingDir: string
): string {
  switch (refusal.code) {
    case 'empty_goal':
      return intl.formatMessage(w.goalEmpty);
    case 'check_required':
      return intl.formatMessage(w.checkRequired);
    case 'bad_cadence':
      return intl.formatMessage(w.cadenceInvalid);
    case 'state_file_outside':
      return intl.formatMessage(w.stateFileOutside, { dir: workingDir });
    case 'empty_state_file':
      return intl.formatMessage(w.stateFileEmpty);
    case 'bad_stop_after':
      return intl.formatMessage(w.stopAfterZero);
    case 'swarm_build':
      return intl.formatMessage(w.swarmBuild);
    case 'unknown_slot': {
      const slot = stepSlots(form.steps).find((s) => !(SLOTS as readonly string[]).includes(s));
      return slot ? intl.formatMessage(w.slotUnknown, { slot: `{${slot}}` }) : refusal.reason;
    }
    default:
      return refusal.reason;
  }
}

function StartLoopBody({
  sessionId,
  request,
  onClose,
  workingDir,
  swarmBuild,
  servedLabel,
  chatProvider,
  chatModel,
  current,
  onChanged,
}: StartLoopFacts & { sessionId: string; request: StartLoopRequest; onClose: () => void }) {
  const intl = useIntl();
  const editing = request.mode === 'edit';
  const [templates, setTemplates] = useState<Read<LoopTemplateDto[]>>({ kind: 'loading' });
  const [form, setForm] = useState<LoopForm | null>(() =>
    request.from ? formFromLoop(request.from) : null
  );
  const [goalTouched, setGoalTouched] = useState(false);
  const [saving, setSaving] = useState(false);
  const [said, setSaid] = useState<Said | null>(null);
  const [confirmReplace, setConfirmReplace] = useState(false);
  const [keepAwake, setKeepAwake] = useState<Read<KeepAwakeState>>({ kind: 'loading' });
  const nodes = useGlanceNodes();

  const readTemplates = useCallback(async () => {
    setTemplates({ kind: 'loading' });
    try {
      const got = await loopsTemplates();
      setTemplates({ kind: 'read', value: got.templates });
    } catch (error) {
      setTemplates({ kind: 'failed', error: errorMessage(error) });
    }
  }, []);

  useEffect(() => {
    void readTemplates();
  }, [readTemplates]);

  useEffect(() => {
    let live = true;
    window.electron.getWakelockState().then(
      (state) => live && setKeepAwake({ kind: 'read', value: state }),
      (error: unknown) => live && setKeepAwake({ kind: 'failed', error: errorMessage(error) })
    );
    return () => {
      live = false;
    };
  }, []);

  const templateList = templates.kind === 'read' ? templates.value : null;
  const templateOf = (id: LoopTemplateId) => templateList?.find((t) => t.id === id) ?? null;

  // A new loop opens on the Software quality loop, with a message's words when "Loop this" sent it.
  useEffect(() => {
    if (form || !templateList) return;
    const first = templateList.find((t) => t.id === 'quality') ?? templateList[0];
    if (first) setForm(formFromTemplate(first, request.goal ?? ''));
  }, [form, templateList, request.goal]);

  const toggleKeepAwake = async (enabled: boolean) => {
    try {
      setKeepAwake({ kind: 'read', value: await window.electron.setWakelock(enabled) });
    } catch (error) {
      setKeepAwake({ kind: 'failed', error: errorMessage(error) });
    }
  };

  const edit = form ? formEdit(form) : null;
  const verdict = edit ? validateLoop(edit, { workingDir, swarmBuild }) : null;
  const stopAfter = form ? parseStopAfter(form.stopAfter) : null;
  const refusal = verdict && 'refusal' in verdict ? verdict.refusal : null;
  const refusalAt: FormField | null = refusal ? refusalField(refusal.code) : null;
  const fieldError = (field: FormField): string | null => {
    if (field === 'stopAfter' && stopAfter?.kind === 'invalid') {
      return intl.formatMessage(w.stopAfterInvalid);
    }
    if (!refusal || !form || refusalAt !== field) return null;
    if (field === 'goal' && !goalTouched) return null;
    return refusalWords(intl, refusal, form, workingDir);
  };
  const canSubmit = verdict !== null && 'ok' in verdict && stopAfter?.kind !== 'invalid' && !saving;

  const liveLoop = current.kind === 'loop' && current.status !== 'ended' ? current.loop : null;
  const needsReplace = !editing && (liveLoop !== null || current.kind === 'unreadable');

  const submit = async (replaceConfirmed: boolean) => {
    if (!verdict || !('ok' in verdict) || stopAfter?.kind === 'invalid') return;
    if (needsReplace && !replaceConfirmed) {
      setConfirmReplace(true);
      return;
    }
    setConfirmReplace(false);
    setSaving(true);
    setSaid(null);
    try {
      const got = editing
        ? await loopsUpdate(sessionId, verdict.ok)
        : await loopsStart({ sessionId, ...verdict.ok });
      if (got.refusal) {
        setSaid({ kind: 'refused', reason: got.refusal.reason });
      } else if (!got.loop) {
        setSaid({ kind: 'failed', error: intl.formatMessage(composerWords.replyNeither) });
      } else {
        onChanged();
        onClose();
      }
    } catch (error) {
      setSaid({ kind: 'failed', error: errorMessage(error) });
    } finally {
      setSaving(false);
    }
  };

  const set = (patch: Partial<LoopForm>) =>
    setForm((prev) => (prev ? { ...prev, ...patch } : prev));

  const header = (
    <div className="flex items-start justify-between gap-3 border-b border-lz-border px-5 py-4">
      <div className="flex min-w-0 items-center gap-2.5">
        <span
          aria-hidden
          className={cx(
            'inline-flex size-7 shrink-0 items-center justify-center',
            RADIUS.control,
            TONE_FILL.accent
          )}
        >
          <Repeat className="size-4" />
        </span>
        <OverlayDialogTitle asChild>
          <h2 className={TYPE.h2}>{intl.formatMessage(editing ? w.titleEdit : w.title)}</h2>
        </OverlayDialogTitle>
      </div>
      <Button
        variant="ghost"
        size="sm"
        iconOnly
        icon={<X />}
        aria-label={intl.formatMessage(w.close)}
        onClick={onClose}
      />
    </div>
  );

  let body: ReactNode;
  if (!form) {
    body =
      templates.kind === 'failed' ? (
        <div className="flex flex-wrap items-center gap-2" data-testid="loop-templates-failed">
          <p className={cx('min-w-0 flex-1', ERROR_TEXT)}>
            {intl.formatMessage(w.templatesFailed, { error: templates.error })}
          </p>
          <Button
            variant="secondary"
            size="sm"
            icon={<RotateCcw />}
            onClick={() => void readTemplates()}
          >
            {intl.formatMessage(w.retry)}
          </Button>
        </div>
      ) : (
        <p
          className={cx('flex items-center gap-2', TYPE.bodyMuted)}
          data-testid="loop-templates-loading"
        >
          <Loader2 aria-hidden className="size-4 animate-spin" />
          {intl.formatMessage(w.templatesLoading)}
        </p>
      );
  } else {
    body = (
      <LoopFormFields
        form={form}
        set={set}
        setForm={setForm}
        templates={templates}
        templateOf={templateOf}
        readTemplates={readTemplates}
        setGoalTouched={setGoalTouched}
        fieldError={fieldError}
        workingDir={workingDir}
        lastStep={nextTickLastStep(editing ? (request.from ?? null) : null)}
      />
    );
  }

  const swap = swapFacts(nodes, chatProvider, chatModel);
  // Why Start is disabled, said beside it — the field that refuses may be scrolled out of view.
  const blocked = !form
    ? null
    : stopAfter?.kind === 'invalid'
      ? { text: intl.formatMessage(w.stopAfterInvalid), quiet: false }
      : refusal
        ? {
            text: refusalWords(intl, refusal, form, workingDir),
            quiet: refusal.code === 'empty_goal' && !goalTouched,
          }
        : null;

  return (
    <div className="flex min-h-0 flex-1 flex-col" data-testid="start-loop-dialog">
      {header}
      <div className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto px-5 py-4">
        {!editing && <p className={TYPE.bodyMuted}>{intl.formatMessage(w.intro)}</p>}
        {body}
        <div className="flex flex-col gap-1.5" data-testid="loop-cost">
          <p className={TYPE.meta} data-testid="loop-cost-line">
            {servedLabel
              ? intl.formatMessage(w.costLine, { served: servedLabel })
              : intl.formatMessage(w.costLineNoModel)}
          </p>
          {swap && (
            <p
              className={cx('text-lz-meta', WEIGHT.semibold, TONE_TEXT.warn)}
              data-testid="loop-swap-line"
            >
              {intl.formatMessage(w.swapLine, swap)}
            </p>
          )}
          <p className={TYPE.meta} data-testid="loop-sleep-line">
            {intl.formatMessage(w.sleepLine)}
          </p>
          <Checkbox
            checked={keepAwake.kind === 'read' && keepAwake.value.enabled}
            disabled={keepAwake.kind === 'loading'}
            onChange={(enabled) => void toggleKeepAwake(enabled)}
            label={intl.formatMessage(w.keepAwake)}
            description={intl.formatMessage(w.keepAwakeHelp)}
            testId="loop-keep-awake"
          />
          {(keepAwake.kind === 'failed' ||
            (keepAwake.kind === 'read' && keepAwake.value.error)) && (
            <p className={ERROR_TEXT} role="alert" data-testid="loop-keep-awake-failed">
              {intl.formatMessage(w.keepAwakeFailed, {
                reason:
                  keepAwake.kind === 'failed' ? keepAwake.error : (keepAwake.value.error ?? ''),
              })}
            </p>
          )}
        </div>
      </div>
      <div className="flex flex-col items-end gap-1.5 border-t border-lz-border px-5 py-3">
        {blocked && !said && (
          <p
            className={cx('self-stretch text-right', blocked.quiet ? TYPE.meta : ERROR_TEXT)}
            data-testid="loop-start-blocked"
          >
            {blocked.text}
          </p>
        )}
        {said && (
          <p role="alert" className={cx('self-stretch', ERROR_TEXT)} data-testid="loop-start-said">
            {said.kind === 'refused'
              ? intl.formatMessage(loopWords.controlRefused, { reason: said.reason })
              : intl.formatMessage(loopWords.controlFailed, { error: said.error })}
          </p>
        )}
        <div className="flex flex-wrap items-center justify-end gap-2">
          <Button variant="ghost" onClick={onClose} data-testid="loop-cancel">
            {intl.formatMessage(w.cancel)}
          </Button>
          <Button
            variant="primary"
            disabled={!canSubmit}
            icon={saving ? <Loader2 className="animate-spin" /> : <Repeat />}
            onClick={() => void submit(false)}
            data-testid="loop-start"
          >
            {intl.formatMessage(editing ? w.save : w.start)}
          </Button>
        </div>
        {!editing && (
          <p className={TYPE.meta} data-testid="loop-first-tick-now">
            {intl.formatMessage(w.firstTickNow)}
          </p>
        )}
      </div>
      <ReplaceDialog
        open={confirmReplace}
        loop={liveLoop}
        unreadable={current.kind === 'unreadable'}
        onKeep={() => setConfirmReplace(false)}
        onReplace={() => void submit(true)}
      />
    </div>
  );
}

function Section({
  label,
  action,
  children,
  testId,
}: {
  label: string;
  action?: ReactNode;
  children: ReactNode;
  testId?: string;
}) {
  return (
    <section className="flex flex-col gap-2" data-testid={testId}>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className={cx(TYPE.body, WEIGHT.semibold)}>{label}</h3>
        {action}
      </div>
      {children}
    </section>
  );
}

function LoopFormFields({
  form,
  set,
  setForm,
  templates,
  templateOf,
  readTemplates,
  setGoalTouched,
  fieldError,
  workingDir,
  lastStep,
}: {
  form: LoopForm;
  set: (patch: Partial<LoopForm>) => void;
  setForm: (update: (prev: LoopForm | null) => LoopForm | null) => void;
  templates: Read<LoopTemplateDto[]>;
  templateOf: (id: LoopTemplateId) => LoopTemplateDto | null;
  readTemplates: () => Promise<void>;
  setGoalTouched: (touched: boolean) => void;
  fieldError: (field: FormField) => string | null;
  workingDir: string;
  lastStep: ReturnType<typeof nextTickLastStep>;
}) {
  const intl = useIntl();
  const current = templateOf(form.template);
  const segments = stepSegments(form.steps, {
    stateFile: form.stateFile,
    check: form.check,
    goal: form.goal,
    workingDir,
    lastNextStep: lastStep,
  });
  const goalError = fieldError('goal');
  const hasCheck = form.check.trim() !== '';

  const templateOptions = TEMPLATE_IDS.map((id) => ({
    value: id,
    label: intl.formatMessage(TEMPLATE_NAME[id]),
    disabled: templateOf(id) === null,
    testId: `loop-template-${id}`,
  }));

  const cadenceOptions = [
    {
      value: 'every' as const,
      label: intl.formatMessage(w.cadenceEvery),
      testId: 'loop-cadence-every',
    },
    {
      value: 'self_paced' as const,
      label: intl.formatMessage(w.cadenceSelfPaced),
      testId: 'loop-cadence-self_paced',
    },
    {
      value: 'back_to_back' as const,
      label: intl.formatMessage(w.cadenceBackToBack),
      testId: 'loop-cadence-back_to_back',
    },
  ];

  const presetOptions = [
    ...EVERY_PRESETS.map((preset) => ({
      value: preset,
      label: preset,
      testId: `loop-every-${preset}`,
    })),
    {
      value: 'custom' as const,
      label: intl.formatMessage(w.presetCustom),
      testId: 'loop-every-custom',
    },
  ];

  const stopAfterField = (
    <input
      key="stop-after"
      className={cx(INPUT, 'mx-1.5 w-16 text-center tnum')}
      inputMode="numeric"
      value={form.stopAfter}
      onChange={(e) => set({ stopAfter: e.target.value })}
      aria-label={intl.formatMessage(w.stopAfterField)}
      data-testid="loop-stop-after"
    />
  );

  return (
    <>
      <Section label={intl.formatMessage(w.startFrom)} testId="loop-templates">
        <Segmented
          aria-label={intl.formatMessage(w.startFrom)}
          options={templateOptions}
          value={form.template}
          onChange={(id) => {
            const template = templateOf(id);
            if (template) setForm((prev) => (prev ? withTemplate(prev, template) : prev));
          }}
          className="max-w-full flex-wrap self-start"
        />
        <p className={TYPE.bodyMuted} data-testid="loop-template-description">
          {intl.formatMessage(TEMPLATE_DESCRIPTION[form.template])}
        </p>
        {templates.kind === 'failed' && (
          <div className="flex flex-wrap items-center gap-2" data-testid="loop-templates-failed">
            <p className={cx('min-w-0 flex-1', ERROR_TEXT)}>
              {intl.formatMessage(w.templatesFailed, { error: templates.error })}
            </p>
            <Button
              variant="secondary"
              size="sm"
              icon={<RotateCcw />}
              onClick={() => void readTemplates()}
            >
              {intl.formatMessage(w.retry)}
            </Button>
          </div>
        )}
      </Section>

      <Section label={intl.formatMessage(w.goal)} testId="loop-goal-section">
        <textarea
          className={FIELD}
          rows={2}
          value={form.goal}
          placeholder={intl.formatMessage(w.goalPlaceholder)}
          onChange={(e) => {
            setGoalTouched(true);
            setForm((prev) => (prev ? withGoal(prev, e.target.value) : prev));
          }}
          onBlur={() => setGoalTouched(true)}
          aria-label={intl.formatMessage(w.goal)}
          aria-invalid={goalError !== null}
          data-testid="loop-goal"
        />
        {form.goal.trim() === '' && (
          <p className={goalError ? ERROR_TEXT : TYPE.meta} data-testid="loop-goal-empty">
            {intl.formatMessage(w.goalEmpty)}
          </p>
        )}
      </Section>

      <Section
        label={intl.formatMessage(w.steps)}
        testId="loop-steps-section"
        action={
          <Button
            variant="ghost"
            size="sm"
            icon={<RotateCcw />}
            disabled={current === null || current.steps === form.steps}
            onClick={() => current && set({ steps: current.steps })}
            data-testid="loop-reset-steps"
          >
            {intl.formatMessage(w.resetSteps)}
          </Button>
        }
      >
        <textarea
          className={TEXTAREA}
          rows={form.steps ? 6 : 2}
          value={form.steps}
          placeholder={intl.formatMessage(w.stepsBlankPlaceholder)}
          onChange={(e) => set({ steps: e.target.value })}
          aria-label={intl.formatMessage(w.steps)}
          aria-invalid={fieldError('steps') !== null}
          data-testid="loop-steps"
        />
        {fieldError('steps') && <p className={ERROR_TEXT}>{fieldError('steps')}</p>}
        {form.steps.trim() !== '' && (
          <div className="flex flex-col gap-1.5" data-testid="loop-steps-preview">
            <p className={cx(TYPE.meta, WEIGHT.semibold)}>{intl.formatMessage(w.stepsPreview)}</p>
            <div
              className={cx(
                'whitespace-pre-wrap break-words px-3 py-2 text-lz-body leading-relaxed text-lz-ink',
                SURFACE.inset,
                RADIUS.control
              )}
            >
              {segments.map((segment, i) =>
                segment.kind === 'text' ? (
                  <span key={i}>{segment.text}</span>
                ) : (
                  <span
                    key={i}
                    title={
                      segment.kind === 'slot'
                        ? intl.formatMessage(w.slotTitle, { name: segment.name })
                        : intl.formatMessage(w.slotUnknown, { slot: `{${segment.name}}` })
                    }
                    data-testid={segment.kind === 'slot' ? 'loop-slot' : 'loop-slot-unknown'}
                    data-slot={segment.name}
                    className={cx(
                      'box-decoration-clone rounded-lz-pill px-1.5 py-px text-[12px] font-lz-semibold',
                      segment.kind === 'slot' ? TONE_FILL.accent : TONE_FILL.err
                    )}
                  >
                    {segment.kind === 'slot'
                      ? segment.name === 'goal_first_line' && segment.value === ''
                        ? intl.formatMessage(w.goalFirstLineEmpty)
                        : segment.value
                      : `{${segment.name}}`}
                  </span>
                )
              )}
            </div>
            <p className={TYPE.meta}>{intl.formatMessage(w.stepsPreviewHelp)}</p>
          </div>
        )}
      </Section>

      <Section label={intl.formatMessage(w.check)} testId="loop-check-section">
        <label className="flex flex-wrap items-center gap-2">
          <span className={cx(TYPE.meta, WEIGHT.semibold)}>{intl.formatMessage(w.command)}</span>
          <input
            className={cx(INPUT, 'min-w-0 flex-1 font-mono')}
            value={form.check}
            placeholder={intl.formatMessage(w.checkPlaceholder)}
            onChange={(e) => set({ check: e.target.value })}
            aria-invalid={fieldError('check') !== null}
            data-testid="loop-check"
          />
        </label>
        <p className={TYPE.meta}>{intl.formatMessage(w.checkHelp, { dir: workingDir })}</p>
        {fieldError('check') && (
          <p className={ERROR_TEXT} data-testid="loop-check-error">
            {fieldError('check')}
          </p>
        )}
      </Section>

      <Section label={intl.formatMessage(w.cadence)} testId="loop-cadence-section">
        <Segmented
          aria-label={intl.formatMessage(w.cadence)}
          options={cadenceOptions}
          value={form.cadenceKind}
          onChange={(cadenceKind) => set({ cadenceKind })}
          className="max-w-full flex-wrap self-start"
        />
        {form.cadenceKind === 'every' && (
          <div className="flex flex-col gap-2">
            <Segmented
              aria-label={intl.formatMessage(w.presets)}
              options={presetOptions}
              value={form.every}
              onChange={(every: EveryPreset | 'custom') => set({ every })}
              className="max-w-full flex-wrap self-start"
            />
            {form.every === 'custom' && (
              <input
                className={cx(INPUT, 'w-28 font-mono')}
                value={form.customEvery}
                placeholder="90m"
                onChange={(e) => set({ customEvery: e.target.value })}
                aria-label={intl.formatMessage(w.customEvery)}
                aria-invalid={fieldError('cadence') !== null}
                data-testid="loop-custom-every"
              />
            )}
            {form.every === 'custom' && (
              <p className={TYPE.meta}>{intl.formatMessage(w.customHelp)}</p>
            )}
            {fieldError('cadence') && (
              <p className={ERROR_TEXT} data-testid="loop-cadence-error">
                {fieldError('cadence')}
              </p>
            )}
          </div>
        )}
        {form.cadenceKind === 'self_paced' && (
          <p className={TYPE.meta}>{intl.formatMessage(w.selfPacedHelp)}</p>
        )}
        {form.cadenceKind === 'back_to_back' && (
          <p className={TYPE.meta}>{intl.formatMessage(w.backToBackHelp)}</p>
        )}
      </Section>

      <Section label={intl.formatMessage(w.stateFile)} testId="loop-state-file-section">
        <input
          className={cx(INPUT, 'w-full font-mono')}
          value={form.stateFile}
          onChange={(e) => set({ stateFile: e.target.value, stateFileTouched: true })}
          aria-label={intl.formatMessage(w.stateFile)}
          aria-invalid={fieldError('stateFile') !== null}
          data-testid="loop-state-file"
        />
        <p className={TYPE.meta}>{intl.formatMessage(w.stateFileHelp)}</p>
        {fieldError('stateFile') && (
          <p className={ERROR_TEXT} data-testid="loop-state-file-error">
            {fieldError('stateFile')}
          </p>
        )}
      </Section>

      <Section label={intl.formatMessage(w.stopWhen)} testId="loop-stop-section">
        <ul className="flex flex-col gap-1.5">
          <StopRule>{intl.formatMessage(hasCheck ? w.stopCheck : w.stopDone)}</StopRule>
          <StopRule>{intl.formatMessage(w.stopYou)}</StopRule>
          <li className={cx('flex flex-wrap items-center', TYPE.body)}>
            <span
              aria-hidden
              className="mr-2 inline-block size-1.5 shrink-0 rounded-full bg-lz-ink-3"
            />
            {intl.formatMessage(w.stopAfter, { field: stopAfterField })}
          </li>
        </ul>
        <p className={TYPE.meta}>{intl.formatMessage(w.stopAfterHelp)}</p>
        {fieldError('stopAfter') && (
          <p className={ERROR_TEXT} data-testid="loop-stop-after-error">
            {fieldError('stopAfter')}
          </p>
        )}
      </Section>
    </>
  );
}

function StopRule({ children }: { children: ReactNode }) {
  return (
    <li className={cx('flex items-center', TYPE.body)}>
      <span aria-hidden className="mr-2 inline-block size-1.5 shrink-0 rounded-full bg-lz-accent" />
      {children}
    </li>
  );
}

function ReplaceDialog({
  open,
  loop,
  unreadable,
  onKeep,
  onReplace,
}: {
  open: boolean;
  loop: LoopRecord | null;
  unreadable: boolean;
  onKeep: () => void;
  onReplace: () => void;
}) {
  const intl = useIntl();
  const last = loop ? lastTick(loop) : undefined;
  return (
    <OverlayDialog
      open={open}
      onClose={onKeep}
      panelClassName={cx('flex w-[26rem] flex-col gap-4 p-5', SURFACE.overlay)}
    >
      <div className="flex flex-col gap-2" data-testid="loop-replace-dialog">
        <OverlayDialogTitle asChild>
          <h2 className={TYPE.h2}>{intl.formatMessage(w.replaceTitle)}</h2>
        </OverlayDialogTitle>
        <p className={TYPE.body}>
          {unreadable
            ? intl.formatMessage(w.replaceBodyUnreadable)
            : last
              ? intl.formatMessage(w.replaceBody, { n: last.n })
              : intl.formatMessage(w.replaceBodyNoTick)}
        </p>
      </div>
      <div className="flex flex-wrap items-center justify-end gap-2">
        <Button variant="ghost" onClick={onKeep} data-testid="loop-replace-keep">
          {intl.formatMessage(w.replaceKeep)}
        </Button>
        <Button variant="destructive" onClick={onReplace} data-testid="loop-replace">
          {intl.formatMessage(w.replace)}
        </Button>
      </div>
    </OverlayDialog>
  );
}
