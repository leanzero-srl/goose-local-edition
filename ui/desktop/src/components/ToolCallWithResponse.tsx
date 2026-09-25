import { AppEvents } from '../constants/events';
import { ToolIconWithStatus, ToolCallStatus } from './ToolCallStatusIndicator';
import { getToolCallIcon } from '../utils/toolIconMapping';
import React, { useEffect, useRef, useState } from 'react';
import { ToolCallArguments, ToolCallArgumentValue } from './ToolCallArguments';
import MarkdownContent from './MarkdownContent';
import {
  ToolRequestMessageContent,
  ToolResponseMessageContent,
  NotificationEvent,
  ToolConfirmationData,
} from '../types/message';
import { cn, snakeToTitleCase } from '../utils';
import { ActivityDisclosure as ToolCallExpandable } from './activity/ActivityDisclosure';
import { toolOutcome } from './activity/toolOutcome';
import { ExternalLink } from 'lucide-react';
import { TooltipWrapper } from './settings/providers/subcomponents/buttons/TooltipWrapper';
import type { CallToolResult } from '@modelcontextprotocol/sdk/types.js';
import type { ContentBlock } from '../types/message';

import McpAppRenderer from './McpApps/McpAppRenderer';
import ToolApprovalButtons from './ToolApprovalButtons';
import { defineMessages, useIntl } from '../i18n';

const i18n = defineMessages({
  viewSubagentSession: {
    id: 'toolCallWithResponse.viewSubagentSession',
    defaultMessage: 'View subagent session',
  },
  toolDetails: {
    id: 'toolCallWithResponse.toolDetails',
    defaultMessage: 'Tool Details',
  },
  code: {
    id: 'toolCallWithResponse.code',
    defaultMessage: 'Code',
  },
  output: {
    id: 'toolCallWithResponse.output',
    defaultMessage: 'Output',
  },
  toolResultAlt: {
    id: 'toolCallWithResponse.toolResultAlt',
    defaultMessage: 'Tool result',
  },
  activityCount: {
    id: 'toolCallWithResponse.activityCount',
    defaultMessage: 'Activity ({count})',
  },
  logs: {
    id: 'toolCallWithResponse.logs',
    defaultMessage: 'Logs',
  },
  loadingSpinner: {
    id: 'toolCallWithResponse.loadingSpinner',
    defaultMessage: 'Loading spinner',
  },
  skipped: {
    id: 'toolCallWithResponse.skipped',
    defaultMessage: 'Skipped',
  },
  repeatSkipped: {
    id: 'toolCallWithResponse.repeatSkipped',
    defaultMessage: 'Skipped — identical to an earlier call this turn, same output',
  },
  repeatSameOutput: {
    id: 'toolCallWithResponse.repeatSameOutput',
    defaultMessage: 'Same call and same output as an earlier call this turn — the model was told',
  },
});

type RepeatMarker = 'skipped' | 'same_output';

// Set by the engine's repeat guard (tool_monitor.rs) and carried by the ACP adapter.
function getRepeatMarker(toolResponse?: ToolResponseMessageContent): RepeatMarker | null {
  const repeat = toolResponse?.metadata?.repeat;
  return repeat === 'skipped' || repeat === 'same_output' ? repeat : null;
}

interface ToolGraphNode {
  tool: string;
  description: string;
  depends_on: number[];
}

type UiMeta = {
  ui?: {
    resourceUri?: string;
  };
  subagent_session_id?: string;
};

type ToolResultValue = {
  content: ContentBlock[];
  structuredContent?: unknown;
  isError: boolean;
  _meta?: UiMeta;
};

type ToolResultWithMeta = {
  status?: string;
  value?: ToolResultValue & {
    _meta?: UiMeta;
  };
};

type ToolRequestWithMeta = ToolRequestMessageContent & {
  _meta?: UiMeta;
  toolCall: {
    status: 'success';
    value: {
      name: string;
      arguments?: Record<string, unknown>;
    };
  };
};

interface ToolCallWithResponseProps {
  sessionId?: string;
  isCancelledMessage: boolean;
  toolRequest: ToolRequestMessageContent;
  toolResponse?: ToolResponseMessageContent;
  notifications?: NotificationEvent[];
  isStreamingMessage?: boolean;
  isPendingApproval: boolean;
  append?: (value: string) => void;
  confirmationContent?: ToolConfirmationData;
  isApprovalClicked?: boolean;
}

