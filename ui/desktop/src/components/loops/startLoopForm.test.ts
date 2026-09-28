import { describe, expect, it } from 'vitest';
import raw from '../../../../../crates/goose/src/session_loops/loops.fixture.json';
import { validateLoop, type LoopRecord, type LoopTickRecord } from './model';
import { loopRecord, waitingRecord } from './railFixtures';
import {
  EVERY_PRESETS,
  formCadence,
  formEdit,
  formFromLoop,
  formFromTemplate,
  nextTickLastStep,
  parseStopAfter,
  refusalField,
  stepSegments,
  withGoal,
  withTemplate,
} from './startLoopForm';
import { TEMPLATES } from './startLoopFixtures';

const [QUALITY, UNTIL_CHECK, WATCH, BLANK] = TEMPLATES;

describe('the Start dialog form', () => {
  it('opens a template with its steps, its suggested cadence and a state file from the goal', () => {
    const form = formFromTemplate(QUALITY, 'Make every test in ui/desktop pass');
    expect(form).toMatchObject({
      template: 'quality',
      steps: QUALITY.steps,
      cadenceKind: 'every',
      every: '10m',
      stateFile: '.goose/loops/make-every-test-in/NOW.md',
      stateFileTouched: false,
      check: '',
      stopAfter: '',
    });
    expect(formFromTemplate(UNTIL_CHECK).cadenceKind).toBe('back_to_back');
    expect(formFromTemplate(BLANK)).toMatchObject({ cadenceKind: 'self_paced', steps: '' });
    expect(formFromTemplate(WATCH)).toMatchObject({ cadenceKind: 'every', every: '30m' });
  });

  it('every preset and a custom value are strings of the one grammar, never seconds', () => {
    const form = formFromTemplate(QUALITY, 'g');
    expect(EVERY_PRESETS).toEqual(['5m', '10m', '30m', '1h']);
    for (const every of EVERY_PRESETS) {
      expect(formCadence({ ...form, every })).toEqual({ kind: 'every', every });
    }
    expect(formCadence({ ...form, every: 'custom', customEvery: '90m' })).toEqual({
      kind: 'every',
      every: '90m',
    });
    expect(formCadence({ ...form, cadenceKind: 'self_paced' })).toEqual({ kind: 'self_paced' });
    expect(formCadence({ ...form, cadenceKind: 'back_to_back' })).toEqual({
      kind: 'back_to_back',
    });
  });

  it('a state file follows the goal until the person types one', () => {
    const form = formFromTemplate(QUALITY, '');
    expect(form.stateFile).toBe('.goose/loops/loop/NOW.md');
    const typed = withGoal(form, 'Watch the nightly build');
    expect(typed.stateFile).toBe('.goose/loops/watch-the-nightly-build/NOW.md');
    const own = withGoal({ ...typed, stateFile: 'NOW.md', stateFileTouched: true }, 'Other goal');
    expect(own.stateFile).toBe('NOW.md');
  });

  it('a template switch takes its steps and cadence, and keeps the goal, check and file', () => {
    const form = { ...formFromTemplate(QUALITY, 'Keep it green'), check: 'pnpm test' };
    const switched = withTemplate(form, UNTIL_CHECK);
    expect(switched).toMatchObject({
      template: 'until_check',
      steps: UNTIL_CHECK.steps,
      cadenceKind: 'back_to_back',
      goal: 'Keep it green',
      check: 'pnpm test',
      stateFile: form.stateFile,
    });
  });

  it('opens an existing loop with every field it holds (Edit, Start a new loop)', () => {
    const loop = loopRecord({ stopAfterTicks: 12, cadence: { kind: 'every', every: '45m' } });
    expect(formFromLoop(loop)).toMatchObject({
      template: 'quality',
      goal: loop.goal,
      steps: '1. Discover…',
      check: loop.check,
      cadenceKind: 'every',
      every: 'custom',
      customEvery: '45m',
      stateFile: loop.stateFile,
      stateFileTouched: true,
      stopAfter: '12',
    });
  });

  it('the edit carries the check only when there is one, and a tick count only when typed', () => {
    const form = formFromTemplate(QUALITY, 'Keep it green');
    expect(formEdit(form)).toEqual({
      goal: 'Keep it green',
      template: 'quality',
      steps: QUALITY.steps,
      cadence: { kind: 'every', every: '10m' },
      stateFile: '.goose/loops/keep-it-green/NOW.md',
      check: null,
    });
    expect(formEdit({ ...form, check: 'pnpm test', stopAfter: '5' })).toMatchObject({
      check: 'pnpm test',
      stopAfterTicks: 5,
    });
    expect(parseStopAfter('')).toEqual({ kind: 'none' });
    expect(parseStopAfter(' 3 ')).toEqual({ kind: 'count', n: 3 });
    expect(parseStopAfter('three')).toEqual({ kind: 'invalid' });
    expect(parseStopAfter('2.5')).toEqual({ kind: 'invalid' });
  });

  it('every refusal validateLoop names lands under its own field', () => {
    const chat = { workingDir: '/w' };
    const cases: [ReturnType<typeof formEdit>, string][] = [
      [formEdit({ ...formFromTemplate(QUALITY, ''), stateFile: 'NOW.md' }), 'goal'],
      [formEdit(formFromTemplate(UNTIL_CHECK, 'go')), 'check'],
      [
        formEdit({ ...formFromTemplate(QUALITY, 'go'), every: 'custom', customEvery: '10' }),
        'cadence',
      ],
      [formEdit({ ...formFromTemplate(QUALITY, 'go'), stateFile: '../NOW.md' }), 'stateFile'],
      [formEdit({ ...formFromTemplate(QUALITY, 'go'), steps: 'Open {foo}' }), 'steps'],
      [formEdit({ ...formFromTemplate(QUALITY, 'go'), stopAfter: '0' }), 'stopAfter'],
    ];
    for (const [edit, field] of cases) {
      const verdict = validateLoop(edit, chat);
      expect('refusal' in verdict, field).toBe(true);
      if ('refusal' in verdict) expect(refusalField(verdict.refusal.code), field).toBe(field);
    }
    const swarm = validateLoop(formEdit(formFromTemplate(QUALITY, 'go')), {
      workingDir: '/w',
      swarmBuild: true,
    });
    expect('refusal' in swarm && refusalField(swarm.refusal.code)).toBe('form');
  });

  it('renders each slot as the fact goose will read, and flags a slot goose does not know', () => {
    const segments = stepSegments(
      'Open {state_file} in {working_dir}; run {check}; {foo}. {goal_first_line} ({last_next_step})',
      {
        stateFile: '.goose/loops/x/NOW.md',
        check: '',
        goal: 'Ship it\nsecond line',
        workingDir: '/w',
        lastNextStep: { kind: 'first' },
      }
    );
    expect(segments).toEqual([
      { kind: 'text', text: 'Open ' },
      { kind: 'slot', name: 'state_file', value: '.goose/loops/x/NOW.md' },
      { kind: 'text', text: ' in ' },
      { kind: 'slot', name: 'working_dir', value: '/w' },
      { kind: 'text', text: '; run ' },
      {
        kind: 'slot',
        name: 'check',
        value: 'no check command is set; run the command that shows the change works and quote it',
      },
      { kind: 'text', text: '; ' },
      { kind: 'unknown', name: 'foo' },
      { kind: 'text', text: '. ' },
      { kind: 'slot', name: 'goal_first_line', value: 'Ship it' },
      { kind: 'text', text: ' (' },
      { kind: 'slot', name: 'last_next_step', value: 'this is the first tick' },
      { kind: 'text', text: ')' },
    ]);
  });

  it('{last_next_step} at the next tick is what the last tick named, or says it named none', () => {
    expect(nextTickLastStep(null)).toEqual({ kind: 'first' });
    expect(nextTickLastStep(waitingRecord())).toEqual({
      kind: 'named',
      text: 'add dormant admins',
    });
    expect(nextTickLastStep(loopRecord())).toEqual({ kind: 'named_none', prev: 5 });
  });

  it('no template splices the goal into a step, whatever the goal says (Q-280)', () => {
    const goals = [
      'Count the files in /Users/mihai/loopwork with ls and report the count. Change nothing.',
      'Watch the nightly build at https://ci.example.com/job/nightly and fix it when it goes red',
      'make scripts/generate_users.js produce every problem class in notes/kickoff.md',
    ];
    for (const template of TEMPLATES) {
      for (const goal of goals) {
        const segments = stepSegments(template.steps, {
          stateFile: '.goose/loops/x/NOW.md',
          check: 'pnpm test',
          goal,
          workingDir: '/w',
          lastNextStep: { kind: 'first' },
        });
        const text = segments
          .map((s) => (s.kind === 'unknown' ? s.name : 'value' in s ? s.value : s.text))
          .join('');
        expect(text, `${template.id}: ${goal}`).not.toContain(goal);
        expect(segments.some((s) => s.kind === 'slot' && s.name === 'goal_first_line')).toBe(false);
      }
    }
  });

  it("{last_next_step} after a yield is the last FINISHED tick's step, as goosed words it (Q-278)", () => {
    const cases = (
      raw as unknown as {
        lastNextStep: { name: string; ticks: LoopTickRecord[]; expect: unknown }[];
      }
    ).lastNextStep;
    expect(cases.length).toBeGreaterThanOrEqual(8);
    for (const c of cases) {
      const record = { ...loopRecord(), ticks: c.ticks } as LoopRecord;
      expect(nextTickLastStep(record), c.name).toEqual(c.expect);
    }
  });
});
