/**
 * The hash route a new chat window opens on (main.ts createChat). Pure, so the choice is tested
 * without Electron.
 *
 * Q-491: a window opened with only a folder (`createChatWindow({ dir, viewType: 'pair' })`, a folder
 * or file opened in the app) used to open `#/pair?` — no session to resume, no message, no recipe —
 * and the pair route had nothing to show: the sidebar and an empty pane. Such a window now carries
 * NEW_SESSION_PARAM, and the pair route starts a session in the window's folder
 * (GOOSE_WORKING_DIR), the same session "New session here" makes.
 */

export const NEW_SESSION_PARAM = 'newSession';

export interface ChatWindowRouteOptions {
  viewType?: string;
  resumeSessionId?: string;
  initialMessage?: string;
  recipeDeeplink?: string;
  recipeId?: string;
}

const ROUTE_BY_VIEW: Record<string, string> = {
  chat: '/',
  pair: '/pair',
  settings: '/settings',
  sessions: '/sessions',
  schedules: '/schedules',
  recipes: '/recipes',
  skills: '/skills',
  permission: '/permission',
  ConfigureProviders: '/configure-providers',
};

/** The route and query, without the leading `#` (HashRouter reads `#/<path>?<query>`). */
export function chatWindowRoute(options: ChatWindowRouteOptions): string {
  const { viewType, resumeSessionId, initialMessage, recipeDeeplink, recipeId } = options;
  const hasRecipe = recipeDeeplink !== undefined || recipeId !== undefined;

  let path = viewType ? (ROUTE_BY_VIEW[viewType] ?? '/') : '/';
  if (path === '/' && (hasRecipe || initialMessage)) {
    path = '/pair';
  }

  const params = new URLSearchParams();
  if (resumeSessionId) {
    params.set('resumeSessionId', resumeSessionId);
    if (path === '/') {
      path = '/pair';
    }
  }

  // The initial message arrives after react-ready (set-initial-message) and a recipe is read from
  // appConfig; each of those makes its own session, so only a pair window with none of them asks
  // for one here.
  if (path === '/pair' && !resumeSessionId && !initialMessage && !hasRecipe) {
    params.set(NEW_SESSION_PARAM, '1');
  }

  return `${path}?${params.toString()}`;
}
