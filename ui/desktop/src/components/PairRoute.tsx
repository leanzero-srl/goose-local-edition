import { useCallback, useEffect, useRef, useState } from 'react';
import { useLocation, useNavigate, useSearchParams } from 'react-router-dom';
import { AlertTriangle, Loader2, MessageSquarePlus, Plus, RotateCcw } from 'lucide-react';
import { defineMessages, useIntl } from '../i18n';
import { createSession } from '../sessions';
import { isRecipeParamsCancelled } from '../acp/errors';
import { AppEvents } from '../constants/events';
import { trackErrorWithContext } from '../utils/analytics';
import { errorMessage } from '../utils/conversionUtils';
import { getInitialWorkingDir } from '../utils/workingDir';
import { folderName } from '../utils/projectNames';
import { NEW_SESSION_PARAM } from '../utils/chatWindowRoute';
import { UserInput } from '../types/message';
import { useConfig } from './ConfigContext';
import { Button, EmptyState } from './lz';

const i18n = defineMessages({
  startingTitle: {
    id: 'pairRoute.startingTitle',
    defaultMessage: 'Starting a session in {folder}',
  },
  failedTitle: {
    id: 'pairRoute.failedTitle',
    defaultMessage: 'Could not start a session in {folder}',
  },
  retry: {
    id: 'pairRoute.retry',
    defaultMessage: 'Try again',
  },
  emptyTitle: {
    id: 'pairRoute.emptyTitle',
    defaultMessage: 'No session is open',
  },
  emptyBody: {
    id: 'pairRoute.emptyBody',
    defaultMessage: 'Start one in this window’s folder, or pick a session from the sidebar.',
  },
  newSessionIn: {
    id: 'pairRoute.newSessionIn',
    defaultMessage: 'New session in {folder}',
  },
});

interface PairRouteState {
  resumeSessionId?: string;
  initialMessage?: UserInput;
  noAutoSubmit?: boolean;
}

export interface ActivePairSession {
  sessionId: string;
  initialMessage?: UserInput;
  noAutoSubmit?: boolean;
}

export function resolveSessionInitialMessage(
  session: { recipe?: { prompt?: string | null } | null },
  initialMessage?: UserInput
): UserInput | undefined {
  return (
    initialMessage ??
    (session.recipe?.prompt ? { msg: session.recipe.prompt, images: [] } : undefined)
  );
}

type StartState = { kind: 'idle' } | { kind: 'starting' } | { kind: 'failed'; error: string };

/**
 * The `/pair` route. It shows nothing itself while a session is open (ChatSessionsContainer, in
 * AppLayout, draws the chat); without one it makes the session the window asked for — a launcher
 * message, a recipe, or (Q-491) a folder-only window's NEW_SESSION_PARAM — in the window's folder,
 * and says so while it does. With nothing asked, it offers the same start instead of an empty pane.
 */
