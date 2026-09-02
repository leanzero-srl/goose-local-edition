import { render, cleanup } from '@testing-library/react';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import SwarmRunPanel from './SwarmRunPanel';
import { buildActivity } from './useSwarmRun';
import { IntlTestWrapper } from '../../i18n/test-utils';

/**
 * THE VERDICT SURFACE — measured on r5's finish (2026-08-30 20:27–20:30Z), Mihai watching:
 * "IF THE RUN PASSED then where in the UI is this being passed on or mentioned?" and, on the silent
 * 3.5-minute persona tail with "0 working" in the fleet, "once again it's locked in place".
 *
 * The fixture is r5's REAL terminal event sequence, values verbatim from
 * benchmark/runs/build/swarm-3node-r0/run.jsonl:
 *   complete_result{passed, verified, 7 known bugs} → [3.5 min reflect call] → persona_learned →
 *   run_overview{run_command verified} → run_finished{11 done, 0 failed}.
 *
 * Two claims under test, both event-driven (truth layer — never file presence):
 *   1. From complete_result until run_finished the TOP surface says the verdict is in and what the
 *      engine is still doing (the wrap-up banner) — dead air here reads as a hang, on the exact
 *      phase where r0 once genuinely hung.
 *   2. From run_finished on, the terminal banner states PASSED/FAILED unmistakably, with the
 *      engine's own passed_means honesty line, the known-bugs count (same array the Known active
 *      bugs section renders), and the engine-stamped run command.
 *
 * `passed_means` is NOT r5's archived line: it is the engine's LIVE wording (swarm.rs `complete_result`,
 * VA-087 — the render-class clause). The surface ships whatever the engine says, verbatim, so the fixture
 * pins the current string and the test fails the moment the engine's honesty line moves again. Under
 * that gate r5's bug #7 (a ReferenceError in the page) would flip `passed` to false — the fixture keeps
 * r5's shape because the claims under test are about the SURFACE, not the engine's partition.
 */
const LIVE_PASSED_MEANS =
  "the gate's criticals closed (engine_critical wording + render-probe criticals: no rows in a browser, an uncaught exception in the page's boot path) AND zero render-class findings among known_active_bugs; other minors ship as known_active_bugs";

const POOL = [
  { id: 'mac-gabee-qwen3.8-27b', model_id: 'gabee-qwen3.8-27b', weight: 2 },
  { id: 'local-mihai-qwen3.8-27b', model_id: 'mihai-qwen3.8-27b', weight: 2 },
  { id: 'worksmacstudio-workhorse-qwen3.8-27b', model_id: 'workhorse-qwen3.8-27b', weight: 2 },
];

// r5's seven known_active_bugs, verbatim.
const R5_BUGS = [
  "POST /api/payments/<id>/note's response does not carry the documented field(s) `id`, `note`, `version` — the spec's endpoint table names them for exactly this endpoint. Return them from this handler; without them the endpoint's contract cannot be verified by anyone, including this gate.",
  "POST /api/webhooks/meridian's response could not be read as JSON on either probe — the spec documents a JSON response for every endpoint, so return the documented body; without it this endpoint's behaviour cannot be verified by anyone, including this gate.",
  "POST /api/drafts's response does not carry the documented field(s) `amount_minor`, `currency`, `counterparty`, `name`, `country`, `note` — the spec's endpoint table names them for exactly this endpoint. Return them from this handler; without them the endpoint's contract cannot be verified by anyone, including this gate.",
  "POST /api/drafts/<id>/submit's response could not be read as JSON on either probe — the spec documents a JSON response for every endpoint, so return the documented body; without it this endpoint's behaviour cannot be verified by anyone, including this gate.",
  "POST /api/drafts/<id>/approve's response could not be read as JSON on either probe — the spec documents a JSON response for every endpoint, so return the documented body; without it this endpoint's behaviour cannot be verified by anyone, including this gate.",
  "POST /api/drafts/<id>/reject's response could not be read as JSON on either probe — the spec documents a JSON response for every endpoint, so return the documented body; without it this endpoint's behaviour cannot be verified by anyone, including this gate.",
  'the page renders but the browser console carries 4 error(s) in normal use (first: ReferenceError: onBrushChangeTracked is not defined) — fix the JS errors; users hit them as broken interactions.',
];

