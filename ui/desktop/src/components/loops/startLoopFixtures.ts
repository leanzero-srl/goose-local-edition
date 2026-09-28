/**
 * The four templates as goosed serves them (`session_loops/templates.rs`, `loops/templates`), for the
 * Start dialog's tests and fixture screenshots. The dialog itself never carries template text: it
 * reads it from goosed every time it opens.
 */
import type { LoopTemplateDto } from '@aaif/goose-sdk';

export const TEMPLATES: LoopTemplateDto[] = [
  {
    id: 'quality',
    name: 'Software quality loop',
    description:
      "Discover what's broken, pick the one thing that matters most, fix it, prove it with a check. Repeat.",
    steps:
      '1. Discover: open {state_file}, then run or read what your goal points at in {working_dir}. List what is broken, missing or confusing, each with the evidence you saw (command output, file:line).\n2. Critique: rank what you found by how much it blocks the goal; pick the ONE item that matters most ({last_next_step}).\n3. Fix: make that change, and only that change.\n4. Prove it. Check to run: {check}. A fix without a quoted result is not done.\n5. Rewrite {state_file}: what is now true, what is next, what you found but did not fix.',
    slots: ['state_file', 'working_dir', 'last_next_step', 'check'],
    suggestedCadence: { kind: 'every', every: '10m' },
    needsCheck: false,
  },
  {
    id: 'until_check',
    name: 'Until a check passes',
    description: 'Keep working until a command you name succeeds — tests, a build, a lint.',
    steps:
      '1. Run the check — {check} — and read why it fails.\n2. Fix the first cause it names.\n3. Run the check again — {check} — and quote the result.\n4. Rewrite {state_file}.',
    slots: ['check', 'state_file'],
    suggestedCadence: { kind: 'back_to_back' },
    needsCheck: true,
  },
  {
    id: 'watch',
    name: 'Watch and act',
    description:
      'Look at something on a schedule and act when it changes — a build, a deploy, a folder.',
    steps:
      '1. Look at what your goal watches (a build, a deploy, a folder, a URL) and compare it with {state_file}.\n2. If nothing changed, say so in one line and report progress.\n3. If something changed, do what the goal asks and quote the evidence.\n4. Rewrite {state_file}.',
    slots: ['state_file'],
    suggestedCadence: { kind: 'every', every: '30m' },
    needsCheck: false,
  },
  {
    id: 'blank',
    name: 'Blank',
    description: 'Your goal, your steps.',
    steps: '',
    slots: [],
    suggestedCadence: { kind: 'self_paced' },
    needsCheck: false,
  },
];
