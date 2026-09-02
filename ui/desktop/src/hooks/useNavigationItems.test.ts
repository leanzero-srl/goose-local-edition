import { describe, expect, it } from 'vitest';
import { NAV_ITEMS, SETTINGS_NAV_ITEM } from './useNavigationItems';

/**
 * Goose Flock nav fixture (pass A declutter + pass C rename + pass D removal). The sidebar is a
 * deliberate, minimal set — Recipes, Apps, Scheduler, Loop, Extensions and Session History were
 * REMOVED from the nav (their routes stay reachable by URL). Pass C: the "Leanzero MLX" entry
 * became "Goose Flock" (/leanzero-swarm — the three-tab management view; /mlx-engine
 * redirects). Pass D (owner): "New Chat" is GONE — sessions start from a project, and the only
 * session-creating affordance is the Projects tree's "+ New session here".
 *
 * Drift guard by design: adding or removing a nav entry MUST fail here so the change is a decision,
 * not an accident. Extend the fixture when the nav legitimately changes.
 */
describe('NAV_ITEMS fixture', () => {
  it('contains exactly the expected entries, in order', () => {
    expect(NAV_ITEMS.map((i) => i.id)).toEqual([
      'skills',
      'memories',
      'benchmark',
      'leanzero-swarm',
    ]);
    expect(NAV_ITEMS.map((i) => i.path)).toEqual([
      '/skills',
      '/memories',
      '/benchmark',
      '/leanzero-swarm',
    ]);
    expect(NAV_ITEMS.map((i) => i.label)).toEqual([
      'Skills',
      'Memories',
      'Benchmark',
      'Goose Flock',
    ]);
  });

  it('has NO New Chat row and no item pointing at "/" — sessions start from projects only', () => {
    expect(NAV_ITEMS.some((i) => i.label === 'New Chat' || i.id === 'home')).toBe(false);
    expect(NAV_ITEMS.some((i) => i.path === '/')).toBe(false);
  });

  it('pins Settings to its own bottom slot', () => {
    expect(SETTINGS_NAV_ITEM.id).toBe('settings');
    expect(SETTINGS_NAV_ITEM.path).toBe('/settings');
  });
});
