import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { MlxRemoteSingleStatus } from '../../acp/mlx-remote-single';

let route: MlxRemoteSingleStatus | null = null;
let readError: string | null = null;
const mockStop = vi.fn();
const mockUnmount = vi.fn();
vi.mock('../../acp/mlx-remote-single', () => ({
  latestMlxRemoteSingleStatus: () => route,
  latestMlxRemoteSingleReadError: () => readError,
  mlxRemoteSingleStop: (...a: unknown[]) => mockStop(...a),
}));
vi.mock('../../acp/mlx-engine', () => ({
  mlxEngineUnmount: (...a: unknown[]) => mockUnmount(...a),
}));

const { dismissPeerHeld, dropRoute, latestPeerHeld, routeUnreachable } =
  await import('./routeSwitch');

const STUDIO: MlxRemoteSingleStatus = {
  state: 'ready',
  peer: 'worksmacstudio-lan-6a972f',
  peerHostname: 'WorksMacStudio.lan',
  peerComputerName: "Work's Mac Studio",
};

beforeEach(() => {
  vi.clearAllMocks();
  dismissPeerHeld();
  route = STUDIO;
  readError = null;
  mockStop.mockResolvedValue({ unmounted: false, unmountError: null, status: { state: 'off' } });
});

describe('dropRoute — the one way chat leaves a route', () => {
  it('a Mac that is not answering: the route goes on THIS Mac at once; the switch never waits on that Mac', async () => {
    route = { ...STUDIO, state: 'reconnecting' };
    expect(routeUnreachable()).toBe(true);
    let answer: (v?: unknown) => void = () => undefined;
    mockUnmount.mockReturnValue(new Promise((r) => (answer = r)));
    const drop = dropRoute();
    await drop.routeGone;
    expect(mockStop).toHaveBeenCalledWith(true);
    await vi.waitFor(() =>
      expect(latestPeerHeld()).toEqual({
        phase: 'asking',
        peerNodeId: STUDIO.peer,
        peerName: "Work's Mac Studio",
      })
    );
    expect(mockUnmount).toHaveBeenCalledWith(STUDIO.peer);
    answer();
    await drop.settled;
    expect(latestPeerHeld()).toBeNull();
  });

  it('a refused free is a quiet held fact with the peer’s words — never a thrown switch', async () => {
    readError = 'remoteSingleStatus: no answer';
    expect(routeUnreachable()).toBe(true);
    mockUnmount.mockRejectedValue(new Error('not connected to the mesh'));
    const drop = dropRoute();
    await expect(drop.routeGone).resolves.toBeUndefined();
    await drop.settled;
    expect(latestPeerHeld()).toMatchObject({ phase: 'held', detail: 'not connected to the mesh' });
  });

  it('a reachable route: the route’s Stop as before; a kept model is held, not a failure', async () => {
    mockStop.mockResolvedValue({
      unmounted: false,
      unmountError: "Work's Mac Studio's engine was left mounted: timeout",
      status: { state: 'off' },
    });
    const drop = dropRoute();
    await expect(drop.routeGone).resolves.toBeUndefined();
    expect(mockStop).toHaveBeenCalledWith(false);
    expect(mockUnmount).not.toHaveBeenCalled();
    expect(latestPeerHeld()).toMatchObject({ phase: 'held', peerName: "Work's Mac Studio" });
  });

  it('a Mac whose goose is gone: the route is withdrawn and that Mac is never asked (Q-111)', async () => {
    route = { ...STUDIO, state: 'reconnecting' };
    const drop = dropRoute('gone');
    await drop.routeGone;
    await drop.settled;
    expect(mockStop).toHaveBeenCalledWith(true);
    expect(mockUnmount).not.toHaveBeenCalled();
    // No "still holds the model" about a Mac whose engine went with its goose.
    expect(latestPeerHeld()).toBeNull();
  });

  it('only a route that could not be withdrawn (another window owns it) is an error', async () => {
    mockStop.mockRejectedValue(new Error('remoteSingleActive: another goose window'));
    const drop = dropRoute('unreachable');
    await expect(drop.routeGone).rejects.toThrow('remoteSingleActive');
    await drop.settled;
    expect(mockUnmount).not.toHaveBeenCalled();
    expect(latestPeerHeld()).toBeNull();
  });
});
