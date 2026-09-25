import type { ToolCall, ToolCallUpdate } from '@agentclientprotocol/sdk';
import type { TokenState } from '../../types/chat';
import type { Message, NotificationEvent } from '../../types/message';

export type AcpChatStateChange =
  | { type: 'messages'; messages: Message[] }
  | { type: 'tokenState'; tokenState: Partial<TokenState> }
  | {
      type: 'sessionInfo';
      name?: string;
      activeRunId?: string | null;
    }
  | { type: 'localSteerConfirmed'; messageId: string }
  | { type: 'notification'; notification: NotificationEvent };

export interface AdapterState {
  messages: Message[];
  localSteerTextByMessageId: Map<string, string>;
}

export interface GooseMessageMeta {
  messageId?: string;
  created?: number;
  steer?: boolean;
}

export interface ToolIdentity {
  toolName?: string;
  extensionName?: string;
  /** The update's title is the model's short label for the call, not the engine's
   *  name-and-argument fallback (acp/server.rs `with_title_from_model_meta`). */
  titleFromModel?: boolean;
}

export const DEFAULT_VISIBLE_MESSAGE_METADATA: Message['metadata'] = {
  userVisible: true,
  agentVisible: true,
};

export function messagesChange(state: AdapterState): AcpChatStateChange[] {
  return [{ type: 'messages', messages: state.messages.map(cloneMessage) }];
}

export function cloneMessage(message: Message): Message {
  return {
    ...message,
    content: message.content.map((content) => ({ ...content })),
    metadata: { ...message.metadata },
  };
}

export function getGooseMessageMeta(update: { _meta?: unknown }): GooseMessageMeta {
  if (!isRecord(update._meta)) {
    return {};
  }

  const goose = update._meta.goose;
  if (!isRecord(goose)) {
    return {};
  }

  return {
    created: typeof goose.created === 'number' ? goose.created : undefined,
    messageId: typeof goose.messageId === 'string' ? goose.messageId : undefined,
    steer: goose.steer === true ? true : undefined,
  };
}

export function getGooseActiveRunId(update: { _meta?: unknown }): string | null | undefined {
  if (!isRecord(update._meta)) {
    return undefined;
  }

  const goose = update._meta.goose;
  if (!isRecord(goose) || !('activeRunId' in goose)) {
    return undefined;
  }

  return typeof goose.activeRunId === 'string' || goose.activeRunId === null
    ? goose.activeRunId
    : undefined;
}

export function rawInputToArguments(rawInput: unknown): Record<string, unknown> {
  return isRecord(rawInput) ? rawInput : {};
}

export function toolIdentity(update: ToolCall | ToolCallUpdate): ToolIdentity {
  if (!isRecord(update._meta)) {
    return {};
  }

  const goose = update._meta.goose;
  if (!isRecord(goose)) {
    return {};
  }
  const titleFromModel = goose.toolTitleFromModel === true ? { titleFromModel: true } : {};
  if (!isRecord(goose.toolCall)) {
    return titleFromModel;
  }

  return {
    toolName: typeof goose.toolCall.toolName === 'string' ? goose.toolCall.toolName : undefined,
    extensionName:
      typeof goose.toolCall.extensionName === 'string' ? goose.toolCall.extensionName : undefined,
    ...titleFromModel,
  };
}

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}
