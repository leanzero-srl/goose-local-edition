import { describe, expect, it } from 'vitest';
import {
  DEFAULT_GLANCE_PREFS,
  buildEngineGlance,
  glancePrefsOf,
  isGlancePrefs,
  isGlancePush,
  mergeGlanceSessions,
} from './engineGlance';
import { INITIAL_SNAPSHOT } from './mlxEngineMonitor';
import { attributeServing } from './mlxServing';
import { toMlxDistributedReport } from './mlxDistributedReport';
import { MLX_DISTRIBUTED_STALE_MS } from './mlxTray';
import {
  CHAT_ROW,
  GLANCE_MODEL,
  SPLIT_READING_BODY,
  TOOL_LABEL_ROW,
  TURN_BESIDE_SIDE_CALL_BODY,
  TURN_WAITING_BEHIND_SIDE_CALL_BODY,
  figure,
  glancePush,
  measuredRead,
  runningSnapshot,
} from './engineGlance.fixtures';
import {
  GENERATING_STATUS,
  IDLE_STATUS,
  PREFILL_STATUS,
} from '../components/leanzero-swarm/mlxLiveStatus.fixtures';
import {
  FLASH_MODEL,
  FLASH_READY,
  FLASH_SERVING,
  HOSTING_RANK_1,
} from '../components/leanzero-swarm/mlxDistributed.fixtures';
import type { MlxRemoteReport } from './mlxRemoteReport';

const NONE = { distributed: null, remote: null, served: [] };

describe('buildEngineGlance — the single engine', () => {
  it('writing: green, the live writing rate leads, the reading rate beside it, the chat it serves', () => {
    const g = buildEngineGlance(
      runningSnapshot(GENERATING_STATUS, {
        serving: attributeServing([CHAT_ROW], 3, [], null),
      }),
      NONE
    );
    expect(g).toMatchObject({
      present: true,
      busy: true,
      phase: 'writing',
      stage: 'generating',
      engine: { mode: 'single' },
      modelId: GLANCE_MODEL,
      hero: { kind: 'writing', tps: 19.9 },
      chat: { sessionId: CHAT_ROW.sessionId, name: 'Refactor the auth flow', work: null },
      waiting: 1,
      progress: null,
    });
    // The other two requests on the engine are nobody this app can name: counted, never named.
    expect(g.otherClients).toBe(2);
  });

  it('Q-185: goose’s fact check for a chat is its chat line with the work named; the turn wins when both run', () => {
    const check = { ...CHAT_ROW, id: 41, work: 'factCheck' as const };
    const onlyCheck = buildEngineGlance(
      runningSnapshot(GENERATING_STATUS, { serving: attributeServing([check], 1, [], null) }),
      NONE
    );
    expect(onlyCheck.chat).toEqual({
      sessionId: CHAT_ROW.sessionId,
      name: 'Refactor the auth flow',
      work: 'factCheck',
    });
    const both = buildEngineGlance(
      runningSnapshot(GENERATING_STATUS, {
        serving: attributeServing([check, CHAT_ROW], 2, [], null),
      }),
      NONE
    );
    expect(both.chat?.work).toBeNull();
  });

  it('reading a prompt on the single engine: size and time lead, and NO bar — it reports no progress', () => {
    const g = buildEngineGlance(runningSnapshot(PREFILL_STATUS), NONE);
    expect(g).toMatchObject({
      phase: 'reading',
      stage: 'prefill',
      busy: true,
      hero: { kind: 'prompt', tokens: 32277, elapsedS: 165.4 },
      progress: null,
    });
  });

  it('idle: grey, not busy, the measured median as the quiet figure and its range behind More', () => {
    const g = buildEngineGlance(
      runningSnapshot(IDLE_STATUS, {
        measured: measuredRead({ writing: figure(29.6, 27.1, 31.0, 5) }),
      }),
      NONE
    );
    expect(g).toMatchObject({
      phase: 'idle',
      stage: 'idle',
      busy: false,
      hero: { kind: 'writingMedian', median: 29.6, runs: 5 },
      ranges: { writing: { low: 27.1, high: 31.0 }, reading: null },
    });
  });

  it('mounting is loading with an indeterminate bar; failed is red with the engine’s own words', () => {
    expect(buildEngineGlance({ ...INITIAL_SNAPSHOT, mode: 'mounting' }, NONE)).toMatchObject({
      present: true,
      busy: true,
      phase: 'loading',
      stage: 'loading',
      progress: 'indeterminate',
    });
    expect(
      buildEngineGlance(
        { ...INITIAL_SNAPSHOT, mode: 'failed', failedError: 'out of memory loading layer 41' },
        NONE
      )
    ).toMatchObject({
      present: true,
      busy: false,
      phase: 'failed',
      stage: 'failed',
      detail: 'out of memory loading layer 41',
    });
  });

  it('off or never read: nothing to speak of', () => {
    expect(buildEngineGlance({ ...INITIAL_SNAPSHOT, mode: 'off' }, NONE)).toMatchObject({
      present: false,
      busy: false,
      stage: 'off',
    });
    expect(buildEngineGlance(INITIAL_SNAPSHOT, NONE).present).toBe(false);
  });

  it('a read of the split’s rank 0 never speaks for the single engine', () => {
    const g = buildEngineGlance(
      runningSnapshot(GENERATING_STATUS, { engine: 'distributed' }),
      NONE
    );
    expect(g.present).toBe(false);
  });
});

