import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { IntlTestWrapper } from '../../i18n/test-utils';
import { Dialog, DialogContent, DialogTitle } from '../ui/dialog';
import { LAYER } from './tokens';

/**
 * Q-176/Q-23 (critic round 2, 3.0.57): the top-right "Goose Swarm" badge (BaseChat, z-[60]) drew
 * over the Report a problem backdrop (the dialog primitive's z-40). Overlays go above page chrome,
 * everywhere: chrome lives in LAYER.chrome, the dialog primitive in LAYER.overlay, and nothing
 * positioned in the page (`absolute`/`sticky`) sits above the overlay unless it IS an overlay (a
 * listbox or menu that opens from a control).
 */
const zOf = (cls: string) => Number(/z-\[?(\d+)\]?/.exec(cls)?.[1] ?? NaN);

const SRC = resolve(__dirname, '../..');
function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return sources(path);
    return /\.tsx?$/.test(name) && !/\.test\.tsx?$/.test(name) ? [path] : [];
  });
}

/** In-page popovers: a listbox or menu that opens from its control and closes with it. */
const POPOVERS = new Set([
  'components/leanzero-swarm/studio.tsx',
  'components/leanzero-swarm/FilterCombobox.tsx',
  'components/lz/Combobox.tsx',
  'components/alerts/AlertBox.tsx',
  'components/ui/Select.tsx',
]);

describe('stacking layers (Q-176)', () => {
  it('page chrome sits below every overlay', () => {
    expect(zOf(LAYER.chrome)).toBeLessThan(zOf(LAYER.overlay));
  });

  it('the dialog primitive’s backdrop and panel are at the overlay layer', () => {
    render(
      <IntlTestWrapper>
        <Dialog open>
          <DialogContent>
            <DialogTitle>Report a problem</DialogTitle>
          </DialogContent>
        </Dialog>
      </IntlTestWrapper>
    );
    const overlay = document.querySelector('[data-slot="dialog-overlay"]');
    expect(overlay?.className).toContain(LAYER.overlay);
    expect(zOf(overlay?.className ?? '')).toBeGreaterThanOrEqual(zOf(LAYER.overlay));
    expect(screen.getByRole('dialog').className).toContain(LAYER.overlay);
  });

  it('no in-page element (absolute or sticky) is stacked above the overlay layer', () => {
    const offenders: string[] = [];
    for (const file of sources(SRC)) {
      const rel = relative(SRC, file);
      if (POPOVERS.has(rel)) continue;
      const text = readFileSync(file, 'utf8');
      for (const m of text.matchAll(/['"`][^'"`\n]*\b(absolute|sticky)\b[^'"`\n]*['"`]/g)) {
        const z = zOf(m[0]);
        if (z >= zOf(LAYER.overlay)) offenders.push(`${rel}: ${m[0]}`);
      }
    }
    expect(offenders).toEqual([]);
  });

  it('the brand corner and the session title are page chrome', () => {
    const baseChat = readFileSync(join(SRC, 'components/BaseChat.tsx'), 'utf8');
    const corner = baseChat.slice(baseChat.indexOf('data-testid="session-brand-corner"'));
    expect(corner.slice(0, 200)).toContain('LAYER.chrome');
    expect(baseChat).not.toMatch(/right-4 z-\[60\]/);
    const header = readFileSync(join(SRC, 'components/SessionActionsHeader.tsx'), 'utf8');
    expect(header).toContain('LAYER.chrome');
  });
});