function getSubagentSessionId(
  toolResponse?: ToolResponseMessageContent,
  notifications?: NotificationEvent[]
): string | null {
  const result = toolResponse?.toolResult as ToolResultWithMeta | undefined;
  const sessionId =
    result?.status === 'success' ? result?.value?._meta?.subagent_session_id : undefined;
  if (typeof sessionId === 'string') return sessionId;

  // Fallback: extract from subagent notifications (e.g. when delegate was cancelled mid-stream)
  if (notifications) {
    for (const n of notifications) {
      const message = n.message as { method?: string; params?: Record<string, unknown> };
      if (message.method !== 'notifications/message') continue;
      const data = message.params?.data;
      if (data && typeof data === 'object' && 'type' in data && 'subagent_id' in data) {
        const record = data as Record<string, unknown>;
        if (record.type === 'subagent_tool_request' && typeof record.subagent_id === 'string') {
          return record.subagent_id;
        }
      }
    }
  }

  return null;
}

function getToolResultContent(toolResult: Record<string, unknown>): ContentBlock[] {
  if (toolResult.status === 'error') {
    const error = toolResult.error;
    return [
      {
        type: 'text',
        text: typeof error === 'string' ? error : JSON.stringify(error ?? toolResult),
      },
    ];
  }
  const value = toolResult.value as ToolResultValue | undefined;
  if (!Array.isArray(value?.content)) return [];
  return value.content.filter((item) => {
    const annotations = (item as { annotations?: { audience?: string[] } }).annotations;
    return !annotations?.audience || annotations.audience.includes('user');
  });
}

interface McpAppWrapperProps {
  toolRequest: ToolRequestMessageContent;
  toolResponse?: ToolResponseMessageContent;
  sessionId: string;
  append?: (value: string) => void;
}

function McpAppWrapper({
  toolRequest,
  toolResponse,
  sessionId,
  append,
}: McpAppWrapperProps): React.ReactNode {
  const requestWithMeta = toolRequest as ToolRequestWithMeta;
  let resourceUri = requestWithMeta._meta?.ui?.resourceUri;

  if (!resourceUri && toolResponse) {
    const resultWithMeta = toolResponse.toolResult as ToolResultWithMeta;
    if (resultWithMeta?.status === 'success' && resultWithMeta.value) {
      resourceUri = resultWithMeta.value._meta?.ui?.resourceUri;
    }
  }

  // Tool names are formatted as "{extension_name}__{tool_name}".
  // Extension names can contain underscores (special chars like parentheses are normalized to "_"),
  // so we must use lastIndexOf to find the delimiter.
  // e.g., "my_server(local)" -> "my_server_local_" -> "my_server_local___get_time"
  const toolCallName =
    requestWithMeta.toolCall.status === 'success' ? requestWithMeta.toolCall.value.name : '';
  const delimiterIndex = toolCallName.lastIndexOf('__');
  const extensionName = delimiterIndex === -1 ? '' : toolCallName.substring(0, delimiterIndex);
  const toolName =
    delimiterIndex === -1 ? toolCallName : toolCallName.substring(delimiterIndex + 2);

  const toolArguments =
    requestWithMeta.toolCall.status === 'success'
      ? requestWithMeta.toolCall.value.arguments
      : undefined;

  const toolInput = { arguments: toolArguments || {} };

  const resultWithMeta = toolResponse?.toolResult as ToolResultWithMeta | undefined;
  const toolResult =
    resultWithMeta?.status === 'success' && resultWithMeta.value
      ? (resultWithMeta.value as unknown as CallToolResult)
      : undefined;

  if (!resourceUri) return null;
  if (requestWithMeta.toolCall.status !== 'success') return null;

  return (
    <div className="mt-3">
      <McpAppRenderer
        resourceUri={resourceUri}
        toolInput={toolInput}
        toolResult={toolResult}
        extensionName={extensionName}
        toolName={toolName}
        sessionId={sessionId}
        append={append}
      />
    </div>
  );
}

