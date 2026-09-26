import type { PlacementCandidate, PlacementNode, PlacementPlan } from '../../acp/mlx-placement';

/**
 * REAL `placementPlan` answers (goosed built at bf03d52c1, isolated profile, peer over ssh
 * `workhorse`, 2026-09-24 14:3x): the MacBook had ~32 GiB available (another session's 27B was
 * resident), the M3 Ultra ~75 GiB. Captured verbatim — the card's tests read what goose sends.
 */
export const PLAN_27B: PlacementPlan = {
  modelId: 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx',
  goal: 'chat',
  candidates: [
    {
      id: 'single:workhorse',
      key: {
        kind: 'single',
        nodes: ['workhorse'],
      },
      nodeNames: ['Work’s Mac Studio'],
      chips: [
        {
          hwModel: 'Mac15,14',
          brand: 'Apple M3 Ultra',
          gpuCores: 60,
        },
      ],
      backend: 'rapid-mlx',
      supported: true,
      fit: {
        status: 'fits',
        context: 262144,
        nodes: [
          {
            name: 'Work’s Mac Studio',
            needBytes: 49937508814,
            budgetBytes: 73593799967,
          },
        ],
        detail:
          'Work’s Mac Studio: 30.5 GiB of weights + 0.1 GiB of KV per 1k tokens against a budget of 68.5 GiB (min(available 75.3 GiB − the 7% reserve, GPU ceiling))',
      },
      speed: {
        decode: {
          estimate: {
            value: 21.909987076386514,
            low: 20.789209348713587,
            high: 23.030764804059437,
          },
          measured: false,
          runs: 0,
        },
        prefill: {
          estimate: {
            value: 335.4486879505508,
            low: 318.2892338202799,
            high: 352.60814208082166,
          },
          measured: false,
          runs: 0,
        },
        throughput: {
          estimate: {
            value: 39.109326931349926,
            low: 37.10873868745375,
            high: 41.109915175246094,
          },
          measured: false,
          runs: 0,
        },
        concurrency: 8,
        basis: [
          'estimated from mlx_lm runs; the single engine (Rapid-MLX, MTP drafting) has not been measured with this model yet',
          'many requests: ×1.78 at 8 concurrent — rapid-mlx, 8 concurrent vs 1 (experiments.jsonl, 2026-08-30)',
        ],
      },
      action: {
        kind: 'unavailable',
        reason:
          'coming: needs the remote engine on Work’s Mac Studio (a single engine there, chat through LeanZero Link)',
      },
      outcome: {
        code: 'best',
      },
    },
    {
      id: 'tensor:jaccl:local+workhorse',
      key: {
        kind: 'tensor',
        nodes: ['local', 'workhorse'],
        link: 'jaccl',
      },
      nodeNames: ['Mihai Macbook', 'Work’s Mac Studio'],
      chips: [
        {
          hwModel: 'Mac16,5',
          brand: 'Apple M4 Max',
          gpuCores: 40,
        },
        {
          hwModel: 'Mac15,14',
          brand: 'Apple M3 Ultra',
          gpuCores: 60,
        },
      ],
      backend: 'mlx_lm',
      supported: true,
      fit: {
        status: 'smallerContext',
        context: 72704,
        nodes: [
          {
            name: 'Mihai Macbook',
            needBytes: 25237460020,
            budgetBytes: 25247177769,
          },
          {
            name: 'Work’s Mac Studio',
            needBytes: 25237460020,
            budgetBytes: 73593799967,
          },
        ],
        detail:
          "the tensor runner's arithmetic: every rank holds 1/2 of each layer plus the unsharded embeddings and head, KV and a prompt cache for the context, × the measured runtime overhead; largest context every rank fits: 72704",
      },
      speed: {
        decode: {
          estimate: {
            value: 13.879991442217912,
            low: 12.437052447812663,
            high: 15.32293043662316,
          },
          measured: false,
          runs: 0,
        },
        prefill: {
          estimate: {
            value: 416.0783815690151,
            low: 372.8236199220567,
            high: 459.33314321597345,
          },
          measured: false,
          runs: 0,
        },
        throughput: {
          estimate: {
            value: 24.517941087713393,
            low: 21.96909994431142,
            high: 27.066782231115365,
          },
          measured: false,
          runs: 0,
        },
        concurrency: 2,
        basis: ['many requests: ×1.77 at 2 concurrent — tensor pair vs one stream (STEP1b soak)'],
      },
      action: {
        kind: 'startSplit',
        setupMatches: false,
      },
      outcome: {
        code: 'bestAvailableNow',
      },
    },
    {
      id: 'single:local',
      key: {
        kind: 'single',
        nodes: ['local'],
      },
      nodeNames: ['Mihai Macbook'],
      chips: [
        {
          hwModel: 'Mac16,5',
          brand: 'Apple M4 Max',
          gpuCores: 40,
        },
      ],
      backend: 'rapid-mlx',
      supported: true,
      fit: {
        status: 'short',
        shortBytes: 11633630465,
        shortNode: 'Mihai Macbook',
        nodes: [
          {
            name: 'Mihai Macbook',
            needBytes: 32908634574,
            budgetBytes: 25247177769,
          },
        ],
        detail:
          'the mount gate refuses: model 30.5 GiB + floor 12.8 GiB exceeds available 32.5 GiB (short 10.8 GiB)',
      },
      speed: {
        decode: {
          estimate: {
            value: 14.713354895660064,
            low: 13.960711801489747,
            high: 15.46599798983038,
          },
          measured: false,
          runs: 0,
        },
        prefill: {
          estimate: {
            value: 223.6326706699464,
            low: 212.19302373665266,
            high: 235.07231760324012,
          },
          measured: false,
          runs: 0,
        },
        throughput: {
          estimate: {
            value: 26.263338488753213,
            low: 24.9198705656592,
            high: 27.606806411847227,
          },
          measured: false,
          runs: 0,
        },
        concurrency: 8,
        basis: [
          'estimated from mlx_lm runs; the single engine (Rapid-MLX, MTP drafting) has not been measured with this model yet',
          'many requests: ×1.78 at 8 concurrent — rapid-mlx, 8 concurrent vs 1 (experiments.jsonl, 2026-08-30)',
        ],
      },
      action: {
        kind: 'mountHere',
      },
      outcome: {
        code: 'doesNotFit',
      },
    },
    {
      id: 'pipeline:jaccl:local+workhorse',
      key: {
        kind: 'pipeline',
        nodes: ['local', 'workhorse'],
        link: 'jaccl',
      },
      nodeNames: ['Mihai Macbook', 'Work’s Mac Studio'],
      chips: [
        {
          hwModel: 'Mac16,5',
          brand: 'Apple M4 Max',
          gpuCores: 40,
        },
        {
          hwModel: 'Mac15,14',
          brand: 'Apple M3 Ultra',
          gpuCores: 60,
        },
      ],
      backend: 'pipeline_qwen4',
      supported: false,
      fit: {
        status: 'unknown',
        nodes: [],
        detail: 'not supported yet: goose splits qwen3_5 tensor-parallel only',
      },
      speed: {
        basis: [],
      },
      action: {
        kind: 'unavailable',
        reason: 'not supported yet: goose splits qwen3_5 tensor-parallel only',
      },
      outcome: {
        code: 'notSupported',
        reason: 'not supported yet: goose splits qwen3_5 tensor-parallel only',
      },
    },
  ],
  best: 'single:workhorse',
  bestAvailable: 'tensor:jaccl:local+workhorse',
  badge: {
    kind: 'fitsPeer',
    name: 'Work’s Mac Studio',
  },
  notes: [],
};

