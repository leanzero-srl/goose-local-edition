import { describe, expect, it } from 'vitest';
import {
  addableMlxMachines,
  machineHasMlxNode,
  mlxDeviceId,
  mlxDeviceRow,
  mlxModelShort,
  mlxRemoteDeviceRow,
  mlxServedAlias,
  sanitizeNodeLabel,
  swarmMachinesFromLink,
} from './nodes';
import type { NodeState, NodesResponse } from '../../acp/leanzero-link';

/**
 * The alias contract, pinned to THE LIVE TRUTH on this machine's config.yaml (the swarm E2E node):
 *
 *   mlx_engine.model_id:          mlx-community/Qwen3.5-9B-MLX-4bit
 *   mlx_engine.served_model_name: workhorse-qwen3.5-9b-4bit-mlx
 *   swarm.devices[0]:             { id: workhorse-mlx, model_id: workhorse-qwen3.5-9b-4bit-mlx,
 *                                   weight: 2, enabled: true, instances: 1, engine: mlx-sidecar }
 *
 * The derivation must reproduce that row exactly — the engine serves the alias and the device's
 * model_id must equal it, so a divergence here is a node that can never route.
 */
describe('mlx node derivations', () => {
  const HF = 'mlx-community/Qwen3.5-9B-MLX-4bit';

  it('reproduces the live workhorse node byte-for-byte', () => {
    expect(mlxModelShort(HF)).toBe('qwen3.5-9b-4bit');
    expect(mlxServedAlias('workhorse', HF)).toBe('workhorse-qwen3.5-9b-4bit-mlx');
    expect(mlxDeviceId('workhorse')).toBe('workhorse-mlx');
    expect(mlxDeviceRow('workhorse', HF, 2)).toEqual({
      id: 'workhorse-mlx',
      model_id: 'workhorse-qwen3.5-9b-4bit-mlx',
      weight: 2,
      enabled: true,
      instances: 1,
      engine: 'mlx-sidecar',
    });
  });

  it('sanitizes labels: case, spaces, exotic characters, stray dashes', () => {
    expect(sanitizeNodeLabel('  My Studio ')).toBe('my-studio');
    expect(sanitizeNodeLabel('node_2')).toBe('node-2');
    expect(sanitizeNodeLabel('Node!!')).toBe('node');
    expect(sanitizeNodeLabel('-edge-')).toBe('edge');
  });

  it('drops the mlx token from the model short (the alias re-appends its own -mlx marker)', () => {
    expect(mlxModelShort('someone/Llama-3-8B-Instruct')).toBe('llama-3-8b-instruct');
    expect(mlxModelShort('mlx-community/Model-MLX-8bit')).toBe('model-8bit');
    // a bare repo id without a publisher still works
    expect(mlxModelShort('Qwen3.5-9B-MLX-4bit')).toBe('qwen3.5-9b-4bit');
  });

  it('a REMOTE machine row is the same shape plus host — never rendered reachable', () => {
    expect(mlxRemoteDeviceRow('mihai', HF, 3)).toEqual({
      id: 'mihai-mlx',
      model_id: 'mihai-qwen3.5-9b-4bit-mlx',
      weight: 3,
      enabled: true,
      instances: 1,
      engine: 'mlx-sidecar',
      host: 'mihai',
    });
  });
});

/**
 * Q-130: the Macs a LeanZero MLX node can be made for come from what the product discovers — the
 * LeanZero Link roster (this Mac, then every linked peer) — never from LM Studio's `lms ps`. The
 * owner's cap holds: one MLX node per Mac.
 */
const node = (over: Partial<NodeState> & Pick<NodeState, 'node_id' | 'hostname'>): NodeState => ({
  status: { type: 'Idle' },
  sessions_active: 0,
  updated_at: '2026-09-26T00:00:00Z',
  ...over,
});
const ROSTER: NodesResponse = {
  self: node({
    node_id: 'n-self',
    hostname: 'Mihai-Macbook-2',
    computer_name: 'Mihai’s MacBook Pro',
  }),
  peers: [
    node({ node_id: 'n-wh', hostname: 'Works-Mac-Studio', computer_name: 'Workhorse' }),
    node({ node_id: 'n-gb', hostname: 'gabee-mbp', status: { type: 'Offline' } }),
  ],
};

describe('swarmMachinesFromLink — the discovery source', () => {
  it('this Mac first, then every linked peer, labelled by the Mac’s one name', () => {
    const out = swarmMachinesFromLink(ROSTER);
    expect(out).toEqual([
      {
        machine: 'mihais-macbook-pro',
        name: 'Mihai’s MacBook Pro',
        local: true,
        names: ['mihais-macbook-pro', 'mihai-macbook-2'],
      },
      {
        machine: 'workhorse',
        name: 'Workhorse',
        local: false,
        names: ['workhorse', 'works-mac-studio'],
      },
      // a peer that predates computer_name is named by its hostname — once
      { machine: 'gabee-mbp', name: 'gabee-mbp', local: false, names: ['gabee-mbp'] },
    ]);
  });

  it('Link not connected answers with this Mac alone — so this Mac alone is offered', () => {
    const out = swarmMachinesFromLink({ self: ROSTER.self, peers: [] });
    expect(out.map((m) => [m.machine, m.local])).toEqual([['mihais-macbook-pro', true]]);
  });
});

describe('addableMlxMachines — the one-node-per-Mac cap', () => {
  const machines = swarmMachinesFromLink(ROSTER);

  it('three linked Macs offer exactly three', () => {
    expect(addableMlxMachines(machines, []).map((m) => m.machine)).toEqual([
      'mihais-macbook-pro',
      'workhorse',
      'gabee-mbp',
    ]);
  });

  it('any local MLX row takes this Mac’s slot whatever its label; a peer’s row by its host', () => {
    const devices = [
      // an older local node whose label is not the Mac's name — still this Mac's engine
      { id: 'mihai-mlx', model_id: 'mihai-x-mlx', weight: 2, enabled: true, engine: 'mlx-sidecar' },
      // a peer row written under the peer's hostname
      {
        id: 'wh-mlx',
        model_id: 'wh-x-mlx',
        weight: 2,
        enabled: true,
        engine: 'mlx-sidecar',
        host: 'works-mac-studio',
      },
    ];
    expect(addableMlxMachines(machines, devices).map((m) => m.machine)).toEqual(['gabee-mbp']);
    expect(machineHasMlxNode(machines[0], devices)).toBe(true);
    expect(machineHasMlxNode(machines[1], devices)).toBe(true);
    expect(machineHasMlxNode(machines[2], devices)).toBe(false);
  });

  it('a cloud row never takes a Mac’s slot (only MLX is capped)', () => {
    const devices = [
      {
        id: 'zai-glm',
        model_id: 'glm-5.3-flash',
        weight: 2,
        enabled: true,
        provider: 'zai',
        host: 'zai',
      },
    ];
    expect(addableMlxMachines(machines, devices)).toHaveLength(3);
  });
});
