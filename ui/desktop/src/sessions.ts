import type { Session } from './types/session';
import type { ExtensionConfig } from './types/extensions';
import { DEFAULT_CHAT_TITLE } from './contexts/ChatContext';
import type { setViewType } from './hooks/useNavigation';
import type { FixedExtensionEntry } from './components/ConfigContext';
import { AppEvents } from './constants/events';
import { acpChatSessionController } from './acp/chatSessionController';
import { getConfiguredGooseExtensions, gooseExtensionName } from './acp/extensions';

/** The engine's own stored placeholder for an unnamed session (acp/server/new_session.rs). The
 *  renderer never shows it verbatim — it normalizes to DEFAULT_CHAT_TITLE ("New Session"). */
const ENGINE_PLACEHOLDER_NAME = 'New Chat';

/** List-surface twin of getSessionDisplayName for rows that only carry a name string
 *  (SessionListItem): the engine placeholder and emptiness both read "New Session". */
export function displaySessionListName(name: string | null | undefined): string {
  const trimmed = (name ?? '').trim();
  if (trimmed === '' || trimmed === ENGINE_PLACEHOLDER_NAME) return DEFAULT_CHAT_TITLE;
  return trimmed;
}

export function getSessionDisplayName(session: Session): string {
  if (session.user_set_name) {
    return session.name;
  }
  if (session.recipe?.title) {
    return session.recipe.title;
  }
  if (shouldShowNewChatTitle(session)) {
    return DEFAULT_CHAT_TITLE;
  }
  return session.name;
}

/**
 * Show the default title only while the session has NO real name. This used to key on
 * `message_count === 0`, but the renderer's in-memory session metadata does not maintain
 * message_count as a conversation streams — so when the backend auto-named the session after the
 * first turns (session_manager::maybe_update_name → session_info_update → SESSION_RENAMED), the
 * header kept saying the default title over a real generated name. The name itself is the fact
 * that matters: placeholder (or empty) means unnamed, anything else is the session's name.
 */
export function shouldShowNewChatTitle(session: Session): boolean {
  if (session.user_set_name || session.recipe?.title) {
    return false;
  }
  const name = (session.name ?? '').trim();
  return name === '' || name === ENGINE_PLACEHOLDER_NAME || name === DEFAULT_CHAT_TITLE;
}

export function resumeSession(session: Session, setView: setViewType) {
  const eventDetail = {
    sessionId: session.id,
    initialMessage: undefined,
  };

  window.dispatchEvent(
    new CustomEvent(AppEvents.ADD_ACTIVE_SESSION, {
      detail: eventDetail,
    })
  );

  setView('pair', {
    disableAnimation: true,
    resumeSessionId: session.id,
  });
}

interface CreateSessionOptions {
  recipeDeeplink?: string;
  recipeId?: string;
  extensionConfigs?: ExtensionConfig[];
  allExtensions?: FixedExtensionEntry[];
}

function selectedExtensionConfigs(options?: CreateSessionOptions): ExtensionConfig[] {
  if (options?.extensionConfigs && options.extensionConfigs.length > 0) {
    return options.extensionConfigs;
  }
  if (options?.allExtensions) {
    return options.allExtensions
      .filter((extension) => extension.enabled)
      .map((extension) => {
        const { enabled: _enabled, ...config } = extension;
        return config as ExtensionConfig;
      });
  }
  return [];
}

async function createAcpSession(
  workingDir: string,
  options?: CreateSessionOptions
): Promise<Session> {
  const selectedNames = new Set(selectedExtensionConfigs(options).map((config) => config.name));
  const gooseExtensions =
    selectedNames.size > 0
      ? (await getConfiguredGooseExtensions())
          .filter((entry) => selectedNames.has(gooseExtensionName(entry.extension)))
          .map((entry) => entry.extension)
      : [];
  return acpChatSessionController.createSession(workingDir, gooseExtensions, {
    recipeId: options?.recipeId,
    recipeDeeplink: options?.recipeDeeplink,
  });
}

export async function createSession(
  workingDir: string,
  options?: CreateSessionOptions
): Promise<Session> {
  return createAcpSession(workingDir, options);
}

export async function startNewSession(
  initialText: string | undefined,
  setView: setViewType,
  workingDir: string,
  options?: {
    recipeDeeplink?: string;
    recipeId?: string;
    allExtensions?: FixedExtensionEntry[];
  }
): Promise<Session> {
  const session = await createSession(workingDir, options);
  window.dispatchEvent(new CustomEvent(AppEvents.SESSION_CREATED, { detail: { session } }));

  const initialMessage = initialText ? { msg: initialText, images: [] } : undefined;

  const eventDetail = {
    sessionId: session.id,
    initialMessage,
  };

  window.dispatchEvent(
    new CustomEvent(AppEvents.ADD_ACTIVE_SESSION, {
      detail: eventDetail,
    })
  );

  setView('pair', {
    disableAnimation: true,
    initialMessage,
    resumeSessionId: session.id,
  });
  return session;
}
