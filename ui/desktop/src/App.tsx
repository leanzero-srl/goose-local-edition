import { useEffect, useState, useRef } from 'react';
import { IpcRendererEvent } from 'electron';
import {
  HashRouter,
  Routes,
  Route,
  Navigate,
  useNavigate,
  useLocation,
  useSearchParams,
} from 'react-router-dom';
import { importNostrSessionFromDeepLink } from './sessionLinks';
import { ErrorUI } from './components/ErrorBoundary';
import { ExtensionInstallModal } from './components/ExtensionInstallModal';
import RecipeParamsModalContainer from './components/RecipeParamsModalContainer';
import { isRecipeParamsCancelled } from './acp/errors';
import { toast, ToastContainer } from 'react-toastify';
import { renderStudioCloseButton } from './toasts';
import AnnouncementModal from './components/AnnouncementModal';
import TelemetryConsentPrompt from './components/TelemetryConsentPrompt';
import OnboardingGuard from './components/onboarding/OnboardingGuard';
import { createSession } from './sessions';
import { acpListSessions, acpDeleteSession } from './acp/sessions';

import { ChatType } from './types/chat';
import ProjectLanding from './components/ProjectLanding';
import { UserInput } from './types/message';

interface PairRouteState {
  resumeSessionId?: string;
  initialMessage?: UserInput;
  noAutoSubmit?: boolean;
}
import SettingsView, { SettingsViewOptions } from './components/settings/SettingsView';
import SessionsView from './components/sessions/SessionsView';
import SchedulesView from './components/schedule/SchedulesView';
import LoopView from './components/loop/LoopView';
import LeanZeroSwarmView from './components/leanzero-swarm/LeanZeroSwarmView';
import BenchmarkView from './components/benchmark/BenchmarkView';
import BenchmarkAutoOpen from './components/benchmark/BenchmarkAutoOpen';
import { ConfirmCloseRunDialog } from './components/lz-dialogs/ConfirmCloseRunDialog';
import type { CloseRunPayload } from './utils/closeGuard';
import ProviderSettings from './components/settings/providers/ProviderSettingsPage';
import { AppLayout } from './components/Layout/AppLayout';
import { ChatProvider, DEFAULT_CHAT_TITLE } from './contexts/ChatContext';
import LauncherView from './components/LauncherView';

import 'react-toastify/dist/ReactToastify.css';
import { useConfig } from './components/ConfigContext';
import { ModelAndProviderProvider } from './components/ModelAndProviderContext';
import { ThemeProvider } from './contexts/ThemeContext';
import { EditionProvider } from './contexts/EditionContext';
import { FeaturesProvider } from './contexts/FeaturesContext';
import PermissionSettingsView from './components/settings/permission/PermissionSetting';

import ExtensionsView, { ExtensionsViewOptions } from './components/extensions/ExtensionsView';
import RecipesView from './components/recipes/RecipesView';
import SkillsView from './components/skills/SkillsView';
import MemoriesView from './components/memories/MemoriesView';
import AppsView from './components/apps/AppsView';
import StandaloneAppView from './components/apps/StandaloneAppView';
import { View, ViewOptions } from './utils/navigationUtils';

import { useNavigation } from './hooks/useNavigation';
import { errorMessage } from './utils/conversionUtils';
import { getInitialWorkingDir } from './utils/workingDir';
import { usePageViewTracking } from './hooks/useAnalytics';
import { trackErrorWithContext } from './utils/analytics';
import { AppEvents } from './constants/events';
import { registerPlatformEventHandlers } from './utils/platform_events';
import { defineMessages, useIntl } from './i18n';
import { StatusDot, SURFACE, TYPE, WEIGHT, cx } from './components/lz';

