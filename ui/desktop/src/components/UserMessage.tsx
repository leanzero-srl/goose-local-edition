import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import ImagePreview from './ImagePreview';
import MarkdownContent from './MarkdownContent';
import { getTextAndImageContent, type Message } from '../types/message';
import MessageCopyLink from './MessageCopyLink';
import { formatMessageTimestamp } from '../utils/timeUtils';
import Edit from './icons/Edit';
import { Button } from './ui/button';
import { defineMessages, useIntl } from '../i18n';
import { cx } from './lz';

const i18n = defineMessages({
  editPlaceholder: {
    id: 'userMessage.editPlaceholder',
    defaultMessage: 'Edit your message...',
  },
  editAriaLabel: {
    id: 'userMessage.editAriaLabel',
    defaultMessage: 'Edit message content',
  },
  emptyError: {
    id: 'userMessage.emptyError',
    defaultMessage: 'Message cannot be empty',
  },
  editInPlaceDescription: {
    id: 'userMessage.editInPlaceDescription',
    defaultMessage:
      '<b>Edit in Place</b> updates this session • <b>Fork Session</b> creates a new session',
  },
  cancel: {
    id: 'userMessage.cancel',
    defaultMessage: 'Cancel',
  },
  cancelAriaLabel: {
    id: 'userMessage.cancelAriaLabel',
    defaultMessage: 'Cancel editing',
  },
  editInPlace: {
    id: 'userMessage.editInPlace',
    defaultMessage: 'Edit in Place',
  },
  editInPlaceAriaLabel: {
    id: 'userMessage.editInPlaceAriaLabel',
    defaultMessage: 'Edit message in place',
  },
  editInPlaceTitle: {
    id: 'userMessage.editInPlaceTitle',
    defaultMessage: 'Update the message in this session',
  },
  forkSession: {
    id: 'userMessage.forkSession',
    defaultMessage: 'Fork Session',
  },
  forkSessionAriaLabel: {
    id: 'userMessage.forkSessionAriaLabel',
    defaultMessage: 'Fork session with edited message',
  },
  forkSessionTitle: {
    id: 'userMessage.forkSessionTitle',
    defaultMessage: 'Create a new session with the edited message',
  },
  editButton: {
    id: 'userMessage.editButton',
    defaultMessage: 'Edit',
  },
  editMessageAriaLabel: {
    id: 'userMessage.editMessageAriaLabel',
    defaultMessage: 'Edit message: {preview}',
  },
  editMessageTitle: {
    id: 'userMessage.editMessageTitle',
    defaultMessage: 'Edit message',
  },
  showFullBrief: {
    id: 'userMessage.showFullBrief',
    defaultMessage: 'Show the full brief',
  },
  showFullMessage: {
    id: 'userMessage.showFullMessage',
    defaultMessage: 'Show the full message',
  },
  showLess: {
    id: 'userMessage.showLess',
    defaultMessage: 'Show less',
  },
});

/** A message taller than this share of the window is a WALL: it renders compactly — its heading
 *  line and the start of its body — until asked (UX audit C4: a seeded ask-AI brief filled the
 *  whole transcript). Measured from the message's own rendered height, not a character count. */
const TALL_SHARE_OF_VIEWPORT = 1 / 3; // ratio: of window.innerHeight — a seeded brief measured 480px in a 1254px window read as a wall at half
/** How much of a wall stays visible while collapsed. */
const COLLAPSED_SHARE_OF_VIEWPORT = 0.2; // ratio: of window.innerHeight

/**
 * Is the element taller than TALL_SHARE_OF_VIEWPORT of the window? Re-measured when the element
 * or the window resizes, before paint, so a wall never flashes open first.
 */
