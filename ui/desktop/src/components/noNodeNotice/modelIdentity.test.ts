import { describe, expect, it } from 'vitest';
import type { MlxEngineSettings } from '../../acp/mlx-engine';
import type { SwarmDeviceRow } from '../settings/swarm/golden';
import { mlxModelShort } from '../leanzero-swarm/nodes';
import { chatNodeOf, nodeNamesModel, servedRepo } from './mlxMount';
import fixture from './model_identity.fixture.json';

/**
 * The router's identity (goose-sidecar `model_identity`) and this window's mirror read the SAME
 * table: a case changed for one is changed for both, so the two cannot drift.
 */
describe('one model, three names — the fixture the router is pinned to', () => {
  it('the tag is the Add-node derivation', () => {
    for (const [repo, tag] of fixture.tags) expect(mlxModelShort(repo)).toBe(tag);
  });

  it('every way (single, remote, tensor, pipeline) matches and refuses exactly as the router does', () => {
    for (const way of ['single', 'remote', 'tensor', 'pipeline']) {
      expect(fixture.cases.some((c) => c.way === way && c.names)).toBe(true);
      expect(fixture.cases.some((c) => c.way === way && !c.names)).toBe(true);
    }
    for (const c of fixture.cases) {
      expect(
        nodeNamesModel(c.nodeId, c.nodeModelId, c.served, c.repo),
        `${c.way} · ${c.nodeId} (${c.nodeModelId}) vs ${c.served}`
      ).toBe(c.names);
    }
  });
});

const HF_27B = 'Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx';
const FLASH = 'rapid-mlx/Qwen3.8-Flash-Next-4bit';
const SETTINGS: MlxEngineSettings = {
  modelId: HF_27B,
  servedModelName: 'mihai-qwen3.8-27b-atlassian-q8-mlx',
  modelsDir: '/models',
  port: 8090,
  spawnCommand: [],
  modelProfiles: {},
};
const PINNED: SwarmDeviceRow = {
  id: 'mihai-mlx',
  model_id: 'mihai-qwen3.8-27b-atlassian-q8-mlx',
  weight: 1,
  enabled: true,
  engine: 'mlx-sidecar',
};
const FLASH_NODE: SwarmDeviceRow = {
  id: 'mihai-flash-mlx',
  model_id: 'mihai-flash-qwen3.8-flash-next-4bit-mlx',
  weight: 1,
  enabled: true,
  engine: 'mlx-sidecar',
};

describe('chatNodeOf — the router’s rule for which node takes this Mac’s engine', () => {
  it('servedRepo inverts the alias for its own model only', () => {
    expect(servedRepo(SETTINGS, 'mihai-qwen3.8-27b-atlassian-q8-mlx')).toBe(HF_27B);
    expect(servedRepo(SETTINGS, FLASH)).toBe(FLASH);
  });

  it('Q-128 11:50: the owner ran Flash across both Macs — the pinned 27B node follows it', () => {
    expect(chatNodeOf([PINNED], SETTINGS, { kind: 'split', modelId: FLASH }, FLASH)).toEqual({
      nodeId: 'mihai-mlx',
      follows: true,
    });
    expect(chatNodeOf([PINNED], SETTINGS, { kind: 'single', modelId: FLASH }, FLASH)).toEqual({
      nodeId: 'mihai-mlx',
      follows: true,
    });
  });

  it('a model nobody here started is never followed — a peer’s Mount, no record', () => {
    expect(chatNodeOf([PINNED], SETTINGS, null, FLASH)).toBeNull();
    expect(
      chatNodeOf(
        [PINNED],
        SETTINGS,
        { kind: 'remoteSingle', modelId: FLASH, peer: 'studio', peerName: 'Studio' },
        FLASH
      )
    ).toBeNull();
    expect(chatNodeOf([PINNED], SETTINGS, { kind: 'split', modelId: HF_27B }, FLASH)).toBeNull();
  });

  it('Q-128 11:57: a node that names the served model is the engine’s node, heavier pins or not', () => {
    const heavy = { ...PINNED, weight: 5 };
    expect(
      chatNodeOf([heavy, FLASH_NODE], SETTINGS, { kind: 'split', modelId: FLASH }, FLASH)
    ).toEqual({ nodeId: 'mihai-flash-mlx', follows: false });
  });

  it('Q-128 12:1x: the 27B split serving its HF id is the pinned node’s own model', () => {
    expect(chatNodeOf([PINNED], { ...SETTINGS, modelId: FLASH }, null, HF_27B)).toEqual({
      nodeId: 'mihai-mlx',
      follows: false,
    });
  });

  it('followers of one engine leave it to the heaviest, the first on a tie; disabled nodes never', () => {
    const a = { ...PINNED, id: 'a-mlx', model_id: 'a', weight: 1 };
    const b = { ...PINNED, id: 'b-mlx', model_id: 'b', weight: 3 };
    const c = { ...PINNED, id: 'c-mlx', model_id: 'c', weight: 3 };
    const off = { ...PINNED, id: 'z-mlx', model_id: 'z', weight: 9, enabled: false };
    expect(chatNodeOf([a, b, c, off], SETTINGS, { kind: 'split', modelId: FLASH }, FLASH)).toEqual({
      nodeId: 'b-mlx',
      follows: true,
    });
  });
});