export default function ToolCallWithResponse({
  sessionId,
  isCancelledMessage,
  toolRequest,
  toolResponse,
  notifications,
  isStreamingMessage,
  isPendingApproval,
  append,
  confirmationContent,
  isApprovalClicked,
}: ToolCallWithResponseProps) {
  const intl = useIntl();
  // Handle both the wrapped ToolResult format and the unwrapped format
  // The server serializes ToolResult<T> as { status: "success", value: T } or { status: "error", error: string }
  const toolCallData = toolRequest.toolCall as Record<string, unknown>;
  const toolCall =
    toolCallData?.status === 'success'
      ? (toolCallData.value as { name: string; arguments: Record<string, unknown> })
      : (toolCallData as { name: string; arguments: Record<string, unknown> });

  if (!toolCall || !toolCall.name) {
    return null;
  }

  const requestWithMeta = toolRequest as ToolRequestWithMeta;
  const resultWithMeta = toolResponse?.toolResult as ToolResultWithMeta;
  const hasMcpAppResourceURI = Boolean(
    requestWithMeta._meta?.ui?.resourceUri || resultWithMeta?.value?._meta?.ui?.resourceUri
  );

  const shouldShowMcpContent = !isPendingApproval;

  const showInlineApproval = isPendingApproval && confirmationContent && sessionId;
  const repeat = getRepeatMarker(toolResponse);
  const requestMeta = toolRequest.metadata;
  const modelLabel =
    requestMeta?.titleFromModel === true &&
    typeof requestMeta.title === 'string' &&
    requestMeta.title.trim()
      ? requestMeta.title.trim()
      : undefined;
  // A Failed card says why on its face (Q-99: a Write that lost `path` showed only the repeat
  // line; "missing field `path`" sat inside the collapsed output). A declined repeat is not a failure.
  const failure = repeat === 'skipped' ? null : toolFailureText(toolResponse?.toolResult);

  return (
    <>
      <div
        className={cn(
          'w-full text-sm font-sans rounded-lg overflow-hidden border',
          showInlineApproval
            ? 'border-lz-warn bg-lz-surface text-lz-ink'
            : 'border-lz-border bg-lz-surface text-lz-ink'
        )}
      >
        <ToolCallView
          {...{
            isCancelledMessage,
            toolCall,
            modelLabel,
            toolResponse,
            notifications,
            isStreamingMessage,
          }}
        />
        {failure && (
          <div
            data-testid="tool-call-failure"
            className="border-t border-lz-border px-4 py-2 font-mono text-xs text-lz-err whitespace-pre-wrap break-words"
          >
            {failure.length > FAILURE_MAX ? `${failure.slice(0, FAILURE_MAX - 1)}…` : failure}
          </div>
        )}
        {repeat && (
          <div className="border-t border-lz-border px-4 py-2 text-xs font-medium text-lz-warn">
            {intl.formatMessage(repeat === 'skipped' ? i18n.repeatSkipped : i18n.repeatSameOutput)}
          </div>
        )}
        {/* Inline approval UI */}
        {showInlineApproval && (
          <div className="border-t border-amber-500/30">
            {confirmationContent.prompt && (
              <div className="px-4 py-2 text-sm text-amber-600 dark:text-amber-400 bg-amber-50/10">
                {confirmationContent.prompt}
              </div>
            )}
            <div className="px-4 pb-2">
              <ToolApprovalButtons
                data={{
                  id: confirmationContent.id,
                  toolName: confirmationContent.toolName,
                  prompt: confirmationContent.prompt ?? undefined,
                  sessionId,
                  isClicked: isApprovalClicked,
                }}
              />
            </div>
          </div>
        )}
      </div>

      {/* MCP App */}
      {shouldShowMcpContent && hasMcpAppResourceURI && sessionId && (
        <McpAppWrapper
          toolRequest={toolRequest}
          toolResponse={toolResponse}
          sessionId={sessionId}
          append={append}
        />
      )}
    </>
  );
}

interface ToolCallViewProps {
  isCancelledMessage: boolean;
  toolCall: {
    name: string;
    arguments: Record<string, unknown>;
  };
  /** The model's short label for this call, when the engine sent one (Q-99). */
  modelLabel?: string;
  toolResponse?: ToolResponseMessageContent;
  notifications?: NotificationEvent[];
  isStreamingMessage?: boolean;
}

interface Progress {
  progress: number;
  progressToken: string;
  total?: number;
  message?: string;
}

interface SubagentToolRequestData {
  type: 'subagent_tool_request';
  subagent_id: string;
  tool_call: {
    name: string;
    arguments?: { tool_graph?: ToolGraphNode[] };
  };
}

const isSubagentToolRequestData = (data: unknown): data is SubagentToolRequestData => {
  if (!data || typeof data !== 'object') {
    return false;
  }
  const record = data as Record<string, unknown>;
  if (record.type !== 'subagent_tool_request') {
    return false;
  }
  if (typeof record.subagent_id !== 'string') {
    return false;
  }
  if (!record.tool_call || typeof record.tool_call !== 'object') {
    return false;
  }
  const toolCall = record.tool_call as Record<string, unknown>;
  return typeof toolCall.name === 'string';
};

