import { describe, expect, it } from 'vitest';
import bundledExtensions from './bundled-extensions.json';
import builtInExtensions from '../../../built-in-extensions.json';

// WR-17 (Gabee 2026-10-31): with memory off by default, "Remember how I like things: ..." had no
// memory tool to call and nothing was saved. Both lists that seed a fresh config carry it on; an
// existing config's stored old default is flipped once by the backend's `memory_default_on`
// migration (crates/goose/src/config/migrations.rs).
describe('memory extension default', () => {
  it.each([
    ['bundled-extensions.json', bundledExtensions],
    ['built-in-extensions.json', builtInExtensions],
  ])('%s ships memory enabled', (_file, list) => {
    const memory = (list as Array<{ id: string; enabled: boolean }>).find((e) => e.id === 'memory');
    expect(memory?.enabled).toBe(true);
  });
});