const i18n = defineMessages({
  shortcutRefusedTitle: {
    id: 'shortcutRefused.title',
    defaultMessage: 'Shortcut ignored while a benchmark is running',
  },
  shortcutRefusedTitleSessionRun: {
    id: 'shortcutRefused.titleSessionRun',
    defaultMessage: 'Shortcut ignored while a session run is live',
  },
  shortcutRefusedSpawn: {
    id: 'shortcutRefused.spawn',
    defaultMessage:
      'Cmd+N would open a second window and a second backend on the live run. Use File > New Window if you really mean it.',
  },
  shortcutRefusedClose: {
    id: 'shortcutRefused.close',
    defaultMessage:
      'Closing this window would drop the live benchmark view. Use File > Close if you really mean it.',
  },
  shortcutRefusedCloseSessionRun: {
    id: 'shortcutRefused.closeSessionRun',
    defaultMessage:
      'Closing this window would drop the live session run. Use File > Close if you really mean it.',
  },
  shortcutRefusedReload: {
    id: 'shortcutRefused.reload',
    defaultMessage: 'Reloading would discard the live log and flock panel state.',
  },
  shortcutRefusedQuit: {
    id: 'shortcutRefused.quit',
    defaultMessage:
      'Quitting would orphan the run and lose its score. Use Goose > Quit if you really mean it.',
  },
  shortcutRefusedNavigate: {
    id: 'shortcutRefused.navigate',
    defaultMessage:
      'Leaving the Benchmark view is one-way while a run is live; use the menu if you really mean it.',
  },
});

type RefusedShortcutAction = 'spawn' | 'close' | 'reload' | 'quit' | 'navigate';

const refusedShortcutMessage = {
  spawn: i18n.shortcutRefusedSpawn,
  close: i18n.shortcutRefusedClose,
  reload: i18n.shortcutRefusedReload,
  quit: i18n.shortcutRefusedQuit,
  navigate: i18n.shortcutRefusedNavigate,
} as const satisfies Record<RefusedShortcutAction, unknown>;

const isRefusedShortcutAction = (value: unknown): value is RefusedShortcutAction =>
  typeof value === 'string' && Object.prototype.hasOwnProperty.call(refusedShortcutMessage, value);

// The shortcut-refused notice: a warn StatusDot beside a title and one body line. The toast
// surface itself is the ToastContainer below (the Studio overlay Panel), so this is its content.
function ShortcutRefusedNotice({ title, body }: { title: string; body: string }) {
  return (
    <div data-testid="shortcut-refused-notice" className="flex items-start gap-3">
      <StatusDot tone="warn" label="Warning" size={10} className="mt-1.5" />
      <div className="flex min-w-0 flex-col gap-0.5">
        <div className={cx(TYPE.body, WEIGHT.semibold)}>{title}</div>
        <div className={TYPE.bodyMuted}>{body}</div>
      </div>
    </div>
  );
}

function PageViewTracker() {
  usePageViewTracking();
  return null;
}

// Route Components — "/" renders the project landing (pass D: no chat input on home; sessions
// start from a project). Hub stays in code but nothing routes to it.
const HubRouteWrapper = () => {
  return <ProjectLanding />;
};

export function resolveSessionInitialMessage(
  session: { recipe?: { prompt?: string | null } | null },
  initialMessage?: UserInput
): UserInput | undefined {
  return (
    initialMessage ??
    (session.recipe?.prompt ? { msg: session.recipe.prompt, images: [] } : undefined)
  );
}