const COMPLETE_RESULT = {
  event: 'complete_result',
  passed: true,
  passed_means: LIVE_PASSED_MEANS,
  verified: true,
  remaining_findings: 7,
  shipped: 'final tree',
  known_active_bugs: R5_BUGS,
  ts: '2026-08-30T20:27:16.856695+00:00',
};

// r6h's REAL terminal verdict, verbatim from
// ~/goose-builds/local-sb7-swarm-r6h-FINISHED-0.4616-passed-507m-3d-draws-393a99351/run.jsonl (seq 1386):
// passed+verified with ONE known bug whose authoring check rates it `critical` — the class the panel
// never showed before VA-129 (the row read as a cosmetic minor).
const R6H_BUG =
  'the app ships NO executable tests (`pytest -q` collected 0) — an empty suite is not a passing suite. Write real tests that assert the spec\'s concrete expected values, and never delete or skip a failing test to go green.';
const R6H_COMPLETE_RESULT = {
  event: 'complete_result',
  passed: true,
  passed_means: LIVE_PASSED_MEANS,
  render_class_known_bugs: 0,
  verified: true,
  remaining_findings: 1,
  shipped: 'final tree',
  known_active_bugs: [R6H_BUG],
  known_active_bugs_severities: ['critical'],
  ts: '2026-09-02T08:55:32.393365+00:00',
};
// r6h's two known_bugs rounds (0 and 1), same finding both times — the union the snapshot supersedes.
const R6H_KNOWN_BUGS_ROUNDS = [
  { event: 'known_bugs', round: 0, count: 1, severities: ['critical'], findings: [R6H_BUG] },
  { event: 'known_bugs', round: 1, count: 1, severities: ['critical'], findings: [R6H_BUG] },
];
// Two bugs of different classes: r6h's critical beside r5's first (a contract-shape finding, medium).
const TWO_BUGS_COMPLETE_RESULT = {
  ...R6H_COMPLETE_RESULT,
  remaining_findings: 2,
  known_active_bugs: [R6H_BUG, R5_BUGS[0]],
  known_active_bugs_severities: ['critical', 'medium'],
};

const PERSONA_LEARNED = {
  event: 'persona_learned',
  stack_key: 'angular',
  written: true,
  runs: 1,
  path: '/Users/mihaiperdum/.config/goose/skills/stack-angular/SKILL.md',
  lessons: 0,
  ts: '2026-08-30T20:30:52.044593+00:00',
};

const RUN_OVERVIEW = {
  event: 'run_overview',
  generated: false,
  run_command: 'python3 -m app --help',
  run_command_lang: 'python',
  run_command_verified: true,
  features: [],
  engage: null,
  next: [],
  ts: '2026-08-30T20:30:52.045565+00:00',
};

const RUN_FINISHED = {
  event: 'run_finished',
  report: {
    done: [
      'boot-contract',
      'brush-contract',
      'decisions',
      'frontend-core',
      'frozen-rules-tests',
      'integrate-verify',
      'ledgerd-service',
      'notifierd-service',
      'skeleton',
      'viz-field',
      'viz-math-oracle',
    ],
    failed: [],
    bonus: ['frozen-rules-tests', 'viz-math-oracle'],
  },
  phases: { research_min: 0.0, planning_min: 137.2, execute_min: 417.9, gates_min: 157.0, total_min: 712.1 },
  ts: '2026-08-30T20:30:52.045877+00:00',
};

const BASE = [
  {
    event: 'run_started',
    prompt: '# Build `meridian-payments-console`\n\nA payments operations console.',
    pool: POOL,
    ts: '2026-08-30T08:38:47.650000+00:00',
  },
  { event: 'pool_resolved', devices: POOL, worker_count: 3 },
  { event: 'phase', phase: 'build' },
  {
    event: 'complete_fix_converged',
    round: 1,
    findings: 7,
    detail: 'the wave changed nothing on the tree — the phase ends here instead of re-measuring an identical tree',
  },
];

// The wrap-up window: verdict landed, run_finished has not — r5 sat here 3.5 minutes.
const WRAPPING = [...BASE, COMPLETE_RESULT];
// The clean finish, full terminal sequence.
const FINISHED = [...BASE, COMPLETE_RESULT, PERSONA_LEARNED, RUN_OVERVIEW, RUN_FINISHED];

