import React, { useCallback, useEffect, useMemo, useRef } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';
import { useEdition } from '../../contexts/EditionContext';
import { motion } from 'framer-motion';
import { useNavigationContext } from './NavigationContext';
import { ProjectsSection } from './ProjectsSection';
import { ThemeSwitch } from './ThemeSwitch';
import { useFeatures } from '../../contexts/FeaturesContext';
import {
  NAV_ITEMS,
  SETTINGS_NAV_ITEM,
  getNavItemLabel,
  type NavItem,
} from '../../hooks/useNavigationItems';
import { FOCUS, MOTION, RADIUS, ROW, SURFACE, TNUM, TONE_FILL, TYPE, WEIGHT, cx } from '../lz';
import { LeanZeroGlyph } from '../ProjectLanding';
import { useIntl } from '../../i18n';

// A 36px icon+label row. Selected = the accent fill with accent ink (never a rail); hover = a
// solid step to surface-2. Studio classes are joined with cx — cn/twMerge deletes text-lz-* steps.
const navItemClass = (active: boolean) =>
  cx(
    'no-drag flex w-full items-center gap-3 px-3 text-left text-lz-body',
    ROW.default,
    RADIUS.control,
    WEIGHT.medium,
    FOCUS,
    MOTION,
    active ? SURFACE.selected : cx('text-lz-ink', SURFACE.hover)
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
      // The current view must be identifiable without reading colours: measured live over CDP a
      // sweep of all 19 nav controls once found ZERO with aria-current or aria-selected set.
      aria-current={active ? 'page' : undefined}
    >
      <Icon className={cx('size-4 shrink-0', !active && 'text-lz-ink-3')} />
      {/* A nav row's own label never truncates (DESIGN.md › Buttons): the row is laid out with room
          for it — "Settings" was clipped when the theme switch shared its row. */}
      <span className="flex-1 whitespace-nowrap">{getNavItemLabel(item, intl)}</span>
      {item.getTag && (
        <span className={cx('text-lz-meta', TNUM, active ? 'text-lz-accent-ink' : 'text-lz-ink-3')}>
          {item.getTag()}
        </span>
      )}
    </button>
  );
};

/** The brand block: a solid accent square carrying the LeanZero mark beside the wordmark. */
const BrandBlock: React.FC = () => (
  <div data-testid="brand-block" className="no-drag flex items-center gap-2.5 px-4 pb-3 pt-1">
    <span
      aria-hidden
      data-testid="brand-mark"
      className={cx(
        'flex size-8 shrink-0 items-center justify-center [&_svg]:size-6',
        RADIUS.control,
        TONE_FILL.accent
      )}
    >
      <LeanZeroGlyph />
    </span>
    <span className={cx(TYPE.h2, 'truncate')}>Goose Swarm</span>
  </div>
);

/**
 * Goose Swarm sidebar: brand block, nav rows, then the Projects tree (pass B — user-curated
 * project folders, each expanding to its own sessions via the server-side cwd filter), then
 * Settings pinned to the bottom. The old recent-chats CHATS section was removed in pass A; the
 * Projects tree is its replacement, with "Unfiled" catching sessions that belong to no project.
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
      // The Goose Swarm hub carries the swarm settings and cloud credentials for the local edition,
      // and the MLX engine tab whenever the agent advertises that capability — so it shows for
      // either fact, and only vanishes in an upstream-flavoured build with no MLX engine.
      if (item.path === '/leanzero-swarm') return isLocal || mlxEngine;
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
      className={cx('flex h-full flex-col bg-lz-surface text-lz-ink outline-none', className)}
    >
      <div className="h-[48px] no-drag" />

      <BrandBlock />

      <nav aria-label="Primary" className="flex flex-col gap-px px-2">
        {visibleItems.map((item) => (
          <NavRow
            key={item.id}
            item={item}
            active={isActive(item.path)}
            onClick={() => navigate(item.path)}
          />
        ))}
      </nav>

      {/* Projects tree — the pass-B replacement for the removed CHATS section. */}
      <ProjectsSection className="mt-2 flex-1" />

      {/* The bottom block under a hairline: the theme switch (System | Light | Dark) on its OWN
          36px row, then Settings as one more full-width nav row. Side by side, the switch left the
          Settings row ~38px for its label at the sidebar's 240px and the word was clipped; stacked,
          every row keeps its whole label. */}
      <div
        data-testid="nav-bottom"
        className={cx('flex flex-col gap-px border-t px-2 py-2', SURFACE.hairline)}
      >
        <ThemeSwitch />
        <NavRow
          item={SETTINGS_NAV_ITEM}
          active={isActive(SETTINGS_NAV_ITEM.path)}
          onClick={() => navigate(SETTINGS_NAV_ITEM.path)}
        />
      </div>
    </motion.div>
  );
};
