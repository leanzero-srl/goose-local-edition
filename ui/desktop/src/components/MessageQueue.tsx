import React, { useState } from 'react';
import { X, Clock, Send, GripVertical, Zap, Sparkles, ChevronDown, ChevronUp } from 'lucide-react';
import { Button } from './ui/button';
import { ImageData } from '../types/message';
import { defineMessages, useIntl } from '../i18n';
import { TONE_FILL, cx } from './lz';
import { queueWords } from './loops/startLoopWords';

const i18n = defineMessages({
  paused: {
    id: 'messageQueue.paused',
    defaultMessage: 'Paused',
  },
  next: {
    id: 'messageQueue.next',
    defaultMessage: 'Next',
  },
  sendNow: {
    id: 'messageQueue.sendNow',
    defaultMessage: 'Send this message now',
  },
  expandQueue: {
    id: 'messageQueue.expandQueue',
    defaultMessage: 'Expand queue',
  },
  queuePausedCompact: {
    id: 'messageQueue.queuePausedCompact',
    defaultMessage: 'Queue paused - click "Send" or add new message to resume',
  },
  queuePaused: {
    id: 'messageQueue.queuePaused',
    defaultMessage: 'Queue Paused',
  },
  messageQueue: {
    id: 'messageQueue.messageQueue',
    defaultMessage: 'Message Queue',
  },
  messageCount: {
    id: 'messageQueue.messageCount',
    defaultMessage: '{count, plural, one {# message} other {# messages}} {status}',
  },
  waiting: {
    id: 'messageQueue.waiting',
    defaultMessage: 'waiting',
  },
  queued: {
    id: 'messageQueue.queued',
    defaultMessage: 'queued',
  },
  clearAll: {
    id: 'messageQueue.clearAll',
    defaultMessage: 'Clear All',
  },
  collapseQueue: {
    id: 'messageQueue.collapseQueue',
    defaultMessage: 'Collapse queue',
  },
  queuePausedExpanded: {
    id: 'messageQueue.queuePausedExpanded',
    defaultMessage: 'Queue paused by interruption. Use "Send Now" or add a new message to resume.',
  },
  save: {
    id: 'messageQueue.save',
    defaultMessage: 'Save',
  },
  cancel: {
    id: 'messageQueue.cancel',
    defaultMessage: 'Cancel',
  },
  clickToEdit: {
    id: 'messageQueue.clickToEdit',
    defaultMessage: '{content} (Click to edit)',
  },
  cannotSendWhileEditing: {
    id: 'messageQueue.cannotSendWhileEditing',
    defaultMessage: 'Cannot send while editing',
  },
  stopAndSend: {
    id: 'messageQueue.stopAndSend',
    defaultMessage: 'Stop current processing and send this message now',
  },
  removeFromQueue: {
    id: 'messageQueue.removeFromQueue',
    defaultMessage: 'Remove this message from queue',
  },
  dragToReorder: {
    id: 'messageQueue.dragToReorder',
    defaultMessage: 'Drag messages to reorder priority',
  },
  afterCompacting: {
    id: 'messageQueue.afterCompacting',
    defaultMessage: 'Sends right after compacting',
  },
});

export interface QueuedMessage {
  id: string;
  content: string;
  timestamp: number;
  images: ImageData[];
}

interface MessageQueueProps {
  queuedMessages: QueuedMessage[];
  onRemoveMessage: (id: string) => void;
  onClearQueue: () => void;
  onStopAndSend?: (messageId: string) => void;
  onEditMessage?: (messageId: string, newContent: string) => void;
  onTriggerQueueProcessing?: () => void;
  editingMessageIdRef?: React.MutableRefObject<string | null>;
  onReorderMessages?: (reorderedMessages: QueuedMessage[]) => void;
  sendingMessageIds?: ReadonlySet<string>;
  className?: string;
  isPaused?: boolean;
  /**
   * The turn running now is loop tick n (DESIGN-SESSION-LOOPS §8.1): each row says what Send now
   * does to it — it steers the tick; left alone, the message is sent after the tick, before the
   * next one.
   */
  steersTick?: number | null;
  /**
   * The chat is compacting (Q-357): a message waits for the compacted conversation — the engine
   * holds one conversation in memory, and a side request would evict it — so it cannot be sent
   * now, only after.
   */
  afterCompaction?: boolean;
}

