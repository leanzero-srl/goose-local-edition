/**
 * The Start dialog's form (DESIGN-SESSION-LOOPS §8.2), as pure functions: what it opens with (a
 * template, a message's words, the loop being edited or an ended one), what a template switch
 * changes, the cadence presets as strings of the ONE cadence grammar, the steps as goose will read
 * them (slots shown as the facts they are), and the edit the dialog sends. Validation is
 * `model.ts`'s `validateLoop` — the same rules goosed applies — never a second copy here.
 */
import type { LoopTemplateDto } from '@aaif/goose-sdk';
import {
  SLOTS,
  defaultStateFile,
  goalFirstLine,
  renderSteps,
  type LastNextStep,
  type LoopCadence,
  type LoopEdit,
  type LoopRecord,
  type LoopRefusalCode,
  type LoopTemplateId,
} from './model';

/** The presets of "Every" — strings in the one grammar (`<n>s|m|h`), never seconds. */
export const EVERY_PRESETS = ['5m', '10m', '30m', '1h'] as const;
export type EveryPreset = (typeof EVERY_PRESETS)[number];

/** "Every" opens on 10m when the template suggests another kind of cadence. */
const EVERY_DEFAULT: EveryPreset = '10m';

export const TEMPLATE_IDS: readonly LoopTemplateId[] = ['quality', 'until_check', 'watch', 'blank'];

export interface LoopForm {
  template: LoopTemplateId;
  goal: string;
  steps: string;
  check: string;
  cadenceKind: LoopCadence['kind'];
  /** The preset picked, or `custom` with the text typed in `customEvery`. */
  every: EveryPreset | 'custom';
  customEvery: string;
  stateFile: string;
  /** The person typed a state file: a goal change no longer rewrites it. */
  stateFileTouched: boolean;
  /** Empty = no tick count. */
  stopAfter: string;
}

function everyOf(text: string): Pick<LoopForm, 'every' | 'customEvery'> {
  const preset = EVERY_PRESETS.find((p) => p === text.trim());
  return preset ? { every: preset, customEvery: '' } : { every: 'custom', customEvery: text };
}

function cadenceFields(
  cadence: LoopCadence
): Pick<LoopForm, 'cadenceKind' | 'every' | 'customEvery'> {
  return cadence.kind === 'every'
    ? { cadenceKind: 'every', ...everyOf(cadence.every) }
    : { cadenceKind: cadence.kind, every: EVERY_DEFAULT, customEvery: '' };
}

/** A new loop from a template (the composer's Loop button, "Loop this" with the message's words). */
export function formFromTemplate(template: LoopTemplateDto, goal = ''): LoopForm {
  return {
    template: template.id,
    goal,
    steps: template.steps,
    check: '',
    ...cadenceFields(template.suggestedCadence),
    stateFile: defaultStateFile(goal),
    stateFileTouched: false,
    stopAfter: '',
  };
}

/** Every field as the loop holds it (Edit, and "Start a new loop" from an ended one). */
export function formFromLoop(loop: LoopRecord): LoopForm {
  return {
    template: loop.template,
    goal: loop.goal,
    steps: loop.steps ?? '',
    check: loop.check ?? '',
    ...cadenceFields(loop.cadence),
    stateFile: loop.stateFile,
    stateFileTouched: true,
    stopAfter: loop.stopAfterTicks != null ? String(loop.stopAfterTicks) : '',
  };
}

/** Switching template takes its steps and its suggested cadence; the goal, check and file stay. */
export function withTemplate(form: LoopForm, template: LoopTemplateDto): LoopForm {
  return {
    ...form,
    template: template.id,
    steps: template.steps,
    ...cadenceFields(template.suggestedCadence),
  };
}

/** The goal changed: an untouched state file follows it. */
export function withGoal(form: LoopForm, goal: string): LoopForm {
  return form.stateFileTouched
    ? { ...form, goal }
    : { ...form, goal, stateFile: defaultStateFile(goal) };
}

