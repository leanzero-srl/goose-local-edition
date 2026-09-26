import type {
  CreateElicitationRequest,
  CreateElicitationResponse,
  ElicitationContentValue,
  ElicitationSchema,
} from '@agentclientprotocol/sdk';
import { v7 as uuidv7 } from 'uuid';
import { acpChatSessionActions, acpElicitationUserInputRequestId } from './chatSessionStore';

type SessionScopedFormElicitationRequest = CreateElicitationRequest & {
  mode: 'form';
  sessionId: string;
  requestedSchema: ElicitationSchema;
};

export interface AcpElicitationRequest {
  id: string;
  sessionId: string;
  request: SessionScopedFormElicitationRequest;
}

interface PendingElicitationRequest {
  request: AcpElicitationRequest;
  resolve: (response: CreateElicitationResponse) => void;
  timeoutId: ReturnType<typeof setTimeout>;
}

const pendingRequests = new Map<string, PendingElicitationRequest>();
export const ACP_ELICITATION_TIMEOUT_SECONDS = 300;

type PendingElicitationsListener = () => void;
const pendingListeners = new Set<PendingElicitationsListener>();

/** The live MCP elicitations still waiting for the person, oldest first. Live only: each dies with
 *  the tool call that asked, so a history replay never resurrects one. */
export function pendingAcpElicitations(sessionId?: string): AcpElicitationRequest[] {
  const out: AcpElicitationRequest[] = [];
  for (const pending of pendingRequests.values()) {
    if (!sessionId || pending.request.sessionId === sessionId) {
      out.push(pending.request);
    }
  }
  return out;
}

export function subscribePendingAcpElicitations(listener: PendingElicitationsListener): () => void {
  pendingListeners.add(listener);
  return () => {
    pendingListeners.delete(listener);
  };
}

function notifyPendingChanged(): void {
  for (const listener of pendingListeners) {
    listener();
  }
}

export async function requestAcpElicitation(
  request: CreateElicitationRequest
): Promise<CreateElicitationResponse> {
  if (!isSessionScopedFormElicitation(request)) {
    return cancelledElicitationResponse();
  }

  const elicitationRequest: AcpElicitationRequest = {
    id: `acp_elicitation_${uuidv7()}`,
    sessionId: request.sessionId,
    request,
  };
  const key = elicitationRequestKey(elicitationRequest.sessionId, elicitationRequest.id);

  return new Promise<CreateElicitationResponse>((resolve) => {
    const timeoutId = setTimeout(() => {
      const pending = pendingRequests.get(key);
      if (!pending) {
        return;
      }

      pendingRequests.delete(key);
      notifyPendingChanged();
      acpChatSessionActions.setElicitationStatus(
        elicitationRequest.sessionId,
        elicitationRequest.id,
        'cancelled'
      );
      acpChatSessionActions.resolveUserInputRequest(
        elicitationRequest.sessionId,
        acpElicitationUserInputRequestId(elicitationRequest.id)
      );
      pending.resolve(cancelledElicitationResponse());
    }, ACP_ELICITATION_TIMEOUT_SECONDS * 1000);

    pendingRequests.set(key, { request: elicitationRequest, resolve, timeoutId });
    notifyPendingChanged();
    acpChatSessionActions.applyElicitationRequest(elicitationRequest);
  });
}

export function resolveAcpElicitationRequest(
  sessionId: string,
  elicitationId: string,
  userData: Record<string, unknown>
): boolean {
  const key = elicitationRequestKey(sessionId, elicitationId);
  const pending = pendingRequests.get(key);
  if (!pending) {
    return false;
  }

  pendingRequests.delete(key);
  clearTimeout(pending.timeoutId);
  notifyPendingChanged();
  acpChatSessionActions.setElicitationStatus(sessionId, elicitationId, 'submitted');
  acpChatSessionActions.resolveUserInputRequest(
    sessionId,
    acpElicitationUserInputRequestId(elicitationId)
  );
  pending.resolve(acceptedElicitationResponse(userData));
  return true;
}

export function cancelAcpElicitationRequestsForSession(sessionId: string): void {
  let changed = false;
  for (const [key, pending] of pendingRequests) {
    if (pending.request.sessionId === sessionId) {
      pendingRequests.delete(key);
      clearTimeout(pending.timeoutId);
      changed = true;
      acpChatSessionActions.setElicitationStatus(sessionId, pending.request.id, 'cancelled');
      pending.resolve(cancelledElicitationResponse());
    }
  }
  if (changed) {
    notifyPendingChanged();
  }
}

function isSessionScopedFormElicitation(
  request: CreateElicitationRequest
): request is SessionScopedFormElicitationRequest {
  return request.mode === 'form' && 'sessionId' in request && typeof request.sessionId === 'string';
}

function acceptedElicitationResponse(userData: Record<string, unknown>): CreateElicitationResponse {
  return {
    action: 'accept',
    content: userData as Record<string, ElicitationContentValue>,
  };
}

function cancelledElicitationResponse(): CreateElicitationResponse {
  return { action: 'cancel' };
}

function elicitationRequestKey(sessionId: string, elicitationId: string): string {
  return `${sessionId}\u0000${elicitationId}`;
}