const formatSubagentToolCall = (data: SubagentToolRequestData): string => {
  const subagentId = data.subagent_id;
  const toolCall = data.tool_call;
  const toolCallName = toolCall.name;

  const shortId = subagentId?.split('_').pop() || subagentId;

  const parts = toolCallName.split('__').reverse();
  const toolName = parts[0] || 'unknown';
  const extensionName = parts.slice(1).reverse().join('__') || '';
  const toolGraph = toolCall.arguments?.tool_graph;

  if (toolName === 'execute_typescript' && toolGraph && toolGraph.length > 0) {
    const plural = toolGraph.length === 1 ? '' : 's';
    const header = `[subagent:${shortId}] ${toolGraph.length} tool call${plural} | execute_typescript`;
    const lines = toolGraph.map((node, idx) => {
      const deps =
        node.depends_on && node.depends_on.length > 0
          ? ` (uses ${node.depends_on.map((d) => d + 1).join(', ')})`
          : '';
      return `  ${idx + 1}. ${node.tool}: ${node.description}${deps}`;
    });
    return [header, ...lines].join('\n');
  }

  return extensionName
    ? `[subagent:${shortId}] ${toolName} | ${extensionName}`
    : `[subagent:${shortId}] ${toolName}`;
};

const logToString = (logMessage: NotificationEvent) => {
  const message = logMessage.message as { method: string; params: unknown };
  const params = message.params as Record<string, unknown>;

  if (
    params &&
    params.data &&
    typeof params.data === 'object' &&
    'type' in params.data &&
    params.data.type === 'subagent_tool_request'
  ) {
    if (isSubagentToolRequestData(params.data)) {
      return formatSubagentToolCall(params.data);
    }
  }

  // Special case for the developer system shell logs
  if (
    params &&
    params.data &&
    typeof params.data === 'object' &&
    'output' in params.data &&
    'stream' in params.data
  ) {
    return `[${params.data.stream}] ${params.data.output}`;
  }

  return typeof params.data === 'string' ? params.data : JSON.stringify(params.data);
};

const notificationToProgress = (notification: NotificationEvent): Progress => {
  const message = notification.message as { method: string; params: unknown };
  return message.params as Progress;
};

// Helper function to extract toolcall name
const getToolName = (toolCallName: string): string => {
  const lastIndex = toolCallName.lastIndexOf('__');
  if (lastIndex === -1) return toolCallName;

  return toolCallName.substring(lastIndex + 2);
};

// Helper function to extract extension name for tooltip
const getExtensionTooltip = (toolCallName: string): string | null => {
  const lastIndex = toolCallName.lastIndexOf('__');
  if (lastIndex === -1) return null;

  const extensionName = toolCallName.substring(0, lastIndex);
  if (!extensionName) return null;

  return `${extensionName} extension`;
};

// The argument keys that name what a call acts on, in the engine's order (acp/server.rs
// `summarize_tool_call`), so a card and the engine's fallback title agree on what to show.
const TELLING_ARGUMENT_KEYS = [
  'path',
  'file',
  'command',
  'query',
  'url',
  'uri',
  'name',
  'pattern',
  'source',
];
const DETAIL_MAX = 80;
// The card's face carries the error's head; the whole of it stays in the Output section.
const FAILURE_MAX = 400;

function tellingArgumentValue(args: Record<string, ToolCallArgumentValue>): string | null {
  const firstLine = (value: string) => {
    const line = value.trim().split('\n')[0];
    return line.length > DETAIL_MAX ? `${line.slice(0, DETAIL_MAX - 1)}…` : line;
  };
  for (const key of TELLING_ARGUMENT_KEYS) {
    const value = args[key];
    if (typeof value === 'string' && value.trim()) return firstLine(value);
  }
  const firstString = Object.values(args).find(
    (value): value is string => typeof value === 'string' && value.trim().length > 0
  );
  return firstString ? firstLine(firstString) : null;
}

/** The failure's own words for a Failed card: the error string, or the error result's text. */
function toolFailureText(toolResult: unknown): string | null {
  if (!toolResult || typeof toolResult !== 'object') return null;
  const record = toolResult as Record<string, unknown>;
  if (record.status === 'error') {
    const error = record.error;
    const text = typeof error === 'string' ? error : error ? JSON.stringify(error) : '';
    return text.trim() || null;
  }
  const value = record.value as ToolResultValue | undefined;
  if (value?.isError !== true || !Array.isArray(value.content)) return null;
  const text = value.content
    .map((item) => ('text' in item && typeof item.text === 'string' ? item.text : ''))
    .filter((t) => t.trim())
    .join('\n')
    .trim();
  return text || null;
}