export function formCadence(form: LoopForm): LoopCadence {
  switch (form.cadenceKind) {
    case 'every':
      return { kind: 'every', every: form.every === 'custom' ? form.customEvery : form.every };
    case 'self_paced':
      return { kind: 'self_paced' };
    case 'back_to_back':
      return { kind: 'back_to_back' };
  }
}

/** The tick count as typed: none, a count, or words that are not one. */
export function parseStopAfter(
  text: string
): { kind: 'none' } | { kind: 'count'; n: number } | { kind: 'invalid' } {
  const t = text.trim();
  if (!t) return { kind: 'none' };
  if (!/^\d+$/.test(t)) return { kind: 'invalid' };
  const n = Number(t);
  return Number.isSafeInteger(n) ? { kind: 'count', n } : { kind: 'invalid' };
}

/** The edit the dialog sends — `validateLoop` decides whether it may. */
export function formEdit(form: LoopForm): LoopEdit {
  const stop = parseStopAfter(form.stopAfter);
  const edit: LoopEdit = {
    goal: form.goal,
    template: form.template,
    steps: form.steps,
    cadence: formCadence(form),
    stateFile: form.stateFile,
    check: form.check.trim() ? form.check : null,
  };
  if (stop.kind === 'count') edit.stopAfterTicks = stop.n;
  return edit;
}

/** Which field a refusal belongs under. */
export type FormField = 'goal' | 'steps' | 'check' | 'cadence' | 'stateFile' | 'stopAfter' | 'form';

export function refusalField(code: LoopRefusalCode): FormField {
  switch (code) {
    case 'empty_goal':
      return 'goal';
    case 'unknown_slot':
      return 'steps';
    case 'check_required':
      return 'check';
    case 'bad_cadence':
      return 'cadence';
    case 'state_file_outside':
    case 'empty_state_file':
      return 'stateFile';
    case 'bad_stop_after':
      return 'stopAfter';
    default:
      return 'form';
  }
}

/** The steps as goose reads them: plain text, and each slot as the fact it names. */
export type StepSegment =
  | { kind: 'text'; text: string }
  | { kind: 'slot'; name: string; value: string }
  | { kind: 'unknown'; name: string };

export interface StepPreviewFacts {
  stateFile: string;
  check: string;
  goal: string;
  workingDir: string;
  lastNextStep: LastNextStep;
}

const SLOT = /\{([a-z][a-z0-9_]*)\}/g;

/** A fact rendered by `renderSteps` without the quoting it carries in the prompt (`path`, "words"). */
function bare(text: string): string {
  const m = /^[`"]([\s\S]*)[`"]$/.exec(text);
  return m ? m[1] : text;
}

export function stepSegments(steps: string, facts: StepPreviewFacts): StepSegment[] {
  const segments: StepSegment[] = [];
  const renderFacts = {
    stateFile: facts.stateFile.trim(),
    check: facts.check.trim() ? facts.check.trim() : null,
    goalFirstLine: goalFirstLine(facts.goal),
    lastNextStep: facts.lastNextStep,
    workingDir: facts.workingDir,
  };
  let at = 0;
  for (const match of steps.matchAll(SLOT)) {
    const index = match.index ?? 0;
    if (index > at) segments.push({ kind: 'text', text: steps.slice(at, index) });
    const name = match[1];
    if ((SLOTS as readonly string[]).includes(name)) {
      segments.push({ kind: 'slot', name, value: bare(renderSteps(match[0], renderFacts).text) });
    } else {
      segments.push({ kind: 'unknown', name });
    }
    at = index + match[0].length;
  }
  if (at < steps.length) segments.push({ kind: 'text', text: steps.slice(at) });
  return segments;
}

/** What `{last_next_step}` says at the loop's next tick: its first, or what the last one named. */
export function nextTickLastStep(loop: LoopRecord | null): LastNextStep {
  const ticks = loop?.ticks ?? [];
  const last = ticks[ticks.length - 1];
  if (!last) return { kind: 'first' };
  const named = last.report?.nextStep?.trim();
  return named ? { kind: 'named', text: named } : { kind: 'named_none', prev: last.n };
}
