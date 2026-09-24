import { describe, expect, it } from 'vitest';
import type { NodeState, NodesResponse } from '../../acp/leanzero-link';
import {
  PERMISSION_KEY,
  SELF_KEY,
  allowsOf,
  macForPlacementNode,
  macName,
  macTarget,
  macsFrom,
  minutesAt,
  peerRefuses,
  refusedBy,
} from './macs';

const node = (overrides: Partial<NodeState>): NodeState => ({
  node_id: 'n',
  hostname: 'host',
  status: { type: 'Idle' },
  sessions_active: 0,
  updated_at: '2026-09-24T12:00:00Z',
  ...overrides,
});

const ROSTER: NodesResponse = {
  self: node({ node_id: 'self-node', hostname: 'mihai-mbp', computer_name: 'Mihai’s MacBook' }),
  peers: [
    node({
      node_id: 'studio-1',
      hostname: 'workhorse',
      computer_name: 'Work’s Mac Studio',
      allows: { manage_models: false, answer_chat: true, run_split: true },
    }),
    node({ node_id: 'old-2', hostname: 'mini', status: { type: 'Offline' } }),
  ],
};

describe('one name per Mac', () => {
  it('is the name its owner gave it, else its hostname — never both', () => {
    expect(macName({ computer_name: 'Work’s Mac Studio', hostname: 'workhorse' })).toBe(
      'Work’s Mac Studio'
    );
    expect(macName({ computer_name: '  ', hostname: 'workhorse' })).toBe('workhorse');
    expect(macName({ hostname: 'mini' })).toBe('mini');
  });

  it('this Mac first, then the roster; with no roster this Mac alone under the caller’s name', () => {
    const [self, studio, mini] = macsFrom(ROSTER, 'This Mac');
    expect(self).toMatchObject({ key: SELF_KEY, isSelf: true, name: 'Mihai’s MacBook' });
    expect(studio).toMatchObject({ key: 'studio-1', name: 'Work’s Mac Studio', online: true });
    expect(mini).toMatchObject({ key: 'old-2', name: 'mini', online: false, allows: null });

    const alone = macsFrom(null, 'This Mac');
    expect(alone).toHaveLength(1);
    expect(alone[0]).toMatchObject({ key: SELF_KEY, nodeId: null, name: 'This Mac' });
  });

  it('an op on this Mac carries no node id; a peer’s carries its Link node id', () => {
    const [self, studio] = macsFrom(ROSTER, 'This Mac');
    expect(macTarget(self)).toBeUndefined();
    expect(macTarget(studio)).toBe('studio-1');
  });
});

describe('the owner’s three switches', () => {
  it('are the three goose config keys the routes read', () => {
    expect(PERMISSION_KEY).toEqual({
      manage: 'LEANZERO_LINK_ALLOW_REMOTE_EXECUTION',
      chat: 'LEANZERO_LINK_ALLOW_CHAT_SERVING',
      split: 'LEANZERO_LINK_ALLOW_DISTRIBUTED_NODE',
    });
  });

  it('a refusal is known only from the peer’s own report — an older peer that said nothing is not refused', () => {
    const [self, studio, mini] = macsFrom(ROSTER, 'This Mac');
    expect(peerRefuses(studio, 'manage')).toBe(true);
    expect(peerRefuses(studio, 'chat')).toBe(false);
    expect(allowsOf(mini, 'manage')).toBeNull();
    expect(peerRefuses(mini, 'manage')).toBe(false);
    // This Mac never refuses itself: its own switches gate the OTHER Macs.
    expect(
      peerRefuses(
        { ...self, allows: { manage_models: false, answer_chat: false, run_split: false } },
        'manage'
      )
    ).toBe(false);
  });

  it('names the switch behind goose’s own refusal text', () => {
    expect(refusedBy('leanzero-link 403: remote model management is disabled on this node')).toBe(
      'manage'
    );
    expect(refusedBy('remote execution disabled')).toBe('manage');
    expect(
      refusedBy('chatServingDisabled: "Let my other Macs use this Mac › Answer chat" is off')
    ).toBe('chat');
    expect(
      refusedBy('servingDisabled: "Let my other Macs use this Mac › Run part of a split model"')
    ).toBe('split');
    expect(refusedBy('connection refused')).toBeNull();
  });
});

describe('placement nodes and copy minutes', () => {
  it('maps local and link:<id> to the Mac, anything else to nothing', () => {
    const macs = macsFrom(ROSTER, 'This Mac');
    expect(macForPlacementNode(macs, 'local')?.key).toBe(SELF_KEY);
    expect(macForPlacementNode(macs, 'link:studio-1')?.name).toBe('Work’s Mac Studio');
    expect(macForPlacementNode(macs, 'link:gone')).toBeNull();
    expect(macForPlacementNode(macs, 'workhorse')).toBeNull();
  });

  it('whole minutes at a measured rate, at least one', () => {
    const GB = 1024 ** 3;
    expect(minutesAt(45 * GB, GB)).toBe(1);
    expect(minutesAt(120 * GB, GB)).toBe(2);
    expect(minutesAt(1, GB)).toBe(1);
  });
});