type ElectronMock = Record<string, unknown>;

const mockRun = (
  events: Array<Record<string, unknown>>,
  activity: Record<string, unknown> = {}
) => {
  const electron = (window as unknown as { electron: ElectronMock }).electron;
  electron.readSwarmRun = vi.fn(async () => ({
    runId: 'swarm-20260830-083847650',
    dir: '/tmp/build',
    events,
    activity,
    activityMtimes: Object.fromEntries(Object.keys(activity).map((k) => [k, Date.now()])),
    clarify: null,
    mtime: Date.now(),
    heartbeat: Date.now(),
    heartbeatExited: false,
    pauseRequested: false,
  }));
};

describe('buildActivity — the verdict is run-level state through the one reducer', () => {
  it('parses complete_result verbatim and turns the phase label to Wrapping up', () => {
    const { verdict, phase, activity, knownActiveBugs } = buildActivity(WRAPPING);
    expect(verdict).toEqual({
      passed: true,
      verified: true,
      passedMeans: LIVE_PASSED_MEANS,
      remainingFindings: 7,
      shipped: 'final tree',
      renderClassKnownBugs: null,
    });
    expect(phase).toBe('Wrapping up');
    expect(knownActiveBugs).toHaveLength(7);
    // r5's archive carries no severities array — no class is claimed, none is invented.
    expect(knownActiveBugs.every((b) => b.severity === null)).toBe(true);
    // The verdict is a compact feed moment too — the compact lane is what a user watches.
    expect(activity.some((a) => a.text.includes('Verdict — PASSED, verified end-to-end'))).toBe(true);
  });

  it('complete_result pairs each known bug with its class and carries the render-class count (r6h, verbatim)', () => {
    const { verdict, knownActiveBugs, activity } = buildActivity([...BASE, ...R6H_KNOWN_BUGS_ROUNDS, R6H_COMPLETE_RESULT]);
    expect(knownActiveBugs).toEqual([{ text: R6H_BUG, severity: 'critical' }]);
    expect(verdict?.renderClassKnownBugs).toBe(0);
    expect(activity.some((a) => a.text === 'Verdict — PASSED, verified end-to-end · 1 known bug ship')).toBe(true);
  });

  it('an EMPTY known_active_bugs snapshot supersedes the known_bugs union — fixed bugs do not linger as known-active', () => {
    const { knownActiveBugs, activity } = buildActivity([
      ...BASE,
      ...R6H_KNOWN_BUGS_ROUNDS,
      { ...R6H_COMPLETE_RESULT, remaining_findings: 0, known_active_bugs: [], known_active_bugs_severities: [] },
    ]);
    expect(knownActiveBugs).toEqual([]);
    expect(activity.some((a) => a.text === 'Verdict — PASSED, verified end-to-end')).toBe(true);
    // A legacy event with NO field keeps the union — absence is not an empty snapshot.
    const legacy = buildActivity([...BASE, ...R6H_KNOWN_BUGS_ROUNDS, { event: 'complete_result', passed: true, verified: true }]);
    expect(legacy.knownActiveBugs.map((b) => b.text)).toEqual([R6H_BUG]);
  });

  it('a mismatched severities array claims no class rather than a wrong one', () => {
    const { knownActiveBugs } = buildActivity([
      ...BASE,
      { ...TWO_BUGS_COMPLETE_RESULT, known_active_bugs_severities: ['critical'] },
    ]);
    expect(knownActiveBugs.map((b) => b.severity)).toEqual([null, null]);
  });

  it('a FAILED gate with zero criticals names the render-class findings that blocked the green', () => {
    const { verdict, activity } = buildActivity([
      ...BASE,
      { ...TWO_BUGS_COMPLETE_RESULT, passed: false, verified: false, render_class_known_bugs: 2 },
    ]);
    expect(verdict?.renderClassKnownBugs).toBe(2);
    expect(activity.some((a) => a.text === 'Verdict — FAILED · 2 findings remain · 2 render-class findings blocked the green')).toBe(true);
  });

  it('run_finished supersedes the wrap-up label with Done and finished=true', () => {
    const { verdict, phase, finished } = buildActivity(FINISHED);
    expect(verdict?.passed).toBe(true);
    expect(phase).toBe('Done');
    expect(finished).toBe(true);
  });

  it('complete_result_revised UPDATES the verdict state, not only the feed', () => {
    const { verdict } = buildActivity([
      ...WRAPPING,
      { event: 'complete_result_revised', verified: false, reason: 'unwired-module-unfixed', evidence: ['app/dead.py'] },
    ]);
    expect(verdict?.verified).toBe(false);
    // passed is deliberately untouched — the engine never flips it red either.
    expect(verdict?.passed).toBe(true);
  });

  it('a FAILED gate renders its own verdict line', () => {
    const { verdict, activity } = buildActivity([
      ...BASE,
      { ...COMPLETE_RESULT, passed: false, verified: false, known_active_bugs: [] },
    ]);
    expect(verdict?.passed).toBe(false);
    expect(activity.some((a) => a.text.includes('Verdict — FAILED · 7 findings remain'))).toBe(true);
  });

  it('persona_learned renders, and its written:false twin renders too', () => {
    const { verbose } = buildActivity(FINISHED);
    expect(verbose.some((a) => a.text === 'Learned a reusable skill for the angular stack')).toBe(true);

    const { verbose: failed } = buildActivity([
      ...WRAPPING,
      { event: 'persona_learned', stack_key: 'angular', written: false, reason: 'the reflection came back empty' },
    ]);
    const twin = failed.find((a) => a.text.startsWith('Stack skill not written'));
    expect(twin).toBeTruthy();
    expect(twin?.sub).toBe('the reflection came back empty');
  });
});

