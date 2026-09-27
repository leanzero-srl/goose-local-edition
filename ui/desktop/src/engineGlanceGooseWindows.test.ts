import { EventEmitter } from 'node:events';
import { describe, expect, it } from 'vitest';
import {
  gooseWindowFacts,
  trackOutOfSight,
  type GooseWindowLike,
} from './engineGlanceGooseWindows';

/** A BrowserWindow as Electron 41 behaves on macOS (Q-226 probe): occlusion is 'hide' / 'show'. */
class FakeWindow extends EventEmitter implements GooseWindowLike {
  visible = true;
  minimized = false;
  destroyed = false;
  bounds = { x: 0, y: 40, width: 940, height: 800 };
  isDestroyed = () => this.destroyed;
  isVisible = () => this.visible;
  isMinimized = () => this.minimized;
  getBounds = () => this.bounds;
  occlude() {
    this.emit('hide');
  }
  unocclude() {
    this.emit('show');
  }
  minimize() {
    this.minimized = true;
    this.emit('hide');
  }
  restore() {
    this.minimized = false;
    this.emit('show');
  }
}

function tracked() {
  const win = new FakeWindow();
  trackOutOfSight(win);
  return win;
}

describe('gooseWindowFacts — which goose windows can be seen (Q-226)', () => {
  it('in view: on screen, whether focused or not', () => {
    const win = tracked();
    expect(gooseWindowFacts([win], null)).toEqual([
      { onScreen: true, focused: false, bounds: win.bounds },
    ]);
    expect(gooseWindowFacts([win], win)[0].focused).toBe(true);
  });

  it('wholly covered, or another app full screen on its Space: out of sight while isVisible() stays true', () => {
    const win = tracked();
    win.occlude();
    expect(win.isVisible()).toBe(true);
    expect(gooseWindowFacts([win], null)[0].onScreen).toBe(false);
    win.unocclude();
    expect(gooseWindowFacts([win], null)[0].onScreen).toBe(true);
  });

  it('minimized, then restored', () => {
    const win = tracked();
    win.minimize();
    expect(gooseWindowFacts([win], null)[0].onScreen).toBe(false);
    win.restore();
    expect(gooseWindowFacts([win], null)[0].onScreen).toBe(true);
  });

  it('hidden (closed to the tray) with no event yet, or destroyed: not counted as seen', () => {
    const hidden = tracked();
    hidden.visible = false;
    const gone = tracked();
    gone.destroyed = true;
    expect(gooseWindowFacts([hidden, gone], null)).toEqual([
      { onScreen: false, focused: false, bounds: hidden.bounds },
    ]);
  });
});
