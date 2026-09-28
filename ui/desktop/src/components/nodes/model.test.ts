import { describe, expect, it } from 'vitest';
import fixture from '../../../../../crates/goose/src/nodes/nodes.fixture.json';
import {
  chatNodeSetCount,
  chatNodeSetOf,
  chatNodesOf,
  effectiveEntry,
  effectiveRole,
  formatRouteModel,
  inheritsFrom,
  namedStrategies,
  newChatsModel,
  parseRouteModel,
  placementMacs,
  ROLES,
  validId,
  type NodeRole,
  type NodeStrategy,
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

  it('reads every chat node set back exactly as goosed builds it (Q-359)', () => {
    expect(fixture.chatNodeSets.length).toBeGreaterThanOrEqual(4);
    for (const { name, session, nodes, answerOnNext, strategy } of fixture.chatNodeSets) {
      expect(chatNodesOf(strategy as NodeStrategy), name).toEqual({
        strategyId: strategy.id,
        session,
        nodes,
        answerOnNext,
      });
      const { chat: _chat, ...named } = strategy;
      expect(chatNodesOf(named as NodeStrategy), `${name}: a named strategy`).toBeNull();
    }
  });

  it('keeps a chat node set out of every strategy list and counts it instead', () => {
    const config = fixture.configs.find((c) => c.name.startsWith('a chat'))!.config as NodesConfig;
    expect(namedStrategies(config).map((s) => s.id)).toEqual(['everyday']);
    expect(chatNodeSetCount(config)).toBe(1);
    expect(chatNodeSetCount(config, 'sonnet')).toBe(1);
    expect(chatNodeSetCount(config, 'flash-here')).toBe(0);
    expect(chatNodeSetOf(config, '20260928_7')?.nodes).toEqual(['27b-both', 'sonnet']);
    expect(chatNodeSetOf(config, 'another-chat')).toBeNull();
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
