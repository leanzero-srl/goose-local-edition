import {
  DEFAULT_GOOSE_MCP_HOST_CAPABILITIES,
  GooseClient,
  type GooseClientCallbacks,
  type LoopsChangedNotification_unstable,
  type LoopsTickDueNotification_unstable,
  type NotesChangedNotification_unstable,
  type NotesDeliverDueNotification_unstable,
} from '@aaif/goose-sdk';
import { PROTOCOL_VERSION, type InitializeResponse } from '@agentclientprotocol/sdk';
import packageJson from '../../package.json';
import {
  handleAcpGooseSessionNotification,
  handleAcpSessionNotification,
} from './chatNotifications';
import { createWebSocketStream } from './createWebSocketStream';
import { requestAcpElicitation } from './elicitationRequests';
import { requestAcpPermission } from './permissionRequests';
import { requestAcpRecipeParams } from './recipeParamRequests';

type InitializedAcpClient = {
  client: GooseClient;
  initializeResponse: InitializeResponse;
};

const ACP_INITIALIZE_TIMEOUT_MS = 10_000;

let clientPromise: Promise<InitializedAcpClient> | null = null;
let resolvedClient: InitializedAcpClient | null = null;

// Session-loop notifications (DESIGN-SESSION-LOOPS §5.1). The generated dispatcher routes them to
// the typed callbacks below; this in-module emitter hands them to the tick driver and the rail.
// A tick offer that arrives before any listener subscribed is held (latest per session) and
// handed to the first listener, so an offer is never lost to mount order.
type Listener<T> = (value: T) => void;

const tickDueListeners = new Set<Listener<LoopsTickDueNotification_unstable>>();
const undeliveredTickDue = new Map<string, LoopsTickDueNotification_unstable>();
const loopsChangedListeners = new Set<Listener<LoopsChangedNotification_unstable>>();
const connectionClosedListeners = new Set<Listener<void>>();
// Notes to another chat (Q-358): goosed offers a due note only to the windows that show its chat,
// so an offer that arrives before the driver subscribed is not held — goosed offers it again when
// the chat's turn ends or the window says again that it shows the chat.
const notesDeliverDueListeners = new Set<Listener<NotesDeliverDueNotification_unstable>>();
const notesChangedListeners = new Set<Listener<NotesChangedNotification_unstable>>();

function emit<T>(listeners: Set<Listener<T>>, value: T, what: string): void {
  for (const listener of [...listeners]) {
    try {
      listener(value);
    } catch (error) {
      console.error(`A ${what} listener threw:`, error);
    }
  }
}

export function onLoopsTickDue(listener: Listener<LoopsTickDueNotification_unstable>): () => void {
  tickDueListeners.add(listener);
  const held = [...undeliveredTickDue.values()];
  undeliveredTickDue.clear();
  for (const offer of held) {
    emit(new Set([listener]), offer, 'loops/tickDue');
  }
  return () => {
    tickDueListeners.delete(listener);
  };
}

export function onLoopsChanged(listener: Listener<LoopsChangedNotification_unstable>): () => void {
  loopsChangedListeners.add(listener);
  return () => {
    loopsChangedListeners.delete(listener);
  };
}

export function onNotesDeliverDue(
  listener: Listener<NotesDeliverDueNotification_unstable>
): () => void {
  notesDeliverDueListeners.add(listener);
  return () => {
    notesDeliverDueListeners.delete(listener);
  };
}

export function onNotesChanged(listener: Listener<NotesChangedNotification_unstable>): () => void {
  notesChangedListeners.add(listener);
  return () => {
    notesChangedListeners.delete(listener);
  };
}

export async function handleNotesDeliverDue(
  due: NotesDeliverDueNotification_unstable
): Promise<void> {
  emit(notesDeliverDueListeners, due, 'notes/deliverDue');
}

export async function handleNotesChanged(
  changed: NotesChangedNotification_unstable
): Promise<void> {
  emit(notesChangedListeners, changed, 'notes/changed');
}

/** The ACP connection ended (its `closed` settled): goosed dropped this window's tick door. */
export function onAcpConnectionClosed(listener: Listener<void>): () => void {
  connectionClosedListeners.add(listener);
  return () => {
    connectionClosedListeners.delete(listener);
  };
}

