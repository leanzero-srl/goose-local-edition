import { AppEvents } from '../constants/events';
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { defineMessages, useIntl } from '../i18n';
import { useLocation, useNavigate } from 'react-router-dom';
import { SearchView } from './conversation/SearchView';
import LoadingGoose from './LoadingGoose';
import ProgressiveMessageList from './ProgressiveMessageList';
import { MainPanelLayout } from './Layout/MainPanelLayout';
import ChatInput from './ChatInput';
import { ScrollArea, ScrollAreaHandle } from './ui/scroll-area';
import { useFileDrop } from '../hooks/useFileDrop';
import { useEdition } from '../contexts/EditionContext';
import { useModelAndProvider } from './ModelAndProviderContext';
import { ChatState } from '../types/chatState';
import type { ChatServedBy } from './chatServedBy/chatServedBy';
import { turnLine } from './chatServedBy/turnLine';
import { useTurnCue } from './chatServedBy/useTurnCue';
import { ChatType } from '../types/chat';
import { useIsMobile } from '../hooks/use-mobile';
import { useNavigationContextSafe } from './Layout/NavigationContext';
import { useChatSession } from '../hooks/useChatSession';
import { acpDeleteSession, acpUpdateWorkingDir } from '../acp/sessions';
import { acpChatSessionActions } from '../acp/chatSessionStore';
import { useNavigation } from '../hooks/useNavigation';
import { RecipeHeader } from './RecipeHeader';
import { RecipeWarningModal } from './ui/RecipeWarningModal';
import { scanRecipe } from '../recipe';
import type { Recipe } from '../recipe';
import RecipeActivities from './recipes/RecipeActivities';
import { getTextAndImageContent, type Message, type UserInput } from '../types/message';
import { substituteParameters } from '../utils/parameterSubstitution';
import { useAutoSubmit } from '../hooks/useAutoSubmit';
import { Goose, LeanZero } from './icons';
import { LEANZERO_NAME, LEANZERO_WEBSITE_URL } from '../branding';
import EnvironmentBadge from './GooseSidebar/EnvironmentBadge';
import SessionActionsHeader from './SessionActionsHeader';
import SwarmRunPanel from './swarm/SwarmRunPanel';
import RunSamplingStrip from './swarm/RunSamplingStrip';
import { useSwarmRun } from './swarm/useSwarmRun';
import SwarmWorkspace from './swarm/SwarmWorkspace';
import MemoryProposalCards from './memories/MemoryProposalCard';
import NeedsYouTray from './sessionActivity/NeedsYouCard';
import { shouldSplitSwarmWorkspace } from './swarm/swarmRunLiveness';
import {
  Button,
  FOCUS,
  LAYER,
  MOTION,
  Panel,
  RADIUS,
  StatusDot,
  SURFACE,
  TONE_FILL,
  TYPE,
  WEIGHT,
  cx,
} from './lz';

const i18n = defineMessages({
  failedToLoadSession: {
    id: 'baseChat.failedToLoadSession',
    defaultMessage: 'Failed to Load Session',
  },
  goHome: {
    id: 'baseChat.goHome',
    defaultMessage: 'Go home',
  },
  promptFailed: {
    id: 'baseChat.promptFailed',
    defaultMessage: 'That message did not go through',
  },
  promptFailedHint: {
    id: 'baseChat.promptFailedHint',
    defaultMessage: 'Your conversation is safe. Send again to retry.',
  },
  dismiss: {
    id: 'baseChat.dismiss',
    defaultMessage: 'Dismiss',
  },
  brandTitle: {
    id: 'baseChat.brandTitle',
    defaultMessage: 'Goose Swarm by LeanZero — open leanzero.net',
  },
});

/**
 * The session's brand, top right: ONE pill that opens leanzero.net (Swarm edition) or the goose
 * docs (standard edition). Q-191 (owner): it reads "LeanZero" — the sidebar already says Goose
 * Swarm, so the corner names the maker — and the WHOLE pill is the link at the 32px dense-row
 * height (`h-lz-row-dense`), not a 20px chip inside an anchor that hugged it. Solid surface on the
 * strong outline, the monogram on the accent fill; hover is the solid surface-2 step.
 */
