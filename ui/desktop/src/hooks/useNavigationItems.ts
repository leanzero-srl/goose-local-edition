import { Brain, Gauge, Settings, Zap } from 'lucide-react';
import { Goose } from '../components/icons';
import type React from 'react';
import { defineMessages, type IntlShape, type MessageDescriptor } from 'react-intl';

export interface NavItem {
  id: string;
  path: string;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  getTag?: () => string;
  tagAlign?: 'left' | 'right';
}

/**
 * Top-level nav items (excluding Settings which is pinned to the bottom).
 *
 * Goose Swarm pass A declutter: Recipes, Apps, Scheduler, Loop, Extensions and Session History left
 * the nav on purpose — their routes and views stay in code and reachable by URL (see App.tsx routes).
 *
 * Pass C: the "Leanzero MLX" entry became the swarm hub — the three-tab management view
 * (LeanZero MLX | Cloud Providers | Swarm Settings). /mlx-engine redirects to /leanzero-swarm.
 *
 * 2026-09-02 (owner): the row is labelled "Goose Swarm" and carries the goose mark; the route id
 * and path stay `leanzero-swarm` — internal ids never move with a rename.
 *
 * Pass D (owner): the "New Chat" row is GONE — sessions start from a project, so the only
 * session-creating affordance is the Projects tree's per-project "+ New session here". The "/"
 * route stays reachable (Settings close, error escapes) and renders the project landing.
 */
export const NAV_ITEMS: NavItem[] = [
  { id: 'skills', path: '/skills', label: 'Skills', icon: Zap },
  { id: 'memories', path: '/memories', label: 'Memories', icon: Brain },
  { id: 'benchmark', path: '/benchmark', label: 'Benchmark', icon: Gauge },
  { id: 'leanzero-swarm', path: '/leanzero-swarm', label: 'Goose Swarm', icon: Goose },
];

/** Settings is rendered separately, pinned to the bottom of the sidebar. */
export const SETTINGS_NAV_ITEM: NavItem = {
  id: 'settings',
  path: '/settings',
  label: 'Settings',
  icon: Settings,
};

// Translation descriptors for nav labels. Kept here next to NAV_ITEMS so the two
// stay in sync.
const navItemMessages = defineMessages({
  skills: {
    id: 'navigation.itemSkills',
    defaultMessage: 'Skills',
  },
  memories: {
    id: 'navigation.itemMemories',
    defaultMessage: 'Memories',
  },
  benchmark: {
    id: 'navigation.itemBenchmark',
    defaultMessage: 'Benchmark',
  },
  'leanzero-swarm': {
    id: 'navigation.itemLeanzeroSwarm',
    defaultMessage: 'Goose Swarm',
  },
  settings: {
    id: 'navigation.itemSettings',
    defaultMessage: 'Settings',
  },
});

const NAV_ITEM_MESSAGES: Record<string, MessageDescriptor> = navItemMessages;

/** Format a NavItem's label using the provided intl instance, falling back to `item.label`. */
export function getNavItemLabel(item: NavItem, intl: IntlShape): string {
  const descriptor = NAV_ITEM_MESSAGES[item.id];
  return descriptor ? intl.formatMessage(descriptor) : item.label;
}
