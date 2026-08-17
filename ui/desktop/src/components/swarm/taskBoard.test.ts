import { describe, it, expect } from 'vitest';
import {
  nodeLabeler,
  deriveTaskBoard,
  buildPhaseTodo,
  foldEvents,
  runAppName,
  boardTitle,
  ranNothing,
  classifyCall,
  type PlanTask,
} from './useSwarmRun';

// Pool shapes VERBATIM from a measured benchmark log (swarm-3node-r0, 2026-08-17) — including the
// truncated workhorse device id ending in a dash, whose last dash-segment is what the old labeler
// printed ("Fleet: 3 nodes — fusi, fusi, fable").
const POOL = [
  {
    id: 'mac-gabee-qwen3.6-27b-fable-fusi',
    model_id: 'gabee-qwen3.6-27b-fable-fusion-711-uncensored-heretic-nm-dau-neo-max-mtp',
    weight: 2,
  },
  {
    id: 'local-mihai-qwen3.6-27b-fable-fusi',
    model_id: 'mihai-qwen3.6-27b-fable-fusion-711-uncensored-heretic-nm-dau-neo-max-mtp',
    weight: 2,
  },
  {
    id: 'worksmacstudio-workhorse-qwen3.6-27b-fable-',
    model_id: 'workhorse-qwen3.6-27b-fable-fusion-711-uncensored-heretic-nm-dau-neo-max-mtp',
    weight: 2,
  },
];
const RUN_STARTED = { event: 'run_started', pool: POOL };
const POOL_RESOLVED = { event: 'pool_resolved', devices: POOL, worker_count: 3 };

describe('nodeLabeler — node NAMES, never truncated model-id fragments (the "fusi, fusi, fable" bug)', () => {
  const label = nodeLabeler([RUN_STARTED, POOL_RESOLVED]);

  it('maps a pool/device id to the node name', () => {
    expect(label('mac-gabee-qwen3.6-27b-fable-fusi')).toBe('gabee');
    expect(label('local-mihai-qwen3.6-27b-fable-fusi')).toBe('mihai');
  });

  it('maps the TRUNCATED trailing-dash device id (the one the old code read as "fable")', () => {
    expect(label('worksmacstudio-workhorse-qwen3.6-27b-fable-')).toBe('workhorse');
  });

  it('maps a model id too — the other spelling the engine uses for the same node', () => {
    expect(label('workhorse-qwen3.6-27b-fable-fusion-711-uncensored-heretic-nm-dau-neo-max-mtp')).toBe(
      'workhorse'
    );
  });

  it('falls back to the id prefix when no pool events exist (older log)', () => {
    const bare = nodeLabeler([]);
    expect(bare('gabee-qwen3.6-27b')).toBe('gabee');
  });
});

