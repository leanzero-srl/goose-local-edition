import type { GooseSessionNotification_unstable } from '@aaif/goose-sdk';
import type { RequestPermissionRequest, SessionNotification } from '@agentclientprotocol/sdk';
import type { Message } from '../api';
import { applyGooseSessionNotification } from './adapter/gooseSessionNotifications';
import { applyContentChunk, applyThoughtChunk } from './adapter/messages';
import { applyPermissionRequest as applyPermissionRequestToState } from './adapter/permissions';
import { type AcpChatStateChange, type AdapterState, cloneMessage } from './adapter/shared';
import { applyToolCall, applyToolCallUpdate } from './adapter/tools';

export type { AcpChatStateChange } from './adapter/shared';

export interface AcpSessionNotificationAdapter {
  apply(notification: SessionNotification): AcpChatStateChange[];
  applyGoose(notification: GooseSessionNotification_unstable): AcpChatStateChange[];
  applyPermissionRequest(request: RequestPermissionRequest): AcpChatStateChange[];
  getMessages(): Message[];
}

export function createAcpSessionNotificationAdapter(
  initialMessages: Message[] = []
): AcpSessionNotificationAdapter {
  const state: AdapterState = {
    messages: initialMessages.map(cloneMessage),
  };

  return {
    apply(notification) {
      return applyAcpSessionNotification(state, notification);
    },
    applyGoose(notification) {
      return applyGooseSessionNotification(state, notification);
    },
    applyPermissionRequest(request) {
      return applyPermissionRequestToState(state, request);
    },
    getMessages() {
      return state.messages.map(cloneMessage);
    },
  };
}

function applyAcpSessionNotification(
  state: AdapterState,
  notification: SessionNotification
): AcpChatStateChange[] {
  const update = notification.update;

  switch (update.sessionUpdate) {
    case 'user_message_chunk':
      return applyContentChunk(state, 'user', update);
    case 'agent_message_chunk':
      return applyContentChunk(state, 'assistant', update);
    case 'agent_thought_chunk':
      return applyThoughtChunk(state, update);
    case 'tool_call':
      return applyToolCall(state, update);
    case 'tool_call_update':
      return applyToolCallUpdate(state, update);
    case 'session_info_update':
      return [
        {
          type: 'sessionInfo',
          ...(update.title ? { name: update.title } : {}),
        },
      ];
    case 'usage_update':
      return [];
    default:
      return [];
  }
}