function ToolCallView({
  isCancelledMessage,
  toolCall,
  modelLabel,
  toolResponse,
  notifications,
  isStreamingMessage = false,
}: ToolCallViewProps) {
  const intl = useIntl();
  const [responseStyle, setResponseStyle] = useState<string>('concise');

  useEffect(() => {
    // Load initial value from settings
    window.electron.getSetting('responseStyle').then(setResponseStyle);

    const handleStyleChange = () => {
      window.electron.getSetting('responseStyle').then(setResponseStyle);
    };

    window.addEventListener(AppEvents.RESPONSE_STYLE_CHANGED, handleStyleChange);

    return () => {
      window.removeEventListener(AppEvents.RESPONSE_STYLE_CHANGED, handleStyleChange);
    };
  }, []);

  const isExpandToolDetails = (() => {
    switch (responseStyle) {
      case 'concise':
        return false;
      case 'detailed':
      default:
        return true;
    }
  })();

  const isToolDetails = toolCall?.arguments && Object.entries(toolCall.arguments).length > 0;

  const loadingStatus = toolOutcome(
    toolResponse?.toolResult,
    Boolean(isStreamingMessage),
    isCancelledMessage
  );
  const toolResults = toolResponse?.toolResult ? getToolResultContent(toolResponse.toolResult) : [];

  const logs = notifications
    ?.filter((notification) => {
      const message = notification.message as { method?: string };
      return message.method === 'notifications/message';
    })
    .map(logToString);

  const progress = notifications
    ?.filter((notification) => {
      const message = notification.message as { method?: string };
      return message.method === 'notifications/progress';
    })
    .map(notificationToProgress)
    .reduce((map, item) => {
      const key = item.progressToken;
      if (!map.has(key)) {
        map.set(key, []);
      }
      map.get(key)!.push(item);
      return map;
    }, new Map<string, Progress[]>());

  const progressEntries = [...(progress?.values() || [])].map(
    (entries) => entries.sort((a, b) => b.progress - a.progress)[0]
  );

  const isRenderingProgress =
    loadingStatus === 'loading' && (progressEntries.length > 0 || (logs || []).length > 0);

  // Function to create a descriptive representation of what the tool is doing
  const getToolDescription = (): string | null => {
    const args = (toolCall.arguments ?? {}) as Record<string, ToolCallArgumentValue>;
    const toolName = getToolName(toolCall.name);

    const getStringValue = (value: ToolCallArgumentValue): string => {
      return typeof value === 'string' ? value : JSON.stringify(value);
    };

    // Generate descriptive text based on tool type
    switch (toolName) {
      case 'text_editor':
        if (args.command === 'write' && args.path) {
          return `Write · ${getStringValue(args.path)}`;
        }
        if (args.command === 'view' && args.path) {
          return `Read · ${getStringValue(args.path)}`;
        }
        if (args.command === 'str_replace' && args.path) {
          return `Edit · ${getStringValue(args.path)}`;
        }
        if (args.command && args.path) {
          return `${getStringValue(args.command)} ${getStringValue(args.path)}`;
        }
        break;

      case 'shell':
        if (modelLabel) {
          return `Shell · ${modelLabel}`;
        }
        if (args.command) {
          return `Shell · ${getStringValue(args.command).split('\n')[0]}`;
        }
        break;

      case 'search':
        if (args.name) {
          return `Search · "${getStringValue(args.name)}"`;
        }
        if (args.mimeType) {
          return `Search · ${getStringValue(args.mimeType)} files`;
        }
        break;

      case 'write':
      case 'edit':
      case 'read': {
        if (args.path) return `${snakeToTitleCase(toolName)} · ${getStringValue(args.path)}`;
        if (args.uri) {
          const uri = getStringValue(args.uri);
          const fileId = uri.replace('gdrive:///', '');
          return `Read · file ${fileId}`;
        }
        if (args.url) {
          return `Read · ${getStringValue(args.url)}`;
        }
        if (modelLabel) return `${snakeToTitleCase(toolName)} · ${modelLabel}`;
        break;
      }

      case 'create_file':
        if (args.name) {
          return `Create · ${getStringValue(args.name)}`;
        }
        break;

      case 'update_file':
        if (args.fileId) {
          return `Update · ${getStringValue(args.fileId)}`;
        }
        break;

      case 'sheets_tool': {
        if (args.operation && args.spreadsheetId) {
          const operation = getStringValue(args.operation);
          const sheetId = getStringValue(args.spreadsheetId);
          return `${operation} in sheet ${sheetId}`;
        }
        break;
      }

      case 'docs_tool': {
        if (args.operation && args.documentId) {
          const operation = getStringValue(args.operation);
          const docId = getStringValue(args.documentId);
          return `${operation} in document ${docId}`;
        }
        break;
      }

      case 'web_scrape':
        if (args.url) {
          return `Read page · ${getStringValue(args.url)}`;
        }
        break;

      case 'remember_memory':
        if (args.category && args.data) {
          return `Store · ${getStringValue(args.category)}: ${getStringValue(args.data)}`;
        }
        break;

      case 'retrieve_memories':
        if (args.category) {
          return `Retrieve · ${getStringValue(args.category)} memories`;
        }
        break;

      case 'screen_capture':
        if (args.window_title) {
          return `Capture window · "${getStringValue(args.window_title)}"`;
        }
        return `Capture screen`;

      case 'automation_script':
        if (args.language) {
          return `${getStringValue(args.language)} script`;
        }
        break;

      case 'delegate': {
        if (args.instructions) {
          const instr = getStringValue(args.instructions);
          const truncated = instr.length > 80 ? instr.substring(0, 80) + '…' : instr;
          return `Delegate · ${truncated}`;
        }
        if (args.source) {
          return `Delegate · ${getStringValue(args.source)}`;
        }
        return 'Delegate task';
      }

      case 'load': {
        if (args.source) {
          return `Load · ${getStringValue(args.source)}`;
        }
        return 'Load source';
      }

      case 'final_output':
        return 'final output';

      case 'computer_control':
        return `Computer action`;

      case 'execute_typescript': {
        const toolGraph = args.tool_graph as unknown as ToolGraphNode[] | undefined;
        if (toolGraph && Array.isArray(toolGraph) && toolGraph.length > 0) {
          if (toolGraph.length === 1) {
            return `${toolGraph[0].description}`;
          }
          if (toolGraph.length === 2) {
            return `${toolGraph[0].tool}, ${toolGraph[1].tool}`;
          }
          return `${toolGraph.length} tools used`;
        }
        return 'Execute code';
      }

      default: {
        // Any MCP tool: its name, then what it is doing — the model's label once it lands, else
        // the call's most telling argument VALUE. Argument names ("Ledger Append kind, text",
        // Q-99) say nothing about this call.
        const toolDisplayName = snakeToTitleCase(toolName);
        if (modelLabel) {
          return `${toolDisplayName} · ${modelLabel}`;
        }
        const detail = tellingArgumentValue(args);
        return detail ? `${toolDisplayName} · ${detail}` : toolDisplayName;
      }
    }

    return null;
  };

  // Get extension tooltip for the current tool
  const extensionTooltip = getExtensionTooltip(toolCall.name);

  // Extract tool label content to avoid duplication
  const getToolLabelContent = () => {
    const description = getToolDescription();
    if (description) {
      return description;
    }
    // Fallback tool name formatting
    return snakeToTitleCase(getToolName(toolCall.name));
  };
  const toolCallStatus: ToolCallStatus = loadingStatus;

  const toolLabel = (
    <span
      className={cn(
        'flex items-center gap-2 min-w-0',
        extensionTooltip && 'cursor-pointer hover:opacity-80'
      )}
    >
      <ToolIconWithStatus ToolIcon={getToolCallIcon(toolCall.name)} status={toolCallStatus} />
      <span className="truncate flex-1 min-w-0">{getToolLabelContent()}</span>
      <span className="shrink-0 text-xs text-text-secondary">
        {loadingStatus === 'pending'
          ? isCancelledMessage
            ? 'Cancelled'
            : 'No result received'
          : loadingStatus === 'loading'
            ? 'Working'
            : getRepeatMarker(toolResponse) === 'skipped'
              ? intl.formatMessage(i18n.skipped)
              : loadingStatus === 'error'
                ? 'Failed'
                : 'Completed'}
      </span>
    </span>
  );
  return (
    <ToolCallExpandable
      isStartExpanded={isRenderingProgress || isExpandToolDetails}
      isForceExpand={false}
      label={
        extensionTooltip ? (
          <TooltipWrapper tooltipContent={extensionTooltip} side="top" align="start">
            {toolLabel}
          </TooltipWrapper>
        ) : (
          toolLabel
        )
      }
    >
      {(() => {
        const code = toolCall.arguments?.code as unknown as string | undefined;
        const toolGraph = toolCall.arguments?.tool_graph as unknown as ToolGraphNode[] | undefined;

        if (
          toolCall.name === 'code_execution__execute_typescript' &&
          (typeof code === 'string' || Array.isArray(toolGraph))
        ) {
          return (
            <div className="border-t border-border-primary">
              <CodeModeView toolGraph={toolGraph} code={code} />
            </div>
          );
        }

        if (isToolDetails) {
          return (
            <div className="border-t border-border-primary">
              <ToolDetailsView toolCall={toolCall} isStartExpanded={isExpandToolDetails} />
            </div>
          );
        }

        return null;
      })()}

      {logs && logs.length > 0 && (
        <div className="border-t border-border-primary">
          <ToolLogsView
            logs={logs}
            working={loadingStatus === 'loading'}
            isStartExpanded={
              loadingStatus === 'loading' || responseStyle === 'detailed' || responseStyle === null
            }
          />
        </div>
      )}

      {toolResults.length === 0 &&
        progressEntries.length > 0 &&
        progressEntries.map((entry, index) => (
          <div className="p-3 border-t border-border-primary" key={index}>
            <ProgressBar progress={entry.progress} total={entry.total} message={entry.message} />
          </div>
        ))}

      {/* Tool Output */}
      {!isCancelledMessage && (
        <>
          {toolResults.map((result, index) => (
            <div key={index} className={cn('border-t border-border-primary')}>
              <ToolResultView
                toolCall={toolCall}
                result={result}
                isStartExpanded={isExpandToolDetails}
              />
            </div>
          ))}
        </>
      )}

      {(() => {
        if (loadingStatus === 'loading') return null;
        const subagentSessionId = getSubagentSessionId(toolResponse, notifications);
        if (!subagentSessionId) return null;
        return (
          <div className="border-t border-border-primary">
            <button
              onClick={() => {
                window.electron.createChatWindow({
                  resumeSessionId: subagentSessionId,
                  viewType: 'pair',
                });
              }}
              className="w-full flex items-center gap-2 px-4 py-2 text-xs text-text-secondary hover:text-text-primary hover:bg-background-secondary transition-colors cursor-pointer"
            >
              <ExternalLink className="w-3 h-3 flex-shrink-0" />
              <span>{intl.formatMessage(i18n.viewSubagentSession)}</span>
            </button>
          </div>
        );
      })()}
    </ToolCallExpandable>
  );
}

