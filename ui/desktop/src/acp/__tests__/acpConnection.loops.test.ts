import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import raw from '../../../../../crates/goose/src/session_loops/loops.fixture.json';

/**
 * Q-228 (L4r): goosed's loop notifications must reach the window. Before this, a notification the
 * desktop defined no callback for fell to the SDK's `extNotification`, which acpConnection did not
 * define, and vanished. These drive the REAL GooseClient and the generated dispatcher over an
 * in-memory stream — the server side speaks JSON-RPC, exactly as goosed's websocket would.
 */

vi.mock('../createWebSocketStream', () => ({ createWebSocketStream: vi.fn() }));
vi.mock('../chatNotifications', () => ({
  handleAcpGooseSessionNotification: vi.fn(),
  handleAcpSessionNotification: vi.fn(),
}));

type Wire = Record<string, unknown>;

function fakeGoosed() {
  let push!: (message: Wire) => void;
  let end!: () => void;
  const readable = new ReadableStream<Wire>({
    start(controller) {
      push = (message) => controller.enqueue(message);
      end = () => controller.close();
    },
  });
  const writable = new WritableStream<Wire>({
    write(message) {
      if (message.method === 'initialize') {
        push({
          jsonrpc: '2.0',
          id: message.id,
          result: { protocolVersion: 1, agentCapabilities: {}, authMethods: [] },
        });
      }
    },
  });
  const notify = (method: string, params: unknown) => push({ jsonrpc: '2.0', method, params });
  return { stream: { readable, writable, close: vi.fn() }, notify, end };
}

async function settle(): Promise<void> {
  for (let i = 0; i < 10; i++) await new Promise((resolve) => setTimeout(resolve, 0));
}

const offer = {
  sessionId: 's1',
  loopId: 'lp_0a1b2c3d',
  n: 2,
  messageId: 'looptick_lp_0a1b2c3d_2_0190aaaa-bbbb-7ccc-8ddd-eeeeffff0000',
  prompt: 'Loop tick 2 — "Make the generator produce every class"',
};

async function connected() {
  vi.resetModules();
  const goosed = fakeGoosed();
  const { createWebSocketStream } = await import('../createWebSocketStream');
  vi.mocked(createWebSocketStream).mockReturnValue(goosed.stream as never);
  const connection = await import('../acpConnection');
  await connection.getAcpClient();
  return { goosed, connection };
}

describe('acpConnection: the loops notifications', () => {
  beforeEach(() => {
    Object.assign(window.electron as unknown as Record<string, unknown>, {
      getAcpUrl: vi.fn(async () => 'ws://127.0.0.1:1/acp'),
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('delivers loops/tickDue off the wire to the tick listener', async () => {
    const { goosed, connection } = await connected();
    const heard = vi.fn();
    connection.onLoopsTickDue(heard);

    goosed.notify('_goose/unstable/loops/tickDue', offer);
    await settle();

    expect(heard).toHaveBeenCalledTimes(1);
    expect(heard).toHaveBeenCalledWith(offer);
  });

  it('holds an offer that arrives before any listener and hands it to the first one', async () => {
    const { goosed, connection } = await connected();

    goosed.notify('_goose/unstable/loops/tickDue', offer);
    await settle();
    const late = vi.fn();
    connection.onLoopsTickDue(late);
    const later = vi.fn();
    connection.onLoopsTickDue(later);

    expect(late).toHaveBeenCalledWith(offer);
    expect(later).not.toHaveBeenCalled();
  });

  it('delivers loops/changed off the wire to the rail listener', async () => {
    const { goosed, connection } = await connected();
    const heard = vi.fn();
    connection.onLoopsChanged(heard);
    const loop = (raw as { records: Array<{ record: unknown }> }).records[0].record;

    goosed.notify('_goose/unstable/loops/changed', { sessionId: 's1', loop });
    await settle();

    expect(heard).toHaveBeenCalledTimes(1);
    expect(heard.mock.calls[0][0]).toMatchObject({ sessionId: 's1', loop: { id: 'lp_0a1b2c3d' } });
  });

  it('names a notification it has no callback for instead of dropping it silently', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    const { goosed } = await connected();

    goosed.notify('_goose/unstable/something/new', { a: 1 });
    await settle();

    expect(warn).toHaveBeenCalledWith(
      'Unhandled goose notification _goose/unstable/something/new',
      {
        a: 1,
      }
    );
  });

  it('detects a dead connection: the close listener fires and the client is no longer ready', async () => {
    const { goosed, connection } = await connected();
    const closed = vi.fn();
    connection.onAcpConnectionClosed(closed);
    expect(connection.isAcpClientReady()).toBe(true);

    goosed.end();
    await settle();

    expect(closed).toHaveBeenCalledTimes(1);
    expect(connection.isAcpClientReady()).toBe(false);
  });
});