export async function handleLoopsTickDue(offer: LoopsTickDueNotification_unstable): Promise<void> {
  if (tickDueListeners.size === 0) {
    undeliveredTickDue.set(offer.sessionId, offer);
    return;
  }
  emit(tickDueListeners, offer, 'loops/tickDue');
}

export async function handleLoopsChanged(change: LoopsChangedNotification_unstable): Promise<void> {
  emit(loopsChangedListeners, change, 'loops/changed');
}

async function handleUnknownExtNotification(
  method: string,
  params: Record<string, unknown>
): Promise<void> {
  console.warn(`Unhandled goose notification ${method}`, params);
}

function createClientCallbacks(): () => GooseClientCallbacks {
  return () => ({
    requestPermission: requestAcpPermission,
    unstable_createElicitation: requestAcpElicitation,
    unstable_sessionRecipeRequestParams: requestAcpRecipeParams,
    sessionUpdate: handleAcpSessionNotification,
    unstable_sessionUpdate: handleAcpGooseSessionNotification,
    unstable_loopsTickDue: handleLoopsTickDue,
    unstable_loopsChanged: handleLoopsChanged,
    unstable_notesDeliverDue: handleNotesDeliverDue,
    unstable_notesChanged: handleNotesChanged,
    extNotification: handleUnknownExtNotification,
  });
}

function connectionEnded(): void {
  resolvedClient = null;
  clientPromise = null;
  emit(connectionClosedListeners, undefined, 'connection-closed');
}

function monitorConnection(client: GooseClient): void {
  client.closed.then(connectionEnded, connectionEnded);
}

async function withTimeout<T>(promise: Promise<T>, timeoutMs: number, message: string): Promise<T> {
  let timeoutId: ReturnType<typeof setTimeout> | null = null;
  const timeout = new Promise<T>((_, reject) => {
    timeoutId = setTimeout(() => reject(new Error(message)), timeoutMs);
  });

  try {
    return await Promise.race([promise, timeout]);
  } finally {
    if (timeoutId !== null) {
      clearTimeout(timeoutId);
    }
  }
}

async function initializeConnection(): Promise<InitializedAcpClient> {
  const wsUrl = await window.electron.getAcpUrl();
  if (!wsUrl) {
    throw new Error('ACP URL is not available');
  }

  const stream = createWebSocketStream(wsUrl);
  const client = new GooseClient(createClientCallbacks(), stream);

  try {
    const initializeResponse = await withTimeout(
      client.initialize({
        protocolVersion: PROTOCOL_VERSION,
        clientCapabilities: {
          elicitation: { form: {} },
          _meta: {
            goose: {
              mcpHostCapabilities: DEFAULT_GOOSE_MCP_HOST_CAPABILITIES,
              customNotifications: true,
              recipeParameterRequests: true,
            },
          },
        },
        clientInfo: {
          name: packageJson.name,
          version: packageJson.version,
        },
      }),
      ACP_INITIALIZE_TIMEOUT_MS,
      `ACP initialize timed out after ${ACP_INITIALIZE_TIMEOUT_MS}ms`
    );

    monitorConnection(client);
    return { client, initializeResponse };
  } catch (error) {
    stream.close();
    throw error;
  }
}

export async function getAcpClient(): Promise<GooseClient> {
  return (await getInitializedAcpClient()).client;
}

export function getAcpClientSync(): GooseClient | null {
  return resolvedClient?.client ?? null;
}

export async function getAcpInitializeResponse(): Promise<InitializeResponse> {
  return (await getInitializedAcpClient()).initializeResponse;
}

export function isAcpClientReady(): boolean {
  return resolvedClient !== null;
}

async function getInitializedAcpClient(): Promise<InitializedAcpClient> {
  if (resolvedClient) {
    return resolvedClient;
  }

  if (!clientPromise) {
    clientPromise = initializeConnection()
      .then((clientState) => {
        resolvedClient = clientState;
        return clientState;
      })
      .catch((error) => {
        clientPromise = null;
        throw error;
      });
  }

  return clientPromise;
}
