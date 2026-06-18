import type { RequestPermissionRequest, RequestPermissionResponse } from '@agentclientprotocol/sdk';
import type { Permission } from '../api';
import { createSessionScopedNotificationRouter } from './sessionScopedNotificationRouter';

interface PendingPermissionRequest {
  request: RequestPermissionRequest;
  resolve: (response: RequestPermissionResponse) => void;
}

const permissionRequestRouter = createSessionScopedNotificationRouter<RequestPermissionRequest>();
const pendingRequests = new Map<string, PendingPermissionRequest>();

export const subscribeToAcpPermissionRequests = permissionRequestRouter.subscribe;

export async function requestAcpPermission(
  request: RequestPermissionRequest
): Promise<RequestPermissionResponse> {
  const key = permissionRequestKey(request.sessionId, request.toolCall.toolCallId);
  const previous = pendingRequests.get(key);
  if (previous) {
    previous.resolve(cancelledPermissionResponse());
  }

  return new Promise<RequestPermissionResponse>((resolve) => {
    pendingRequests.set(key, { request, resolve });

    permissionRequestRouter
      .route(request)
      .then((routed) => {
        if (!routed) {
          const pending = pendingRequests.get(key);
          if (pending?.resolve === resolve) {
            pendingRequests.delete(key);
            resolve(cancelledPermissionResponse());
          }
        }
      })
      .catch((error) => {
        console.warn('Failed to route ACP permission request:', error);
        const pending = pendingRequests.get(key);
        if (pending?.resolve === resolve) {
          pendingRequests.delete(key);
          resolve(cancelledPermissionResponse());
        }
      });
  });
}

export function resolveAcpPermissionRequest(
  sessionId: string,
  toolCallId: string,
  action: Permission
): boolean {
  const key = permissionRequestKey(sessionId, toolCallId);
  const pending = pendingRequests.get(key);
  if (!pending) {
    return false;
  }

  pendingRequests.delete(key);
  pending.resolve(permissionResponseForAction(pending.request, action));
  return true;
}

export function cancelAcpPermissionRequestsForSession(sessionId: string): void {
  for (const [key, pending] of pendingRequests) {
    if (pending.request.sessionId === sessionId) {
      pendingRequests.delete(key);
      pending.resolve(cancelledPermissionResponse());
    }
  }
}

function permissionResponseForAction(
  request: RequestPermissionRequest,
  action: Permission
): RequestPermissionResponse {
  if (action === 'cancel') {
    return cancelledPermissionResponse();
  }

  const optionId = permissionOptionIdForAction(request, action);
  if (!optionId) {
    return cancelledPermissionResponse();
  }

  return {
    outcome: {
      outcome: 'selected',
      optionId,
    },
  };
}

function permissionOptionIdForAction(
  request: RequestPermissionRequest,
  action: Permission
): string | undefined {
  const kind = permissionOptionKindForAction(action);
  if (!kind) {
    return undefined;
  }

  return request.options.find((candidate) => candidate.kind === kind)?.optionId;
}

function permissionOptionKindForAction(action: Permission) {
  switch (action) {
    case 'allow_once':
      return 'allow_once';
    case 'always_allow':
      return 'allow_always';
    case 'deny_once':
      return 'reject_once';
    case 'always_deny':
      return 'reject_always';
    case 'cancel':
      return undefined;
  }
}

function cancelledPermissionResponse(): RequestPermissionResponse {
  return {
    outcome: {
      outcome: 'cancelled',
    },
  };
}

function permissionRequestKey(sessionId: string, toolCallId: string): string {
  return `${sessionId}\u0000${toolCallId}`;
}
