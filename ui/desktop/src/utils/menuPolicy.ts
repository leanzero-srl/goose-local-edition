// Electron's default application menu ships Reload, Force Reload and Toggle Developer Tools with
// their accelerators, and main.ts keeps that default menu and only inserts into it. In the packaged
// app a stray Cmd+R on the window that drives a live benchmark reloads the renderer mid-run, so
// those three are switched off in packaged builds only. Pure module: main.ts hands it the real
// Menu tree (MenuItem.visible/enabled are dynamically settable and Menu.items is an array), the
// test hands it a plain object tree, and neither needs a cast.

export const DEV_ONLY_MENU_ROLES = ['reload', 'forcereload', 'toggledevtools'] as const;

export type MenuItemLike = {
  role?: string;
  visible: boolean;
  enabled: boolean;
  submenu?: { items: MenuItemLike[] };
};

const devOnlyRoles: ReadonlySet<string> = new Set(DEV_ONLY_MENU_ROLES);

// Both flags, on purpose. `visible = false` only removes the mouse and mnemonic paths: on macOS a
// hidden item keeps its key equivalent (acceleratorWorksWhenHidden defaults to true) and on
// Windows/Linux the accelerator table checks enabled, never visible. `enabled = false` is what
// actually silences the key on every platform.
export function hideDevOnlyMenuItems(items: MenuItemLike[], isPackaged: boolean): string[] {
  if (!isPackaged) return [];
  const hidden: string[] = [];
  for (const item of items) {
    if (item.role && devOnlyRoles.has(item.role.toLowerCase())) {
      item.visible = false;
      item.enabled = false;
      hidden.push(item.role);
    }
    if (item.submenu) {
      hidden.push(...hideDevOnlyMenuItems(item.submenu.items, isPackaged));
    }
  }
  return hidden;
}
