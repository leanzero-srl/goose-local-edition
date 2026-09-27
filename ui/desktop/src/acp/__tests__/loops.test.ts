import { beforeEach, describe, expect, it, vi } from 'vitest';
import meta from '../../../../../crates/goose/acp-meta.json';
import { getAcpClient } from '../acpConnection';
import {
  loopsControl,
  loopsGet,
  loopsList,
  loopsReady,
  loopsStart,
  loopsTemplates,
  loopsTickRefused,
  loopsUpdate,
  loopsWake,
} from '../loops';

vi.mock('../acpConnection', () => ({
  getAcpClient: vi.fn(),
}));

const extMethod = vi.fn();

describe('the loops client', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    extMethod.mockResolvedValue({});
    vi.mocked(getAcpClient).mockResolvedValue({ extMethod } as unknown as Awaited<
      ReturnType<typeof getAcpClient>
    >);
  });

  it('calls exactly the methods goosed registers, with their params', async () => {
    await loopsGet('s1');
    await loopsStart({
      sessionId: 's1',
      goal: 'Make every test pass',
      template: 'until_check',
      steps: '1. Run the check — {check}.',
      cadence: { kind: 'back_to_back' },
      stateFile: '.goose/loops/make-every-test-pass/NOW.md',
      check: 'pnpm test',
    });
    await loopsUpdate('s1', {
      goal: 'g',
      template: 'blank',
      cadence: { kind: 'self_paced' },
      stateFile: 'NOW.md',
    });
    await loopsControl('s1', 'tickNow');
    await loopsTickRefused('s1', 'lp_0a1b2c3d', 3, { kind: 'queued_message' });
    await loopsReady('s1');
    await loopsWake();
    await loopsTemplates();
    await loopsList();

    const calls = extMethod.mock.calls.map(([method]) => method as string);
    const registered = new Set(meta.methods.map((m) => m.method));
    for (const method of calls) expect(registered.has(method), method).toBe(true);
    expect(calls).toEqual([
      '_goose/unstable/loops/get',
      '_goose/unstable/loops/start',
      '_goose/unstable/loops/update',
      '_goose/unstable/loops/control',
      '_goose/unstable/loops/tickRefused',
      '_goose/unstable/loops/ready',
      '_goose/unstable/loops/wake',
      '_goose/unstable/loops/templates',
      '_goose/unstable/loops/list',
    ]);
    expect(extMethod.mock.calls[3][1]).toEqual({ sessionId: 's1', action: 'tickNow' });
    expect(extMethod.mock.calls[4][1]).toEqual({
      sessionId: 's1',
      loopId: 'lp_0a1b2c3d',
      n: 3,
      reason: { kind: 'queued_message' },
    });
  });

  it('hands a named refusal back as it came, never as an empty loop', async () => {
    extMethod.mockResolvedValue({
      refusal: { code: 'runner_absent', reason: 'The loop runner is not in this build' },
    });
    const response = await loopsControl('s1', 'pause');
    expect(response.loop).toBeUndefined();
    expect(response.refusal?.reason).toBe('The loop runner is not in this build');
  });

  it('registers both loop notifications in the generated contract', () => {
    const notifications = meta.notifications.map((n) => n.method);
    expect(notifications).toContain('_goose/unstable/loops/tickDue');
    expect(notifications).toContain('_goose/unstable/loops/changed');
  });
});
