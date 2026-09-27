import { describe, expect, it } from 'vitest';
import fixture from '../../../../../crates/goose/src/nodes/nodes.fixture.json';
import {
  effectiveEntry,
  effectiveRole,
  formatRouteModel,
  inheritsFrom,
  newChatsModel,
  parseRouteModel,
  placementMacs,
  ROLES,
  validId,
  type NodeRole,
  type NodeStrategyRoles,
  type NodesConfig,
  type RouteModel,
} from './model';

describe('the nodes model — the fixture goosed is pinned to', () => {
  it('reads every fixture config as the typed shape and writes it back unchanged', () => {
    expect(fixture.configs.length).toBeGreaterThanOrEqual(3);
    for (const { name, config } of fixture.configs) {
      const typed = config as NodesConfig;
      expect(JSON.parse(JSON.stringify(typed)), name).toEqual(config);
      expect(typed.version, name).toBe(1);
    }
  });

  it('parses and formats every model id of the grammar exactly as the router does', () => {
    for (const { id, route } of fixture.modelIds) {
      const parsed = parseRouteModel(id);
      expect(parsed, id).toEqual(route);
      if (parsed) expect(formatRouteModel(parsed)).toBe(id);
    }
  });

  it('refuses malformed ids rather than reading them as another node (negative controls)', () => {
    const malformed = ['node:', 'node:a:b', 'node:a@b', 'strategy:x@nope', 'strategy:', ' swarm'];
    for (const id of malformed) expect(parseRouteModel(id), id).toBeNull();
    expect(validId('a b')).toBe(true);
    expect(validId(' a')).toBe(false);
    expect(validId('')).toBe(false);
  });

  it('inherits every role exactly as the fixture says', () => {
    expect(fixture.effectiveRole.length).toBe(42);
    for (const { set, role, expect: expected } of fixture.effectiveRole) {
      const roles: NodeStrategyRoles = {};
      for (const r of set as NodeRole[]) {
        roles[r] = { chain: [{ node: 'n', weight: 1 }], when: 'failover', ifNotLoaded: 'load' };
      }
      expect(effectiveRole(roles, role as NodeRole), `${role} in [${set.join(', ')}]`).toBe(
        expected
      );
    }
  });

  it('covers every role, and Chat and Build are the only pair that inherit from each other', () => {
    expect([...ROLES].sort()).toEqual(
      ['backend', 'build', 'chat', 'frontend', 'planning', 'testing'].sort()
    );
    const cycles = ROLES.filter((r) => inheritsFrom(inheritsFrom(r)) === r);
    expect(cycles.sort()).toEqual(['build', 'chat']);
  });

  it('writes the global default for each "new chats start on"', () => {
    expect(newChatsModel({ kind: 'auto' })).toBe('swarm');
    expect(newChatsModel({ kind: 'node', id: 'flash' })).toBe('node:flash');
    expect(newChatsModel({ kind: 'strategy', id: 'everyday' })).toBe('strategy:everyday');
  });

  it('gives a pinned way its Macs and a follows node none', () => {
    expect(placementMacs({ kind: 'follows' })).toBeNull();
    expect(
      placementMacs({ kind: 'pipeline', macs: ['local', 'link:studio'], link: 'jaccl' })
    ).toEqual({ macs: ['local', 'link:studio'], link: 'jaccl' });
    const everyday = fixture.configs[1].config.strategies[0];
    const build = effectiveEntry(everyday as never, 'testing');
    expect(build?.when).toBe('share');
    const route: RouteModel = { kind: 'strategy', id: 'everyday', role: 'build' };
    expect(formatRouteModel(route)).toBe('strategy:everyday@build');
  });
});