describe('buildEngineGlance — the split across Macs', () => {
  const fresh = (status = FLASH_READY) => ({
    distributed: { report: toMlxDistributedReport(status), ageMs: 0 },
    remote: null,
    served: [],
  });

  it('the owner’s screenshot: reading an 80.3K prompt for 3m 11s at 237 tok/s, with the bar', () => {
    const g = buildEngineGlance(
      runningSnapshot(SPLIT_READING_BODY, { engine: 'distributed', modelId: FLASH_MODEL }),
      fresh()
    );
    expect(g).toMatchObject({
      phase: 'reading',
      stage: 'prefill',
      busy: true,
      engine: { mode: 'distributed', nodeNames: ['MacBook Pro', 'workhorse'], backend: 'jaccl' },
      modelId: FLASH_MODEL,
      hero: { kind: 'prompt', tokens: 80300, elapsedS: 191 },
      second: { kind: 'reading', tps: 237 },
      progress: { done: 45200, total: 80300, unit: 'tokens' },
      waiting: 0,
    });
    // Each Mac's memory against its budget — behind More on the card.
    expect(g.nodes.map((n) => [n.name, n.peakGb])).toEqual([
      ['MacBook Pro', 61.0],
      ['workhorse', 42.5],
    ]);
  });

  it('serving with no live read of rank 0: the supervisor’s in-flight count, green', () => {
    const g = buildEngineGlance(INITIAL_SNAPSHOT, fresh(FLASH_SERVING));
    expect(g).toMatchObject({ stage: 'serving', busy: true, phase: 'writing', hero: null });
  });

  it('a report older than three polls claims nothing live', () => {
    const g = buildEngineGlance(INITIAL_SNAPSHOT, {
      distributed: {
        report: toMlxDistributedReport(FLASH_SERVING),
        ageMs: MLX_DISTRIBUTED_STALE_MS + 1,
      },
      remote: null,
      served: [],
    });
    expect(g).toMatchObject({ stage: 'stale', phase: 'idle', busy: false });
  });

  it('admission held by the watchdog is orange whatever the run says', () => {
    const g = buildEngineGlance(INITIAL_SNAPSHOT, {
      distributed: {
        report: { ...toMlxDistributedReport(FLASH_READY), admissionOpen: false },
        ageMs: 0,
      },
      remote: null,
      served: [],
    });
    expect(g).toMatchObject({ stage: 'held', phase: 'held', busy: true });
  });

  it('a rank this Mac serves for another Mac', () => {
    const g = buildEngineGlance(INITIAL_SNAPSHOT, {
      distributed: { report: toMlxDistributedReport(HOSTING_RANK_1), ageMs: 0 },
      remote: null,
      served: [],
    });
    expect(g.engine.mode).toBe('hosting');
    expect(g.present).toBe(true);
  });
});

describe('buildEngineGlance — chat routed to a linked Mac', () => {
  const route = (over: Partial<MlxRemoteReport> = {}): MlxRemoteReport => ({
    state: 'ready',
    peerName: 'Work’s Mac Studio',
    modelId: GLANCE_MODEL,
    baseUrl: 'http://127.0.0.1:3001/relay',
    activeRequests: 1,
    lastError: null,
    ...over,
  });

  it('up and read: that Mac’s engine, in the same figures', () => {
    const g = buildEngineGlance(runningSnapshot(GENERATING_STATUS, { engine: 'remote' }), {
      distributed: null,
      remote: route(),
      served: [],
    });
    expect(g).toMatchObject({
      engine: { mode: 'remote', peerName: 'Work’s Mac Studio' },
      stage: 'generating',
      phase: 'writing',
    });
  });

  it('contact lost: reconnecting, amber, with the read’s own words', () => {
    const g = buildEngineGlance(
      {
        ...INITIAL_SNAPSHOT,
        engine: 'remote',
        mode: 'reconnecting',
        statusDetail: 'connection refused',
      },
      { distributed: null, remote: route(), served: [] }
    );
    expect(g).toMatchObject({
      stage: 'reconnecting',
      phase: 'loading',
      busy: true,
      detail: 'connection refused',
    });
  });

  it('its goose said it quit: away, not reconnecting forever', () => {
    const g = buildEngineGlance(
      {
        ...INITIAL_SNAPSHOT,
        engine: 'remote',
        mode: 'reconnecting',
        contact: {
          lostSinceMs: 1,
          lostForMs: 5,
          longestComebackMs: null,
          comebacks: 0,
          saidQuit: true,
          pollMs: 2000,
        },
      },
      { distributed: null, remote: route({ state: 'reconnecting' }), served: [] }
    );
    expect(g).toMatchObject({ stage: 'away', phase: 'held', busy: false });
  });
});

