import { describe, expect, it } from 'vitest';
import { nodeGlance, usedByOf, STATE_CHIP, type NodeState } from './nodeGlance';
import {
  CONFIG,
  LOADS_FLASH,
  MODEL_27B,
  MODEL_FLASH,
  NODE_CLOUD,
  NODE_ENDPOINT,
  NODE_FLASH,
  NODE_LOCAL_27B,
  NODE_POOL,
  NODE_POOL_LEFT,
  NODE_SPLIT,
  NODE_STUDIO,
  OPENROUTER,
  PLANS,
  STUDIO,
  STUDIO_KEY,
  STUDIO_MAC,
  SELF_MAC,
  WAY_LOADING,
  WAY_REMOTE,
  WAY_SPLIT,
  candidate,
  engineGlance,
  facts,
  provider,
} from './nodeGlance.fixtures';
import type { ResolvedNodeDef } from './model';

const read = <T>(value: T) => ({ kind: 'read' as const, value });

describe('nodeGlance — MLX nodes', () => {
  it('serving: the split under its HF id, served by an alias (Q-128), reads Serving with the glance', () => {
    const g = nodeGlance(
      NODE_SPLIT,
      facts({
        residency: read({ kind: 'serving' }),
        serving: WAY_SPLIT,
        servingNodeName: NODE_SPLIT.def.name,
        glance: engineGlance(),
      })
    );
    expect(g.state).toBe('serving');
    expect(g.line).toEqual({
      kind: 'live',
      stage: 'generating',
      hero: { kind: 'writing', tps: 11.2 },
      chat: 'Kickoff notes',
    });
    expect(g.action).toBe('stop');
    expect(g.where).toEqual({ kind: 'split', count: 2 });
    expect(g.figures).toEqual({
      kind: 'live',
      hero: { kind: 'writing', tps: 11.2 },
      second: { kind: 'readingMedian', median: 154, runs: 3 },
    });
    expect(g.memory).toEqual([
      { mac: 'Mihai Macbook', usedGb: 38.6, budgetGb: 61.8, kind: 'peak' },
      { mac: 'Work’s Mac Studio', usedGb: 38.6, budgetGb: 66.2, kind: 'peak' },
    ]);
    expect(g.displaces).toBeNull();
  });

  it('serving while idle: no hero figure is claimed as live', () => {
    const g = nodeGlance(
      NODE_SPLIT,
      facts({
        residency: read({ kind: 'serving' }),
        serving: WAY_SPLIT,
        glance: engineGlance({ busy: false, stage: 'idle', chat: null }),
      })
    );
    expect(g.line).toEqual({ kind: 'live', stage: 'idle', hero: null, chat: null });
  });

  it('a remote single serving while this Mac is idle: this Mac’s node is displaced, never serving', () => {
    const g = nodeGlance(
      NODE_FLASH,
      facts({
        residency: read({
          kind: 'notRunning',
          otherWay: 'Qwen3.8-Flash-Next-4bit on Work’s Mac Studio',
        }),
        serving: WAY_REMOTE,
        glance: engineGlance({ modelId: MODEL_FLASH }),
        loads: read(LOADS_FLASH),
      })
    );
    expect(g.state).toBe('displaced');
    expect(g.displaces).toBe('Qwen3.8-Flash-Next-4bit on Work’s Mac Studio');
    expect(g.line).toEqual({ kind: 'startsIn', medianMs: 48000, count: 3 });
    expect(g.action).toBe('start');
    expect(g.figures).toMatchObject({ kind: 'plan', goal: 'chat' });
    expect(g.memory).toEqual([
      { mac: 'Mihai Macbook', usedGb: 18.2, budgetGb: 61.8, kind: 'need' },
    ]);
  });

  it('displaced by a way on another Mac names the serving node when one names it', () => {
    const g = nodeGlance(
      NODE_STUDIO,
      facts({
        residency: read({ kind: 'notRunning', otherWay: 'Qwen3.8-27B split across both' }),
        serving: WAY_SPLIT,
        servingNodeName: NODE_SPLIT.def.name,
      })
    );
    expect(g.state).toBe('displaced');
    expect(g.displaces).toBe('27B Atlassian · both Macs');
    expect(g.where).toEqual({ kind: 'mac', name: 'Work’s Mac Studio' });
  });

  it('ready with no measured load says so — never an estimate', () => {
    const g = nodeGlance(NODE_FLASH, facts());
    expect(g.state).toBe('ready');
    expect(g.line).toEqual({ kind: 'firstStart' });
    expect(g.displaces).toBeNull();
  });

  it('ready with measured loads carries the median and its count', () => {
    const g = nodeGlance(NODE_FLASH, facts({ loads: read(LOADS_FLASH) }));
    expect(g.line).toEqual({ kind: 'startsIn', medianMs: 48000, count: 3 });
  });

  it('a load history that could not be read is its words', () => {
    const g = nodeGlance(
      NODE_FLASH,
      facts({ loads: { kind: 'failed', error: 'store unreadable' } })
    );
    expect(g.line).toEqual({ kind: 'loadsUnread', error: 'store unreadable' });
  });

  it('loading: the phase and the weights moved so far', () => {
    const g = nodeGlance(
      NODE_FLASH,
      facts({
        residency: read({ kind: 'loading', phase: null }),
        serving: WAY_LOADING,
        glance: engineGlance({
          stage: 'loading',
          progress: { done: 12.4 * 1024 ** 3, total: 31 * 1024 ** 3, unit: 'bytes' },
        }),
      })
    );
    expect(g.state).toBe('loading');
    expect(g.line).toEqual({
      kind: 'loadPhase',
      phase: 'loading',
      progress: { done: 12.4 * 1024 ** 3, total: 31 * 1024 ** 3 },
    });
    expect(g.action).toBeNull();
  });

  it('waiting: the loader’s own words', () => {
    const g = nodeGlance(
      NODE_FLASH,
      facts({
        residency: read({
          kind: 'waiting',
          reason: '27B is answering 1; loading Flash when it finishes',
        }),
      })
    );
    expect(g.state).toBe('waiting');
    expect(g.line).toEqual({
      kind: 'words',
      text: '27B is answering 1; loading Flash when it finishes',
    });
  });

  it('needs a step: the model is not on the Studio yet', () => {
    const g = nodeGlance(
      NODE_STUDIO,
      facts({ modelsOn: (key) => (key === STUDIO_KEY ? [] : null) })
    );
    expect(g.state).toBe('needsStep');
    expect(g.line).toEqual({ kind: 'copyFirst', mac: 'Work’s Mac Studio' });
    expect(g.action).toBe('openRunIt');
  });

  it('needs a step: the model is not on this Mac', () => {
    const g = nodeGlance(NODE_FLASH, facts({ modelsOn: (key) => (key === 'self' ? [] : null) }));
    expect(g.line).toEqual({ kind: 'copyFirst', mac: null });
  });

  it('needs a step: the Studio does not let this Mac run part of a split there', () => {
    const g = nodeGlance(
      NODE_SPLIT,
      facts({
        macs: [
          SELF_MAC,
          { ...STUDIO_MAC, allows: { manage_models: true, answer_chat: true, run_split: false } },
        ],
      })
    );
    expect(g.state).toBe('needsStep');
    expect(g.line).toEqual({
      kind: 'permissionOff',
      mac: 'Work’s Mac Studio',
      permission: 'split',
    });
  });

  it('needs a step: the planner’s unavailable action, in its words', () => {
    const unavailable = candidate({
      kind: 'single',
      nodes: [STUDIO],
      names: ['Work’s Mac Studio'],
      needGb: [46.5],
      budgetGb: [66.2],
      action: { kind: 'unavailable', reason: 'needs the remote engine on Work’s Mac Studio' },
    });
    const plans = {
      kind: 'read' as const,
      plans: new Map([[MODEL_27B, { ...PLANS[0], candidates: [unavailable] }]]),
      storeErrors: [],
    };
    const g = nodeGlance(NODE_STUDIO, facts({ plans }));
    expect(g.state).toBe('needsStep');
    expect(g.line).toEqual({ kind: 'words', text: 'needs the remote engine on Work’s Mac Studio' });
  });

  it('can’t run: too big for this Mac, with the planner’s arithmetic for Details', () => {
    const g = nodeGlance(NODE_LOCAL_27B, facts());
    expect(g.state).toBe('cantRun');
    expect(g.line).toMatchObject({ kind: 'outcome' });
    expect(g.action).toBe('details');
    expect(g.detail).toContain('Mihai Macbook');
  });

  it('can’t run: the Studio is not connected to LeanZero Link', () => {
    const g = nodeGlance(NODE_SPLIT, facts({ macs: [SELF_MAC, { ...STUDIO_MAC, online: false }] }));
    expect(g.state).toBe('cantRun');
    expect(g.line).toEqual({ kind: 'notConnected', mac: 'Work’s Mac Studio' });
  });

  it('can’t run: a Link peer the roster does not know is kept by its planner name', () => {
    const g = nodeGlance(NODE_SPLIT, facts({ macs: [SELF_MAC] }));
    expect(g.line).toEqual({ kind: 'notConnected', mac: 'Work’s Mac Studio' });
  });

  it('a peer the planner reaches by ssh host is judged by the plan, not the Link roster', () => {
    const ssh: ResolvedNodeDef = {
      ...NODE_STUDIO,
      def: { ...NODE_STUDIO.def, placement: { kind: 'single', macs: ['workhorse'] } },
    };
    const sshCandidate = candidate({
      kind: 'single',
      nodes: ['workhorse'],
      names: ['Work’s Mac Studio'],
      needGb: [46.5],
      budgetGb: [66.2],
    });
    const plans = {
      kind: 'read' as const,
      plans: new Map([[MODEL_27B, { ...PLANS[0], candidates: [sshCandidate] }]]),
      storeErrors: [],
    };
    const g = nodeGlance(ssh, facts({ plans, macs: [SELF_MAC] }));
    expect(g.state).toBe('ready');
    expect(g.where).toEqual({ kind: 'mac', name: 'Work’s Mac Studio' });
  });

  it('can’t run: the engine’s refusal last time, verbatim', () => {
    const g = nodeGlance(
      NODE_FLASH,
      facts({
        residency: read({ kind: 'refusedLastTime', reason: 'engine exited: out of memory' }),
      })
    );
    expect(g.state).toBe('cantRun');
    expect(g.line).toEqual({ kind: 'words', text: 'engine exited: out of memory' });
  });

  it('can’t run: the plan offers no such way for this model', () => {
    const plans = {
      kind: 'read' as const,
      plans: new Map([[MODEL_27B, { ...PLANS[0], candidates: [] }]]),
      storeErrors: [],
    };
    const g = nodeGlance(NODE_SPLIT, facts({ plans }));
    expect(g.state).toBe('cantRun');
    expect(g.line).toEqual({ kind: 'noSuchWay' });
  });

  it('held by a build: every MLX card but the build’s own way', () => {
    const holder = { node: NODE_FLASH.def.id, way: 'Flash on this Mac' };
    expect(nodeGlance(NODE_SPLIT, facts({ buildHolder: holder })).state).toBe('heldByBuild');
    expect(nodeGlance(NODE_SPLIT, facts({ buildHolder: holder })).line).toEqual({
      kind: 'heldByBuild',
      way: 'Flash on this Mac',
    });
    expect(nodeGlance(NODE_FLASH, facts({ buildHolder: holder })).state).toBe('ready');
  });

  it('follows: a pool node names what this Mac’s engine serves', () => {
    const g = nodeGlance(
      NODE_POOL,
      facts({ residency: read({ kind: 'serving' }), serving: WAY_SPLIT, glance: engineGlance() })
    );
    expect(g.state).toBe('follows');
    expect(g.line).toEqual({ kind: 'follows', serving: WAY_SPLIT });
    expect(g.action).toBe('pinWay');
    expect(g.figures).toMatchObject({ kind: 'live' });
  });

  it('follows with nothing serving says so', () => {
    const g = nodeGlance(NODE_POOL, facts());
    expect(g.line).toEqual({ kind: 'follows', serving: null });
    expect(g.figures).toBeNull();
  });

  it('a pool node whose device left the pool', () => {
    const g = nodeGlance(NODE_POOL_LEFT, facts());
    expect(g.state).toBe('cantRun');
    expect(g.line).toEqual({ kind: 'leftPool' });
  });

  it('an unreadable pool is its error, not a state guessed from nothing', () => {
    const g = nodeGlance(
      {
        ...NODE_POOL,
        model: null,
        modelFrom: { kind: 'poolUnreadable', error: 'swarm: bad yaml' },
      },
      facts()
    );
    expect(g.state).toBe('unknown');
    expect(g.line).toEqual({ kind: 'words', text: 'swarm: bad yaml' });
  });

  it('residency unknown (an unreadable route record) is named, never guessed', () => {
    const g = nodeGlance(
      NODE_SPLIT,
      facts({ residency: read({ kind: 'unknown', reason: 'the route record is unreadable' }) })
    );
    expect(g.state).toBe('unknown');
    expect(g.line).toEqual({ kind: 'words', text: 'the route record is unreadable' });
  });

  it('a residency read that failed, and one still in flight', () => {
    expect(
      nodeGlance(NODE_SPLIT, facts({ residency: { kind: 'failed', error: 'no goosed' } })).line
    ).toEqual({
      kind: 'words',
      text: 'no goosed',
    });
    expect(nodeGlance(NODE_SPLIT, facts({ residency: { kind: 'reading' } })).line).toEqual({
      kind: 'readingState',
    });
  });

  it('a plan read that failed keeps Start and says why there is no figure', () => {
    const g = nodeGlance(
      NODE_FLASH,
      facts({ plans: { kind: 'failed', error: 'planner: Link down' } })
    );
    expect(g.state).toBe('ready');
    expect(g.line).toEqual({ kind: 'planFailed', error: 'planner: Link down' });
    expect(g.action).toBe('start');
    expect(g.figures).toBeNull();
  });

  it('a model the planner did not plan', () => {
    const g = nodeGlance(
      NODE_FLASH,
      facts({ plans: { kind: 'read', plans: new Map(), storeErrors: [] } })
    );
    expect(g.line).toEqual({ kind: 'noPlan', error: null });
  });
});