export const PLAN_FLASH: PlacementPlan = {
  modelId: 'rapid-mlx/Qwen3.8-Flash-Next-4bit',
  goal: 'chat',
  candidates: [
    {
      id: 'single:workhorse',
      key: {
        kind: 'single',
        nodes: ['workhorse'],
      },
      nodeNames: ['Work’s Mac Studio'],
      chips: [
        {
          hwModel: 'Mac15,14',
          brand: 'Apple M3 Ultra',
          gpuCores: 60,
        },
      ],
      backend: 'rapid-mlx',
      supported: true,
      fit: {
        status: 'short',
        shortBytes: 31158459041,
        shortNode: 'Work’s Mac Studio',
        nodes: [
          {
            name: 'Work’s Mac Studio',
            needBytes: 104752259008,
            budgetBytes: 73593799967,
          },
        ],
        detail:
          'Work’s Mac Studio: 97.5 GiB of weights + 0.0 GiB of KV per 1k tokens against a budget of 68.5 GiB (min(available 75.3 GiB − the 7% reserve, GPU ceiling))',
      },
      speed: {
        decode: {
          estimate: {
            value: 27.343314912131714,
            low: 25.94460215856315,
            high: 66.94246035347588,
          },
          measured: false,
          runs: 0,
        },
        prefill: {
          estimate: {
            value: 825.2912619271793,
            low: 783.0745293482747,
            high: 867.5079945060838,
          },
          measured: false,
          runs: 0,
        },
        throughput: {
          estimate: {
            value: 48.80781711815511,
            low: 46.31111485303522,
            high: 119.49229173095443,
          },
          measured: false,
          runs: 0,
        },
        concurrency: 8,
        basis: [
          'estimated from mlx_lm runs; the single engine (Rapid-MLX, MTP drafting) has not been measured with this model yet',
          "MoE: 17% of a dense model's bandwidth use, fitted to our Flash run; up to 51% where other MoE runs reached more",
          'many requests: ×1.78 at 8 concurrent — rapid-mlx, 8 concurrent vs 1 (experiments.jsonl, 2026-08-30)',
        ],
      },
      action: {
        kind: 'unavailable',
        reason:
          'coming: needs the remote engine on Work’s Mac Studio (a single engine there, chat through LeanZero Link)',
      },
      outcome: {
        code: 'doesNotFit',
      },
    },
    {
      id: 'pipeline:jaccl:local+workhorse',
      key: {
        kind: 'pipeline',
        nodes: ['local', 'workhorse'],
        link: 'jaccl',
      },
      nodeNames: ['Mihai Macbook', 'Work’s Mac Studio'],
      chips: [
        {
          hwModel: 'Mac16,5',
          brand: 'Apple M4 Max',
          gpuCores: 40,
        },
        {
          hwModel: 'Mac15,14',
          brand: 'Apple M3 Ultra',
          gpuCores: 60,
        },
      ],
      backend: 'pipeline_qwen4',
      supported: true,
      fit: {
        status: 'short',
        shortBytes: 14715588048,
        shortNode: 'Work’s Mac Studio',
        nodes: [
          {
            name: 'Mihai Macbook',
            needBytes: 39867734872,
            budgetBytes: 25247213630,
          },
          {
            name: 'Work’s Mac Studio',
            needBytes: 88309423168,
            budgetBytes: 73593835120,
          },
        ],
        detail: "the fork's planner (pipeline_qwen4 plan) at 2 slots: does not fit",
      },
      speed: {
        decode: {
          estimate: {
            value: 26.422645937056576,
            low: 23.675795096646397,
            high: 66.9987603246081,
          },
          measured: false,
          runs: 0,
        },
        prefill: {
          estimate: {
            value: 808.4486300425449,
            low: 724.4037617068417,
            high: 892.493498378248,
          },
          measured: false,
          runs: 0,
        },
        throughput: {
          estimate: {
            value: 55.19397151296263,
            low: 49.4561053129947,
            high: 139.9529660114036,
          },
          measured: false,
          runs: 0,
        },
        concurrency: 2,
        basis: [
          "MoE: 17% of a dense model's bandwidth use, fitted to our Flash run; up to 51% where other MoE runs reached more",
          'many requests: ×2.09 at 2 concurrent — Flash pipeline batch 2 vs one stream (2026-09-24)',
        ],
      },
      action: {
        kind: 'startSplit',
        setupMatches: true,
      },
      outcome: {
        code: 'doesNotFit',
      },
    },
    {
      id: 'single:local',
      key: {
        kind: 'single',
        nodes: ['local'],
      },
      nodeNames: ['Mihai Macbook'],
      chips: [
        {
          hwModel: 'Mac16,5',
          brand: 'Apple M4 Max',
          gpuCores: 40,
        },
      ],
      backend: 'rapid-mlx',
      supported: true,
      fit: {
        status: 'short',
        shortBytes: 83571626739,
        shortNode: 'Mihai Macbook',
        nodes: [
          {
            name: 'Mihai Macbook',
            needBytes: 104752259008,
            budgetBytes: 25247177769,
          },
        ],
        detail:
          'the mount gate refuses: model 97.5 GiB + floor 12.8 GiB exceeds available 32.5 GiB (short 77.8 GiB)',
      },
      speed: {
        decode: {
          estimate: {
            value: 18.626141707307255,
            low: 17.673344943655643,
            high: 50.45492880645262,
          },
          measured: false,
          runs: 0,
        },
        prefill: {
          estimate: {
            value: 550.1946962825864,
            low: 522.0501812115627,
            high: 578.3392113536099,
          },
          measured: false,
          runs: 0,
        },
        throughput: {
          estimate: {
            value: 33.247662947543446,
            low: 31.546920724425316,
            high: 90.06204791951791,
          },
          measured: false,
          runs: 0,
        },
        concurrency: 8,
        basis: [
          'estimated from mlx_lm runs; the single engine (Rapid-MLX, MTP drafting) has not been measured with this model yet',
          "MoE: 17% of a dense model's bandwidth use, fitted to our Flash run; up to 51% where other MoE runs reached more",
          'many requests: ×1.78 at 8 concurrent — rapid-mlx, 8 concurrent vs 1 (experiments.jsonl, 2026-08-30)',
        ],
      },
      action: {
        kind: 'mountHere',
      },
      outcome: {
        code: 'doesNotFit',
      },
    },
    {
      id: 'tensor:jaccl:local+workhorse',
      key: {
        kind: 'tensor',
        nodes: ['local', 'workhorse'],
        link: 'jaccl',
      },
      nodeNames: ['Mihai Macbook', 'Work’s Mac Studio'],
      chips: [
        {
          hwModel: 'Mac16,5',
          brand: 'Apple M4 Max',
          gpuCores: 40,
        },
        {
          hwModel: 'Mac15,14',
          brand: 'Apple M3 Ultra',
          gpuCores: 60,
        },
      ],
      backend: 'mlx_lm',
      supported: false,
      fit: {
        status: 'unknown',
        nodes: [],
        detail: 'not supported yet: goose splits qwen4_exp by layer range (pipeline) only',
      },
      speed: {
        basis: [],
      },
      action: {
        kind: 'unavailable',
        reason: 'not supported yet: goose splits qwen4_exp by layer range (pipeline) only',
      },
      outcome: {
        code: 'notSupported',
        reason: 'not supported yet: goose splits qwen4_exp by layer range (pipeline) only',
      },
    },
  ],
  badge: {
    kind: 'tooBig',
    shortBytes: 14715588048,
  },
  notes: [],
};