describe('buildEngineGlance — which request the card leads with (Q-218)', () => {
  const split = (body: unknown, rows = [CHAT_ROW, TOOL_LABEL_ROW]) =>
    buildEngineGlance(
      runningSnapshot(body, {
        engine: 'distributed',
        modelId: FLASH_MODEL,
        serving: attributeServing(rows, 2, [], null),
      }),
      {
        distributed: { report: toMlxDistributedReport(FLASH_READY), ageMs: 0 },
        remote: null,
        served: [],
      }
    );

  it('screenshot 26: the chat’s 77k prompt leads, 1% read at ITS rate — the 174-token side call is named by its kind', () => {
    const g = split(TURN_BESIDE_SIDE_CALL_BODY);
    expect(g).toMatchObject({
      stage: 'prefill',
      phase: 'reading',
      hero: { kind: 'prompt', tokens: 77000, elapsedS: 9 },
      second: { kind: 'reading', tps: 85.6 },
      progress: { done: 770, total: 77000, unit: 'tokens' },
      chat: { sessionId: CHAT_ROW.sessionId, work: null },
      side: ['toolLabel'],
      otherClients: 0,
    });
  });

  it('the turn waiting for a slot while the side call is read: queued, never "Reading prompt" for the chat', () => {
    const g = split(TURN_WAITING_BEHIND_SIDE_CALL_BODY);
    expect(g.stage).toBe('queued');
    expect(g.phase).toBe('held');
    expect(g.hero).toMatchObject({ kind: 'queued' });
    expect(g.side).toEqual(['toolLabel']);
  });

  it('negative control — no chat turn on the engine, only goose’s call for it: the card speaks for the engine', () => {
    const g = split(TURN_BESIDE_SIDE_CALL_BODY, [TOOL_LABEL_ROW]);
    expect(g.chat).toMatchObject({ work: 'toolLabel' });
    expect(g.side).toEqual([]);
    // Engine-wide, the headline still names the largest prompt being read, never the longest-read.
    expect(g.hero).toMatchObject({ kind: 'prompt', tokens: 77000 });
    // Engine-wide reading rate: both reads summed.
    expect(g.second).toMatchObject({ kind: 'reading', tps: 89.5 });
  });

  it('the single engine: the lead’s own writing rate, not the sum with a side call writing beside it', () => {
    const body = {
      ...GENERATING_STATUS,
      requests: [
        GENERATING_STATUS.requests[1],
        {
          ...GENERATING_STATUS.requests[1],
          request_id: 'side-title',
          prompt_tokens: 300,
          completion_tokens: 12,
          tokens_per_second: 40,
        },
      ],
    };
    const g = buildEngineGlance(
      runningSnapshot(body, {
        serving: attributeServing([CHAT_ROW, { ...CHAT_ROW, id: 3, work: 'title' }], 2, [], null),
      }),
      NONE
    );
    expect(g.hero).toEqual({ kind: 'writing', tps: 19.9 });
    expect(g.side).toEqual(['title']);
  });
});

describe('the glance prefs and sessions', () => {
  it('defaults: in the sidebar, and on the desktop only while goose is in the background', () => {
    expect(DEFAULT_GLANCE_PREFS).toEqual({
      inApp: true,
      desktop: 'away',
      desktopCollapsed: false,
      desktopPlace: null,
    });
    expect(glancePrefsOf(undefined)).toEqual(DEFAULT_GLANCE_PREFS);
  });

  it('a stored value missing a field keeps the rest; one with a foreign value is refused', () => {
    expect(glancePrefsOf({ desktop: 'busy' })).toEqual({
      ...DEFAULT_GLANCE_PREFS,
      desktop: 'busy',
    });
    expect(glancePrefsOf({ desktop: 'always' })).toEqual(DEFAULT_GLANCE_PREFS);
    expect(
      isGlancePrefs({ ...DEFAULT_GLANCE_PREFS, desktopPlace: { displayId: 1, corner: 'middle' } })
    ).toBe(false);
  });

  it('two windows’ sessions add up; one question reported by both counts once', () => {
    const q = { sessionId: 's1', sessionName: 'Auth', question: 'Which branch?' };
    expect(
      mergeGlanceSessions([
        { running: 1, needsYou: [q] },
        { running: 2, needsYou: [q] },
      ])
    ).toEqual({ running: 3, needsYou: [q] });
  });

  it('a push is recognised only with an engine, sessions and prefs', () => {
    expect(isGlancePush(glancePush(runningSnapshot(IDLE_STATUS)))).toBe(true);
    expect(isGlancePush({ engine: {}, sessions: { running: 0, needsYou: [] } })).toBe(false);
  });
});
