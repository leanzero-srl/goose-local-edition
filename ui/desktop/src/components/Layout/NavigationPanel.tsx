import React, { useCallback, useEffect, useMemo, useRef } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';
import { useEdition } from '../../contexts/EditionContext';
import { motion } from 'framer-motion';
import { useNavigationContext } from './NavigationContext';
import { ProjectsSection } from './ProjectsSection';
import { useFeatures } from '../../contexts/FeaturesContext';
import {
  NAV_ITEMS,
  SETTINGS_NAV_ITEM,
  getNavItemLabel,
  type NavItem,
} from '../../hooks/useNavigationItems';
import { cn } from '../../utils';
import { useIntl } from '../../i18n';

const navItemClass = (active: boolean) =>
  cn(
    'flex flex-row items-center gap-3 outline-none no-drag w-full',
    'rounded-full px-3 py-2 text-sm font-medium transition-colors',
    active
      ? 'bg-background-tertiary text-text-primary'
      : 'text-text-primary hover:bg-background-tertiary/60'
  );

interface NavRowProps {
  item: NavItem;
  active: boolean;
  onClick: () => void;
}

export const NavRow: React.FC<NavRowProps> = ({ item, active, onClick }) => {
  const intl = useIntl();
  const Icon = item.icon;
  return (
    <button
      onClick={onClick}
      className={navItemClass(active)}
      // The active view was styled and nothing more: bg-background-tertiary against a transparent
      // sibling, with no aria-current and no aria-selected anywhere in the panel. So the current view
      // was visible to a sighted mouse user and invisible to everything else — a screen reader, and any
      // automated check of "which view am I on". Measured live over CDP on #/benchmark: the Benchmark
      // row computed rgb(71,78,87) against rgba(0,0,0,0), and a sweep of all 19 nav controls found ZERO
      // with either attribute set.
      aria-current={active ? 'page' : undefined}
    >
      <Icon className="w-5 h-5 flex-shrink-0 text-text-secondary" />
      <span className="text-left flex-1 truncate">{getNavItemLabel(item, intl)}</span>
      {item.getTag && (
        <span className="text-xs font-mono text-text-secondary">{item.getTag()}</span>
      )}
    </button>
  );
};

/**
 * Goose Swarm sidebar: nav rows, then the Projects tree (pass B — user-curated project folders,
 * each expanding to its own sessions via the server-side cwd filter), then Settings pinned to the
 * bottom. The old recent-chats CHATS section was removed in pass A; the Projects tree is its
 * replacement, with "Unfiled" catching sessions that belong to no registered project.
 */
export const Navigation: React.FC<{ className?: string }> = ({ className }) => {
  const { isNavExpanded } = useNavigationContext();
  const location = useLocation();
  const navigate = useNavigate();

  const { isLocal } = useEdition();
  const { mlxEngine } = useFeatures();

  const visibleItems = useMemo<NavItem[]>(() => {
    return NAV_ITEMS.filter((item) => {
      // Benchmark measures a local fleet, so it has no meaning in an upstream-flavoured build.
      if (item.path === '/benchmark') return isLocal;
      // MLX Engine only exists when the connected agent actually advertises the capability —
      // a nav entry to a surface the backend cannot serve would be a lie.
      if (item.path === '/mlx-engine') return mlxEngine;
      return true;
    });
  }, [isLocal, mlxEngine]);

  const isActive = useCallback((path: string) => location.pathname === path, [location.pathname]);

  const navFocusRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (isNavExpanded) {
      requestAnimationFrame(() => navFocusRef.current?.focus());
    }
  }, [isNavExpanded]);

  if (!isNavExpanded) return null;

  return (
    <motion.div
      ref={navFocusRef}
      tabIndex={-1}
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      transition={{ duration: 0.15 }}
      className={cn('bg-background-primary outline-none flex flex-col h-full', className)}
    >
      <div className="h-[48px] no-drag" />

      {/* Nav items */}
      <div className="px-2 flex flex-col gap-0.5">
        {visibleItems.map((item) => (
          <NavRow
            key={item.id}
            item={item}
            active={isActive(item.path)}
            onClick={() => navigate(item.path)}
          />
        ))}
      </div>

      {/* Projects tree — the pass-B replacement for the removed CHATS section. */}
      <ProjectsSection className="flex-1 mt-3" />

      {/* Settings pinned to bottom */}
      <div className="px-2 pt-2 pb-2 border-t border-border-secondary">
        <NavRow
          item={SETTINGS_NAV_ITEM}
          active={isActive(SETTINGS_NAV_ITEM.path)}
          onClick={() => navigate(SETTINGS_NAV_ITEM.path)}
        />
      </div>
    </motion.div>
  );
};