export const NODES: PlacementNode[] = [
  {
    id: 'local',
    name: 'Mihai Macbook',
    chip: {
      hwModel: 'Mac16,5',
      brand: 'Apple M4 Max',
      gpuCores: 40,
    },
    bandwidthGbs: 546,
    bandwidthSource: 'support.apple.com/en-us/122211',
    totalBytes: 137438953472,
    availableBytes: 34867904512,
    ceilingBytes: 115448725504,
  },
  {
    id: 'workhorse',
    name: 'Work’s Mac Studio',
    chip: {
      hwModel: 'Mac15,14',
      brand: 'Apple M3 Ultra',
      gpuCores: 60,
    },
    bandwidthGbs: 819,
    bandwidthSource: 'support.apple.com/en-us/122211',
    totalBytes: 103079215104,
    availableBytes: 80809345024,
    ceilingBytes: 83494174720,
  },
];

/** One way's runs as goose measured them: [median, slowest, fastest, runs]. */
export type MeasuredRunsFixture = readonly [number, number, number, number];

/**
 * A chat plan for `modelId` whose ways carry MEASURED figures — PLAN_27B's first candidate re-keyed
 * per way, so every other field is one goose really sent.
 */
export function measuredPlan(
  modelId: string,
  ways: ReadonlyArray<{
    key: PlacementCandidate['key'];
    decode?: MeasuredRunsFixture;
    prefill?: MeasuredRunsFixture;
  }>
): PlacementPlan {
  const base = (PLAN_27B.candidates ?? [])[0];
  const figure = (m: MeasuredRunsFixture | undefined, value: number) =>
    m
      ? {
          estimate: { value: m[0], low: m[1], high: m[2] },
          measured: true,
          runs: m[3],
          lastMeasuredMs: 1_790_000_000_000,
        }
      : { estimate: { value, low: value, high: value }, measured: false, runs: 0 };
  return {
    ...PLAN_27B,
    modelId,
    candidates: ways.map((way) => ({
      ...base,
      id: `${way.key.kind}:${way.key.nodes.join('+')}`,
      key: way.key,
      speed: {
        ...base.speed,
        decode: figure(way.decode, base.speed.decode?.estimate.value ?? 0),
        prefill: figure(way.prefill, base.speed.prefill?.estimate.value ?? 0),
      },
    })),
  };
}