interface ToolDetailsViewProps {
  toolCall: {
    name: string;
    arguments: Record<string, unknown>;
  };
  isStartExpanded: boolean;
}

function ToolDetailsView({ toolCall, isStartExpanded }: ToolDetailsViewProps) {
  const intl = useIntl();
  return (
    <ToolCallExpandable
      label={<span className="pl-4 font-sans text-sm">{intl.formatMessage(i18n.toolDetails)}</span>}
      isStartExpanded={isStartExpanded}
    >
      <div className="pr-4 pl-8">
        {toolCall.arguments && (
          <ToolCallArguments args={toolCall.arguments as Record<string, ToolCallArgumentValue>} />
        )}
      </div>
    </ToolCallExpandable>
  );
}

interface CodeModeViewProps {
  toolGraph?: ToolGraphNode[];
  code?: string;
}

function CodeModeView({ toolGraph, code }: CodeModeViewProps) {
  const intl = useIntl();
  const renderGraph = () => {
    const graph = toolGraph ?? [];
    if (graph.length === 0) return null;

    const lines: string[] = [];

    graph.forEach((node, index) => {
      const deps =
        node.depends_on.length > 0 ? ` (uses ${node.depends_on.map((d) => d + 1).join(', ')})` : '';
      lines.push(`${index + 1}. ${node.tool}: ${node.description}${deps}`);
    });

    return lines.join('\n');
  };

  return (
    <div className="px-4 py-2">
      {toolGraph && (
        <pre className="font-mono text-xs text-textSubtle whitespace-pre-wrap">{renderGraph()}</pre>
      )}
      {code && (
        <div className="border-t border-border-primary -mx-4 mt-2">
          <ToolCallExpandable
            label={<span className="pl-4 font-sans text-sm">{intl.formatMessage(i18n.code)}</span>}
            isStartExpanded={false}
          >
            <MarkdownContent
              content={'```typescript\n' + code + '\n```'}
              className="whitespace-pre-wrap max-w-full overflow-x-auto"
            />
          </ToolCallExpandable>
        </div>
      )}
    </div>
  );
}

