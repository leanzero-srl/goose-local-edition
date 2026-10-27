import { describe, expect, it } from 'vitest';
import {
  RANK_MARKER,
  macNameOfHost,
  numLayersOf,
  parseGooseRanks,
  pipelineLayers,
  sshClientOf,
} from './peerRank';
import { PeerRankWatch } from '../peerRankWatch';

const b64 = (v: unknown) => Buffer.from(JSON.stringify(v)).toString('base64');
const BOOT = 'import base64,sys;exec(base64.b64decode(sys.argv[1]))';
const PIPELINE_SPEC = {
  rank: 1,
  size: 2,
  backend: 'jaccl',
  served_id: 'qwen4-flash',
  model_dir: '/Users/w/.goose/models/qwen4-flash',
  program: 'pipelineServeFormation',
  serve_args: ['--model', '/Users/w/.goose/models/qwen4-flash', '--split', '17'],
};
const TENSOR_SPEC = {
  rank: 1,
  size: 2,
  backend: 'ring',
  served_id: 'qwen3.8-27b',
  model_dir: '/m/qwen',
  program: 'mlxLmServerTransientTail',
};
const rankLine = (
  pid: number,
  spec: unknown,
  python = '/Users/w/.goose/distributed/fork/bin/python'
) =>
  `${pid} 1 ${python} -c ${BOOT} ${b64('print(1)')} ${b64(spec)} ${RANK_MARKER} goose-distributed-owner=ab12`;

describe('parseGooseRanks — a rank of another Mac on this Mac’s process list (owner 2026-10-27)', () => {
  it('reads the rank, the model and the layer split from goose’s own command line', () => {
    const listing = [
      '1 0 /sbin/launchd',
      rankLine(501, PIPELINE_SPEC),
      // The requester’s ssh carrier and a grep that merely names the marker are not ranks.
      `600 1 /usr/bin/ssh -tt gabee echo GOOSE_RANK_PID=$$; exec python -c x ${b64(PIPELINE_SPEC)} ${RANK_MARKER}`,
      `700 1 /usr/bin/grep ${RANK_MARKER}`,
    ].join('\n');
    expect(parseGooseRanks(listing)).toEqual([
      {
        pid: 501,
        ppid: 1,
        rank: 1,
        size: 2,
        backend: 'jaccl',
        servedId: 'qwen4-flash',
        modelDir: '/Users/w/.goose/models/qwen4-flash',
        splitStarts: [17],
      },
    ]);
  });

  it('a tensor split has no layer starts; our own pid is left out', () => {
    const listing = `${rankLine(501, TENSOR_SPEC)}\n${rankLine(502, TENSOR_SPEC)}`;
    const ranks = parseGooseRanks(listing, [502]);
    expect(ranks.map((r) => [r.pid, r.splitStarts])).toEqual([[501, null]]);
  });
});

describe('pipelineLayers — the part of the model a rank runs', () => {
  it('rank 1 of a 2-way split at 17 on a 48-layer model runs 17–47; rank 0 runs 0–16', () => {
    expect(pipelineLayers(1, [17], 48)).toEqual({ first: 17, last: 47 });
    expect(pipelineLayers(0, [17], 48)).toEqual({ first: 0, last: 16 });
    expect(pipelineLayers(1, [16, 32], 48)).toEqual({ first: 16, last: 31 });
  });
  it('the last rank without the model’s layer count is not guessed', () => {
    expect(pipelineLayers(1, [17], null)).toBeNull();
  });
  it('reads num_hidden_layers at the top level or in text_config', () => {
    expect(numLayersOf('{"num_hidden_layers": 48}')).toBe(48);
    expect(numLayersOf('{"text_config": {"num_hidden_layers": 64}}')).toBe(64);
    expect(numLayersOf('not json')).toBeNull();
  });
});

describe('the requester, from the rank’s ssh session', () => {
  it('SSH_CONNECTION names the client address; a reverse name reads as its first label', () => {
    expect(
      sshClientOf('python -c x SSH_CONNECTION=100.122.51.13 51234 100.113.253.80 22 HOME=/x')
    ).toBe('100.122.51.13');
    expect(sshClientOf('python HOME=/x')).toBeNull();
    expect(macNameOfHost('works-mac-studio.tail1234.ts.net.')).toBe('works-mac-studio');
    expect(macNameOfHost(null)).toBeNull();
  });
});

describe('PeerRankWatch — main’s scan', () => {
  it('a new worker rank: named by its ssh client, its layers read; reported once per change', async () => {
    const changes: unknown[] = [];
    let listing = rankLine(501, PIPELINE_SPEC);
    const watch = new PeerRankWatch({
      listProcesses: async () => listing,
      processEnvironment: async () => 'python SSH_CONNECTION=100.122.51.13 5 100.113.253.80 22',
      reverseLookup: async () => 'workhorse.tail1234.ts.net',
      readFile: async () => '{"num_hidden_layers": 48}',
      ownPid: 1,
      onChanged: (ranks) => changes.push(ranks),
      warn: () => undefined,
    });
    await watch.scan();
    await watch.scan();
    expect(changes).toEqual([
      [
        {
          pid: 501,
          rank: 1,
          size: 2,
          modelId: 'qwen4-flash',
          backend: 'jaccl',
          requester: 'workhorse',
          layers: { first: 17, last: 47 },
        },
      ],
    ]);
    listing = '';
    await watch.scan();
    expect(changes[changes.length - 1]).toEqual([]);
  });

  it('no reverse name: the address itself; rank 0 (the requester’s own) never counts', async () => {
    const changes: unknown[] = [];
    const watch = new PeerRankWatch({
      listProcesses: async () =>
        `${rankLine(501, TENSOR_SPEC)}\n${rankLine(502, { ...TENSOR_SPEC, rank: 0 })}`,
      processEnvironment: async () => 'python SSH_CONNECTION=192.168.0.2 5 192.168.0.1 22',
      reverseLookup: async () => {
        throw new Error('ENOTFOUND');
      },
      readFile: async () => '',
      ownPid: 1,
      onChanged: (ranks) => changes.push(ranks),
      warn: () => undefined,
    });
    await watch.scan();
    expect(changes).toEqual([
      [
        {
          pid: 501,
          rank: 1,
          size: 2,
          modelId: 'qwen3.8-27b',
          backend: 'ring',
          requester: '192.168.0.2',
          layers: null,
        },
      ],
    ]);
  });
});
