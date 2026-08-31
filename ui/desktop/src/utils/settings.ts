export interface ExternalGoosedConfig {
  enabled: boolean;
  url: string;
  secret: string;
  certFingerprint?: string;
}

// Pass D (owner): sessions start from projects only. `newChat` (Cmd+T fresh chat) and
// `quickLauncher` (floating input that created a project-less session) left the shortcut set with
// their affordances; a stored value under either key is simply ignored. `newChatWindow` keeps its
// storage key but the menu item it drives is "New Window" — it opens a window on the project
// landing and creates no session.
export interface KeyboardShortcuts {
  focusWindow: string | null;
  newChatWindow: string | null;
  openDirectory: string | null;
  settings: string | null;
  find: string | null;
  findNext: string | null;
  findPrevious: string | null;
  alwaysOnTop: string | null;
  toggleNavigation: string | null;
}

export type DefaultKeyboardShortcuts = {
  [K in keyof KeyboardShortcuts]: string;
};

// prettier-ignore
export type LanguageSetting =
  | 'system' | 'en' | 'es' | 'fr' | 'de' | 'it' | 'pt' | 'id' | 'ms' | 'vi'
  | 'hi' | 'ja' | 'ko' | 'ru' | 'tr' | 'zh-CN' | 'zh-TW';

export interface Settings {
  // Desktop app settings
  showMenuBarIcon: boolean;
  disableAutoDownload: boolean;
  showDockIcon: boolean;
  enableWakelock: boolean;
  enableNotifications: boolean;
  spellcheckEnabled: boolean;
  externalGoosed: ExternalGoosedConfig;
  globalShortcut?: string | null;
  keyboardShortcuts: KeyboardShortcuts;

  // UI preferences (migrated from localStorage)
  theme: 'dark' | 'light';
  useSystemTheme: boolean;
  language: LanguageSetting;
  responseStyle: string;
  showPricing: boolean;
  seenAnnouncementIds: string[];

  // Goose Swarm (edition value 'local') — the local/swarm-model UX skin (presentation only, never
  // gates capability). OPTIONAL on purpose: absence means "never explicitly chosen", which lets the
  // renderer derive the edition from the active provider exactly like the Rust resolver
  // (crates/goose-cli/src/edition.rs). A default here would masquerade as an explicit choice and
  // permanently suppress derivation.
  edition?: 'standard' | 'local';
}

export type SettingKey = keyof Settings;

export const defaultKeyboardShortcuts: DefaultKeyboardShortcuts = {
  focusWindow: 'CommandOrControl+Alt+G',
  newChatWindow: 'CommandOrControl+N',
  openDirectory: 'CommandOrControl+O',
  settings: 'CommandOrControl+,',
  find: 'CommandOrControl+F',
  findNext: 'CommandOrControl+G',
  findPrevious: 'CommandOrControl+Shift+G',
  alwaysOnTop: 'CommandOrControl+Shift+T',
  toggleNavigation: 'CommandOrControl+/',
};

export const defaultSettings: Settings = {
  // Desktop app settings
  showMenuBarIcon: true,
  disableAutoDownload: false,
  showDockIcon: true,
  enableWakelock: false,
  enableNotifications: true,
  spellcheckEnabled: true,
  keyboardShortcuts: defaultKeyboardShortcuts,
  externalGoosed: {
    enabled: false,
    url: '',
    secret: '',
  },

  // UI preferences
  theme: 'light',
  useSystemTheme: true,
  language: 'system',
  responseStyle: 'concise',
  showPricing: true,
  seenAnnouncementIds: [],
};

export function getKeyboardShortcuts(settings: Settings): KeyboardShortcuts {
  if (!settings.keyboardShortcuts && settings.globalShortcut !== undefined) {
    return {
      ...defaultKeyboardShortcuts,
      focusWindow: settings.globalShortcut,
    };
  }
  return { ...defaultKeyboardShortcuts, ...settings.keyboardShortcuts };
}