interface ToolResultViewProps {
  toolCall: {
    name: string;
    arguments: Record<string, unknown>;
  };
  result: ContentBlock;
  isStartExpanded: boolean;
}

function ToolResultView({ result, isStartExpanded }: ToolResultViewProps) {
  const intl = useIntl();
  const hasText = (c: ContentBlock): c is ContentBlock & { text: string } =>
    'text' in c && typeof (c as Record<string, unknown>).text === 'string';

  const hasImage = (c: ContentBlock): c is ContentBlock & { data: string; mimeType: string } => {
    if (!('data' in c && 'mimeType' in c)) return false;
    const mimeType = (c as Record<string, unknown>).mimeType;
    return typeof mimeType === 'string' && mimeType.startsWith('image');
  };

  const hasResource = (c: ContentBlock): c is ContentBlock & { resource: unknown } =>
    'resource' in c;

  return (
    <ToolCallExpandable
      label={<span className="pl-4 py-1 font-sans text-sm">{intl.formatMessage(i18n.output)}</span>}
      isStartExpanded={isStartExpanded}
    >
      <div className="pl-4 pr-4 py-4">
        {hasText(result) && (
          <pre className="font-mono text-xs whitespace-pre-wrap max-w-full overflow-x-auto">
            {result.text.trim()}
          </pre>
        )}
        {hasImage(result) && (
          <img
            src={`data:${result.mimeType};base64,${result.data}`}
            alt={intl.formatMessage(i18n.toolResultAlt)}
            className="max-w-full h-auto rounded-md my-2"
            onError={(e) => {
              console.error('Failed to load image');
              e.currentTarget.style.display = 'none';
            }}
          />
        )}
        {hasResource(result) && (
          <pre className="font-sans text-sm">{JSON.stringify(result, null, 2)}</pre>
        )}
      </div>
    </ToolCallExpandable>
  );
}

