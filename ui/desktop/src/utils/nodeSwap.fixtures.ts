import type {
  NodeResidency,
  NodesReadResponse_unstable,
  NodesResidencyResponse_unstable,
  ResolvedNodeDef,
} from '@aaif/goose-sdk';

/**
 * Live J3 on 3.0.65 (2026-09-28, ~/goose-builds/quality/LIVE-2026-09-28-3.0.65, 13-… to 19-…png):
 * strategy "Quick" (id `new-strategy`) — Chat on the 27B single on this Mac, Build on the 27B split
 * across both Macs — and a chat that delegates, so each delegate call swaps twice. The pool's MLX
 * device `mihai-mlx` is adopted as "Mihai Macbook engine" (it follows this Mac's engine).
 */

export const J3_MODEL = 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx';
export const J3_STRATEGY = 'strategy:new-strategy';

export const J3_CHAT_NODE: ResolvedNodeDef = {
  def: {
    id: 'qwen3-8-27b-atlassian-q8-mlx-this-mac',
    name: 'Qwen3.8-27B-Atlassian-Q8-mlx · this Mac',
    kind: 'mlx',
    model: J3_MODEL,
    placement: { kind: 'single', macs: ['local'] },
    origin: 'user',
  },
  model: J3_MODEL,
  modelFrom: { kind: 'own' },
};

export const J3_BUILD_NODE: ResolvedNodeDef = {
  def: {
    id: 'qwen3-8-27b-atlassian-q8-mlx-both-macs',
    name: 'Qwen3.8-27B-Atlassian-Q8-mlx · both Macs',
    kind: 'mlx',
    model: J3_MODEL,
    placement: {
      kind: 'tensor',
      macs: ['local', 'link:worksmacstudio-lan-9c1e2a'],
      link: 'jaccl',
    },
    origin: 'user',
  },
  model: J3_MODEL,
  modelFrom: { kind: 'own' },
};

export const J3_POOL_NODE: ResolvedNodeDef = {
  def: {
    id: 'mihai-mlx',
    name: 'Mihai Macbook engine',
    kind: 'mlx',
    placement: { kind: 'follows' },
    poolDevice: 'mihai-mlx',
    origin: 'pool',
  },
  model: 'mihai-qwen3.8-27b-atlassian-q8-mlx',
  modelFrom: { kind: 'pool' },
};

export const J3_READ: NodesReadResponse_unstable = {
  config: {
    version: 1,
    defs: [J3_POOL_NODE.def, J3_CHAT_NODE.def, J3_BUILD_NODE.def],
    strategies: [
      {
        id: 'new-strategy',
        name: 'Quick',
        roles: {
          chat: { chain: [{ node: J3_CHAT_NODE.def.id, weight: 1 }] },
          build: { chain: [{ node: J3_BUILD_NODE.def.id, weight: 1 }] },
        },
      },
    ],
    forNewChats: { kind: 'strategy', id: 'new-strategy' },
  },
  nodes: [J3_POOL_NODE, J3_CHAT_NODE, J3_BUILD_NODE],
  stored: true,
  lmStudioHidden: 0,
};

function residency(
  rows: Record<string, NodeResidency>,
  serving: NodesResidencyResponse_unstable['serving'] = null
): NodesResidencyResponse_unstable {
  return {
    nodes: Object.entries(rows).map(([node, r]) => ({ node, residency: r })),
    serving,
    loaderInstalled: true,
  };
}

/**
 * 19-j3-delegate-swap.png: the delegate's Build demand — the loader stopped the 27B single (its
 * status reads "failed", exit status 143: the stop's SIGTERM) and loads the split. The screen said
 * "Failed · Single · this Mac" on the glance, "Failed" on the nav chip and the chip, and "No model is
 * mounted — mihai-mlx" on the composer.
 */
export const J3_SWAP_TO_SPLIT = residency({
  [J3_POOL_NODE.def.id]: { kind: 'notRunning' },
  [J3_CHAT_NODE.def.id]: { kind: 'notRunning' },
  [J3_BUILD_NODE.def.id]: { kind: 'loading' },
});

/**
 * 17-j3-t1-02.png: the swap back — the 27B single mounting on this Mac for the chat's own turn,
 * its weights going in. The composer said "No model is mounted — mihai-mlx · The saved MLX model
 * serves mihai-flash…; the node wants mihai-qwen3.8-27b…".
 */
export const J3_SWAP_TO_SINGLE = residency(
  {
    [J3_POOL_NODE.def.id]: { kind: 'loading', phase: 'loading' },
    [J3_CHAT_NODE.def.id]: { kind: 'loading', phase: 'loading' },
    [J3_BUILD_NODE.def.id]: { kind: 'notRunning' },
  },
  {
    kind: 'single',
    macs: ['local'],
    modelId: J3_MODEL,
    servedModelId: 'mihai-qwen3.8-27b-atlassian-q8-mlx',
    macNames: ['Mihai Macbook'],
    loadPhase: 'loading',
  }
);

/** 18-j3-t1w-14.png: between swaps — the 27B single serves, the loader is not at work. */
export const J3_SERVING_SINGLE = residency(
  {
    [J3_POOL_NODE.def.id]: { kind: 'serving' },
    [J3_CHAT_NODE.def.id]: { kind: 'serving' },
    [J3_BUILD_NODE.def.id]: {
      kind: 'notRunning',
      otherWay: 'Qwen3.8-27B-Atlassian-Q8-mlx on Mihai Macbook',
    },
  },
  {
    kind: 'single',
    macs: ['local'],
    modelId: J3_MODEL,
    servedModelId: 'mihai-qwen3.8-27b-atlassian-q8-mlx',
    macNames: ['Mihai Macbook'],
  }
);

/** The negative control: the loader loads the 27B single and ITS mount fails — a real failure. */
export const J3_OWN_LOAD = residency({
  [J3_POOL_NODE.def.id]: { kind: 'loading' },
  [J3_CHAT_NODE.def.id]: { kind: 'loading' },
  [J3_BUILD_NODE.def.id]: { kind: 'notRunning' },
});

/** The chat's turn queued in the loader behind another chat's reply (a wait, in its words). */
export const J3_WAITING = residency(
  {
    [J3_POOL_NODE.def.id]: { kind: 'serving' },
    [J3_CHAT_NODE.def.id]: { kind: 'serving' },
    [J3_BUILD_NODE.def.id]: {
      kind: 'waiting',
      reason:
        'Qwen3.8-27B-Atlassian-Q8-mlx on Mihai Macbook is answering 1 reply; loading Qwen3.8-27B-Atlassian-Q8-mlx · both Macs when it finishes',
    },
  },
  {
    kind: 'single',
    macs: ['local'],
    modelId: J3_MODEL,
    servedModelId: 'mihai-qwen3.8-27b-atlassian-q8-mlx',
    macNames: ['Mihai Macbook'],
  }
);

/** The stopped 27B single's own words on 19-j3-delegate-swap.png (the swap's SIGTERM). */
export const J3_EXIT_143 =
  'the engine process (pid 1709) exited: exit status: 143 — not restarted automatically; Mount restarts it (the crash breaker applies). Last log lines: INFO: Shutting down';

/** A real failure's words (the negative control): the engine died on its own. */
export const REAL_FAILURE =
  'the engine process (pid 2211) exited: exit status: 1 — not restarted automatically; Mount restarts it (the crash breaker applies). Last log lines: RuntimeError: [metal::malloc] Resource limit exceeded';
