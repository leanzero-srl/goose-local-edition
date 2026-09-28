import { describe, expect, it } from 'vitest';
import { createIntl } from 'react-intl';
import {
  chatNodeAvailability,
  chatNodeIds,
  chatNodesNow,
  type GlancedNode,
  type ServingNow,
} from './chatNodeAvailability';
import { addLineText, notAddableText } from './AddChatNodeDialog';
import type { NodeGlance, NodeState, GlanceLine, GlanceWhere } from './nodeGlance';
import type { NodesConfig, ResolvedNodeDef } from './model';
import {
  CONFIG,
  NODE_CLOUD,
  NODE_FLASH,
  NODE_LOCAL_27B,
  NODE_POOL,
  NODE_SPLIT,
  NODE_STUDIO,
} from './nodeGlance.fixtures';

const intl = createIntl({ locale: 'en', defaultLocale: 'en', messages: {} });

function glanced(
  node: ResolvedNodeDef,
  state: NodeState,
  line: GlanceLine,
  where: GlanceWhere | null = null
): GlancedNode {
  const glance: NodeGlance = {
    state,
    line,
    action: null,
    where,
    figures: null,
    memory: [],
    detail: null,
    displaces: null,
  };
  return { node, glance };
}

const NOTHING: ServingNow = { node: null, mac: null, otherChat: null };
const THIS_MAC: GlanceWhere = { kind: 'thisMac' };
const STUDIO_WHERE: GlanceWhere = { kind: 'mac', name: 'Work’s Mac Studio' };

const cloudReady = glanced(
  NODE_CLOUD,
  'cloudReady',
  { kind: 'cloudAlways', provider: 'OpenRouter', endpoint: false },
  { kind: 'provider', name: 'OpenRouter' }
);
const studioServing = glanced(
  NODE_STUDIO,
  'serving',
  { kind: 'live', stage: 'running', hero: null, chat: null },
  STUDIO_WHERE
);
const flashNotLoaded = glanced(
  NODE_FLASH,
  'ready',
  { kind: 'startsIn', medianMs: 48_000, count: 3 },
  THIS_MAC
);

const words = (a: ReturnType<typeof chatNodeAvailability>) =>
  a.addable ? addLineText(intl, a.line) : notAddableText(intl, a.reason);

