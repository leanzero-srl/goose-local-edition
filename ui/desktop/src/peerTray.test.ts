import { describe, expect, it } from 'vitest';
import fixture from './peerTray.fixture.json';
import {
  PEER_TRAY_HOLD_MS,
  controllerTrayPayload,
  parsePeerTrayRecord,
  peerTrayView,
  type PeerTrayRecord,
} from './peerTray';

const record = (patch: Partial<PeerTrayRecord> = {}): PeerTrayRecord => ({
  ...(parsePeerTrayRecord(JSON.stringify(fixture)) as PeerTrayRecord),
  ...patch,
});
const T = fixture.updatedMs;

describe('peer tray record', () => {
  it('parses the fixture the Rust relay serializes byte for byte', () => {
    const parsed = parsePeerTrayRecord(JSON.stringify(fixture));
    expect(parsed).toMatchObject({
      controller: "Work's Mac Studio",
      rank: 1,
      size: 2,
      rankPid: 7452,
      title: 'Benchmark · MiMo-V2.6-Flash · 86/150',
    });
  });

  it('refuses a half-written, foreign or future-version file', () => {
    expect(parsePeerTrayRecord('{"v":1,"contr')).toBeNull();
    expect(parsePeerTrayRecord('[]')).toBeNull();
    expect(parsePeerTrayRecord(JSON.stringify({ ...fixture, v: 2 }))).toBeNull();
    expect(parsePeerTrayRecord(JSON.stringify({ ...fixture, rank: -1 }))).toBeNull();
  });
});

describe('peerTrayView', () => {
  it('shows the controller title with this Mac role while fresh', () => {
    const read = peerTrayView(record(), T + 5_000, null);
    expect(read.expired).toBe(false);
    expect(read.view?.title).toBe(
      "Benchmark · MiMo-V2.6-Flash · 86/150 · rank 1/2 (for Work's Mac Studio)"
    );
    expect(read.view?.lines).toEqual([
      "Rank 1 of 2 for Work's Mac Studio",
      'Benchmark running: Gauntlet sb-7.2',
      'Calls: 86 of 150',
    ]);
  });

  it('composes its own title from the run facts when the controller sent none', () => {
    const loading = record({
      title: null,
      lines: [],
      state: 'starting',
      loadPhase: 'loading',
      loadedBytes: 40 * 1024 ** 3,
      plannedBytes: 100 * 1024 ** 3,
    });
    expect(peerTrayView(loading, T, null).view?.title).toBe(
      "Split · MiMo-V2.6-Flash · loading shards 40.0/100.0 GB · rank 1/2 (for Work's Mac Studio)"
    );
    expect(peerTrayView(record({ title: null, state: 'serving' }), T, null).view?.title).toBe(
      "Split · MiMo-V2.6-Flash · serving · rank 1/2 (for Work's Mac Studio)"
    );
  });

  it('a failed run names the error, then clears after the hold', () => {
    const failed = record({ state: 'failed', error: 'rank 1 died (exit 137)' });
    const read = peerTrayView(failed, T + 1_000, null);
    expect(read.view?.title).toBe("Split failed · rank 1/2 (for Work's Mac Studio)");
    expect(read.view?.lines).toContain('Error: rank 1 died (exit 137)');
    expect(peerTrayView(failed, T + PEER_TRAY_HOLD_MS + 1, null)).toEqual({
      view: null,
      expired: true,
    });
  });

  it('a stopped run says so, then clears', () => {
    const stopped = record({ state: 'stopped' });
    expect(peerTrayView(stopped, T + 10, null).view?.title).toBe(
      "Rank 1/2 stopped (for Work's Mac Studio)"
    );
    expect(peerTrayView(stopped, T + PEER_TRAY_HOLD_MS + 11, null).expired).toBe(true);
  });

  it('a rank gone from this Mac stops at once even while the record is fresh', () => {
    const gone = T + 4_000;
    expect(peerTrayView(record(), T + 5_000, gone).view?.title).toBe(
      "Rank 1/2 stopped (for Work's Mac Studio)"
    );
    expect(peerTrayView(record(), gone + PEER_TRAY_HOLD_MS + 1, gone).expired).toBe(true);
  });

  it('a stale record never keeps the old title', () => {
    const stale = T + 30_000 + 1;
    // The rank still runs: it says the controller went quiet.
    const alive = peerTrayView(record(), stale + 14_000, null);
    expect(alive.view?.title).toBe("Rank 1/2 · no word from Work's Mac Studio");
    expect(alive.view?.title).not.toContain('86/150');
    // No pid to check: stale means stopped, and then it clears.
    const noPid = record({ rankPid: null });
    expect(peerTrayView(noPid, stale, null).view?.title).toBe(
      "Rank 1/2 stopped (for Work's Mac Studio)"
    );
    expect(peerTrayView(noPid, stale + PEER_TRAY_HOLD_MS, null).expired).toBe(true);
  });
});

describe('controllerTrayPayload', () => {
  it('writes only while a run holds a title', () => {
    expect(controllerTrayPayload('Benchmark · x · 3/150', ['Calls: 3'], true, 7)).toEqual({
      title: 'Benchmark · x · 3/150',
      lines: ['Calls: 3'],
      updatedMs: 7,
    });
    expect(controllerTrayPayload('Benchmark · x', [], false, 7)).toBeNull();
    expect(controllerTrayPayload('  ', [], true, 7)).toBeNull();
  });
});
