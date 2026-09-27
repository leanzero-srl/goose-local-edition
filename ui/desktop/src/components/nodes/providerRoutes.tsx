import type { ReactElement } from 'react';
import { Navigate, useSearchParams } from 'react-router-dom';
import LeanZeroSwarmView from '../leanzero-swarm/LeanZeroSwarmView';
import NodesView from './NodesView';
import { mlxHref, retiredProvidersTabHref } from '../../utils/navigationUtils';

/**
 * `/leanzero-swarm` is the Providers view. Its old tabs keep working as redirects: `?tab=swarm`
 * (Swarm Settings) opens the Nodes page and `?tab=link` (My Macs) opens LeanZero MLX's My Macs tab,
 * replacing the history entry so Back never lands on the dead link again.
 */
export function ProvidersRoute() {
  const [searchParams] = useSearchParams();
  const moved = retiredProvidersTabHref(searchParams.get('tab'));
  if (moved) return <Navigate to={moved} replace />;
  return <LeanZeroSwarmView />;
}

/** The Providers and Nodes routes, one table — App.tsx mounts it and the route tests mount it. */
export const PROVIDER_ROUTES: readonly { path: string; element: ReactElement }[] = [
  { path: 'nodes', element: <NodesView /> },
  { path: 'leanzero-swarm', element: <ProvidersRoute /> },
  // The old engine-window path stays alive as a redirect — no dead links.
  { path: 'mlx-engine', element: <Navigate to={mlxHref('engine')} replace /> },
];
