import { Brain, Cpu, Gauge, MessageSquarePlus, Settings, Zap } from 'lucide-react';
import type { LucideIcon } from 'lucide-react';
import { defineMessages, type IntlShape, type MessageDescriptor } from 'react-intl';

export interface NavItem {
  id: string;
  path: string;
  label: string;
  icon: LucideIcon;
  getTag?: () => string;
  tagAlign?: 'left' | 'right';
}

/**
 * Top-level nav items (excluding Settings which is pinned to the bottom).
 *
 * Goose Swarm pass A declutter: the nav shows ONLY New Chat, Skills, Memories, Benchmark and the
 * Leanzero MLX entry. Recipes, Apps, Scheduler, Loop, Extensions and Session History left the nav
 * on purpose — their routes and views stay in code and reachable by URL (see App.tsx routes).
 */
export const NAV_ITEMS: NavItem[] = [
  { id: 'home', path: '/', label: 'New Chat', icon: MessageSquarePlus },
  { id: 'skills', path: '/skills', label: 'Skills', icon: Zap },
  { id: 'memories', path: '/memories', label: 'Memories', icon: Brain },
  { id: 'benchmark', path: '/benchmark', label: 'Benchmark', icon: Gauge },
  { id: 'mlx', path: '/mlx-engine', label: 'Leanzero MLX', icon: Cpu },
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
  home: {
    id: 'navigation.itemHome',
    defaultMessage: 'New Chat',
  },
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
  mlx: {
    id: 'navigation.itemMlx',
    defaultMessage: 'Leanzero MLX',
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