const PairRouteWrapper = ({
  activeSessions,
}: {
  activeSessions: Array<{
    sessionId: string;
    initialMessage?: UserInput;
    noAutoSubmit?: boolean;
  }>;
  setActiveSessions: (
    sessions: Array<{ sessionId: string; initialMessage?: UserInput; noAutoSubmit?: boolean }>
  ) => void;
}) => {
  const { extensionsList } = useConfig();
  const location = useLocation();
  const routeState =
    (location.state as PairRouteState) || (window.history.state as PairRouteState) || {};
  const [searchParams, setSearchParams] = useSearchParams();
  const isCreatingSessionRef = useRef(false);
  const navigate = useNavigate();

  const resumeSessionId = searchParams.get('resumeSessionId') ?? undefined;
  const recipeDeeplinkFromConfig = window.appConfig?.get('recipeDeeplink') as string | undefined;
  const recipeIdFromConfig = window.appConfig?.get('recipeId') as string | undefined;
  const initialMessage = routeState.initialMessage;
  const noAutoSubmit = routeState.noAutoSubmit;

  // Create session if we have an initialMessage, recipeDeeplink, or recipeId but no sessionId
  useEffect(() => {
    if (
      (initialMessage || recipeDeeplinkFromConfig || recipeIdFromConfig) &&
      !resumeSessionId &&
      !isCreatingSessionRef.current
    ) {
      isCreatingSessionRef.current = true;

      (async () => {
        try {
          const newSession = await createSession(getInitialWorkingDir(), {
            recipeDeeplink: recipeDeeplinkFromConfig,
            recipeId: recipeIdFromConfig,
            allExtensions: extensionsList,
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

          setSearchParams((prev) => {
            prev.set('resumeSessionId', newSession.id);
            return prev;
          });
        } catch (error) {
          if (isRecipeParamsCancelled(error)) {
            navigate('/');
            return;
          }
          console.error('Failed to create session:', error);
          trackErrorWithContext(error, {
            component: 'PairRouteWrapper',
            action: 'create_session',
            recoverable: true,
          });
        } finally {
          isCreatingSessionRef.current = false;
        }
      })();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [
    initialMessage,
    recipeDeeplinkFromConfig,
    recipeIdFromConfig,
    resumeSessionId,
    setSearchParams,
    extensionsList,
  ]);

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

  return null;
};

const SettingsRoute = () => {
  const location = useLocation();
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const setView = useNavigation();

  // Get viewOptions from location.state, history.state, or URL search params
  const viewOptions =
    (location.state as SettingsViewOptions) || (window.history.state as SettingsViewOptions) || {};

  // If section is provided via URL search params, add it to viewOptions
  const sectionFromUrl = searchParams.get('section');
  if (sectionFromUrl) {
    viewOptions.section = sectionFromUrl;
  }

  return <SettingsView onClose={() => navigate('/')} setView={setView} viewOptions={viewOptions} />;
};

const SessionsRoute = () => {
  return <SessionsView />;
};

const SchedulesRoute = () => {
  const navigate = useNavigate();
  return <SchedulesView onClose={() => navigate('/')} />;
};

const RecipesRoute = () => {
  return <RecipesView />;
};

const BenchmarkRoute = () => {
  return <BenchmarkView />;
};

const LoopRoute = () => {
  return <LoopView />;
};

const LeanZeroSwarmRoute = () => {
  return <LeanZeroSwarmView />;
};

const SkillsRoute = () => {
  return <SkillsView />;
};

const MemoriesRoute = () => {
  return <MemoriesView />;
};

const PermissionRoute = () => {
  const location = useLocation();
  const navigate = useNavigate();
  const parentView = location.state?.parentView as View;
  const parentViewOptions = location.state?.parentViewOptions as ViewOptions;

  return (
    <PermissionSettingsView
      onClose={() => {
        // Navigate back to parent view with options
        switch (parentView) {
          case 'chat':
            navigate('/');
            break;
          case 'pair':
            navigate('/pair');
            break;
          case 'settings':
            navigate('/settings', { state: parentViewOptions });
            break;
          case 'sessions':
            navigate('/sessions');
            break;
          case 'schedules':
            navigate('/schedules');
            break;
          case 'recipes':
            navigate('/recipes');
            break;
          case 'skills':
            navigate('/skills');
            break;
          default:
            navigate('/');
        }
      }}
    />
  );
};

const ConfigureProvidersRoute = () => {
  const navigate = useNavigate();

  return (
    <div className="w-screen h-screen bg-background-primary">
      <ProviderSettings
        onClose={() => navigate('/settings', { state: { section: 'models' } })}
        isOnboarding={false}
      />
    </div>
  );
};

const ExtensionsRoute = () => {
  const navigate = useNavigate();
  const location = useLocation();

  // Get viewOptions from location.state or history.state (for deep link extensions)
  const viewOptions =
    (location.state as ExtensionsViewOptions) ||
    (window.history.state as ExtensionsViewOptions) ||
    {};

  return (
    <ExtensionsView
      onClose={() => navigate(-1)}
      setView={(view, options) => {
        switch (view) {
          case 'chat':
            navigate('/');
            break;
          case 'pair':
            navigate('/pair', { state: options });
            break;
          case 'settings':
            navigate('/settings', { state: options });
            break;
          default:
            navigate('/');
        }
      }}
      viewOptions={viewOptions}
    />
  );
};

export function AppInner() {
  const [fatalError, setFatalError] = useState<string | null>(null);
  // main's question when this window is closed with the mouse on a live swarm run (closeGuard.ts).
  const [closeRunPrompt, setCloseRunPrompt] = useState<CloseRunPayload | null>(null);

  const nostrImportInFlight = useRef<string | null>(null);

  const navigate = useNavigate();
  const setView = useNavigation();
  const intl = useIntl();

  const [chat, setChat] = useState<ChatType>({
    sessionId: '',
    name: DEFAULT_CHAT_TITLE,
    messages: [],
    recipe: null,
  });

  const MAX_ACTIVE_SESSIONS = 10;

  const [activeSessions, setActiveSessions] = useState<
    Array<{ sessionId: string; initialMessage?: UserInput; noAutoSubmit?: boolean }>
  >([]);

  useEffect(() => {
    const handleAddActiveSession = (event: Event) => {
      const { sessionId, initialMessage, noAutoSubmit } = (
        event as CustomEvent<{
          sessionId: string;
          initialMessage?: UserInput;
          noAutoSubmit?: boolean;
        }>
      ).detail;

      setActiveSessions((prev) => {
        const existingIndex = prev.findIndex((s) => s.sessionId === sessionId);

        if (existingIndex !== -1) {
          // Session exists - move to end of LRU list (most recently used)
          const existing = prev[existingIndex];
          return [...prev.slice(0, existingIndex), ...prev.slice(existingIndex + 1), existing];
        }

        // New session - add to end with LRU eviction if needed
        const newSession = { sessionId, initialMessage, noAutoSubmit };
        const updated = [...prev, newSession];
        if (updated.length > MAX_ACTIVE_SESSIONS) {
          return updated.slice(updated.length - MAX_ACTIVE_SESSIONS);
        }
        return updated;
      });
    };

    const handleClearInitialMessage = (event: Event) => {
      const { sessionId } = (event as CustomEvent<{ sessionId: string }>).detail;

      setActiveSessions((prev) => {
        return prev.map((session) => {
          if (session.sessionId === sessionId) {
            return { ...session, initialMessage: undefined };
          }
          return session;
        });
      });
    };

    const handleSessionDeleted = (event: Event) => {
      const { sessionId } = (event as CustomEvent<{ sessionId: string }>).detail;

      setActiveSessions((prev) => {
        return prev.filter((session) => session.sessionId !== sessionId);
      });
    };

    window.addEventListener(AppEvents.ADD_ACTIVE_SESSION, handleAddActiveSession);
    window.addEventListener(AppEvents.CLEAR_INITIAL_MESSAGE, handleClearInitialMessage);
    window.addEventListener(AppEvents.SESSION_DELETED, handleSessionDeleted);
    return () => {
      window.removeEventListener(AppEvents.ADD_ACTIVE_SESSION, handleAddActiveSession);
      window.removeEventListener(AppEvents.CLEAR_INITIAL_MESSAGE, handleClearInitialMessage);
      window.removeEventListener(AppEvents.SESSION_DELETED, handleSessionDeleted);
    };
  }, []);

  const { addExtension } = useConfig();

  useEffect(() => {
    try {
      window.electron.reactReady();
    } catch (error) {
      console.error('Error sending reactReady:', error);
      setFatalError(`React ready notification failed: ${errorMessage(error, 'Unknown error')}`);
    }
  }, []);

  useEffect(() => {
    acpListSessions()
      .then(({ sessions }) => {
        const phantom = sessions.filter(
          (s) => s.messageCount === 0 && !s.userSetName && !s.hasRecipe
        );
        for (const s of phantom) {
          acpDeleteSession(s.id).catch(() => {});
        }
      })
      .catch(() => {});
  }, []);

  useEffect(() => {
    const handleOpenSharedSession = async (_event: IpcRendererEvent, ...args: unknown[]) => {
      const link = args[0] as string;
      window.electron.logInfo('Opening session share link');

      if (!link.startsWith('goose://sessions/nostr')) {
        toast.error('Unsupported session share link');
        navigate('/sessions');
        return;
      }

      if (nostrImportInFlight.current === link) {
        window.electron.logInfo('Skipping duplicate Nostr deep link import');
        return;
      }
      nostrImportInFlight.current = link;

      try {
        await importNostrSessionFromDeepLink(link);
        navigate('/sessions');
      } catch (error) {
        console.error('Unexpected error opening Nostr session share:', error);
        trackErrorWithContext(error, {
          component: 'AppInner',
          action: 'open_nostr_session_share',
          recoverable: true,
        });
        toast.error(`Failed to import Nostr session: ${errorMessage(error, 'Unknown error')}`);
        navigate('/sessions');
      } finally {
        if (nostrImportInFlight.current === link) {
          nostrImportInFlight.current = null;
        }
      }
    };
    window.electron.on('open-shared-session', handleOpenSharedSession);
    return () => {
      window.electron.off('open-shared-session', handleOpenSharedSession);
    };
  }, [navigate]);

  // Prevent default drag and drop behavior globally to avoid opening files in new windows
  // but allow our React components to handle drops in designated areas
  useEffect(() => {
    const preventDefaults = (e: globalThis.DragEvent) => {
      // Only prevent default if we're not over a designated drop zone
      const target = e.target as HTMLElement;
      const isOverDropZone = target.closest('[data-drop-zone="true"]') !== null;

      if (!isOverDropZone) {
        e.preventDefault();
        e.stopPropagation();
      }
    };

    const handleDragOver = (e: globalThis.DragEvent) => {
      // Always prevent default for dragover to allow dropping
      e.preventDefault();
      e.stopPropagation();
    };

    const handleDrop = (e: globalThis.DragEvent) => {
      // Only prevent default if we're not over a designated drop zone
      const target = e.target as HTMLElement;
      const isOverDropZone = target.closest('[data-drop-zone="true"]') !== null;

      if (!isOverDropZone) {
        e.preventDefault();
        e.stopPropagation();
      }
    };

    // Add event listeners to document to catch drag events
    document.addEventListener('dragenter', preventDefaults, false);
    document.addEventListener('dragleave', preventDefaults, false);
    document.addEventListener('dragover', handleDragOver, false);
    document.addEventListener('drop', handleDrop, false);

    return () => {
      document.removeEventListener('dragenter', preventDefaults, false);
      document.removeEventListener('dragleave', preventDefaults, false);
      document.removeEventListener('dragover', handleDragOver, false);
      document.removeEventListener('drop', handleDrop, false);
    };
  }, []);

  useEffect(() => {
    const handleFatalError = (_event: IpcRendererEvent, ...args: unknown[]) => {
      const errorMessage = args[0] as string;
      console.error('Encountered a fatal error:', errorMessage);
      setFatalError(errorMessage);
    };
    window.electron.on('fatal-error', handleFatalError);
    return () => {
      window.electron.off('fatal-error', handleFatalError);
    };
  }, []);

  useEffect(() => {
    const handleSetView = (_event: IpcRendererEvent, ...args: unknown[]) => {
      const newView = args[0] as View;
      const section = args[1] as string | undefined;

      if (section && newView === 'settings') {
        navigate(`/settings?section=${section}`);
      } else {
        navigate(`/${newView}`);
      }
    };

    window.electron.on('set-view', handleSetView);
    return () => window.electron.off('set-view', handleSetView);
  }, [navigate]);

  useEffect(() => {
    const handleNewChat = (_event: IpcRendererEvent, ..._args: unknown[]) => {
      navigate('/');
    };

    window.electron.on('new-chat', handleNewChat);
    return () => window.electron.off('new-chat', handleNewChat);
  }, [navigate]);

  useEffect(() => {
    const handleShortcutRefused = (_event: IpcRendererEvent, ...args: unknown[]) => {
      const payload = args[0] as { action?: unknown; reason?: unknown } | undefined;
      const action = payload?.action;
      if (!isRefusedShortcutAction(action)) return;
      // main sends reason: 'benchmark' | 'session-run' (shortcutGuard.ts); the notice names the
      // run it is protecting instead of calling every refusal a benchmark.
      const sessionRun = payload?.reason === 'session-run';
      const title = sessionRun ? i18n.shortcutRefusedTitleSessionRun : i18n.shortcutRefusedTitle;
      const body =
        sessionRun && action === 'close'
          ? i18n.shortcutRefusedCloseSessionRun
          : refusedShortcutMessage[action];
      toast.warn(
        <ShortcutRefusedNotice title={intl.formatMessage(title)} body={intl.formatMessage(body)} />,
        {
          position: 'top-right',
          closeButton: true,
          hideProgressBar: true,
          autoClose: 5000,
          icon: false,
        }
      );
    };

    window.electron.on('shortcut-refused', handleShortcutRefused);
    return () => window.electron.off('shortcut-refused', handleShortcutRefused);
  }, [intl]);

  // The mouse-close guard: main kept this window on `close` because its renderer holds a live swarm
  // run, and asks here. The dialog answers through confirmCloseRunReply — true closes the window for
  // real (the dialog stays up, disabled, while the window goes), false leaves everything as it was.
  useEffect(() => {
    const handleConfirmCloseRun = (_event: IpcRendererEvent, ...args: unknown[]) => {
      const payload = args[0] as Partial<CloseRunPayload> | undefined;
      if (!Array.isArray(payload?.runs)) return;
      setCloseRunPrompt({ runs: payload.runs });
    };
    window.electron.on('confirm-close-run', handleConfirmCloseRun);
    return () => window.electron.off('confirm-close-run', handleConfirmCloseRun);
  }, []);

  useEffect(() => {
    const handleFocusInput = (_event: IpcRendererEvent, ..._args: unknown[]) => {
      const inputField = document.querySelector('input[type="text"], textarea') as HTMLInputElement;
      if (inputField) {
        inputField.focus();
      }
    };
    window.electron.on('focus-input', handleFocusInput);
    return () => {
      window.electron.off('focus-input', handleFocusInput);
    };
  }, []);

  // Handle initial message from launcher
  const isProcessingRef = useRef(false);

  useEffect(() => {
    const handleSetInitialMessage = async (_event: IpcRendererEvent, ...args: unknown[]) => {
      const initialMessage = args[0] as string;
      const options = (args[1] as { noAutoSubmit?: boolean } | undefined) || {};

      if (initialMessage && !isProcessingRef.current) {
        isProcessingRef.current = true;
        navigate('/pair', {
          state: {
            initialMessage: { msg: initialMessage, images: [] },
            noAutoSubmit: options.noAutoSubmit,
          },
        });
        setTimeout(() => {
          isProcessingRef.current = false;
        }, 1000);
      }
    };
    window.electron.on('set-initial-message', handleSetInitialMessage);
    return () => {
      window.electron.off('set-initial-message', handleSetInitialMessage);
    };
  }, [navigate]);

  // Register platform event handlers for app lifecycle management
  useEffect(() => {
    return registerPlatformEventHandlers();
  }, []);

  const closeRunDialog = closeRunPrompt ? (
    <ConfirmCloseRunDialog
      runs={closeRunPrompt.runs}
      onKeepRunning={() => {
        setCloseRunPrompt(null);
        window.electron.confirmCloseRunReply(false);
      }}
      onStopAndClose={() => window.electron.confirmCloseRunReply(true)}
    />
  ) : null;

  if (fatalError) {
    return (
      <>
        {closeRunDialog}
        <ErrorUI error={errorMessage(fatalError)} />
      </>
    );
  }

  return (
    <>
      {closeRunDialog}
      <PageViewTracker />
      <ToastContainer
        aria-label="Toast notifications"
        toastClassName={() =>
          cx(
            'relative mb-4 flex min-h-16 cursor-pointer justify-between overflow-hidden p-3 text-lz-ink',
            SURFACE.overlay
          )
        }
        style={{ width: '450px' }}
        className="mt-6"
        position="top-right"
        autoClose={3000}
        closeOnClick
        pauseOnHover
        // The toast content renders Studio StatusDots for its tone; the library icon is off.
        icon={false}
        // Every close is the Studio ghost icon Button — a per-toast `closeButton: true` resolves
        // to this container renderer (react-toastify substitutes it for `true`).
        closeButton={renderStudioCloseButton}
      />
      <ExtensionInstallModal addExtension={addExtension} setView={setView} />
      <RecipeParamsModalContainer />
      <BenchmarkAutoOpen />
      <div className="relative w-screen h-screen overflow-hidden bg-background-secondary flex flex-col">
        <div className="titlebar-drag-region" />
        <div style={{ position: 'relative', width: '100%', height: '100%' }}>
          <Routes>
            <Route path="launcher" element={<LauncherView />} />
            <Route path="configure-providers" element={<ConfigureProvidersRoute />} />
            <Route path="standalone-app" element={<StandaloneAppView />} />
            <Route
              path="/"
              element={
                <OnboardingGuard>
                  <ChatProvider chat={chat} setChat={setChat} contextKey="hub">
                    <AppLayout activeSessions={activeSessions} />
                  </ChatProvider>
                </OnboardingGuard>
              }
            >
              <Route index element={<HubRouteWrapper />} />
              <Route
                path="pair"
                element={
                  <PairRouteWrapper
                    activeSessions={activeSessions}
                    setActiveSessions={setActiveSessions}
                  />
                }
              />
              <Route path="settings" element={<SettingsRoute />} />
              <Route
                path="extensions"
                element={
                  <ChatProvider chat={chat} setChat={setChat} contextKey="extensions">
                    <ExtensionsRoute />
                  </ChatProvider>
                }
              />
              <Route path="apps" element={<AppsView />} />
              <Route path="sessions" element={<SessionsRoute />} />
              <Route path="schedules" element={<SchedulesRoute />} />
              <Route path="recipes" element={<RecipesRoute />} />
              <Route path="loop" element={<LoopRoute />} />
              <Route path="leanzero-swarm" element={<LeanZeroSwarmRoute />} />
              {/* The old engine-window path stays alive as a redirect — no dead links. */}
              <Route path="mlx-engine" element={<Navigate to="/leanzero-swarm" replace />} />
              <Route path="benchmark" element={<BenchmarkRoute />} />
              <Route path="skills" element={<SkillsRoute />} />
              <Route path="memories" element={<MemoriesRoute />} />
              <Route path="permission" element={<PermissionRoute />} />
            </Route>
          </Routes>
        </div>
      </div>
    </>
  );
}

export default function App() {
  return (
    <EditionProvider>
      <ThemeProvider>
        <FeaturesProvider>
          <ModelAndProviderProvider>
            <HashRouter>
              <AppInner />
            </HashRouter>
            <AnnouncementModal />
            <TelemetryConsentPrompt />
          </ModelAndProviderProvider>
        </FeaturesProvider>
      </ThemeProvider>
    </EditionProvider>
  );
}