const BRAND_PILL = cx(
  'no-drag inline-flex h-lz-row-dense items-center gap-2 pl-1.5 pr-3 text-lz-body text-lz-ink',
  WEIGHT.medium,
  RADIUS.control,
  SURFACE.outline,
  'bg-lz-surface',
  SURFACE.hover,
  MOTION,
  FOCUS
);

export function SessionBrand({ isLocal }: { isLocal: boolean }) {
  const intl = useIntl();
  if (isLocal) {
    return (
      <a
        href={LEANZERO_WEBSITE_URL}
        target="_blank"
        rel="noopener noreferrer"
        className={BRAND_PILL}
        title={intl.formatMessage(i18n.brandTitle)}
        data-testid="local-edition-badge"
      >
        <span
          aria-hidden
          data-testid="brand-mark"
          className={cx(
            'inline-flex size-5 shrink-0 items-center justify-center rounded-[4px] [&_svg]:size-3',
            TONE_FILL.accent
          )}
        >
          <LeanZero />
        </span>
        {LEANZERO_NAME}
      </a>
    );
  }
  return (
    <a
      href="https://goose-docs.ai"
      target="_blank"
      rel="noopener noreferrer"
      className={BRAND_PILL}
      data-testid="goose-brand"
    >
      <span
        aria-hidden
        className="inline-flex size-5 shrink-0 items-center justify-center [&_svg]:size-4"
      >
        <Goose className="goose-icon-animation" />
      </span>
      goose
    </a>
  );
}

/**
 * A prompt that failed to submit — the socket dropped, the backend restarted, the machine slept.
 * Deliberately a BANNER and not the full-screen wall below: the conversation is intact both in the
 * store and on disk, and replacing it with an error page made a recoverable blip look exactly like
 * data loss. A Panel with a warn dot, dismissible, and the history stays on screen behind it.
 */
export function SubmitErrorBanner({ error, onDismiss }: { error: string; onDismiss: () => void }) {
  const intl = useIntl();
  return (
    <div role="status" data-testid="submit-error-banner" className="mx-4 mt-2 shrink-0">
      <Panel padded={false}>
        <div className="flex items-start gap-3 px-4 py-3">
          <StatusDot tone="warn" label={intl.formatMessage(i18n.promptFailed)} className="mt-1.5" />
          <div className="min-w-0 flex-1">
            <p className={cx(TYPE.body, WEIGHT.semibold)}>
              {intl.formatMessage(i18n.promptFailed)}
            </p>
            <p className={cx(TYPE.bodyMuted, 'break-words')}>{error}</p>
            <p className={cx(TYPE.meta, 'mt-0.5')}>{intl.formatMessage(i18n.promptFailedHint)}</p>
          </div>
          <Button variant="ghost" size="sm" onClick={onDismiss}>
            {intl.formatMessage(i18n.dismiss)}
          </Button>
        </div>
      </Panel>
    </div>
  );
}

/** The session could not be loaded at all: a Panel with an err dot and the one way back. */
export function SessionLoadErrorPanel({
  error,
  onGoHome,
}: {
  error: string;
  onGoHome: () => void;
}) {
  const intl = useIntl();
  return (
    <div data-testid="session-load-error" className="flex flex-col items-center justify-center p-8">
      <Panel className="mb-4 w-full max-w-md">
        <div className="flex items-start gap-3">
          <StatusDot
            tone="err"
            label={intl.formatMessage(i18n.failedToLoadSession)}
            className="mt-1.5"
          />
          <div className="min-w-0 flex-1">
            <h3 className={TYPE.h2}>{intl.formatMessage(i18n.failedToLoadSession)}</h3>
            <p className={cx(TYPE.bodyMuted, 'mt-1 break-words')}>{error}</p>
          </div>
        </div>
      </Panel>
      <Button variant="secondary" onClick={onGoHome}>
        {intl.formatMessage(i18n.goHome)}
      </Button>
    </div>
  );
}

