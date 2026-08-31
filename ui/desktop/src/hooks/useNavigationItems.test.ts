import { describe, expect, it } from 'vitest';
import { NAV_ITEMS, SETTINGS_NAV_ITEM } from './useNavigationItems';

/**
 * Goose Swarm pass A nav fixture. The sidebar is a deliberate, minimal set — Recipes, Apps,
 * Scheduler, Loop, Extensions and Session History were REMOVED from the nav (their routes stay
 * reachable by URL), and the recent-chats section is gone until the Projects tree lands.
 *
 * Drift guard by design: adding or removing a nav entry MUST fail here so the change is a decision,
 * not an accident. Extend the fixture when the nav legitimately changes.
 */
describe('NAV_ITEMS fixture', () => {
  it('contains exactly the pass-A entries, in order', () => {
    expect(NAV_ITEMS.map((i) => i.id)).toEqual(['home', 'skills', 'memories', 'benchmark', 'mlx']);
    expect(NAV_ITEMS.map((i) => i.path)).toEqual([
      '/',
      '/skills',
      '/memories',
      '/benchmark',
      '/mlx-engine',
    ]);
    expect(NAV_ITEMS.map((i) => i.label)).toEqual([
      'New Chat',
      'Skills',
      'Memories',
      'Benchmark',
      'Leanzero MLX',
    ]);
  });

  it('pins Settings to its own bottom slot', () => {
    expect(SETTINGS_NAV_ITEM.id).toBe('settings');
    expect(SETTINGS_NAV_ITEM.path).toBe('/settings');
  });
});
