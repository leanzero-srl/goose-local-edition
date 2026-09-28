/**
 * A loop chat as the rail sees it — the record and the transcript it points into — for the rail's
 * tests and the fixture screenshots (DESIGN-SESSION-LOOPS §8.4, §10.2). Built from the Changes
 * fixtures, so a tick's files come through the one diff model.
 */
import type { Message } from '../../types/message';
import { diffResponse, editRequest, message, unified } from '../changes/fixtures';
import { tickId, type LoopRecord, type LoopTickRecord } from './model';

export const LOOP_ID = 'lp_0a1b2c3d';
export const WORKING_DIR = '/w';
export const STATE_FILE = '.goose/loops/users-csv/NOW.md';
export const CHECK = 'node scripts/validate_users.js';

export function markerId(n: number): string {
  return tickId(LOOP_ID, n, `${String(n).repeat(8)}aaaabbbbccccdddd`.slice(0, 32));
}

function marker(n: number, created: number): Message {
  const id = markerId(n);
  return {
    id,
    role: 'user',
    created,
    content: [
      {
        type: 'text',
        text: `Loop tick ${n} — "Make scripts/generate_users.js produce every problem class" · every 10 min\nState file: ${STATE_FILE} — read it before anything else; rewrite it before you call loop_report.\nFinish by calling loop_report; calling it ends this tick.`,
      },
    ],
    metadata: {
      userVisible: true,
      agentVisible: true,
      loopTick: { loopId: LOOP_ID, n, messageId: id },
    },
  };
}

function write(callId: string, path: string, added: number, removed: number): Message[] {
  const body = [
    `@@ -1,${removed} +1,${added} @@`,
    ...Array.from({ length: removed }, (_, i) => `-old ${i + 1}`),
    ...Array.from({ length: added }, (_, i) => `+new ${i + 1}`),
  ].join('\n');
  return [
    message('assistant', [editRequest(callId, path)]),
    message('user', [
      diffResponse(callId, { path, unified: unified(path, `${body}\n`), added, removed }),
    ]),
  ];
}

function said(text: string): Message {
  return message('assistant', [{ type: 'text', text }]);
}

const T0 = Date.parse('2026-09-27T22:01:00Z') / 1000;

/** The transcript: five tick markers, each followed by that tick's work. */
export const LOOP_MESSAGES: Message[] = [
  message('user', [
    { type: 'text', text: 'Make scripts/generate_users.js produce every problem class' },
  ]),
  said('I will set up a loop for this.'),
  marker(1, T0),
  ...write('t1a', '/w/scripts/generate_users.js', 38, 7),
  ...write('t1b', `/w/${STATE_FILE}`, 3, 0),
  said('Read kickoff.md; listed 6 problem classes.'),
  marker(2, T0 + 600),
  said('Provider error: stream ended early'),
  marker(3, T0 + 1200),
  ...write('t3a', '/w/scripts/generate_users.js', 12, 2),
  ...write('t3b', `/w/${STATE_FILE}`, 2, 1),
  said('Added case-only duplicate emails.'),
  marker(4, T0 + 1800),
  ...write('t4a', `/w/${STATE_FILE}`, 1, 1),
  said('Nothing to change this time; the next step stays the same.'),
  marker(5, T0 + 2400),
  said('Working on svc- accounts…'),
];

const at = (offsetSeconds: number) =>
  new Date((T0 + offsetSeconds) * 1000).toISOString().replace(/\.\d{3}Z$/, 'Z');

export const TICKS: LoopTickRecord[] = [
  {
    n: 1,
    origin: 'first',
    startedAt: at(0),
    endedAt: at(450),
    firstMessageId: markerId(1),
    wrote: ['scripts/generate_users.js'],
    report: {
      verdict: 'progress',
      summary: 'Read kickoff.md; listed 6 problem classes and scaffolded the generator.',
      nextStep: 'add case-only duplicate emails',
    },
    outcome: { kind: 'progress' },
    check: {
      command: CHECK,
      startedAt: at(450),
      endedAt: at(452),
      ran: true,
      exit: 1,
      outputTail: 'checking 400 rows\nmissing class: duplicate emails',
      logPath: '/data/loops/lp_0a1b2c3d/check-1.log',
    },
    tokens: { input: 14000, output: 2100, total: 16100 },
    served: { node: '27B · both Macs', rank: 1, atMs: (T0 + 1) * 1000 },
  },
  {
    n: 2,
    origin: 'cadence',
    startedAt: at(600),
    endedAt: at(842),
    firstMessageId: markerId(2),
    wrote: [],
    outcome: {
      kind: 'failed',
      errorClass: 'provider',
      error: 'Provider error: stream ended early',
    },
  },
  {
    n: 3,
    origin: 'cadence',
    startedAt: at(1200),
    endedAt: at(1572),
    firstMessageId: markerId(3),
    wrote: ['scripts/generate_users.js'],
    report: {
      verdict: 'progress',
      summary: 'Added case-only duplicate emails and 30 no-email rows; the generator now seeds 42.',
      nextStep: 'add svc- service accounts with no last_login',
    },
    outcome: { kind: 'progress' },
    check: {
      command: CHECK,
      startedAt: at(1572),
      endedAt: at(1574),
      ran: true,
      exit: 1,
      outputTail: 'checking 400 rows\nmissing svc- accounts',
    },
    tokens: { input: 21000, output: 3300, total: 24300 },
  },
  {
    n: 4,
    origin: 'cadence',
    startedAt: at(1800),
    endedAt: at(1990),
    firstMessageId: markerId(4),
    wrote: [],
    report: {
      verdict: 'progress',
      summary: 'Re-read the generator; nothing else was broken.',
      nextStep: 'check the svc- account format against kickoff.md',
    },
    outcome: { kind: 'progress' },
    check: {
      command: CHECK,
      startedAt: at(1990),
      endedAt: at(1992),
      ran: true,
      exit: 1,
      outputTail: 'missing svc- accounts',
    },
  },
  {
    n: 5,
    origin: 'cadence',
    startedAt: at(2400),
    firstMessageId: markerId(5),
    wrote: [],
  },
];

/** The loop mid-tick 5, every 10 min, with the four ended ticks above. */
export function loopRecord(patch: Partial<LoopRecord> = {}): LoopRecord {
  return {
    id: LOOP_ID,
    goal: 'Make scripts/generate_users.js produce every problem class in notes/kickoff.md\nKeep the seed at 42.',
    template: 'quality',
    steps: '1. Discover…',
    cadence: { kind: 'every', every: '10m' },
    stateFile: STATE_FILE,
    check: CHECK,
    status: 'running',
    createdAt: at(-60),
    startedAt: at(-60),
    ticks: TICKS,
    ...patch,
  };
}

/** The same loop between ticks: tick 5 ended, the next one due at +3000 s. */
export function waitingRecord(patch: Partial<LoopRecord> = {}): LoopRecord {
  const ended: LoopTickRecord = {
    ...TICKS[4],
    endedAt: at(2700),
    report: {
      verdict: 'progress',
      summary: 'Added 12 svc- accounts with no last_login.',
      nextStep: 'add dormant admins',
    },
    outcome: { kind: 'progress' },
    wrote: ['scripts/generate_users.js'],
  };
  return loopRecord({
    status: 'waiting',
    nextTick: { at: at(3000), reason: { kind: 'cadence' } },
    ticks: [...TICKS.slice(0, 4), ended],
    ...patch,
  });
}

/** A moment during tick 5 (2 minutes in). */
export const NOW_MS = (T0 + 2520) * 1000;
