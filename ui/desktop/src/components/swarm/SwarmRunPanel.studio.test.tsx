import { cleanup, fireEvent, render } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import SwarmRunPanel from './SwarmRunPanel';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { allClasses, assertStudioClean } from '../lz/assertStudioClean';
import { missingUtilities } from '../lz/compileStudioCss';

/**
 * EVERY CLASS THE PANEL EMITS COMPILES, AND NOTHING BANNED RENDERS. The residue polish moved the
 * sub-components (call rows, lane rows, the confidence surfaces, the checklist, the board rows, the
 * clarify prompt) onto the Studio utilities; a class that compiles to nothing is a silent no-op, so this
 * fixture renders a run rich enough to mount all of them — a done lane with calls and an app-error, a
 * failed lane with its "Why it failed", a running lane, a pending ask with its confidence breakdown —
 * expands everything it can reach, and measures the union of classes against the real pipeline.
 */

const POOL = [
  { id: 'mac-gabee-qwen3.6-27b', model_id: 'gabee-qwen3.6-27b', weight: 2 },
  { id: 'local-mihai-qwen3.6-27b', model_id: 'mihai-qwen3.6-27b', weight: 2 },
  { id: 'worksmacstudio-workhorse-qwen3.6-27b', model_id: 'workhorse-qwen3.6-27b', weight: 2 },
];

const BREAKDOWN = {
  final: 73,
  agreement: 88,
  agreement_reason: '3 drafts agree: count spread 1, file-overlap 100% (role-normalized)',
  spec_clarity: 73,
  spec_clarity_reason: 'the storage backend and the export format are undecided',
  product_specified: true,
  open_decisions: ['storage backend', 'export format'],
};

const EVENTS: Array<Record<string, unknown>> = [
  {
    event: 'run_started',
    prompt: '# Build `vendorsync`\n\nA small operations tool.',
    pool: POOL,
    ts: '2026-08-17T13:54:13.000000+00:00',
  },
  { event: 'pool_resolved', devices: POOL, worker_count: 3, ts: '2026-08-17T13:54:14+00:00' },
  { event: 'phase', phase: 'open' },
  { event: 'slices_opened', count: 2, weights: [3, 2], slices: ['store', 'api'], secs: 41 },
  { event: 'phase', phase: 'synthesis' },
  {
    event: 'plan_loaded',
    task_count: 3,
    plan_confidence: 73,
    ask_floor: 80,
    plan_confidence_breakdown: BREAKDOWN,
    tasks: [
      {
        id: 'store',
        description: 'Build the store',
        files: ['store.py'],
        deps: [],
        difficulty: 'medium',
      },
      {
        id: 'api',
        description: 'Build the api',
        files: ['api.py'],
        deps: ['store'],
        difficulty: 'hard',
      },
      {
        id: 'integrate-verify',
        description: 'Sink',
        files: [],
        deps: ['store', 'api'],
        difficulty: 'hard',
      },
    ],
  },
  { event: 'low_confidence_ask', questions: [{ question: 'Which storage backend?' }] },
  { event: 'phase', phase: 'build', ts: '2026-08-17T14:02:00+00:00' },
  {
    event: 'task_dispatched',
    task_id: 'store',
    device: POOL[0].id,
    model: POOL[0].model_id,
    ts: '2026-08-17T14:02:01+00:00',
  },
  { event: 'task_dispatched', task_id: 'api', device: POOL[1].id, model: POOL[1].model_id },
  {
    event: 'task_completed',
    task_id: 'store',
    status: 'done',
    device: POOL[0].id,
    attempts: 1,
    elapsed_ms: 155142,
    tool_calls: [
      {
        name: 'shell',
        summary: 'pytest -q',
        ok: false,
        result: 'FAILED tests/test_store.py::test_add - AssertionError',
      },
      { name: 'text_editor', summary: 'write /tmp/build/store.py', ok: true, result: '' },
    ],
    ts: '2026-08-17T14:04:37+00:00',
  },
  {
    event: 'task_completed',
    task_id: 'api',
    status: 'failed',
    device: POOL[1].id,
    attempts: 2,
    elapsed_ms: 90000,
    error: 'ModuleNotFoundError: No module named store',
    tool_calls: [],
    ts: '2026-08-17T14:06:00+00:00',
  },
  {
    event: 'task_dispatched',
    task_id: 'integrate-verify',
    device: POOL[2].id,
    model: POOL[2].model_id,
  },
];