function SubagentLogEntry({ log }: { log: string }) {
  const subagentMatch = log.match(/^\[subagent:(\w+)\]\s*([\s\S]*)/);
  if (!subagentMatch) {
    return <span className="font-sans text-sm text-textSubtle">{log}</span>;
  }

  const [, , rest] = subagentMatch;
  const [firstLine, ...detailLines] = rest.split('\n');
  const parts = firstLine.split(' | ');
  const toolName = parts[0]?.trim() || firstLine;
  const extensionName = parts[1]?.trim();

  return (
    <div className="font-sans text-sm text-textSubtle">
      <span className="flex items-center gap-1.5">
        <span className="inline-block w-1.5 h-1.5 rounded-full bg-blue-400 flex-shrink-0" />
        <span className="font-medium text-text-secondary">{toolName}</span>
        {extensionName && <span className="text-textSubtle opacity-60">· {extensionName}</span>}
      </span>
      {detailLines.length > 0 && (
        <pre className="ml-3 mt-0.5 text-xs text-textSubtle whitespace-pre-wrap">
          {detailLines.join('\n')}
        </pre>
      )}
    </div>
  );
}

function ToolLogsView({
  logs,
  working,
  isStartExpanded,
}: {
  logs: string[];
  working: boolean;
  isStartExpanded?: boolean;
}) {
  const intl = useIntl();
  const boxRef = useRef<HTMLDivElement>(null);

  // Whenever logs update, jump to the newest entry
  useEffect(() => {
    if (boxRef.current) {
      boxRef.current.scrollTop = boxRef.current.scrollHeight;
    }
  }, [logs.length]);
  // normally we do not want to put .length on an array in react deps:
  //
  // if the objects inside the array change but length doesn't change you want updates
  //
  // in this case, this is array of strings which once added do not change so this cuts
  // down on the possibility of unwanted runs

  const subagentLogCount = logs.filter((l) => l.startsWith('[subagent:')).length;
  const labelText =
    subagentLogCount > 0
      ? intl.formatMessage(i18n.activityCount, { count: subagentLogCount })
      : intl.formatMessage(i18n.logs);

  return (
    <ToolCallExpandable
      label={
        <span className="pl-4 py-1 font-sans text-sm flex items-center">
          <span>{labelText}</span>
          {working && (
            <div className="mx-2 inline-block">
              <span
                className="inline-block animate-spin rounded-full border-2 border-t-transparent border-current"
                style={{ width: 8, height: 8 }}
                role="status"
                aria-label={intl.formatMessage(i18n.loadingSpinner)}
              />
            </div>
          )}
        </span>
      }
      isStartExpanded={isStartExpanded}
    >
      <div
        ref={boxRef}
        className={`flex flex-col items-start space-y-2 overflow-y-auto p-4 ${working ? 'max-h-[4rem]' : 'max-h-[20rem]'}`}
      >
        {logs.map((log, i) => (
          <SubagentLogEntry key={i} log={log} />
        ))}
      </div>
    </ToolCallExpandable>
  );
}

const ProgressBar = ({ progress, total, message }: Omit<Progress, 'progressToken'>) => {
  const isDeterminate = typeof total === 'number';
  const percent = isDeterminate ? Math.min((progress / total!) * 100, 100) : 0;

  return (
    <div className="w-full space-y-2">
      {message && <div className="font-sans text-sm text-textSubtle">{message}</div>}

      <div className="w-full bg-background-subtle rounded-full h-4 overflow-hidden relative">
        {isDeterminate ? (
          <div
            className="bg-primary h-full transition-all duration-300"
            style={{ width: `${percent}%` }}
          />
        ) : (
          <div className="absolute inset-0 animate-indeterminate bg-primary" />
        )}
      </div>
    </div>
  );
};
