import { NavigateFunction } from 'react-router-dom';
import type { Recipe } from '../recipe';
import { UserInput } from '../types/message';

export type View =
  | 'chat'
  | 'pair'
  | 'settings'
  | 'extensions'
  | 'moreModels'
  | 'configureProviders'
  | 'configPage'
  | 'ConfigureProviders'
  | 'settingsV2'
  | 'sessions'
  | 'schedules'
  | 'loading'
  | 'recipes'
  | 'skills'
  | 'permission'
  | 'mlxEngine'
  | 'nodes';

/**
 * The Providers view (route `/leanzero-swarm`, ids never move with a rename) keeps two tabs; its
 * LeanZero MLX tab has four inner tabs, routed by `mlx=`. Nodes has its own place in the left nav
 * (Q-193), with two tabs. Every tab lives in the URL, so a deep link opens it and Back restores it
 * (Q-203).
 */
export const PROVIDERS_ROUTE = '/leanzero-swarm';
export const NODES_ROUTE = '/nodes';

export const PROVIDERS_TABS = ['mlx', 'cloud'] as const;
export type ProvidersTab = (typeof PROVIDERS_TABS)[number];
export const MLX_TABS = ['engine', 'macs', 'models', 'sampling'] as const;
export type MlxTab = (typeof MLX_TABS)[number];
export const NODES_TABS = ['nodes', 'strategies'] as const;
export type NodesTab = (typeof NODES_TABS)[number];

function oneOf<T extends string>(values: readonly T[], value: string | null | undefined): T | null {
  return value != null && (values as readonly string[]).includes(value) ? (value as T) : null;
}

export const providersTabOf = (value: string | null | undefined): ProvidersTab =>
  oneOf(PROVIDERS_TABS, value) ?? 'mlx';
export const mlxTabOf = (value: string | null | undefined): MlxTab =>
  oneOf(MLX_TABS, value) ?? 'engine';
export const nodesTabOf = (value: string | null | undefined): NodesTab =>
  oneOf(NODES_TABS, value) ?? 'nodes';

export const mlxHref = (tab: MlxTab): string => `${PROVIDERS_ROUTE}?tab=mlx&mlx=${tab}`;
export const cloudHref = (): string => `${PROVIDERS_ROUTE}?tab=cloud`;
export const nodesHref = (tab: NodesTab = 'nodes'): string => `${NODES_ROUTE}?tab=${tab}`;
/** One node's card on the Nodes page (design §5.2): the glance, My Macs and Run it link here. */
export const nodeHref = (id: string): string =>
  `${nodesHref('nodes')}&node=${encodeURIComponent(id)}`;

/**
 * Where an old Providers tab lives now: `swarm` (Swarm Settings) moved to the Nodes page and `link`
 * (My Macs) moved inside LeanZero MLX. Null when `tab` is still a Providers tab.
 */
export function retiredProvidersTabHref(tab: string | null): string | null {
  if (tab === 'swarm') return NODES_ROUTE;
  if (tab === 'link') return mlxHref('macs');
  return null;
}

/**
 * The route a Providers section name opens — main's `set-view` deep links send one ('mlx' from the
 * tray and the engine glance, 'macs' from the Link tray). A LeanZero MLX inner tab opens under
 * LeanZero MLX; 'mlx' opens its Engine tab BY NAME, so a view left on another inner tab does not stay
 * there.
 */
export function providersHref(section: string | null | undefined): string {
  const retired = retiredProvidersTabHref(section ?? null);
  if (retired) return retired;
  if (section === 'mlx') return mlxHref('engine');
  if (section === 'cloud') return cloudHref();
  const inner = oneOf(MLX_TABS, section);
  return inner ? mlxHref(inner) : PROVIDERS_ROUTE;
}

export type ViewOptions = {
  showEnvVars?: boolean;
  deepLinkConfig?: unknown;
  error?: string;
  recipe?: Recipe;
  parentView?: View;
  parentViewOptions?: ViewOptions;
  disableAnimation?: boolean;
  initialMessage?: UserInput;
  resumeSessionId?: string;
  pendingScheduleDeepLink?: string;
};

export const createNavigationHandler = (navigate: NavigateFunction) => {
  return (view: View, options?: ViewOptions) => {
    switch (view) {
      case 'chat':
        navigate('/', { state: options });
        break;
      case 'pair': {
        // Put resumeSessionId in URL search params (not just state) so that:
        // 1. The sidebar can read it to highlight the active session
        // 2. Page refresh preserves which session is active
        // 3. Browser back/forward navigation works correctly
        const searchParams = new URLSearchParams();
        if (options?.resumeSessionId) {
          searchParams.set('resumeSessionId', options.resumeSessionId);
        }
        const url = searchParams.toString() ? `/pair?${searchParams.toString()}` : '/pair';
        navigate(url, { state: options });
        break;
      }
      case 'settings':
        navigate('/settings', { state: options });
        break;
      case 'sessions':
        navigate('/sessions', { state: options });
        break;
      case 'schedules':
        navigate('/schedules', { state: options });
        break;
      case 'recipes':
        navigate('/recipes', { state: options });
        break;
      case 'skills':
        navigate('/skills', { state: options });
        break;
      case 'permission':
        navigate('/permission', { state: options });
        break;
      case 'ConfigureProviders':
        navigate('/configure-providers', { state: options });
        break;
      case 'extensions':
        navigate('/extensions', { state: options });
        break;
      case 'mlxEngine':
        // The engine window folded into the Providers view (its LeanZero MLX tab).
        navigate(mlxHref('engine'), { state: options });
        break;
      case 'nodes':
        navigate(NODES_ROUTE, { state: options });
        break;
      default:
        navigate('/', { state: options });
    }
  };
};