describe('nodeGlance — cloud and endpoint nodes', () => {
  it('ready: always available, billed by the provider', () => {
    const g = nodeGlance(NODE_CLOUD, facts());
    expect(g.state).toBe('cloudReady');
    expect(g.line).toEqual({ kind: 'cloudAlways', provider: 'OpenRouter', endpoint: false });
    expect(g.where).toEqual({ kind: 'provider', name: 'OpenRouter' });
    expect(g.memory).toEqual([]);
  });

  it('key missing: the provider is not set up', () => {
    const g = nodeGlance(
      NODE_CLOUD,
      facts({
        provider: read(
          provider('openrouter', 'OpenRouter', {
            is_configured: false,
            credentials_saved: false,
            connection_checked: false,
          })
        ),
      })
    );
    expect(g.state).toBe('keyMissing');
    expect(g.line).toEqual({ kind: 'keyMissing', provider: 'OpenRouter' });
    expect(g.action).toBe('setUp');
  });

  it('key missing when the provider is not in the list at all', () => {
    expect(nodeGlance(NODE_CLOUD, facts({ provider: read(null) })).state).toBe('keyMissing');
  });

  it('failing: the last check’s error, verbatim', () => {
    const g = nodeGlance(
      NODE_CLOUD,
      facts({ provider: read({ ...OPENROUTER, connection_error: '401: invalid key' }) })
    );
    expect(g.state).toBe('failing');
    expect(g.line).toEqual({ kind: 'words', text: '401: invalid key' });
  });

  it('an endpoint says “always available” with its own name', () => {
    const g = nodeGlance(
      NODE_ENDPOINT,
      facts({
        provider: read(provider('custom_desk_vllm', 'Desk vLLM', { provider_type: 'Custom' })),
      })
    );
    expect(g.line).toEqual({ kind: 'cloudAlways', provider: 'Desk vLLM', endpoint: true });
  });

  it('while the provider list is read: checking, not ready', () => {
    const g = nodeGlance(NODE_CLOUD, facts({ provider: { kind: 'reading' } }));
    expect(g.state).toBe('unknown');
    expect(g.line).toEqual({ kind: 'checking', provider: 'openrouter' });
  });
});

describe('usedByOf', () => {
  it('lists every role whose chain names the node, with its rank', () => {
    expect(usedByOf(CONFIG, NODE_SPLIT.def.id)).toEqual([
      { role: 'chat', rank: 1, strategyId: 'everyday', strategyName: 'Everyday' },
      { role: 'build', rank: 1, strategyId: 'everyday', strategyName: 'Everyday' },
    ]);
    expect(usedByOf(CONFIG, NODE_CLOUD.def.id).map((u) => u.rank)).toEqual([2, 2]);
    expect(usedByOf(CONFIG, NODE_FLASH.def.id)).toEqual([]);
  });
});

describe('STATE_CHIP', () => {
  it('every state has a look, and only follows/unknown are outlines', () => {
    const outlines = (Object.keys(STATE_CHIP) as NodeState[]).filter(
      (s) => 'outline' in STATE_CHIP[s]
    );
    expect(outlines.sort()).toEqual(['follows', 'unknown']);
  });

  it('the Studio key the fixtures use is a Link key', () => {
    expect(STUDIO).toBe(`link:${STUDIO_KEY}`);
  });
});