function useIsWall(ref: React.RefObject<HTMLDivElement | null>, deps: unknown[]): boolean {
  const [wall, setWall] = useState(false);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return undefined;
    const measure = () => setWall(el.scrollHeight > window.innerHeight * TALL_SHARE_OF_VIEWPORT);
    measure();
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(measure);
    observer?.observe(el);
    window.addEventListener('resize', measure);
    return () => {
      observer?.disconnect();
      window.removeEventListener('resize', measure);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
  return wall;
}

interface UserMessageProps {
  message: Message;
  onMessageUpdate?: (messageId: string, newContent: string, editType?: 'fork' | 'edit') => void;
  /** The conversation's first user message — a seeded brief, when collapsed, says so. */
  opensConversation?: boolean;
}

export default function UserMessage({
  message,
  onMessageUpdate,
  opensConversation = false,
}: UserMessageProps) {
  const intl = useIntl();
  const contentRef = useRef<HTMLDivElement | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const [isEditing, setIsEditing] = useState(false);
  const [editContent, setEditContent] = useState('');
  const [error, setError] = useState<string | null>(null);

  const { textContent, imagePaths } = getTextAndImageContent(message);
  const timestamp = formatMessageTimestamp(message.created);
  const isWall = useIsWall(contentRef, [textContent, isEditing]);
  const [expanded, setExpanded] = useState(false);
  const collapsed = isWall && !expanded;

  // Effect to handle message content changes and ensure persistence
  useEffect(() => {
    // If we're not editing, update the edit content to match the current message
    if (!isEditing) {
      setEditContent(textContent);
    }
  }, [message.content, textContent, message.id, isEditing]);

  // Initialize edit mode with current message content
  const initializeEditMode = useCallback(() => {
    setEditContent(textContent);
    setError(null);
    window.electron.logInfo(`Entering edit mode with content: ${textContent}`);
  }, [textContent]);

  // Handle edit button click
  const handleEditClick = useCallback(() => {
    const newEditingState = !isEditing;
    setIsEditing(newEditingState);

    // Initialize edit content when entering edit mode
    if (newEditingState) {
      initializeEditMode();
      window.electron.logInfo(`Edit interface shown for message: ${message.id}`);

      // Focus the textarea after a brief delay to ensure it's rendered
      setTimeout(() => {
        if (textareaRef.current) {
          textareaRef.current.focus();
          textareaRef.current.setSelectionRange(
            textareaRef.current.value.length,
            textareaRef.current.value.length
          );
        }
      }, 50);
    }

    window.electron.logInfo(`Edit state toggled: ${newEditingState} for message: ${message.id}`);
  }, [isEditing, initializeEditMode, message.id]);

  // Handle content changes in edit mode
  const handleContentChange = useCallback((e: React.ChangeEvent<HTMLTextAreaElement>) => {
    const newContent = e.target.value;
    setEditContent(newContent);
    setError(null); // Clear any previous errors
    window.electron.logInfo(`Content changed: ${newContent}`);
  }, []);

  const handleSave = useCallback(
    (editType: 'fork' | 'edit' = 'fork') => {
      if (editContent.trim().length === 0) {
        setError(intl.formatMessage(i18n.emptyError));
        return;
      }

      setIsEditing(false);

      if (editType === 'edit' && editContent.trim() === textContent.trim()) {
        return;
      }

      if (onMessageUpdate && message.id) {
        onMessageUpdate(message.id, editContent, editType);
      }
    },
    [editContent, textContent, onMessageUpdate, message.id, intl]
  );

  // Handle cancel action
  const handleCancel = useCallback(() => {
    window.electron.logInfo('Cancel clicked - reverting to original content');
    setIsEditing(false);
    setEditContent(textContent); // Reset to original content
    setError(null);
  }, [textContent]);

  // Handle keyboard events for accessibility
  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      window.electron.logInfo(
        `Key pressed: ${e.key}, metaKey: ${e.metaKey}, ctrlKey: ${e.ctrlKey}`
      );

      if (e.key === 'Escape') {
        e.preventDefault();
        handleCancel();
      } else if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        window.electron.logInfo('Cmd+Enter detected, calling handleSave');
        handleSave();
      }
    },
    [handleCancel, handleSave]
  );

  // Auto-resize textarea based on content
  useEffect(() => {
    if (textareaRef.current && isEditing) {
      textareaRef.current.style.height = 'auto';
      textareaRef.current.style.height = `${Math.min(textareaRef.current.scrollHeight, 200)}px`;
    }
  }, [editContent, isEditing]);

  return (
    <div className="w-full mt-[16px] opacity-0 animate-[appear_150ms_ease-in_forwards]">
      <div className="flex flex-col group">
        {isEditing ? (
          // Truly wide, centered, in-place edit box replacing the bubble
          <div className="w-full max-w-4xl mx-auto text-text-primary rounded-xl border border-border-primary shadow-lg py-4 px-4 my-2 transition-all duration-200 ease-in-out">
            <textarea
              ref={textareaRef}
              value={editContent}
              onChange={handleContentChange}
              onKeyDown={handleKeyDown}
              className="w-full resize-none bg-transparent text-text-primary placeholder:text-text-secondary border rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-400 focus:border-blue-400 transition-all duration-200 text-base leading-relaxed"
              style={{
                minHeight: '120px',
                maxHeight: '300px',
                padding: '16px',
                fontFamily: 'inherit',
                lineHeight: '1.6',
                wordBreak: 'break-word',
                overflowWrap: 'break-word',
              }}
              placeholder={intl.formatMessage(i18n.editPlaceholder)}
              aria-label={intl.formatMessage(i18n.editAriaLabel)}
              aria-describedby={error ? `error-${message.id}` : undefined}
            />
            {/* Error message */}
            {error && (
              <div
                id={`error-${message.id}`}
                className="text-red-400 text-xs mt-2 mb-2"
                role="alert"
                aria-live="polite"
              >
                {error}
              </div>
            )}
            <div className="flex justify-between items-center mt-4">
              <div className="text-xs text-text-secondary">
                {intl.formatMessage(i18n.editInPlaceDescription, {
                  b: (chunks: React.ReactNode) => <span className="font-semibold">{chunks}</span>,
                })}
              </div>
              <div className="flex gap-3">
                <Button
                  onClick={handleCancel}
                  variant="ghost"
                  aria-label={intl.formatMessage(i18n.cancelAriaLabel)}
                >
                  {intl.formatMessage(i18n.cancel)}
                </Button>
                <Button
                  onClick={() => handleSave('edit')}
                  variant="secondary"
                  aria-label={intl.formatMessage(i18n.editInPlaceAriaLabel)}
                  title={intl.formatMessage(i18n.editInPlaceTitle)}
                >
                  {intl.formatMessage(i18n.editInPlace)}
                </Button>
                <Button
                  onClick={() => handleSave('fork')}
                  aria-label={intl.formatMessage(i18n.forkSessionAriaLabel)}
                  title={intl.formatMessage(i18n.forkSessionTitle)}
                >
                  {intl.formatMessage(i18n.forkSession)}
                </Button>
              </div>
            </div>
          </div>
        ) : (
          // Normal message display
          <div className="message flex justify-end w-full">
            <div className="flex-col max-w-[85%] w-fit">
              <div className="flex flex-col group">
                {textContent.trim() && (
                  <div className="flex flex-col bg-text-primary text-background-primary rounded-xl py-2.5 px-4">
                    <div
                      data-testid="user-message-body"
                      data-collapsed={collapsed || undefined}
                      className={cx(collapsed && 'overflow-hidden')}
                      style={
                        collapsed
                          ? { maxHeight: `${window.innerHeight * COLLAPSED_SHARE_OF_VIEWPORT}px` }
                          : undefined
                      }
                    >
                      <div
                        ref={contentRef}
                        // COPYING A MESSAGE MUST GIVE BACK ITS SOURCE. The rendered DOM has already consumed
                        // the markdown — a mouse-selection copy of a prompt full of `code` and a numbered
                        // list yields flat prose, so pasting it into a new chat loses every marker and the
                        // message re-renders plain. The copy BUTTON always got this right (it puts the source
                        // on text/plain); a hand selection did not. When the selection covers essentially the
                        // whole message, hand over the source instead. A partial selection is left alone —
                        // someone quoting one sentence wants that sentence, not the whole markdown blob.
                        onCopy={(e) => {
                          const sel = window.getSelection();
                          const el = contentRef.current;
                          if (!sel || !el || sel.isCollapsed) return;
                          const picked = sel.toString().replace(/\s+/g, ' ').trim();
                          const whole = (el.textContent ?? '').replace(/\s+/g, ' ').trim();
                          if (!whole || picked.length < whole.length * 0.9) return;
                          e.clipboardData.setData('text/plain', textContent);
                          e.preventDefault();
                        }}
                      >
                        <MarkdownContent
                          content={textContent}
                          className="!text-inherit prose-a:!text-inherit prose-headings:!text-inherit prose-strong:!text-inherit prose-em:!text-inherit prose-li:!text-inherit prose-p:!text-inherit user-message"
                        />
                      </div>
                    </div>
                    {isWall && (
                      <div className="mt-2 flex border-t border-current pt-2">
                        <button
                          type="button"
                          data-testid="user-message-toggle"
                          aria-expanded={expanded}
                          onClick={() => setExpanded((open) => !open)}
                          className="h-7 rounded-lz-control border border-current px-2.5 text-lz-meta font-lz-medium hover:bg-background-primary hover:text-text-primary focus:outline-none focus-visible:ring-2 focus-visible:ring-lz-accent"
                        >
                          {intl.formatMessage(
                            expanded
                              ? i18n.showLess
                              : opensConversation
                                ? i18n.showFullBrief
                                : i18n.showFullMessage
                          )}
                        </button>
                      </div>
                    )}
                  </div>
                )}

                {imagePaths.length > 0 && (
                  <div className="flex flex-wrap gap-2 mt-2">
                    {imagePaths.map((imagePath, index) => (
                      <ImagePreview key={index} src={imagePath} />
                    ))}
                  </div>
                )}

                <div className="relative h-[22px] flex justify-end text-right">
                  <div className="absolute w-40 font-mono right-0 text-xs text-text-secondary pt-1 transition-all duration-200 group-hover:-translate-y-4 group-hover:opacity-0">
                    {timestamp}
                  </div>
                  <div className="absolute right-0 pt-1 flex items-center gap-2">
                    <button
                      onClick={handleEditClick}
                      onKeyDown={(e) => {
                        if (e.key === 'Enter' || e.key === ' ') {
                          e.preventDefault();
                          handleEditClick();
                        }
                      }}
                      className="flex items-center gap-1 text-xs text-text-secondary hover:cursor-pointer hover:text-text-primary transition-all duration-200 opacity-0 group-hover:opacity-100 -translate-y-4 group-hover:translate-y-0 focus:outline-none focus:ring-2 focus:ring-blue-400 focus:ring-opacity-50 rounded"
                      aria-label={intl.formatMessage(i18n.editMessageAriaLabel, {
                        preview: `${textContent.substring(0, 50)}${textContent.length > 50 ? '...' : ''}`,
                      })}
                      aria-expanded={isEditing}
                      title={intl.formatMessage(i18n.editMessageTitle)}
                    >
                      <Edit className="h-3 w-3" />
                      <span>{intl.formatMessage(i18n.editButton)}</span>
                    </button>
                    <MessageCopyLink text={textContent} contentRef={contentRef} />
                  </div>
                </div>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