export default function PairRoute({ activeSessions }: { activeSessions: ActivePairSession[] }) {
  const intl = useIntl();
  const { getExtensions } = useConfig();
  const location = useLocation();
  const routeState =
    (location.state as PairRouteState) || (window.history.state as PairRouteState) || {};
  const [searchParams, setSearchParams] = useSearchParams();
  const isCreatingSessionRef = useRef(false);
  const navigate = useNavigate();
  const [start, setStart] = useState<StartState>({ kind: 'idle' });

  const resumeSessionId = searchParams.get('resumeSessionId') ?? undefined;
  const newSessionAsked = searchParams.get(NEW_SESSION_PARAM) === '1';
  const recipeDeeplinkFromConfig = window.appConfig?.get('recipeDeeplink') as string | undefined;
  const recipeIdFromConfig = window.appConfig?.get('recipeId') as string | undefined;
  const initialMessage = routeState.initialMessage;
  const noAutoSubmit = routeState.noAutoSubmit;
  const workingDir = getInitialWorkingDir();
  const folder = folderName(workingDir);

  const sessionAsked = Boolean(
    initialMessage || recipeDeeplinkFromConfig || recipeIdFromConfig || newSessionAsked
  );

  useEffect(() => {
    if (
      !sessionAsked ||
      resumeSessionId ||
      start.kind === 'failed' ||
      isCreatingSessionRef.current
    ) {
      return;
    }
    isCreatingSessionRef.current = true;
    setStart({ kind: 'starting' });

    (async () => {
      try {
        // The extension list may still be loading in a window this young; an empty cache would
        // start the session with no extensions at all, so it is read when empty.
        const allExtensions = await getExtensions(false);
        const newSession = await createSession(workingDir, {
          recipeDeeplink: recipeDeeplinkFromConfig,
          recipeId: recipeIdFromConfig,
          allExtensions,
        });
        const sessionInitialMessage = resolveSessionInitialMessage(newSession, initialMessage);

        window.dispatchEvent(
          new CustomEvent(AppEvents.ADD_ACTIVE_SESSION, {
            detail: {
              sessionId: newSession.id,
              initialMessage: sessionInitialMessage,
              noAutoSubmit,
            },
          })
        );

        // Replace, not push: going back must not land on the request and start another session.
        // The router applies this in a transition, so the request stays claimed (the ref, and
        // 'starting') until resumeSessionId is in the URL — releasing it here re-ran this effect
        // against the old URL and made a session per render.
        setSearchParams(
          (prev) => {
            const next = new URLSearchParams(prev);
            next.delete(NEW_SESSION_PARAM);
            next.set('resumeSessionId', newSession.id);
            return next;
          },
          { replace: true }
        );
      } catch (error) {
        isCreatingSessionRef.current = false;
        if (isRecipeParamsCancelled(error)) {
          setStart({ kind: 'idle' });
          navigate('/');
          return;
        }
        console.error('Failed to create session:', error);
        trackErrorWithContext(error, {
          component: 'PairRoute',
          action: 'create_session',
          recoverable: true,
        });
        setStart({ kind: 'failed', error: errorMessage(error, String(error)) });
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [
    sessionAsked,
    initialMessage,
    recipeDeeplinkFromConfig,
    recipeIdFromConfig,
    resumeSessionId,
    start.kind,
    setSearchParams,
    getExtensions,
  ]);

  // The session the request made is open: the request is done.
  useEffect(() => {
    if (!resumeSessionId) return;
    isCreatingSessionRef.current = false;
    setStart((prev) => (prev.kind === 'starting' ? { kind: 'idle' } : prev));
  }, [resumeSessionId]);

  // Add resumed session to active sessions if not already there
  useEffect(() => {
    if (resumeSessionId && !activeSessions.some((s) => s.sessionId === resumeSessionId)) {
      window.dispatchEvent(
        new CustomEvent(AppEvents.ADD_ACTIVE_SESSION, {
          detail: {
            sessionId: resumeSessionId,
            initialMessage: initialMessage,
            noAutoSubmit,
          },
        })
      );
    }
  }, [resumeSessionId, activeSessions, initialMessage, noAutoSubmit]);

  const askForSession = useCallback(() => {
    setSearchParams(
      (prev) => {
        const next = new URLSearchParams(prev);
        next.set(NEW_SESSION_PARAM, '1');
        return next;
      },
      { replace: true }
    );
  }, [setSearchParams]);

  if (resumeSessionId) return null;

  if (start.kind === 'failed') {
    return (
      <div data-testid="pair-route-failed" className="absolute inset-0 overflow-y-auto">
        <EmptyState
          icon={<AlertTriangle />}
          title={intl.formatMessage(i18n.failedTitle, { folder })}
          body={start.error}
          action={
            <Button
              variant="primary"
              icon={<RotateCcw />}
              title={workingDir}
              // The request is still in the URL or route state; clearing the failure makes it again.
              onClick={() => setStart({ kind: 'idle' })}
            >
              {intl.formatMessage(i18n.retry)}
            </Button>
          }
        />
      </div>
    );
  }

  if (sessionAsked || start.kind === 'starting') {
    return (
      <div
        data-testid="pair-route-starting"
        role="status"
        className="absolute inset-0 overflow-y-auto"
      >
        <EmptyState
          icon={<Loader2 className="animate-spin" />}
          title={intl.formatMessage(i18n.startingTitle, { folder })}
          body={workingDir}
        />
      </div>
    );
  }

  return (
    <div data-testid="pair-route-empty" className="absolute inset-0 overflow-y-auto">
      <EmptyState
        icon={<MessageSquarePlus />}
        title={intl.formatMessage(i18n.emptyTitle)}
        body={intl.formatMessage(i18n.emptyBody)}
        action={
          <Button variant="primary" icon={<Plus />} title={workingDir} onClick={askForSession}>
            {intl.formatMessage(i18n.newSessionIn, { folder })}
          </Button>
        }
      />
    </div>
  );
}