/** "Sends right after compacting" — why the message waits, in the row's own words. */
function AfterCompactionChip() {
  const intl = useIntl();
  return (
    <span
      data-testid="queue-after-compaction"
      className={cx(
        'inline-block max-w-full break-words rounded-lz-control px-2 py-0.5 text-[11px] font-lz-semibold',
        TONE_FILL.secondary
      )}
    >
      {intl.formatMessage(i18n.afterCompacting)}
    </span>
  );
}

/**
 * "Queued · Send now steers tick {n}" — a solid chip, the row's own words for what it waits on. It
 * wraps rather than truncates: cut short in a narrow window it lost the tick's number.
 */
function SteersTickChip({ n }: { n: number }) {
  const intl = useIntl();
  return (
    <span
      data-testid="queue-steers-tick"
      className={cx(
        'inline-block max-w-full break-words rounded-lz-control px-2 py-0.5 text-[11px] font-lz-semibold',
        TONE_FILL.accent
      )}
    >
      {intl.formatMessage(queueWords.queuedSteers, { n })}
    </span>
  );
}

export const MessageQueue: React.FC<MessageQueueProps> = ({
  queuedMessages,
  onRemoveMessage,
  onClearQueue,
  onStopAndSend,
  onEditMessage,
  onTriggerQueueProcessing,
  editingMessageIdRef,
  onReorderMessages,
  sendingMessageIds,
  className = '',
  isPaused = false,
  steersTick = null,
  afterCompaction = false,
}) => {
  const intl = useIntl();
  const sendNow = afterCompaction ? undefined : onStopAndSend;
  const [isExpanded, setIsExpanded] = useState(true);
  const [draggedItem, setDraggedItem] = useState<string | null>(null);
  const [dragOverItem, setDragOverItem] = useState<string | null>(null);
  const [hoveredMessage, setHoveredMessage] = useState<string | null>(null);
  const [editingMessage, setEditingMessage] = useState<string | null>(null);
  const [editContent, setEditContent] = useState<string>('');
  const isSendingMessage = (messageId: string) => sendingMessageIds?.has(messageId) ?? false;

  if (queuedMessages.length === 0) {
    return null;
  }

  const handleDragStart = (e: React.DragEvent, messageId: string) => {
    if (isSendingMessage(messageId)) {
      e.preventDefault();
      return;
    }

    setDraggedItem(messageId);
    e.dataTransfer.effectAllowed = 'move';
    e.dataTransfer.setData('text/html', messageId);
  };

  const handleDragOver = (e: React.DragEvent, messageId: string) => {
    if (isSendingMessage(messageId)) {
      return;
    }

    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
    setDragOverItem(messageId);
  };

  const handleDragLeave = () => {
    setDragOverItem(null);
  };

  const handleDrop = (e: React.DragEvent, targetMessageId: string) => {
    e.preventDefault();

    if (!draggedItem || !onReorderMessages || isSendingMessage(targetMessageId)) return;

    const draggedIndex = queuedMessages.findIndex((msg) => msg.id === draggedItem);
    const targetIndex = queuedMessages.findIndex((msg) => msg.id === targetMessageId);

    if (draggedIndex === -1 || targetIndex === -1 || draggedIndex === targetIndex) {
      setDraggedItem(null);
      setDragOverItem(null);
      return;
    }

    const newMessages = [...queuedMessages];
    const [removed] = newMessages.splice(draggedIndex, 1);
    newMessages.splice(targetIndex, 0, removed);

    onReorderMessages(newMessages);
    setDraggedItem(null);
    setDragOverItem(null);
  };

  const handleDragEnd = () => {
    setDraggedItem(null);
    setDragOverItem(null);
  };

  const formatTimestamp = (timestamp: number) => {
    const now = Date.now();
    const diff = now - timestamp;
    if (diff < 60000) return 'now';
    if (diff < 3600000) return `${Math.floor(diff / 60000)}m`;
    return `${Math.floor(diff / 3600000)}h`;
  };

  const nextMessage = queuedMessages[0];
  const remainingCount = queuedMessages.length - 1;
  const nextMessageIsSending = isSendingMessage(nextMessage.id);
  const hasSendingMessages = queuedMessages.some((message) => isSendingMessage(message.id));

  // Compact View
  if (!isExpanded) {
    return (
      <div className={`relative ${className}`}>
        {/* Compact Header */}
        <div
          className="flex items-center justify-between px-4 py-2.5 bg-background-primary border-b border-border-primary cursor-pointer hover:bg-background-secondary transition-all duration-200"
          onClick={() => setIsExpanded(true)}
        >
          <div className="flex items-center gap-3 flex-1 min-w-0">
            <div className="flex items-center gap-2">
              {isPaused ? (
                <div className="w-2 h-2 rounded-full bg-amber-500 animate-pulse" />
              ) : (
                <div className="w-2 h-2 rounded-full bg-blue-500 animate-pulse" />
              )}
              <span className="text-sm font-medium text-text-primary">
                {isPaused ? intl.formatMessage(i18n.paused) : intl.formatMessage(i18n.next)}
              </span>
            </div>

            {/* Next message preview */}
            <div className="flex-1 min-w-0">
              <p className="text-sm text-text-secondary truncate" title={nextMessage.content}>
                {nextMessage.content.length > 40
                  ? `${nextMessage.content.substring(0, 40)}...`
                  : nextMessage.content}
              </p>
            </div>

            {afterCompaction ? (
              <AfterCompactionChip />
            ) : (
              steersTick != null && <SteersTickChip n={steersTick} />
            )}

            {/* Queue count */}
            {remainingCount > 0 && (
              <div className="flex items-center gap-1 text-xs text-text-secondary bg-background-secondary border border-border-primary px-2 py-1 rounded-full font-medium">
                <span>+{remainingCount}</span>
              </div>
            )}
          </div>

          <div className="flex items-center gap-2">
            {/* Quick Send Now button */}
            {sendNow && (
              <Button
                variant="ghost"
                size="sm"
                onClick={(e) => {
                  e.stopPropagation();
                  if (nextMessageIsSending) return;
                  sendNow(nextMessage.id);
                }}
                disabled={nextMessageIsSending}
                className="h-7 px-2 text-xs text-text-info hover:bg-background-secondary"
                title={
                  steersTick != null
                    ? intl.formatMessage(queueWords.sendNowSteers, { n: steersTick })
                    : intl.formatMessage(i18n.sendNow)
                }
              >
                <Send className="w-3 h-3" />
              </Button>
            )}

            {/* Expand button */}
            <Button
              variant="ghost"
              size="sm"
              className="h-7 w-7 p-0 text-text-secondary hover:text-text-primary"
              title={intl.formatMessage(i18n.expandQueue)}
            >
              <ChevronDown className="w-4 h-4" />
            </Button>
          </div>
        </div>

        {/* Paused state indicator */}
        {isPaused && (
          <div className={cx('px-4 py-1.5', TONE_FILL.warn)}>
            <div className="flex items-center gap-2 text-xs font-medium">
              <Zap className="w-3 h-3" />
              <span>{intl.formatMessage(i18n.queuePausedCompact)}</span>
            </div>
          </div>
        )}
      </div>
    );
  }

  // Expanded View
  return (
    <div className={`relative ${className}`}>
      {/* Expanded Header */}
      <div className="flex items-center justify-between px-4 py-3 bg-background-primary border-b border-border-primary">
        <div className="flex items-center gap-3">
          <div className="relative">
            {isPaused ? (
              <div className="flex items-center gap-2">
                <div className="w-2 h-2 rounded-full bg-amber-500 animate-pulse" />
                <Clock className="w-4 h-4 text-lz-warn" />
              </div>
            ) : (
              <div className="flex items-center gap-2">
                <div className="w-2 h-2 rounded-full bg-blue-500 animate-pulse" />
                <Sparkles className="w-4 h-4 text-lz-accent" />
              </div>
            )}
          </div>
          <div className="flex flex-col">
            <span className="text-sm font-medium text-text-primary">
              {isPaused
                ? intl.formatMessage(i18n.queuePaused)
                : intl.formatMessage(i18n.messageQueue)}
            </span>
            <span className="text-xs text-text-secondary">
              {intl.formatMessage(i18n.messageCount, {
                count: queuedMessages.length,
                status: isPaused
                  ? intl.formatMessage(i18n.waiting)
                  : intl.formatMessage(i18n.queued),
              })}
            </span>
          </div>
        </div>

        <div className="flex items-center gap-2">
          {queuedMessages.length > 1 && (
            <Button
              variant="ghost"
              size="sm"
              onClick={onClearQueue}
              disabled={hasSendingMessages}
              className="text-xs h-7 px-3 text-text-secondary hover:text-text-danger hover:bg-background-secondary transition-colors"
            >
              {intl.formatMessage(i18n.clearAll)}
            </Button>
          )}

          {/* Collapse button */}
          <Button
            variant="ghost"
            size="sm"
            onClick={() => setIsExpanded(false)}
            className="h-7 w-7 p-0 text-text-secondary hover:text-text-primary"
            title={intl.formatMessage(i18n.collapseQueue)}
          >
            <ChevronUp className="w-4 h-4" />
          </Button>
        </div>
      </div>

      {/* Status Banner for Paused State */}
      {isPaused && (
        <div className={cx('px-4 py-2', TONE_FILL.warn)}>
          <div className="flex items-center gap-2 text-sm font-medium">
            <Zap className="w-4 h-4" />
            <span>{intl.formatMessage(i18n.queuePausedExpanded)}</span>
          </div>
        </div>
      )}

      {/* Message Bubbles */}
      <div className="p-4 space-y-3 bg-background-primary max-h-80 overflow-y-auto">
        {queuedMessages.map((message, index) => {
          const isSending = isSendingMessage(message.id);
          const isEditing = editingMessage === message.id;
          return (
            <div
              key={message.id}
              className="group relative"
              draggable={Boolean(onReorderMessages && !isSending)}
              onDragStart={(e) => handleDragStart(e, message.id)}
              onDragOver={(e) => handleDragOver(e, message.id)}
              onDragLeave={handleDragLeave}
              onDrop={(e) => handleDrop(e, message.id)}
              onDragEnd={handleDragEnd}
              onMouseEnter={() => setHoveredMessage(message.id)}
              onMouseLeave={() => setHoveredMessage(null)}
            >
              {/* Main message bubble */}
              <div
                data-testid="queue-bubble"
                className={`relative flex items-center gap-3 rounded-xl px-4 py-3 border transition-colors duration-150 ease-out ${
                  isSending
                    ? 'bg-background-secondary border-dashed border-border-info cursor-wait'
                    : draggedItem === message.id
                      ? 'bg-background-secondary border-lz-accent ring-2 ring-inset ring-lz-accent'
                      : dragOverItem === message.id
                        ? 'bg-background-secondary border-green-600 ring-2 ring-inset ring-green-600'
                        : hoveredMessage === message.id
                          ? 'bg-background-tertiary border-border-primary'
                          : 'bg-background-secondary hover:bg-background-tertiary border-border-primary hover:border-border-primary'
                }`}
              >
                {/* Priority indicator */}
                <div className="flex items-center gap-2">
                  <div
                    className={`flex items-center justify-center w-6 h-6 rounded-full text-xs font-semibold transition-colors ${
                      index === 0
                        ? 'bg-lz-accent text-lz-accent-ink'
                        : 'bg-background-secondary text-text-secondary'
                    }`}
                  >
                    {index + 1}
                  </div>

                  {/* Drag handle */}
                  {onReorderMessages && !isSending && (
                    <div
                      className={`opacity-0 group-hover:opacity-100 transition-all duration-200 cursor-grab active:cursor-grabbing ${
                        hoveredMessage === message.id ? 'opacity-100' : ''
                      }`}
                    >
                      <GripVertical className="w-4 h-4 text-text-secondary hover:text-text-primary" />
                    </div>
                  )}
                </div>

                {/* Message content */}
                <div className="flex-1 min-w-0">
                  {isEditing ? (
                    <div className="space-y-2">
                      <textarea
                        value={editContent}
                        onChange={(e) => setEditContent(e.target.value)}
                        disabled={isSending}
                        className="w-full text-sm bg-background-primary border border-border-primary rounded-md px-2 py-1 resize-none focus:outline-none focus:ring-2 focus:ring-ring focus:border-ring"
                        rows={Math.min(Math.ceil(editContent.length / 60), 4)}
                        autoFocus
                      />
                      <div className="flex gap-2">
                        <Button
                          variant="outline"
                          size="sm"
                          disabled={isSending}
                          onClick={() => {
                            if (isSending) return;
                            if (onEditMessage) {
                              onEditMessage(message.id, editContent);
                            }
                            setEditingMessage(null);
                            if (editingMessageIdRef) editingMessageIdRef.current = null;
                            // Trigger queue processing if system is ready
                            if (onTriggerQueueProcessing) {
                              setTimeout(onTriggerQueueProcessing, 100);
                            }
                            setEditContent('');
                          }}
                          className="h-6 px-2 text-xs"
                        >
                          {intl.formatMessage(i18n.save)}
                        </Button>
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => {
                            setEditingMessage(null);
                            if (editingMessageIdRef) editingMessageIdRef.current = null;
                            // Trigger queue processing if system is ready
                            if (onTriggerQueueProcessing) {
                              setTimeout(onTriggerQueueProcessing, 100);
                            }
                            setEditContent('');
                          }}
                          className="h-6 px-2 text-xs"
                        >
                          {intl.formatMessage(i18n.cancel)}
                        </Button>
                      </div>
                    </div>
                  ) : (
                    <p
                      className={`text-sm text-text-primary leading-relaxed rounded px-1 py-0.5 transition-colors ${
                        isSending
                          ? 'cursor-not-allowed'
                          : 'cursor-pointer hover:bg-background-secondary'
                      }`}
                      title={intl.formatMessage(i18n.clickToEdit, { content: message.content })}
                      onClick={() => {
                        if (isSending) return;
                        setEditingMessage(message.id);
                        if (editingMessageIdRef) editingMessageIdRef.current = message.id;
                        setEditContent(message.content);
                      }}
                    >
                      {message.content.length > 80
                        ? `${message.content.substring(0, 80)}...`
                        : message.content}
                    </p>
                  )}
                  {!isEditing && (afterCompaction || steersTick != null) && (
                    <div className="mt-1">
                      {afterCompaction ? (
                        <AfterCompactionChip />
                      ) : (
                        steersTick != null && <SteersTickChip n={steersTick} />
                      )}
                    </div>
                  )}
                </div>

                {/* Right side actions */}
                <div className="flex items-center gap-2 flex-shrink-0">
                  <span className="text-xs text-text-secondary font-mono">
                    {formatTimestamp(message.timestamp)}
                  </span>

                  {/* Send Now button - inline */}
                  {sendNow && (
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => sendNow(message.id)}
                      disabled={isEditing || isSending}
                      className={`h-7 w-7 p-0 rounded-full transition-all duration-200 ${
                        isEditing || isSending
                          ? 'cursor-not-allowed'
                          : 'hover:bg-background-secondary'
                      }`}
                      title={
                        isEditing
                          ? intl.formatMessage(i18n.cannotSendWhileEditing)
                          : steersTick != null
                            ? intl.formatMessage(queueWords.sendNowSteers, { n: steersTick })
                            : intl.formatMessage(i18n.stopAndSend)
                      }
                    >
                      <Send className="w-3 h-3" />
                    </Button>
                  )}

                  {/* Remove button */}
                  <Button
                    variant="ghost"
                    size="sm"
                    disabled={isSending}
                    onClick={() => onRemoveMessage(message.id)}
                    className="text-text-secondary transition-colors h-6 w-6 p-0 hover:bg-background-secondary hover:text-text-danger rounded-full"
                    title={intl.formatMessage(i18n.removeFromQueue)}
                  >
                    <X className="w-3 h-3" />
                  </Button>
                </div>
              </div>

              {/* Drop indicator with enhanced visuals */}
              {dragOverItem === message.id && draggedItem !== message.id && (
                <div className="absolute inset-0 border-2 border-lz-ok rounded-xl pointer-events-none" />
              )}

              {/* Next up indicator */}
              {index === 0 && !isPaused && (
                <div className="absolute -top-2 -right-2 bg-lz-accent text-lz-accent-ink text-xs px-2 py-1 rounded-full font-medium">
                  {intl.formatMessage(i18n.next)}
                </div>
              )}
            </div>
          );
        })}
      </div>

      {/* Drag instructions */}
      {onReorderMessages && queuedMessages.length > 1 && (
        <div className="px-4 pb-3 text-xs text-text-secondary flex items-center gap-2">
          <GripVertical className="w-3 h-3" />
          <span>{intl.formatMessage(i18n.dragToReorder)}</span>
        </div>
      )}
    </div>
  );
};

export default MessageQueue;