describe('chatNodeAvailability — who can join a chat (Q-359)', () => {
  it('before C3, a second Mac model beside the set’s one says the exact words, with no Add', () => {
    const got = chatNodeAvailability(flashNotLoaded, [studioServing, cloudReady], NOTHING);
    expect(got.addable).toBe(false);
    expect(words(got)).toBe(
      'Can’t run beside 27B · Work’s Mac Studio yet: your Macs serve goose one model at a time'
    );
  });

  it('a Mac model on the Mac the set’s one runs on says so, naming the Mac', () => {
    const here = glanced(
      NODE_LOCAL_27B,
      'serving',
      { kind: 'live', stage: 'running', hero: null, chat: null },
      THIS_MAC
    );
    const got = chatNodeAvailability(flashNotLoaded, [here], NOTHING);
    expect(words(got)).toBe(
      'Runs on this Mac, where 27B · this Mac answers this chat. A Mac runs one model at a time for goose'
    );
  });

  it('a split lead holds every Mac of the split', () => {
    const split = glanced(
      NODE_SPLIT,
      'serving',
      { kind: 'live', stage: 'running', hero: null, chat: null },
      { kind: 'split', count: 2 }
    );
    const got = chatNodeAvailability(flashNotLoaded, [split], NOTHING);
    expect(words(got)).toBe(
      '27B Atlassian · both Macs runs across both Macs, so no Mac is free for another model'
    );
  });

  it('a cloud node joins a Mac model; a Mac model joins a cloud lead', () => {
    const cloud = chatNodeAvailability(cloudReady, [studioServing], NOTHING);
    expect(cloud).toEqual({
      addable: true,
      line: { kind: 'glance', line: cloudReady.glance.line },
    });
    expect(words(cloud)).toBe('Always available · billed by OpenRouter');
    const flash = chatNodeAvailability(flashNotLoaded, [cloudReady], NOTHING);
    expect(flash.addable).toBe(true);
    expect(words(flash)).toBe('Not loaded · starts in about 48s · median of 3 loads');
  });

  it('a first start not measured is said so — never an estimate', () => {
    const fresh = glanced(NODE_FLASH, 'ready', { kind: 'firstStart' }, THIS_MAC);
    expect(words(chatNodeAvailability(fresh, [], NOTHING))).toBe('First start not measured yet');
  });

  it('a node that cannot run keeps the Nodes page’s own words and cannot be added', () => {
    const away = glanced(
      NODE_STUDIO,
      'cantRun',
      { kind: 'notConnected', mac: 'Work’s Mac Studio' },
      STUDIO_WHERE
    );
    const got = chatNodeAvailability(away, [cloudReady], NOTHING);
    expect(got).toEqual({
      addable: false,
      reason: { kind: 'glance', state: 'cantRun', line: away.glance.line },
    });
    expect(words(got)).toBe('Work’s Mac Studio is not connected to LeanZero Link');
  });

  it('a node whose way would stop another chat’s answer says whose — and stays addable', () => {
    const displaced = glanced(
      NODE_FLASH,
      'displaced',
      { kind: 'startsIn', medianMs: 48_000, count: 3 },
      THIS_MAC
    );
    const got = chatNodeAvailability(displaced, [cloudReady], {
      node: '27B · Work’s Mac Studio',
      mac: { kind: 'mac', name: 'Work’s Mac Studio' },
      otherChat: 'Release notes',
    });
    expect(got.addable).toBe(true);
    expect(words(got)).toBe(
      'Work’s Mac Studio is answering chat “Release notes” on 27B · Work’s Mac Studio. Adding it stops that after its answer'
    );
    // The engine answering THIS chat (or none) is no other chat's answer.
    expect(words(chatNodeAvailability(displaced, [cloudReady], NOTHING))).toBe(
      'Not loaded · starts in about 48s · median of 3 loads'
    );
  });

  it('a second name for the same way and model is the same engine: addable', () => {
    const twin = glanced(
      { ...NODE_STUDIO, def: { ...NODE_STUDIO.def, id: 'studio-twin', name: 'Studio twin' } },
      'serving',
      { kind: 'live', stage: 'running', hero: null, chat: null },
      STUDIO_WHERE
    );
    expect(chatNodeAvailability(twin, [studioServing], NOTHING).addable).toBe(true);
  });

  it('a node that follows this Mac’s engine is a Mac model too', () => {
    const pool = glanced(NODE_POOL, 'follows', { kind: 'follows', serving: null }, THIS_MAC);
    const got = chatNodeAvailability(pool, [studioServing], NOTHING);
    expect(got.addable).toBe(false);
  });
});

describe('chatNodesNow — what the chat runs on', () => {
  const withSet: NodesConfig = {
    ...CONFIG,
    strategies: [
      ...(CONFIG.strategies ?? []),
      {
        id: 'chat-7',
        name: 'This chat’s nodes (7)',
        chat: '7',
        roles: {
          chat: { chain: [{ node: NODE_SPLIT.def.id, weight: 1 }], when: 'failover' },
          build: {
            chain: [
              { node: NODE_SPLIT.def.id, weight: 1 },
              { node: NODE_CLOUD.def.id, weight: 1 },
            ],
            when: 'share',
          },
        },
      },
    ],
  };

  it('its own set, one node, a named strategy’s chat chain, or none by name', () => {
    expect(chatNodesNow(withSet, '7', 'strategy:chat-7')).toEqual({
      kind: 'set',
      strategyId: 'chat-7',
      nodes: [NODE_SPLIT.def.id, NODE_CLOUD.def.id],
      answerOnNext: false,
    });
    expect(chatNodesNow(withSet, '7', `node:${NODE_FLASH.def.id}`)).toEqual({
      kind: 'node',
      node: NODE_FLASH.def.id,
    });
    const named = chatNodesNow(withSet, '7', 'strategy:everyday');
    expect(named.kind).toBe('strategy');
    expect(chatNodeIds(named)).toEqual([NODE_SPLIT.def.id, NODE_CLOUD.def.id]);
    expect(named.kind === 'strategy' && named.answerOnNext).toBe(true);
    expect(chatNodesNow(withSet, '7', 'swarm')).toEqual({ kind: 'none' });
    expect(chatNodesNow(withSet, '7', null)).toEqual({ kind: 'none' });
  });

  it('another chat’s set is never this chat’s', () => {
    expect(chatNodesNow(withSet, '8', 'strategy:chat-7')).toEqual({ kind: 'none' });
  });
});