// A compact but complete run mid-build, in measured event shapes: one task done, one running, one queued
// behind deps, a per-task verify running on the third node, a repair twin, and a failed task.
const GABEE = POOL[0].model_id;
const EVENTS: Array<Record<string, unknown>> = [
  RUN_STARTED,
  POOL_RESOLVED,
  {
    event: 'plan_loaded',
    task_count: 4,
    plan_confidence: 88,
    ask_floor: 85,
    tasks: [
      { id: 'store', description: 'Build the store', files: ['store.py'], deps: [], difficulty: 'medium' },
      { id: 'api', description: 'Build the api', files: ['api.py'], deps: ['store'], difficulty: 'hard' },
      { id: 'cli', description: 'The CLI', files: ['cli.py'], deps: ['store', 'api'], difficulty: 'easy' },
      { id: 'web', description: 'The web UI', files: ['web/app.js'], deps: [], difficulty: 'hard' },
      { id: 'integrate-verify', description: 'Sink', files: [], deps: ['store', 'api', 'cli'], difficulty: 'hard' },
    ],
  },
  { event: 'task_dispatched', task_id: 'store', device: 'mac-gabee-qwen3.6-27b-fable-fusi', model: GABEE },
  {
    event: 'task_completed',
    task_id: 'store',
    status: 'done',
    device: 'mac-gabee-qwen3.6-27b-fable-fusi',
    model: GABEE,
    attempts: 1,
    elapsed_ms: 155142,
    tool_calls: [{ name: 'write', ok: true }],
  },
  { event: 'task_dispatched', task_id: 'web', device: 'local-mihai-qwen3.6-27b-fable-fusi' },
  {
    event: 'task_completed',
    task_id: 'web',
    status: 'failed',
    device: 'local-mihai-qwen3.6-27b-fable-fusi',
    attempts: 3,
    elapsed_ms: 900000,
    tool_calls: [],
  },
  { event: 'task_dispatched', task_id: 'api', device: 'local-mihai-qwen3.6-27b-fable-fusi' },
  {
    event: 'task_dispatched',
    task_id: 'verify::store',
    device: 'worksmacstudio-workhorse-qwen3.6-27b-fable-',
  },
  { event: 'replanned', added: ['extra-a', 'extra-b'] },
  { event: 'complete_fix_dispatched', round: 0, twin: 0, model: POOL[2].model_id, task_id: 'complete-fix::twin0' },
];
const PLAN: PlanTask[] = [
  { id: 'store', description: 'Build the store', files: ['store.py'], deps: [], difficulty: 'medium' },
  { id: 'api', description: 'Build the api', files: ['api.py'], deps: ['store'], difficulty: 'hard' },
  { id: 'cli', description: 'The CLI', files: ['cli.py'], deps: ['store', 'api'], difficulty: 'easy' },
  { id: 'web', description: 'The web UI', files: ['web/app.js'], deps: [], difficulty: 'hard' },
  { id: 'integrate-verify', description: 'Sink', files: [], deps: ['store', 'api', 'cli'], difficulty: 'hard' },
];

function board() {
  const { lanes, fixLanes } = foldEvents(EVENTS, {});
  const phaseTodo = buildPhaseTodo(EVENTS, {}, { clarifyPending: false });
  return deriveTaskBoard({ plan: PLAN, phaseTodo, lanes, fixLanes });
}

describe('deriveTaskBoard — ONE board: running / queued / done, engine-truth states', () => {
  it('groups running work: the build task, the per-task verify, and the repair twin', () => {
    const b = board();
    const ids = b.running.map((r) => r.id).sort();
    expect(ids).toEqual(['api', 'complete-fix::twin0', 'verify::store']);
    const kinds = Object.fromEntries(b.running.map((r) => [r.id, r.kind]));
    expect(kinds['api']).toBe('build');
    expect(kinds['verify::store']).toBe('verify');
    expect(kinds['complete-fix::twin0']).toBe('repair');
  });

  it('queued rows keep their deps — that is the visible plan', () => {
    const b = board();
    const cli = b.queued.find((r) => r.id === 'cli');
    expect(cli?.deps).toEqual(['store', 'api']);
    expect(cli?.difficulty).toBe('easy');
    // the sink + the e2e verdict row queue too — verification is planned work
    expect(b.queued.find((r) => r.id === 'integrate-verify')?.title).toBe('Integrate & verify');
  });

  it("a finished build task is 'unverified', NEVER green-done, and carries duration + node", () => {
    const b = board();
    const store = b.done.find((r) => r.id === 'store');
    expect(store?.state).toBe('unverified');
    expect(store?.elapsedMs).toBe(155142);
    expect(store?.device).toBe('gabee'); // canonical node name, from the lane join
  });

  it('a failed task lands in DONE, visibly distinct', () => {
    const b = board();
    const web = b.done.find((r) => r.id === 'web');
    expect(web?.state).toBe('failed');
    expect(web?.attempts).toBe(3);
  });

  it('rows carry their LANE so the tool-call card is the row expansion, not a parallel list', () => {
    const b = board();
    expect(b.running.find((r) => r.id === 'api')?.lane?.taskId).toBe('api');
  });

  it('replan bookkeeping is header data, not a fake task row', () => {
    const b = board();
    expect(b.addedByReplan).toBe(2);
    expect([...b.running, ...b.queued, ...b.done].some((r) => /replan/.test(r.id))).toBe(false);
  });

  it('a scheduler deadlock surfaces as `stuck`, never a silent row', () => {
    const evs = [...EVENTS, { event: 'scheduler_stuck', remaining: 2 }];
    const { lanes, fixLanes } = foldEvents(evs, {});
    const phaseTodo = buildPhaseTodo(evs, {}, { clarifyPending: false });
    const b = deriveTaskBoard({ plan: PLAN, phaseTodo, lanes, fixLanes });
    expect(b.stuck).toMatch(/Scheduler blocked/);
  });

  it('verify machinery is named for what it is', () => {
    expect(boardTitle('verify::api')).toBe('Verify api');
    expect(boardTitle('verify-e2e::0')).toBe('End-to-end verify 0');
    expect(boardTitle('complete-fix::twin2')).toBe('Repair twin 2');
    expect(boardTitle('integrate-verify')).toBe('Integrate & verify');
    expect(boardTitle('test-meridian')).toBe('Test meridian');
  });
});

