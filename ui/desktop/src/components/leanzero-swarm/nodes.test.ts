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
} from './nodes';

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
 * The owner's cap: one MLX node per swarm MACHINE. Machines come from `lms ps` (fleet-machines
 * IPC, local flag included) unioned with the LM Link model-id prefixes; already-added machines
 * leave the addable list — a 3-machine swarm offers exactly 3, then 2, then 1, then none.
 */
describe('addableMlxMachines — the machine cap', () => {
  const fleetModels = ['gabee-qwen3.8-27b', 'mihai-qwen3.8-27b', 'workhorse-qwen3.8-27b'];
  const ipc = [
    { machine: 'workhorse', local: true },
    { machine: 'mihai', local: false },
    { machine: 'gabee', local: false },
  ];

  it('a 3-machine swarm offers exactly 3, with the IPC local flag carried', () => {
    const out = addableMlxMachines(ipc, fleetModels, []);
    expect(out.map((m) => m.machine).sort()).toEqual(['gabee', 'mihai', 'workhorse']);
    expect(out.find((m) => m.machine === 'workhorse')?.local).toBe(true);
    expect(out.find((m) => m.machine === 'mihai')?.local).toBe(false);
  });

  it('an already-added machine leaves the list — by id convention OR by host', () => {
    const devices = [
      // the local convention: id '<machine>-mlx'
      { id: 'workhorse-mlx', model_id: 'workhorse-x-mlx', weight: 2, enabled: true, engine: 'mlx-sidecar' },
      // the remote shape: host names the machine
      { id: 'weird-id', model_id: 'mihai-x-mlx', weight: 2, enabled: true, engine: 'mlx-sidecar', host: 'mihai' },
    ];
    const out = addableMlxMachines(ipc, fleetModels, devices);
    expect(out.map((m) => m.machine)).toEqual(['gabee']);
    expect(machineHasMlxNode('workhorse', devices)).toBe(true);
    expect(machineHasMlxNode('mihai', devices)).toBe(true);
    expect(machineHasMlxNode('gabee', devices)).toBe(false);
  });

  it('HTTP-only discovery still yields machines (as remote) when lms is unavailable', () => {
    const out = addableMlxMachines([], fleetModels, []);
    expect(out.map((m) => m.machine).sort()).toEqual(['gabee', 'mihai', 'workhorse']);
    expect(out.every((m) => m.local === false)).toBe(true);
  });

  it('an LM Studio node with an mlx-sidecar row does not block CLOUD nodes (only MLX is capped)', () => {
    const devices = [
      { id: 'zai-glm', model_id: 'glm-5.3-flash', weight: 2, enabled: true, provider: 'zai', host: 'zai' },
    ];
    // a cloud row's host names the PROVIDER, never a machine — it must not consume a machine slot
    const out = addableMlxMachines(ipc, fleetModels, devices);
    expect(out).toHaveLength(3);
  });
});