describe('SwarmRunPanel — the r5 terminal sequence renders the verdict, not dead air', () => {
  beforeEach(() => {
    const electron = (window as unknown as { electron: ElectronMock }).electron;
    electron.fleetStatus = vi.fn(async () => ({}));
    electron.swarmSetPaused = vi.fn(async () => true);
    electron.swarmAddNote = vi.fn(async () => true);
    electron.revealInFinder = vi.fn(async () => undefined);
    electron.writeFile = vi.fn(async () => true);
  });
  afterEach(() => cleanup());

  it('between complete_result and run_finished the wrap-up banner owns the top: verdict in, still working', async () => {
    // HEAD engines stream the reflect call into a keyed supervision digest; its open phase names the wait.
    mockRun(WRAPPING, {
      reflect: { model: 'gabee-qwen3.8-27b', phase: 'processing', last_text: '' },
    });
    const { findByTestId, queryByTestId, container } = render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" />
      </IntlTestWrapper>
    );

    const banner = await findByTestId('wrapup-banner');
    expect(banner.textContent).toContain('Verdict is in — PASSED, verified end-to-end · 7 known bugs ship');
    expect(banner.textContent).toContain('Wrapping up — learning a reusable stack skill');
    // The run is NOT over: no terminal banner yet, and the header phase chip says what this stretch is.
    expect(queryByTestId('terminal-banner')).toBeNull();
    expect(container.textContent).toContain('Wrapping up');
  });

  it('without a live reflect digest the wrap-up line still renders from the events alone', async () => {
    mockRun(WRAPPING);
    const { findByTestId } = render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" />
      </IntlTestWrapper>
    );
    const banner = await findByTestId('wrapup-banner');
    expect(banner.textContent).toContain('Wrapping up — writing the final run record');
  });

  it('after run_finished the terminal banner states PASSED with the honesty line, 7 known bugs and the run command', async () => {
    mockRun(FINISHED);
    const { findByTestId, queryByTestId, findByText, container } = render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" />
      </IntlTestWrapper>
    );

    const banner = await findByTestId('terminal-banner');
    expect(banner.textContent).toContain('PASSED — verified end-to-end');
    // The honesty line, verbatim from the engine.
    expect(banner.textContent).toContain(`Passed means: ${LIVE_PASSED_MEANS}`);
    // The bug chip counts the SAME array the Known active bugs section renders.
    const chip = await findByTestId('verdict-bug-chip');
    expect(chip.textContent).toBe('7 known bugs shipped');
    await findByText('Known active bugs');
    expect(container.textContent).toContain("POST /api/webhooks/meridian's response could not be read as JSON");
    // The engine-stamped, engine-verified run command sits in the verdict block.
    expect(banner.textContent).toContain('python3 -m app --help');
    expect(banner.textContent).toContain('goose ran this command itself');
    // The wrap-up banner has flipped away.
    expect(queryByTestId('wrapup-banner')).toBeNull();
  });

  it('two known bugs render VERBATIM with their class chips, and the verdict chip counts the same two', async () => {
    mockRun([...BASE, TWO_BUGS_COMPLETE_RESULT, RUN_OVERVIEW, RUN_FINISHED]);
    const { findByTestId, findAllByTestId, container } = render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" />
      </IntlTestWrapper>
    );
    const banner = await findByTestId('terminal-banner');
    expect(banner.textContent).toContain('PASSED — verified end-to-end');
    expect((await findByTestId('verdict-bug-chip')).textContent).toBe('2 known bugs shipped');
    const block = await findByTestId('known-active-bugs');
    expect(block.textContent).toContain('the run passed — these are what it passed WITH');
    expect((await findByTestId('known-bugs-count-chip')).textContent).toBe('2');
    const classes = await findAllByTestId('known-bug-class');
    expect(classes.map((c) => c.textContent)).toEqual(['critical', 'medium']);
    // Each bug's text verbatim — InlineMarkdown renders the backticked span as code, so match on the
    // surrounding plain text of each finding.
    expect(block.textContent).toContain('the app ships NO executable tests');
    expect(block.textContent).toContain('an empty suite is not a passing suite');
    expect(block.textContent).toContain("POST /api/payments/<id>/note's response does not carry the documented field(s)");
    // Zero render-class on a pass: no render-class chip.
    expect(container.querySelector('[data-testid="known-bugs-render-class-chip"]')).toBeNull();
  });

  it('a zero-bug verdict after known_bugs rounds renders NO block and NO chip', async () => {
    mockRun([
      ...BASE,
      ...R6H_KNOWN_BUGS_ROUNDS,
      { ...R6H_COMPLETE_RESULT, remaining_findings: 0, known_active_bugs: [], known_active_bugs_severities: [] },
      RUN_OVERVIEW,
      RUN_FINISHED,
    ]);
    const { findByTestId, queryByTestId, queryByText } = render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" />
      </IntlTestWrapper>
    );
    const banner = await findByTestId('terminal-banner');
    expect(banner.textContent).toContain('PASSED — verified end-to-end');
    expect(queryByTestId('verdict-bug-chip')).toBeNull();
    expect(queryByTestId('known-active-bugs')).toBeNull();
    expect(queryByText('Known active bugs')).toBeNull();
  });

  it('a FAILED verdict on render-class findings says so in the banner and on the block', async () => {
    mockRun([
      ...BASE,
      { ...TWO_BUGS_COMPLETE_RESULT, passed: false, verified: false, render_class_known_bugs: 2 },
      RUN_OVERVIEW,
      { ...RUN_FINISHED, report: { ...RUN_FINISHED.report, failed: ['integrate-verify'] } },
    ]);
    const { findByTestId } = render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" />
      </IntlTestWrapper>
    );
    const banner = await findByTestId('terminal-banner');
    expect(banner.textContent).toContain('FAILED — 2 findings remain · 2 render-class findings blocked the green');
    const block = await findByTestId('known-active-bugs');
    expect(block.textContent).toContain('still active when the run ended — 2 render-class findings blocked the green');
    expect((await findByTestId('known-bugs-render-class-chip')).textContent).toBe('2 render-class');
  });

  it('a retracted green ships as PASSED — but not verified, never as a clean pass', async () => {
    mockRun([
      ...FINISHED.slice(0, FINISHED.length - 1),
      { event: 'complete_result_revised', verified: false, reason: 'unwired-module-unfixed', evidence: ['app/dead.py'] },
      RUN_FINISHED,
    ]);
    const { findByTestId } = render(
      <IntlTestWrapper>
        <SwarmRunPanel workingDir="/tmp/build" />
      </IntlTestWrapper>
    );
    const banner = await findByTestId('terminal-banner');
    expect(banner.textContent).toContain('PASSED — but the engine retracted its verification');
    expect(banner.textContent).not.toContain('verified end-to-end');
  });
});