describe('runAppName — the RUN HEADER identity', () => {
  it("reads the brief's first heading", () => {
    expect(runAppName('# Build `vendorsync`\n\nA small operations tool…', '/x/runs/r0')).toBe('vendorsync');
  });
  it('falls back to the run directory basename for a prose brief', () => {
    expect(runAppName('Make me a todo app with sqlite', '/x/runs/swarm-3node-r0')).toBe('swarm-3node-r0');
  });
  it("never renders empty — 'build' as the last resort", () => {
    expect(runAppName(undefined, undefined)).toBe('build');
  });
});

// VERBATIM from the live incident (2026-08-17): the worker ran
// `python3 -m pytest test_meridian.py -v 2>&1 | head -80`; the file did not exist, pytest printed this —
// and the `| head` pipe made the exit code 0, so the panel painted the call GREEN. Twice.
const PYTEST_RAN_NOTHING = `ERROR: file or directory not found: test_meridian.py

============================= test session starts ==============================
platform darwin -- Python 3.9.6, pytest-8.3.4, pluggy-1.5.0
collected 0 items

============================ no tests ran in 0.00s =============================`;

describe('ranNothing — exit 0 through a pipe must not paint green when the output proves nothing ran', () => {
  it('detects the verbatim measured pytest output', () => {
    expect(ranNothing(PYTEST_RAN_NOTHING)).toBe(true);
  });

  it('classifyCall downgrades the lying-green call to ran-nothing (solid amber, never plain ok)', () => {
    const m = classifyCall({
      name: 'shell',
      summary: 'python3 -m pytest test_meridian.py -v 2>&1 | head -80',
      ok: true,
      result: PYTEST_RAN_NOTHING,
    });
    expect(m.kind).toBe('ran-nothing');
    expect(m.outcome).toMatch(/nothing ran/);
  });

  it('a genuinely green test run stays ok', () => {
    const m = classifyCall({
      name: 'shell',
      summary: 'python3 -m pytest -q',
      ok: true,
      result: '12 passed in 1.24s',
    });
    expect(m.kind).toBe('ok');
  });

  it('catches the other ran-nothing signatures, but not "exited with code 0"', () => {
    expect(ranNothing('zsh: command not found: pnpm')).toBe(true);
    expect(ranNothing('cat: /tmp/x: No such file or directory')).toBe(true);
    expect(ranNothing('Command exited with code 2')).toBe(true);
    expect(ranNothing('Command exited with code 0')).toBe(false);
    expect(ranNothing('ok: all checks passed')).toBe(false);
  });
});
