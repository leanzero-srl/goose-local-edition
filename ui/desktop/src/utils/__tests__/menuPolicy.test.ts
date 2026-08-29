import { describe, it, expect } from 'vitest';
import { hideDevOnlyMenuItems, DEV_ONLY_MENU_ROLES } from '../menuPolicy';
import type { MenuItemLike } from '../menuPolicy';

type Node = MenuItemLike & { label?: string; type?: string; submenu?: { items: Node[] } };

const item = (props: Partial<Node>): Node => ({ visible: true, enabled: true, ...props });

// Mirrors the shape of Electron's default macOS menu after main.ts has inserted the role-less
// configured items (Find…, Toggle Navigation).
const defaultMenu = (): Node[] => [
  item({
    label: 'Goose',
    submenu: {
      items: [item({ role: 'about' }), item({ type: 'separator' }), item({ role: 'quit' })],
    },
  }),
  item({ label: 'File', submenu: { items: [item({ role: 'close' })] } }),
  item({
    label: 'Edit',
    submenu: {
      items: [
        item({ role: 'selectAll' }),
        item({ label: 'Find…' }),
        item({ label: 'Find Next' }),
        item({ label: 'Find Previous' }),
      ],
    },
  }),
  item({
    label: 'View',
    submenu: {
      items: [
        item({ role: 'reload' }),
        item({ role: 'forceReload' }),
        item({ role: 'toggleDevTools' }),
        item({ type: 'separator' }),
        item({ role: 'resetZoom' }),
        item({ role: 'togglefullscreen' }),
        item({ label: 'Toggle Navigation' }),
      ],
    },
  }),
];

const flatten = (items: Node[]): Node[] =>
  items.flatMap((node) => [node, ...(node.submenu ? flatten(node.submenu.items) : [])]);

const byRole = (items: Node[], role: string): Node => {
  const found = flatten(items).find((node) => node.role === role);
  if (!found) throw new Error(`no item with role ${role}`);
  return found;
};

const byLabel = (items: Node[], label: string): Node => {
  const found = flatten(items).find((node) => node.label === label);
  if (!found) throw new Error(`no item labelled ${label}`);
  return found;
};

describe('hideDevOnlyMenuItems', () => {
  it('returns nothing and touches nothing in a development build', () => {
    const menu = defaultMenu();

    expect(hideDevOnlyMenuItems(menu, false)).toEqual([]);

    for (const node of flatten(menu)) {
      expect(node.visible).toBe(true);
      expect(node.enabled).toBe(true);
    }
  });

  it('hides and disables exactly the three dev-only roles in a packaged build', () => {
    const menu = defaultMenu();

    expect(hideDevOnlyMenuItems(menu, true)).toEqual(['reload', 'forceReload', 'toggleDevTools']);

    // `enabled` is asserted alongside `visible` because a hidden Electron item keeps its key
    // equivalent by default (acceleratorWorksWhenHidden is true on macOS, and Windows/Linux never
    // consult visibility for accelerators). Only enabled=false stops Cmd+R / Cmd+Shift+R /
    // Cmd+Alt+I from firing, so a "simplification" back to visible-only reintroduces the hazard.
    for (const role of ['reload', 'forceReload', 'toggleDevTools']) {
      expect(byRole(menu, role).visible).toBe(false);
      expect(byRole(menu, role).enabled).toBe(false);
    }

    const untouched = flatten(menu).filter(
      (node) => !node.role || !DEV_ONLY_MENU_ROLES.includes(node.role.toLowerCase() as never)
    );
    expect(untouched.length).toBe(flatten(menu).length - 3);
    for (const node of untouched) {
      expect(node.visible).toBe(true);
      expect(node.enabled).toBe(true);
    }
  });

  it('keeps close, quit, zoom, fullscreen and the role-less configured items live when packaged', () => {
    const menu = defaultMenu();
    hideDevOnlyMenuItems(menu, true);

    for (const role of ['close', 'quit', 'resetZoom', 'togglefullscreen']) {
      expect(byRole(menu, role).visible).toBe(true);
      expect(byRole(menu, role).enabled).toBe(true);
    }
    for (const label of ['Find…', 'Find Next', 'Find Previous', 'Toggle Navigation']) {
      expect(byLabel(menu, label).visible).toBe(true);
      expect(byLabel(menu, label).enabled).toBe(true);
    }
  });

  it('matches roles regardless of casing', () => {
    const menu = [
      item({
        label: 'View',
        submenu: { items: [item({ role: 'FORCERELOAD' }), item({ role: 'ToggleDevTools' })] },
      }),
    ];

    expect(hideDevOnlyMenuItems(menu, true)).toEqual(['FORCERELOAD', 'ToggleDevTools']);
    expect(byRole(menu, 'FORCERELOAD').visible).toBe(false);
    expect(byRole(menu, 'FORCERELOAD').enabled).toBe(false);
    expect(byRole(menu, 'ToggleDevTools').visible).toBe(false);
    expect(byRole(menu, 'ToggleDevTools').enabled).toBe(false);
  });

  it('leaves items without a role untouched, including separators', () => {
    const menu = [
      item({
        label: 'View',
        submenu: {
          items: [item({ type: 'separator' }), item({ label: 'Plain' }), item({ role: 'reload' })],
        },
      }),
    ];

    hideDevOnlyMenuItems(menu, true);

    expect(menu[0].visible).toBe(true);
    expect(menu[0].submenu?.items[0].visible).toBe(true);
    expect(menu[0].submenu?.items[0].enabled).toBe(true);
    expect(byLabel(menu, 'Plain').visible).toBe(true);
    expect(byLabel(menu, 'Plain').enabled).toBe(true);
    expect(byRole(menu, 'reload').visible).toBe(false);
  });

  it('accepts a tree shaped like Electron MenuItem without a cast', () => {
    // A superset of MenuItemLike (extra label/type/accelerator fields, nested Menu-like submenu)
    // must be accepted structurally, which is what lets main.ts pass Menu.items straight in.
    const electronLike = [
      {
        label: 'View',
        type: 'submenu',
        accelerator: undefined,
        visible: true,
        enabled: true,
        submenu: {
          items: [
            { role: 'toggleDevTools', accelerator: 'Alt+Cmd+I', visible: true, enabled: true },
          ],
        },
      },
    ];

    expect(hideDevOnlyMenuItems(electronLike, true)).toEqual(['toggleDevTools']);
    expect(electronLike[0].submenu.items[0].enabled).toBe(false);
  });
});