const CLARIFY = {
  pending: true,
  questions: [
    {
      question: 'Which storage backend should the ledger use?',
      options: ['sqlite', 'postgres'],
      resolves: 'storage backend',
    },
    { question: 'Ship a CLI alongside the web UI?', options: [] },
  ],
  planConfidence: 73,
  confidence: {
    final: 73,
    agreement: 88,
    agreementReason: BREAKDOWN.agreement_reason,
    specClarity: 73,
    specClarityReason: BREAKDOWN.spec_clarity_reason,
    productSpecified: true,
    openDecisions: BREAKDOWN.open_decisions,
  },
  answerPath: '/tmp/build/.swarm/answers.json',
};

type ElectronMock = Record<string, unknown>;
const electron = () => (window as unknown as { electron: ElectronMock }).electron;

/** lucide stamps `lucide lucide-<name>` identifiers on its svgs — names, not utilities. */
const utilitiesOf = (classes: string[]) => classes.filter((c) => !c.startsWith('lucide'));

describe('SwarmRunPanel — every rendered class compiles and nothing banned renders', () => {
  beforeEach(() => {
    const e = electron();
    e.readSwarmRun = vi.fn(async () => ({
      runId: 'swarm-studio',
      dir: '/tmp/build',
      events: EVENTS,
      activity: {
        store: {
          model: POOL[0].model_id,
          tool_calls: 2,
          thinking_chars: 1200,
          errors: 1,
          last_text: 'the store owns the ledger file',
          calls: [
            {
              name: 'shell',
              summary: 'pytest -q',
              ok: false,
              result: 'FAILED tests/test_store.py::test_add',
            },
            { name: 'text_editor', summary: 'write /tmp/build/store.py', ok: true, result: '' },
          ],
        },
        api: {
          model: POOL[1].model_id,
          tool_calls: 0,
          last_text: 'wiring the routes',
          error: 'ModuleNotFoundError: No module named store',
        },
        'integrate-verify': {
          model: POOL[2].model_id,
          last_text: 'booting the app',
          tool_calls: 1,
          calls: [{ name: 'shell', summary: 'python -m app', ok: null }],
        },
      },
      activityMtimes: { store: Date.now(), api: Date.now(), 'integrate-verify': Date.now() },
      clarify: CLARIFY,
      mtime: Date.now(),
      heartbeat: Date.now(),
      heartbeatExited: false,
      pauseRequested: false,
    }));
    e.fleetStatus = vi.fn(async () => ({}));
    e.swarmSetPaused = vi.fn(async () => true);
    e.swarmAddNote = vi.fn(async () => true);
    e.revealInFinder = vi.fn(async () => undefined);
    e.writeFile = vi.fn(async () => true);
  });
  afterEach(() => cleanup());

  it('compiles every class it emits, expanded as far as a click can take it', async () => {
    const { container, findByText, findAllByRole } = render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" />
      </IntlTestWrapper>
    );
    await findByText('Swarm run');
    await findByText('Work');
    await findAllByRole('button');
    const seen = new Set<string>(allClasses(container));
    assertStudioClean(container);
    // Expand: every toggle (zone headers, lane rows, board rows, call rows, plan drawer) — collect the
    // classes after each sweep so a row that closes on the next click is still measured.
    for (let sweep = 0; sweep < 2; sweep++) {
      const toggles = Array.from(
        container.querySelectorAll<HTMLElement>('button, [role="button"]')
      ).filter((el) => !/send|pause|resume|copy|reveal/i.test(el.textContent ?? ''));
      for (const el of toggles) fireEvent.click(el);
      for (const c of allClasses(container)) seen.add(c);
      assertStudioClean(container);
    }
    expect(seen.size).toBeGreaterThan(80);
    const missing = await missingUtilities(utilitiesOf([...seen]));
    expect(missing).toEqual([]);
  });
});