interface BaseChatProps {
  setChat: (chat: ChatType) => void;
  onMessageSubmit?: (message: string) => void;
  renderHeader?: () => React.ReactNode;
  customChatInputProps?: Record<string, unknown>;
  customMainLayoutProps?: Record<string, unknown>;
  contentClassName?: string;
  disableSearch?: boolean;
  suppressEmptyState: boolean;
  sessionId: string;
  isActiveSession: boolean;
  initialMessage?: UserInput;
  noAutoSubmit?: boolean;
}

export default function BaseChat({
  setChat,
  renderHeader,
  customChatInputProps = {},
  customMainLayoutProps = {},
  sessionId,
  initialMessage,
  noAutoSubmit,
  isActiveSession,
}: BaseChatProps) {
  const location = useLocation();
  const navigate = useNavigate();
  const scrollRef = useRef<ScrollAreaHandle>(null);
  const chatInputRef = useRef<HTMLTextAreaElement>(null);
  const disableAnimation = location.state?.disableAnimation || false;
  const [hasStartedUsingRecipe, setHasStartedUsingRecipe] = React.useState(false);
  const [hasNotAcceptedRecipe, setHasNotAcceptedRecipe] = useState<boolean>();
  const [served, setServed] = useState<ChatServedBy | null>(null);
  const intl = useIntl();
  const [hasRecipeSecurityWarnings, setHasRecipeSecurityWarnings] = useState(false);
  const isMobile = useIsMobile();
  const { isLocal } = useEdition();
  const navContext = useNavigationContextSafe();
  const setView = useNavigation();
  const isNavCollapsed = !navContext?.isNavExpanded;
  const headerSpacingClassName = isMobile || isNavCollapsed ? 'pt-16' : 'pt-12';
  const headerBarClassName = isMobile || isNavCollapsed ? 'h-16' : 'h-12';
  const { droppedFiles, setDroppedFiles, handleDrop, handleDragOver } = useFileDrop();
  const onStreamFinish = useCallback(() => {}, []);

  const {
    session,
    messages,
    chatState,
    updateSession,
    handleSubmit,
    onSteerQueuedMessage,
    submitElicitationResponse,
    stopStreaming,
    sessionLoadError,
    submitError,
    tokenState,
    notifications: toolCallNotifications,
    pauseQueueOnStop,
    queueProcessingBlocked,
    onMessageUpdate,
  } = useChatSession({
    sessionId,
    onStreamFinish,
  });

  const handleWorkingDirChange = useCallback(
    async (newDir: string) => {
      if (!session) {
        throw new Error('Cannot update working directory before ACP session is loaded');
      }
      await acpUpdateWorkingDir(session.id, newDir);
      updateSession((currentSession) => ({ ...currentSession, working_dir: newDir }));
    },
    [session, updateSession]
  );

  // #137: the chat-level indicator had no idea the swarm was HELD, so it kept saying "goose is working on
  // it…" through a deliberate pause while every fleet node sat idle. Mihai read that as a hang and pressed
  // resume on a run that was fine. Same working_dir already handed to SwarmRunPanel, so this reads the SAME
  // engine truth the panel does (last run_paused with no later run_unpaused) — not a second, divergent guess.
  // residentGate: a NEW session in a dir with leftover .swarm state must open CLEAN — a stale run
  // (dead/absent heartbeat, started before this mount) renders nothing. A live run, or one this
  // session starts, attaches exactly as before.
  const swarmRun = useSwarmRun(session?.working_dir, 500, { residentGate: true });
  // What THIS turn waits on (the Mac reconnecting, a check after it, a silent stream, a prompt
  // being read), from the composer's one served-by derivation; null = the default words.
  const turnCue = useTurnCue(served, chatState, messages);

  const recipe = session?.recipe as Recipe | null | undefined;

  const resolvedInitialMessage = useMemo((): UserInput | undefined => {
    if (!initialMessage) return undefined;
    if (recipe?.prompt && session?.user_recipe_values) {
      return {
        ...initialMessage,
        msg: substituteParameters(initialMessage.msg, session.user_recipe_values),
      };
    }
    return initialMessage;
  }, [initialMessage, recipe?.prompt, session?.user_recipe_values]);

  // noAutoSubmit only suppresses auto-submitting the initial prompt of a fresh session
  // (goose://new-session?prompt=...). Once the conversation has messages, later flows
  // such as forks or resumes should auto-submit normally.
  const suppressInitialAutoSubmit = noAutoSubmit && messages.length === 0;
  const canAutoSubmit =
    !suppressInitialAutoSubmit &&
    (session?.session_type === 'scheduled' || !recipe || hasNotAcceptedRecipe === false);

  useAutoSubmit({
    sessionId,
    session,
    messages,
    chatState,
    initialMessage: resolvedInitialMessage,
    canAutoSubmit,
    handleSubmit,
  });

  useEffect(() => {
    let streamState: 'idle' | 'loading' | 'streaming' | 'error' = 'idle';
    if (chatState === ChatState.LoadingConversation) {
      streamState = 'loading';
    } else if (
      chatState === ChatState.Streaming ||
      chatState === ChatState.Thinking ||
      chatState === ChatState.Compacting
    ) {
      streamState = 'streaming';
    } else if (sessionLoadError || submitError) {
      streamState = 'error';
    }

    window.dispatchEvent(
      new CustomEvent(AppEvents.SESSION_STATUS_UPDATE, {
        detail: {
          sessionId,
          streamState,
          messageCount: messages.length,
        },
      })
    );
  }, [sessionId, chatState, messages.length, sessionLoadError, submitError]);

  // Generate command history from user messages (most recent first)
  const commandHistory = useMemo(() => {
    return messages
      .reduce<string[]>((history, message) => {
        if (message.role === 'user') {
          const text = getTextAndImageContent(message).textContent.trim();
          if (text) {
            history.push(text);
          }
        }
        return history;
      }, [])
      .reverse();
  }, [messages]);

  const chatInputSubmit = (input: UserInput) => {
    if (recipe && input.msg.trim()) {
      setHasStartedUsingRecipe(true);
    }
    handleSubmit(input);
  };

  const sessionModel = session?.model_config?.model_name ?? null;
  const sessionProvider = session?.provider_name ?? null;
  const sessionLoaded = session !== undefined;
  // A resumed session pinned to a provider this edition no longer offers (omlx/lmstudio in the
  // local edition) is moved to swarm/swarm the moment its metadata lands; a no-op everywhere else.
  const { migrateLegacySessionProvider } = useModelAndProvider();
  useEffect(() => {
    if (!sessionId || !sessionProvider) return;
    void migrateLegacySessionProvider(sessionId, sessionProvider);
  }, [sessionId, sessionProvider, migrateLegacySessionProvider]);
  const latestInference = useMemo(() => {
    for (let i = messages.length - 1; i >= 0; i--) {
      const message = messages[i];
      if (
        message.role === 'assistant' &&
        message.metadata.userVisible &&
        message.metadata.inference
      ) {
        return message.metadata.inference;
      }
    }
    return null;
  }, [messages]);
  // When the person last asked something (a tool result rides a user message too; it is no question):
  // a project-level memory card older than this is not pinned under it (Q-172).
  const lastQuestionAt = useMemo(() => {
    for (let i = messages.length - 1; i >= 0; i--) {
      const message = messages[i];
      if (
        message.role === 'user' &&
        message.metadata.userVisible &&
        message.content.some((c) => c.type === 'text')
      ) {
        return message.created;
      }
    }
    return null;
  }, [messages]);

  useEffect(() => {
    if (!recipe || !isActiveSession || session?.session_type === 'scheduled') return;

    (async () => {
      const accepted = await window.electron.hasAcceptedRecipeBefore(recipe);
      setHasNotAcceptedRecipe(!accepted);

      if (!accepted) {
        const scanResult = await scanRecipe(recipe);
        setHasRecipeSecurityWarnings(scanResult.has_security_warnings);
      }
    })();
  }, [recipe, isActiveSession, session?.session_type]);

  const handleRecipeAccept = async (accept: boolean) => {
    if (recipe && accept) {
      await window.electron.recordRecipeHash(recipe);
      setHasNotAcceptedRecipe(false);
      return;
    }

    if (sessionId) {
      try {
        await acpDeleteSession(sessionId);
        window.dispatchEvent(new CustomEvent(AppEvents.SESSION_DELETED, { detail: { sessionId } }));
      } catch (error) {
        console.error('Failed to delete declined recipe session:', error);
      }
    }
    setView('chat');
  };

  // Track if this is the initial render for session resuming
  const initialRenderRef = useRef(true);

  // Auto-scroll when messages are loaded (for session resuming)
  const handleRenderingComplete = React.useCallback(() => {
    // Only force scroll on the very first render
    if (initialRenderRef.current && messages.length > 0) {
      initialRenderRef.current = false;
      if (scrollRef.current?.scrollToBottom) {
        scrollRef.current.scrollToBottom();
      }
    } else if (scrollRef.current?.isFollowing) {
      if (scrollRef.current?.scrollToBottom) {
        scrollRef.current.scrollToBottom();
      }
    }
  }, [messages.length]);

  // Listen for global scroll-to-bottom requests (e.g., from MCP App message actions)
  useEffect(() => {
    const handleGlobalScrollRequest = () => {
      // Add a small delay to ensure content has been rendered
      setTimeout(() => {
        if (scrollRef.current?.scrollToBottom) {
          scrollRef.current.scrollToBottom();
        }
      }, 200);
    };

    window.addEventListener(AppEvents.SCROLL_CHAT_TO_BOTTOM, handleGlobalScrollRequest);
    return () =>
      window.removeEventListener(AppEvents.SCROLL_CHAT_TO_BOTTOM, handleGlobalScrollRequest);
  }, []);

  useEffect(() => {
    if (
      isActiveSession &&
      sessionId &&
      chatInputRef.current &&
      chatState !== ChatState.LoadingConversation
    ) {
      const timeoutId = setTimeout(() => {
        chatInputRef.current?.focus();
      }, 100);
      return () => clearTimeout(timeoutId);
    }
    return undefined;
  }, [isActiveSession, sessionId, chatState]);

  useEffect(() => {
    const handleSessionForked = (event: Event) => {
      const customEvent = event as CustomEvent<{
        newSessionId: string;
        shouldStartAgent?: boolean;
        editedMessage?: string;
      }>;
      window.dispatchEvent(new CustomEvent(AppEvents.SESSION_CREATED));
      const { newSessionId, shouldStartAgent, editedMessage } = customEvent.detail;

      const params = new URLSearchParams();
      params.set('resumeSessionId', newSessionId);
      if (shouldStartAgent) {
        params.set('shouldStartAgent', 'true');
      }

      navigate(`/pair?${params.toString()}`, {
        state: {
          disableAnimation: true,
          initialMessage: editedMessage ? { msg: editedMessage, images: [] } : undefined,
        },
      });
    };

    window.addEventListener(AppEvents.SESSION_FORKED, handleSessionForked);

    return () => {
      window.removeEventListener(AppEvents.SESSION_FORKED, handleSessionForked);
    };
  }, [location.pathname, navigate]);

  const lastSetNameRef = useRef<string>('');

  useEffect(() => {
    const currentSessionName = session?.name;
    if (currentSessionName && currentSessionName !== lastSetNameRef.current) {
      lastSetNameRef.current = currentSessionName;
      setChat({
        messages,
        recipe,
        sessionId,
        name: currentSessionName,
      });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [session?.name, setChat]);

  // If we have a recipe prompt and user recipe values, substitute parameters
  let recipePrompt = '';
  if (messages.length === 0 && recipe?.prompt) {
    recipePrompt = session?.user_recipe_values
      ? substituteParameters(recipe.prompt, session.user_recipe_values)
      : recipe.prompt;
  }

  const initialPrompt =
    noAutoSubmit && messages.length === 0 && resolvedInitialMessage?.msg
      ? resolvedInitialMessage.msg
      : recipePrompt;

  if (sessionLoadError) {
    return (
      <div className="h-full flex flex-col min-h-0">
        <MainPanelLayout
          backgroundColor={'bg-background-primary'}
          removeTopPadding={true}
          {...customMainLayoutProps}
        >
          {renderHeader && renderHeader()}
          <div className="flex flex-col flex-1 min-h-0 relative">
            <div className="flex-1 flex items-center justify-center">
              <SessionLoadErrorPanel
                error={sessionLoadError}
                onGoHome={() => {
                  setView('chat');
                }}
              />
            </div>
          </div>
        </MainPanelLayout>
      </div>
    );
  }

  // The run gets its own pane only while a LOCAL swarm build is actually underway. Presence and progress
  // decide that and nothing else — no staleness timer may hide a run whose model is merely slow.
  const showSwarmWorkspace = shouldSplitSwarmWorkspace({ isLocal, run: swarmRun });
  const conversationPane = (
    <div
      className="relative flex flex-1 min-h-0 min-w-0 flex-col bg-background-primary"
      data-testid="conversation-pane"
    >
      <ScrollArea
        ref={scrollRef}
        className={cx('flex-1 min-h-0 relative pr-1 pb-10', !isLocal && headerSpacingClassName)}
        autoScroll
        onDrop={handleDrop}
        onDragOver={handleDragOver}
        data-drop-zone="true"
        paddingX={6}
        paddingY={0}
      >
        {recipe?.title && (
          <div className="sticky top-0 z-10 bg-background-primary px-0 -mx-6 mb-6 pt-6">
            <RecipeHeader title={recipe.title} />
          </div>
        )}

        {recipe && (
          <div className={hasStartedUsingRecipe ? 'mb-6' : ''}>
            <RecipeActivities
              append={(text: string) => handleSubmit({ msg: text, images: [] })}
              activities={Array.isArray(recipe.activities) ? recipe.activities : null}
              title={recipe.title}
              parameterValues={session?.user_recipe_values || {}}
            />
          </div>
        )}

        {messages.length > 0 || recipe ? (
          <>
            <SearchView>
              <ProgressiveMessageList
                messages={messages}
                chat={{ sessionId }}
                toolCallNotifications={toolCallNotifications}
                append={(text: string) => handleSubmit({ msg: text, images: [] })}
                isUserMessage={(m: Message) => m.role === 'user'}
                isStreamingMessage={chatState !== ChatState.Idle}
                onRenderingComplete={handleRenderingComplete}
                onMessageUpdate={onMessageUpdate}
                submitElicitationResponse={submitElicitationResponse}
              />
            </SearchView>

            {/* FRAME 1.14 event B: what the agent asked to remember at the end of the turn — a card,
                never a modal, never an elicitation. Not edition-gated: a memory proposal is
                upstream-worthy, so it renders on every build. */}
            <MemoryProposalCards
              sessionId={sessionId}
              chatState={chatState}
              lastQuestionAt={lastQuestionAt}
              className="mt-2"
            />

            <div className="block h-8" />
          </>
        ) : null}

        {/* With the split up, the run has its OWN pane — leaving these here too would render the whole
            panel twice. Inline in the conversation is the layout for every other moment. */}
        {isLocal && !showSwarmWorkspace && (
          <RunSamplingStrip
            workingDir={session?.working_dir}
            active={swarmRun.inProgress}
            sessionModel={sessionModel}
            className="mb-2"
          />
        )}
        {isLocal && !showSwarmWorkspace && (
          <SwarmRunPanel workingDir={session?.working_dir} run={swarmRun} className="mb-2" />
        )}
      </ScrollArea>

      {chatState !== ChatState.Idle && (
        <div className="absolute bottom-1 left-4 z-20 pointer-events-none">
          <LoadingGoose
            chatState={chatState}
            {...turnLine(intl, chatState, swarmRun.held, turnCue, messages[messages.length - 1])}
          />
        </div>
      )}

      {/* What the model (or an extension) needs from the person: pinned outside the scroll area so
          it never scrolls away, and kept until it is answered or dismissed. */}
      <NeedsYouTray
        sessionId={sessionId}
        chatState={chatState}
        sendAnswer={(text) => chatInputSubmit({ msg: text, images: [] })}
        submitElicitationResponse={submitElicitationResponse}
        className="relative z-10 mx-4 mb-2"
      />

      <div
        data-testid="chat-input-card"
        className={cx(
          SURFACE.card,
          'relative z-10 mx-4 mb-4 overflow-hidden',
          !disableAnimation && 'animate-[fadein_400ms_ease-in_forwards]'
        )}
      >
        <ChatInput
          inputRef={chatInputRef}
          sessionId={sessionId}
          handleSubmit={chatInputSubmit}
          chatState={chatState}
          onStop={stopStreaming}
          onSteerQueuedMessage={onSteerQueuedMessage}
          pauseQueueOnStop={pauseQueueOnStop}
          queueProcessingBlocked={queueProcessingBlocked}
          commandHistory={commandHistory}
          initialValue={initialPrompt}
          setView={setView}
          totalTokens={tokenState?.totalTokens ?? session?.usage?.total_tokens ?? undefined}
          accumulatedInputTokens={
            tokenState?.accumulatedInputTokens ??
            session?.accumulated_usage?.input_tokens ??
            undefined
          }
          accumulatedOutputTokens={
            tokenState?.accumulatedOutputTokens ??
            session?.accumulated_usage?.output_tokens ??
            undefined
          }
          accumulatedCost={tokenState?.accumulatedCost ?? session?.accumulated_cost ?? undefined}
          droppedFiles={droppedFiles}
          onFilesProcessed={() => setDroppedFiles([])}
          messages={messages}
          disableAnimation={disableAnimation}
          recipe={recipe}
          recipeAccepted={!hasNotAcceptedRecipe}
          initialPrompt={initialPrompt}
          sessionModel={sessionModel}
          sessionProvider={sessionProvider}
          sessionLoaded={sessionLoaded}
          workingDir={session?.working_dir}
          onWorkingDirChange={handleWorkingDirChange}
          latestInference={latestInference}
          onServedChange={setServed}
          {...customChatInputProps}
        />
      </div>
    </div>
  );

  const runPane = showSwarmWorkspace ? (
    <ScrollArea className="flex-1 min-h-0" paddingX={3} paddingY={0}>
      <div className="pb-4">
        <RunSamplingStrip
          workingDir={session?.working_dir}
          active={swarmRun.inProgress}
          sessionModel={sessionModel}
          className="mb-3"
        />
        <SwarmRunPanel workingDir={session?.working_dir} run={swarmRun} />
      </div>
    </ScrollArea>
  ) : null;

  return (
    <div className="h-full flex flex-col min-h-0">
      <MainPanelLayout
        backgroundColor={'bg-background-primary'}
        removeTopPadding={true}
        {...customMainLayoutProps}
      >
        {/* Custom header */}
        {renderHeader && renderHeader()}

        {submitError && !sessionLoadError && (
          <SubmitErrorBanner
            error={submitError}
            onDismiss={() => acpChatSessionActions.clearSubmitError(sessionId)}
          />
        )}

        {/* Chat container with sticky recipe header */}
        <div className="flex flex-col flex-1 min-h-0 relative">
          {/* The top bar's hairline — the session title (SessionActionsHeader) and the brand chip sit
              on this band; it never takes a click, so the window drag region stays whole. */}
          <div
            aria-hidden
            data-testid="session-topbar-hairline"
            className={cx(
              'pointer-events-none absolute inset-x-0 top-0 z-20 border-b',
              headerBarClassName,
              SURFACE.hairline
            )}
          />
          {/* Brand — top right, one pill (Q-191); page chrome, so every overlay covers it */}
          <div
            data-testid="session-brand-corner"
            className={cx('absolute top-2 right-4 flex flex-row items-center gap-2', LAYER.chrome)}
          >
            <SessionBrand isLocal={isLocal} />
            <EnvironmentBadge className="translate-y-px" />
          </div>

          <SessionActionsHeader session={session} onSessionChange={updateSession} />

          {isLocal ? (
            <div className={cx('flex flex-1 min-h-0 flex-col', headerSpacingClassName)}>
              <SwarmWorkspace
                active={showSwarmWorkspace}
                conversation={conversationPane}
                run={runPane}
              />
            </div>
          ) : (
            conversationPane
          )}
        </div>
      </MainPanelLayout>

      {recipe && isActiveSession && session?.session_type !== 'scheduled' && (
        <RecipeWarningModal
          isOpen={!!hasNotAcceptedRecipe}
          onConfirm={() => handleRecipeAccept(true)}
          onCancel={() => handleRecipeAccept(false)}
          recipeDetails={{
            title: recipe.title,
            description: recipe.description,
            instructions: recipe.instructions || undefined,
          }}
          hasSecurityWarnings={hasRecipeSecurityWarnings}
        />
      )}
    </div>
  );
}
