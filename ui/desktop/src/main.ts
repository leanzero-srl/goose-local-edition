import type { OpenDialogOptions, OpenDialogReturnValue } from 'electron';
import {
  app,
  App,
  BrowserWindow,
  dialog,
  globalShortcut,
  ipcMain,
  Menu,
  MenuItem,
  net,
  Notification,
  powerSaveBlocker,
  screen,
  session,
  shell,
  Tray,
} from 'electron';
import { pathToFileURL, format as formatUrl, URLSearchParams } from 'node:url';
import { Buffer } from 'node:buffer';
import fs from 'node:fs/promises';
import fsSync from 'node:fs';
import started from 'electron-squirrel-startup';
import path from 'node:path';
import os from 'node:os';
import { execFileSync, spawn, execFile } from 'child_process';
import 'dotenv/config';
import { checkBackendStatus } from './backendStatus';
import { startGooseServe, findGooseBinaryPath } from './gooseServe';
import { GooseServeLeaseRegistry, type GooseServeLease } from './gooseServeLeaseRegistry';
import { acpWebSocketUrlFromHttpBase, normalizeAcpHttpBaseUrl } from './acp/url';
import { expandTilde } from './utils/pathUtils';
import log from './utils/logger';
import { ensureWinShims } from './utils/winShims';
import { addRecentDir, loadRecentDirs } from './utils/recentDirs';
import { formatAppName, errorMessage, formatErrorForLogging } from './utils/conversionUtils';
import { isRetiredGooseChatApp } from './utils/retiredApps';
import type { Settings, SettingKey } from './utils/settings';
import { defaultSettings, getKeyboardShortcuts } from './utils/settings';
import * as crypto from 'crypto';
import * as yaml from 'yaml';
import windowStateKeeper from 'electron-window-state';
import {
  getUpdateAvailable,
  registerUpdateIpcHandlers,
  setAutoDownloadDisabled,
  setTrayRef,
  setupAutoUpdater,
  updateTrayMenu,
} from './utils/autoUpdater';
import { UPDATES_ENABLED } from './updates';
import './utils/recipeHash';
import type { GooseApp } from './types/apps';
import installExtension, { REACT_DEVELOPER_TOOLS } from 'electron-devtools-installer';
import { BLOCKED_PROTOCOLS, WEB_PROTOCOLS } from './utils/urlSecurity';
import { buildCSP } from './utils/csp';

function shouldSetupUpdater(): boolean {
  // Setup updater if either the flag is enabled OR dev updates are enabled
  return UPDATES_ENABLED || process.env.ENABLE_DEV_UPDATES === 'true';
}

// =======================================================================
// Native menu localization
// -----------------------------------------------------------------------
// Electron's main process can't use react-intl (which runs in the renderer),
// so the native menu bar is translated here with a small hand-maintained
// dictionary. Only Simplified Chinese is filled in right now; other locales
// fall through to the original English labels. Keep the keys in sync with
// the raw label strings used below.
// =======================================================================

const MENU_TRANSLATIONS_ZH_CN: Record<string, string> = {
  // Top-level
  File: '文件',
  Edit: '编辑',
  View: '视图',
  Window: '窗口',
  Help: '帮助',
  // Context menu
  'Add to dictionary': '添加到词典',
  Cut: '剪切',
  Copy: '复制',
  Paste: '粘贴',
  // Goose-added items
  'New Window': '新建窗口',
  Settings: '设置',
  'Find…': '查找…',
  'Find Next': '查找下一个',
  'Find Previous': '查找上一个',
  'Use Selection for Find': '用所选内容查找',
  Find: '查找',
  'New Chat': '新建聊天',
  'New Chat Window': '新建聊天窗口',
  'Open Directory...': '打开目录…',
  'Recent Directories': '最近的目录',
  'Focus Goose Window': '聚焦 Goose 窗口',
  'Quick Launcher': '快速启动器',
  'Always on Top': '窗口置顶',
  'Toggle Navigation': '切换导航',
  'About Goose': '关于 Goose',
  // Electron's default role-based labels we want to translate as well.
  // (The menu role itself still provides the correct behaviour; only the
  // display string is overridden.)
  Undo: '撤销',
  Redo: '重做',
  'Select All': '全选',
  Delete: '删除',
  Speech: '语音',
  Reload: '重新加载',
  'Force Reload': '强制重新加载',
  'Toggle Developer Tools': '切换开发者工具',
  'Actual Size': '实际大小',
  'Reset Zoom': '重置缩放',
  'Zoom In': '放大',
  'Zoom Out': '缩小',
  'Toggle Full Screen': '切换全屏',
  'Toggle Fullscreen': '切换全屏',
  Minimize: '最小化',
  Close: '关闭',
  'Close Window': '关闭窗口',
  Quit: '退出',
  Exit: '退出',
  'Bring All to Front': '全部置于最前',
  'Emoji & Symbols': '表情符号',
  'Start Dictation…': '开始听写…',
  'Hide Goose': '隐藏 Goose',
  'Hide Others': '隐藏其他',
  'Show All': '全部显示',
  Services: '服务',
};

function detectMenuLocale(): string {
  return getConfiguredGooseLocale() ?? 'en';
}

function menuT(label: string): string {
  // Normalize underscores to hyphens so POSIX-style tags like "zh_CN" work.
  const lower = detectMenuLocale().replace(/_/g, '-').toLowerCase();
  const isTraditional = /^zh-(hant|tw|hk|mo)\b/.test(lower);
  const isSimplifiedChinese = !isTraditional && (lower === 'zh' || lower.startsWith('zh-'));
  if (isSimplifiedChinese) {
    return MENU_TRANSLATIONS_ZH_CN[label] ?? label;
  }
  return label;
}

/**
 * Recursively translate `label` on every item in the given menu, including nested submenus.
 * Electron's default application menu comes with English labels that are not otherwise
 * configurable, so we post-process them here before calling `Menu.setApplicationMenu`.
 */
function translateMenuLabels(items: MenuItem[]): void {
  for (const item of items) {
    if (item.label) {
      const translated = menuT(item.label);
      if (translated !== item.label) {
        // MenuItem.label is a writable property on the main-process side, even though
        // the TS type sometimes claims otherwise. Cast through unknown for safety.
        (item as unknown as { label: string }).label = translated;
      }
    }
    if (item.submenu && item.submenu.items) {
      translateMenuLabels(item.submenu.items);
    }
  }
}

// Settings management
const SETTINGS_FILE = path.join(app.getPath('userData'), 'settings.json');
const STARTUP_LOGS_DIR = path.join(app.getPath('userData'), 'logs', 'startup');
const validLanguageSettings = new Set<Settings['language']>([
  'system',
  'en',
  'es',
  'fr',
  'de',
  'it',
  'pt',
  'id',
  'ms',
  'vi',
  'hi',
  'ja',
  'ko',
  'ru',
  'tr',
  'zh-CN',
  'zh-TW',
]);

function isValidLanguageSetting(value: unknown): value is Settings['language'] {
  return typeof value === 'string' && validLanguageSettings.has(value as Settings['language']);
}

function getSettings(): Settings {
  if (fsSync.existsSync(SETTINGS_FILE)) {
    let stored: Partial<Settings>;
    try {
      const data = fsSync.readFileSync(SETTINGS_FILE, 'utf8');
      stored = JSON.parse(data) as Partial<Settings>;
    } catch (err) {
      console.error('Failed to read settings.json, using defaults:', err);
      return defaultSettings;
    }
    return {
      ...defaultSettings,
      ...stored,
      externalGoosed: {
        ...defaultSettings.externalGoosed,
        ...(stored.externalGoosed ?? {}),
      },
      keyboardShortcuts: {
        ...defaultSettings.keyboardShortcuts,
        ...(stored.keyboardShortcuts ?? {}),
      },
    };
  }
  return defaultSettings;
}

function updateSettings(modifier: (settings: Settings) => void): void {
  const settings = getSettings();
  modifier(settings);
  fsSync.writeFileSync(SETTINGS_FILE, JSON.stringify(settings, null, 2));
}

function getConfiguredGooseLocale(): string | undefined {
  const language = getSettings().language;
  if (isValidLanguageSetting(language) && language !== 'system') {
    return language;
  }

  if (process.env.GOOSE_LOCALE) {
    return process.env.GOOSE_LOCALE;
  }

  try {
    return app.isReady() ? app.getSystemLocale() || undefined : undefined;
  } catch {
    return undefined;
  }
}

function listGitWorktreeDirs(dir: string): Promise<string[]> {
  return new Promise((resolve) => {
    if (!dir?.trim()) {
      resolve([]);
      return;
    }

    execFile(
      'git',
      ['-C', dir, 'worktree', 'list', '--porcelain'],
      { timeout: 3000 },
      (error, stdout) => {
        if (error) {
          resolve([]);
          return;
        }

        const dirs = stdout
          .split('\n')
          .filter((line) => line.startsWith('worktree '))
          .map((line) => line.slice('worktree '.length).trim())
          .filter(Boolean)
          .filter((worktreeDir, index, allDirs) => allDirs.indexOf(worktreeDir) === index);

        resolve(dirs);
      }
    );
  });
}

async function configureProxy() {
  const httpsProxy = process.env.HTTPS_PROXY || process.env.https_proxy;
  const httpProxy = process.env.HTTP_PROXY || process.env.http_proxy;
  const noProxy = process.env.NO_PROXY || process.env.no_proxy || '';

  const proxyUrl = httpsProxy || httpProxy;

  if (proxyUrl) {
    console.log('[Main] Configuring proxy');
    await session.defaultSession.setProxy({
      proxyRules: proxyUrl,
      proxyBypassRules: noProxy,
    });
    console.log('[Main] Proxy configured successfully');
  }
}

if (started) app.quit();

// Certificate trust for active backend leases. Renderer requests and
// main-process net.fetch both pin to the exact cert fingerprint. Each backend
// lease owns a trust record so old windows keep working after settings change.
interface BackendCertificateTrust {
  hostname: string;
  fingerprint: string | null;
}

interface BackendCertificateTrustRegistration {
  trust: BackendCertificateTrust;
  release: () => void;
}

const trustedBackendCertificates = new Set<BackendCertificateTrust>();

function normalizeHostname(hostname: string): string {
  return hostname.toLowerCase();
}

function normalizeFingerprint(fp: string): string {
  if (fp.startsWith('sha256/')) {
    const b64 = fp.slice('sha256/'.length);
    const buf = Buffer.from(b64, 'base64');
    return Array.from(buf)
      .map((b) => b.toString(16).padStart(2, '0'))
      .join(':')
      .toUpperCase();
  }
  return fp.toUpperCase();
}

function trustBackendCertificate(
  hostname: string,
  fingerprint: string | null
): BackendCertificateTrustRegistration {
  const trust: BackendCertificateTrust = {
    hostname: normalizeHostname(hostname),
    fingerprint: fingerprint ? normalizeFingerprint(fingerprint) : null,
  };
  trustedBackendCertificates.add(trust);
  return {
    trust,
    release: () => {
      trustedBackendCertificates.delete(trust);
    },
  };
}

function getBackendCertificateTrusts(hostname: string): BackendCertificateTrust[] {
  const normalizedHostname = normalizeHostname(hostname);
  return [...trustedBackendCertificates].filter((trust) => trust.hostname === normalizedHostname);
}

function verifyBackendCertificate(hostname: string, fingerprint: string): boolean {
  const normalizedFingerprint = normalizeFingerprint(fingerprint);
  const trusts = getBackendCertificateTrusts(hostname);
  if (trusts.length === 0) {
    return false;
  }

  if (trusts.some((trust) => trust.fingerprint === normalizedFingerprint)) {
    return true;
  }

  const tofuTrust = trusts.find((trust) => trust.fingerprint === null);
  if (tofuTrust) {
    // TOFU: pin the certificate from the first successful handshake.
    tofuTrust.fingerprint = normalizedFingerprint;
    return true;
  }

  return false;
}

function isTrustedHost(hostname: string): boolean {
  return getBackendCertificateTrusts(hostname).length > 0;
}

// Renderer requests: pin to the exact cert once known.
app.on('certificate-error', (event, _webContents, url, _error, certificate, callback) => {
  const parsed = new URL(url);
  if (!isTrustedHost(parsed.hostname)) {
    callback(false);
    return;
  }

  event.preventDefault();
  callback(verifyBackendCertificate(parsed.hostname, certificate.fingerprint));
});

app.whenReady().then(() => {
  appConfig.GOOSE_LOCALE = getConfiguredGooseLocale();
});

// Main-process net.fetch: pin to the exact cert once known.
app.whenReady().then(() => {
  session.defaultSession.setCertificateVerifyProc((request, callback) => {
    if (!isTrustedHost(request.hostname)) {
      callback(-3);
      return;
    }

    const match = verifyBackendCertificate(request.hostname, request.certificate.fingerprint);
    callback(match ? 0 : -2);
  });
});

if (process.env.ENABLE_PLAYWRIGHT) {
  const debugPort = process.env.PLAYWRIGHT_DEBUG_PORT || '9222';
  console.log(`[Main] Enabling Playwright remote debugging on port ${debugPort}`);
  app.commandLine.appendSwitch('remote-debugging-port', debugPort);
}

// Goose Local Edition — stop the macOS Keychain "Goose Safe Storage" password prompt that appears on
// EVERY launch. That item is Chromium's cookie-store encryption key; its Keychain ACL is bound to the app's
// code signature, but this self-signed build has no stable Team ID so the ACL is invalidated on every
// rebuild and macOS re-prompts for the OS password. On macOS, Chromium's OSCrypt uses the Keychain
// UNCONDITIONALLY and ignores --password-store (that switch is Linux-only) — so the real fix is
// --use-mock-keychain, which makes Chromium use an in-memory mock keychain and never touch the real one.
// This only affects Electron's own cookie store (a localhost-only app has ~nothing there); goose's real
// secrets use the separate Rust keyring and are unaffected. Opt back in with GOOSE_USE_KEYCHAIN=1.
if (process.env.GOOSE_USE_KEYCHAIN !== '1') {
  app.commandLine.appendSwitch('use-mock-keychain');
  app.commandLine.appendSwitch('password-store', 'basic'); // Linux parity; a no-op on macOS.
}

// In development mode, force registration as the default protocol client
// In production, register normally
if (MAIN_WINDOW_VITE_DEV_SERVER_URL) {
  // Development mode - force registration
  console.log('[Main] Development mode: Forcing protocol registration for goose://');
  app.setAsDefaultProtocolClient('goose');

  if (process.platform === 'darwin') {
    try {
      // Reset the default handler to ensure dev version takes precedence
      spawn('open', ['-a', process.execPath, '--args', '--reset-protocol-handler', 'goose'], {
        detached: true,
        stdio: 'ignore',
      });
    } catch {
      console.warn('[Main] Could not reset protocol handler');
    }
  }
} else {
  // Production mode - normal registration
  app.setAsDefaultProtocolClient('goose');
}

// Apply single instance lock on Windows and Linux where it's needed for deep links
// macOS uses the 'open-url' event instead
let gotTheLock = true;
let openUrlHandledLaunch = false;
if (process.platform !== 'darwin') {
  gotTheLock = app.requestSingleInstanceLock();

  if (!gotTheLock) {
    app.quit();
  } else {
    app.on('second-instance', (_event, commandLine) => {
      const protocolUrl = commandLine.find((arg) => arg.startsWith('goose://'));
      if (protocolUrl) {
        const parsedUrl = new URL(protocolUrl);
        // If it's a bot/recipe URL, handle it directly by creating a new window
        if (parsedUrl.hostname === 'bot' || parsedUrl.hostname === 'recipe') {
          app.whenReady().then(async () => {
            const recentDirs = loadRecentDirs();
            const openDir = recentDirs.length > 0 ? recentDirs[0] : null;

            const deeplinkData = parseRecipeDeeplink(protocolUrl);
            const scheduledJobId = parsedUrl.searchParams.get('scheduledJob');

            await createChat(app, {
              dir: openDir || undefined,
              recipeDeeplink: deeplinkData?.config,
              scheduledJobId: scheduledJobId || undefined,
              recipeParameters: deeplinkData?.parameters,
            });
          });
          return; // Skip the rest of the handler
        }

        // Handle new-session URL by creating a fresh chat window
        if (parsedUrl.hostname === 'new-session') {
          app.whenReady().then(async () => {
            const recentDirs = loadRecentDirs();
            const openDir = recentDirs.length > 0 ? recentDirs[0] : null;
            const prompt = parsedUrl.searchParams.get('prompt') || undefined;
            await createChat(app, {
              dir: openDir || undefined,
              initialMessage: prompt,
              initialMessageNoAutoSubmit: prompt !== undefined,
            });
          });
          return;
        }

        if (parsedUrl.hostname === 'resume') {
          app.whenReady().then(async () => {
            const recentDirs = loadRecentDirs();
            const openDir = recentDirs.length > 0 ? recentDirs[0] : null;
            await createResumeChatWindow(parsedUrl, openDir || undefined);
          });
          return;
        }

        // For non-bot URLs, continue with normal handling
        handleProtocolUrl(protocolUrl, parsedUrl);
      }

      // Only focus existing windows for non-bot/recipe URLs
      const existingWindows = BrowserWindow.getAllWindows();
      if (existingWindows.length > 0) {
        const mainWindow = existingWindows[0];
        if (mainWindow.isMinimized()) {
          mainWindow.restore();
        }
        mainWindow.focus();
      }
    });
  }

  // Handle protocol URLs on Windows and Linux startup
  const protocolUrl = process.argv.find((arg) => arg.startsWith('goose://'));
  if (protocolUrl) {
    app.whenReady().then(async () => {
      let parsedUrl: URL;
      try {
        parsedUrl = new URL(protocolUrl);
      } catch (error) {
        log.warn('[Main] Ignoring invalid startup protocol URL:', errorMessage(error));
        return;
      }

      openUrlHandledLaunch = true;
      try {
        await handleProtocolUrl(protocolUrl, parsedUrl);
      } catch (error) {
        log.error('[Main] Failed to handle startup protocol URL:', errorMessage(error));
        if (BrowserWindow.getAllWindows().length === 0) {
          const { dirPath } = parseArgs();
          await createNewWindow(app, dirPath);
        }
      }
    });
  }
}

const pendingDeepLinks = new Map<number, string>();

function queuePendingDeepLink(windowId: number, url: string): void {
  if (pendingDeepLinks.get(windowId) === url) {
    return;
  }
  pendingDeepLinks.set(windowId, url);
}

const reactReadyWindows = new Set<number>();

const DEEPLINK_BURST_DEDUP_MS = 2000;
const recentSessionDeepLinkSends = new Map<string, number>();

function pruneExpiredSessionDeepLinkSends(now: number): void {
  for (const [url, sentAt] of recentSessionDeepLinkSends) {
    if (now - sentAt >= DEEPLINK_BURST_DEDUP_MS) {
      recentSessionDeepLinkSends.delete(url);
    }
  }
}

function isBurstDuplicateSessionDeepLink(url: string): boolean {
  const now = Date.now();
  pruneExpiredSessionDeepLinkSends(now);
  const sentAt = recentSessionDeepLinkSends.get(url);
  return sentAt !== undefined && now - sentAt < DEEPLINK_BURST_DEDUP_MS;
}

function recordSessionDeepLinkSend(url: string): void {
  const now = Date.now();
  recentSessionDeepLinkSends.set(url, now);
  pruneExpiredSessionDeepLinkSends(now);
}

function sendOpenSharedSession(window: BrowserWindow, url: string): void {
  if (isBurstDuplicateSessionDeepLink(url)) {
    log.info('[Main] Ignoring burst duplicate session deep link');
    return;
  }
  recordSessionDeepLinkSend(url);
  window.webContents.send('open-shared-session', url);
}

function deliverExtensionOrSessionDeepLink(
  url: string,
  parsedUrl: URL,
  targetWindow: BrowserWindow
): void {
  if (!reactReadyWindows.has(targetWindow.id) || targetWindow.webContents.isLoadingMainFrame()) {
    queuePendingDeepLink(targetWindow.id, url);
    return;
  }

  if (parsedUrl.hostname === 'extension') {
    targetWindow.webContents.send('add-extension', url);
  } else if (parsedUrl.hostname === 'sessions') {
    sendOpenSharedSession(targetWindow, url);
  }
}

function getResumeSessionId(parsedUrl: URL): string | null {
  try {
    const sessionId = decodeURIComponent(parsedUrl.pathname.replace(/^\/+/, '')).trim();
    return sessionId || null;
  } catch {
    return null;
  }
}

async function createResumeChatWindow(parsedUrl: URL, dir?: string): Promise<boolean> {
  const resumeSessionId = getResumeSessionId(parsedUrl);
  if (!resumeSessionId) {
    log.warn('[Main] Ignoring goose://resume URL without a session id');
    return false;
  }

  await createChat(app, { dir, resumeSessionId });
  return true;
}

async function handleProtocolUrl(url: string, parsedUrl: URL) {
  if (!url) return;

  const recentDirs = loadRecentDirs();
  const openDir = recentDirs.length > 0 ? recentDirs[0] : null;

  if (parsedUrl.hostname === 'new-session') {
    const prompt = parsedUrl.searchParams.get('prompt') || undefined;
    await createChat(app, {
      dir: openDir || undefined,
      initialMessage: prompt,
      initialMessageNoAutoSubmit: prompt !== undefined,
    });
    return;
  } else if (parsedUrl.hostname === 'resume') {
    await createResumeChatWindow(parsedUrl, openDir || undefined);
    return;
  } else if (parsedUrl.hostname === 'bot' || parsedUrl.hostname === 'recipe') {
    const existingWindows = BrowserWindow.getAllWindows();
    const targetWindow =
      existingWindows.length > 0
        ? existingWindows[0]
        : await createChat(app, { dir: openDir || undefined });
    if (!targetWindow) return;
    await processProtocolUrl(url, parsedUrl, targetWindow);
  } else {
    const existingWindows = BrowserWindow.getAllWindows();
    let targetWindow: BrowserWindow | undefined;
    if (existingWindows.length > 0) {
      targetWindow = existingWindows[0];
      if (targetWindow.isMinimized()) {
        targetWindow.restore();
      }
      targetWindow.focus();
    } else {
      targetWindow = await createChat(app, { dir: openDir || undefined });
    }

    if (!targetWindow) return;

    if (targetWindow.webContents.isLoadingMainFrame()) {
      queuePendingDeepLink(targetWindow.id, url);
    } else {
      await processProtocolUrl(url, parsedUrl, targetWindow);
    }
  }
}

async function processProtocolUrl(url: string, parsedUrl: URL, window: BrowserWindow) {
  const recentDirs = loadRecentDirs();
  const openDir = recentDirs.length > 0 ? recentDirs[0] : null;

  if (parsedUrl.hostname === 'extension') {
    window.webContents.send('add-extension', url);
  } else if (parsedUrl.hostname === 'sessions') {
    sendOpenSharedSession(window, url);
  } else if (parsedUrl.hostname === 'bot' || parsedUrl.hostname === 'recipe') {
    const deeplinkData = parseRecipeDeeplink(url);
    const scheduledJobId = parsedUrl.searchParams.get('scheduledJob');

    await createChat(app, {
      dir: openDir || undefined,
      recipeDeeplink: deeplinkData?.config,
      scheduledJobId: scheduledJobId || undefined,
      recipeParameters: deeplinkData?.parameters,
    });
  }
}

let windowDeeplinkURL: string | null = null;

app.on('open-url', async (_event, url) => {
  if (process.platform !== 'win32') {
    const parsedUrl = new URL(url);

    log.info(
      '[Main] Received open-url event:',
      url.includes('key=') ? url.replace(/key=[^&]+/, 'key=REDACTED') : url
    );

    await app.whenReady();

    const recentDirs = loadRecentDirs();
    const openDir = recentDirs.length > 0 ? recentDirs[0] : null;

    // Handle new-session URL by creating a fresh chat window
    if (parsedUrl.hostname === 'new-session') {
      log.info('[Main] Detected new-session URL, creating new chat window');
      openUrlHandledLaunch = true;
      const prompt = parsedUrl.searchParams.get('prompt') || undefined;
      await createChat(app, {
        dir: openDir || undefined,
        initialMessage: prompt,
        initialMessageNoAutoSubmit: prompt !== undefined,
      });
      return;
    }

    if (parsedUrl.hostname === 'resume') {
      log.info('[Main] Detected resume URL, creating session resume window');
      openUrlHandledLaunch = await createResumeChatWindow(parsedUrl, openDir || undefined);
      return;
    }

    // Handle bot/recipe URLs by directly creating a new window
    if (parsedUrl.hostname === 'bot' || parsedUrl.hostname === 'recipe') {
      log.info('[Main] Detected bot/recipe URL, creating new chat window');
      openUrlHandledLaunch = true;
      const deeplinkData = parseRecipeDeeplink(url);
      if (deeplinkData) {
        windowDeeplinkURL = url;
      }
      const scheduledJobId = parsedUrl.searchParams.get('scheduledJob');

      await createChat(app, {
        dir: openDir || undefined,
        recipeDeeplink: deeplinkData?.config,
        scheduledJobId: scheduledJobId || undefined,
        recipeParameters: deeplinkData?.parameters,
      });
      windowDeeplinkURL = null;
      return;
    }

    // For extension/session URLs, send to existing window or store pending for new one
    const existingWindows = BrowserWindow.getAllWindows();
    if (existingWindows.length > 0) {
      const targetWindow = existingWindows[0];
      if (targetWindow.isMinimized()) targetWindow.restore();
      targetWindow.focus();
      if (parsedUrl.hostname === 'extension' || parsedUrl.hostname === 'sessions') {
        deliverExtensionOrSessionDeepLink(url, parsedUrl, targetWindow);
      }
    } else {
      openUrlHandledLaunch = true;
      const newWindow = await createChat(app, { dir: openDir || undefined });
      if (!newWindow) return;
      queuePendingDeepLink(newWindow.id, url);
    }
  }
});

// Handle macOS drag-and-drop onto dock icon
app.on('will-finish-launching', () => {
  if (process.platform === 'darwin') {
    app.setAboutPanelOptions({
      applicationName: 'Goose',
      applicationVersion: app.getVersion(),
    });
  }
});

// Handle drag-and-drop onto dock icon
app.on('open-file', async (event, filePath) => {
  event.preventDefault();
  await handleFileOpen(filePath);
});

// Handle multiple files/folders (macOS only)
if (process.platform === 'darwin') {
  // Use type assertion for non-standard Electron event
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  app.on('open-files' as any, async (event: any, filePaths: string[]) => {
    event.preventDefault();
    for (const filePath of filePaths) {
      await handleFileOpen(filePath);
    }
  });
}

async function handleFileOpen(filePath: string) {
  try {
    if (!filePath || typeof filePath !== 'string') {
      return;
    }

    const stats = fsSync.lstatSync(filePath);
    let targetDir = filePath;

    // If it's a file, use its parent directory
    if (stats.isFile()) {
      targetDir = path.dirname(filePath);
    }

    // Add to recent directories
    addRecentDir(targetDir);

    // Create new window for the directory
    const newWindow = await createChat(app, { dir: targetDir });

    // Focus the new window
    if (newWindow) {
      newWindow.show();
      newWindow.focus();
      newWindow.moveTop();
    }
  } catch (error) {
    console.error('Failed to handle file open:', error);

    // Show user-friendly error notification
    new Notification({
      title: 'Goose',
      body: `Could not open directory: ${path.basename(filePath)}`,
    }).show();
  }
}

declare var MAIN_WINDOW_VITE_DEV_SERVER_URL: string;
declare var MAIN_WINDOW_VITE_NAME: string;

function getAppUrl(): URL {
  return MAIN_WINDOW_VITE_DEV_SERVER_URL
    ? new URL(MAIN_WINDOW_VITE_DEV_SERVER_URL)
    : pathToFileURL(path.join(__dirname, `../renderer/${MAIN_WINDOW_VITE_NAME}/index.html`));
}

// Parse command line arguments
const parseArgs = () => {
  let dirPath = null;

  // Remove first two elements in dev mode (electron and script path)
  const args = !dirPath && app.isPackaged ? process.argv : process.argv.slice(2);
  for (let i = 0; i < args.length; i++) {
    if (args[i] === '--dir' && i + 1 < args.length) {
      dirPath = args[i + 1];
      break;
    }
  }

  return { dirPath };
};

interface BundledConfig {
  defaultProvider?: string;
  defaultModel?: string;
  predefinedModels?: string;
  version?: string;
}

const getBundledConfig = (): BundledConfig => {
  //{env-macro-start}//
  //needed when goose is bundled for a specific provider
  //{env-macro-end}//
  return {
    defaultProvider: process.env.GOOSE_DEFAULT_PROVIDER,
    defaultModel: process.env.GOOSE_DEFAULT_MODEL,
    predefinedModels: process.env.GOOSE_PREDEFINED_MODELS,
    version: process.env.GOOSE_VERSION,
  };
};

const { defaultProvider, defaultModel, predefinedModels, version } = getBundledConfig();

const resolveGoosePathRoot = (): string | undefined => {
  const pathRoot = process.env.GOOSE_PATH_ROOT?.trim();
  if (pathRoot) {
    return expandTilde(pathRoot);
  }
  return undefined;
};

const GENERATED_SECRET = crypto.randomBytes(32).toString('hex');

interface ExternalBackend {
  source: 'env' | 'settings';
  url: string;
  secret: string;
  certFingerprint?: string;
}

const getExternalBackendUrlFromEnv = (): string | null => {
  if (!process.env.GOOSE_EXTERNAL_BACKEND) {
    return null;
  }

  const configuredUrl = process.env.GOOSE_EXTERNAL_BACKEND_URL?.trim();
  if (configuredUrl) {
    return configuredUrl;
  }

  return `http://127.0.0.1:${process.env.GOOSE_PORT || '3000'}`;
};

const getExternalBackendFromEnv = (): ExternalBackend | null => {
  const url = getExternalBackendUrlFromEnv();
  if (!url) {
    return null;
  }

  const secret = process.env.GOOSE_SERVER__SECRET_KEY;
  if (!secret) {
    throw new Error(
      'GOOSE_SERVER__SECRET_KEY must be set when using GOOSE_EXTERNAL_BACKEND. ' +
        'Set it to the same value on both the server and the desktop client.'
    );
  }

  return {
    source: 'env',
    url,
    secret,
  };
};

const getServerSecret = (settings: Settings): string => {
  if (settings.externalGoosed?.enabled && settings.externalGoosed.secret) {
    return settings.externalGoosed.secret;
  }
  return GENERATED_SECRET;
};

const getActiveExternalBackend = (settings: Settings): ExternalBackend | null => {
  const envBackend = getExternalBackendFromEnv();
  if (envBackend) {
    return envBackend;
  }

  if (settings.externalGoosed?.enabled && settings.externalGoosed.url) {
    return {
      source: 'settings',
      url: settings.externalGoosed.url,
      secret: getServerSecret(settings),
      certFingerprint: settings.externalGoosed.certFingerprint,
    };
  }

  return null;
};

const getExternalBackendForCsp = (settings: Settings) => {
  const envUrl = getExternalBackendUrlFromEnv();
  if (!envUrl) {
    return settings.externalGoosed;
  }

  return {
    ...settings.externalGoosed,
    enabled: true,
    url: envUrl,
  };
};

let appConfig = {
  GOOSE_DEFAULT_PROVIDER: defaultProvider,
  GOOSE_DEFAULT_MODEL: defaultModel,
  GOOSE_PREDEFINED_MODELS: predefinedModels,
  GOOSE_PATH_ROOT: resolveGoosePathRoot(),
  GOOSE_WORKING_DIR: '',
  // Start with the env-var override; the OS region locale is filled in after app.ready
  // (see updateLocaleFromSystem below) since getSystemLocale() cannot be called earlier.
  GOOSE_LOCALE: process.env.GOOSE_LOCALE || undefined,
  // If GOOSE_ALLOWLIST_WARNING env var is not set, defaults to false (strict blocking mode)
  GOOSE_ALLOWLIST_WARNING: process.env.GOOSE_ALLOWLIST_WARNING === 'true',
  GOOSE_DISABLE_NOSTR_SHARING: process.env.GOOSE_DISABLE_NOSTR_SHARING === 'true',
};

const windowMap = new Map<number, BrowserWindow>();
const appWindows = new Map<string, BrowserWindow>();

const gooseServeLeases = new GooseServeLeaseRegistry(log);

const windowPowerSaveBlockers = new Map<number, number>(); // windowId -> blockerId
// Track pending initial messages per window
const pendingInitialMessages = new Map<number, string>(); // windowId -> initialMessage
const pendingInitialMessageNoAutoSubmit = new Set<number>(); // windowIds whose initialMessage should NOT auto-submit

interface CreateChatOptions {
  initialMessage?: string;
  initialMessageNoAutoSubmit?: boolean;
  dir?: string;
  resumeSessionId?: string;
  viewType?: string;
  recipeDeeplink?: string;
  recipeId?: string;
  scheduledJobId?: string;
  recipeParameters?: Record<string, string>;
}

const createChat = async (
  app: App,
  options: CreateChatOptions = {}
): Promise<BrowserWindow | undefined> => {
  const {
    initialMessage,
    initialMessageNoAutoSubmit,
    dir,
    resumeSessionId,
    viewType,
    recipeDeeplink,
    recipeId,
    scheduledJobId,
    recipeParameters,
  } = options;
  const settings = getSettings();

  let externalBackend: ExternalBackend | null;
  try {
    externalBackend = getActiveExternalBackend(settings);
  } catch (error) {
    dialog.showMessageBoxSync({
      type: 'error',
      title: 'External Backend Misconfigured',
      message: 'The external backend environment is invalid.',
      detail: errorMessage(error),
      buttons: ['Quit'],
    });
    app.quit();
    return;
  }

  if (externalBackend?.certFingerprint) {
    const url = externalBackend.url;
    const usesHttps = (() => {
      try {
        return new URL(url).protocol === 'https:';
      } catch {
        return false;
      }
    })();

    if (!usesHttps) {
      const response = dialog.showMessageBoxSync({
        type: 'error',
        title: 'External Backend Misconfigured',
        message: 'Certificate fingerprint requires an HTTPS external backend URL.',
        detail: 'Use an https:// URL or remove the configured certificate fingerprint.',
        buttons: ['Disable External Backend & Retry', 'Quit'],
        defaultId: 0,
        cancelId: 1,
      });

      if (response === 0) {
        updateSettings((s) => {
          if (s.externalGoosed) {
            s.externalGoosed.enabled = false;
          }
        });
        return createChat(app, options);
      }

      app.quit();
      return;
    }
  }

  const serverSecret = externalBackend ? externalBackend.secret : GENERATED_SECRET;
  let workingDir = dir || os.homedir();
  let gooseServeLease: GooseServeLease | null = null;

  if (externalBackend) {
    let externalCertificateTrust: BackendCertificateTrustRegistration | null = null;

    try {
      const externalBaseUrl = normalizeAcpHttpBaseUrl(externalBackend.url);
      const externalBase = new URL(externalBaseUrl);
      if (externalBase.protocol === 'https:') {
        externalCertificateTrust = trustBackendCertificate(
          externalBase.hostname,
          externalBackend.certFingerprint ?? null
        );
      }

      const externalBackendReady = await checkBackendStatus({
        baseUrl: externalBaseUrl,
        serverSecret,
        fetch: net.fetch as unknown as typeof globalThis.fetch,
      });
      if (!externalBackendReady) {
        externalCertificateTrust?.release();
        const canDisableExternalBackend = externalBackend.source === 'settings';
        const response = dialog.showMessageBoxSync({
          type: 'error',
          title: 'External Backend Unreachable',
          message: `Could not connect to external backend at ${externalBaseUrl}`,
          detail:
            'The external backend must be running and the configured secret must match GOOSE_SERVER__SECRET_KEY on the server.',
          buttons: canDisableExternalBackend
            ? ['Disable External Backend & Retry', 'Quit']
            : ['Quit'],
          defaultId: 0,
          cancelId: canDisableExternalBackend ? 1 : 0,
        });

        if (canDisableExternalBackend && response === 0) {
          updateSettings((s) => {
            if (s.externalGoosed) {
              s.externalGoosed.enabled = false;
            }
          });
          return createChat(app, options);
        }

        app.quit();
        return;
      }

      const leaseCertificateTrust = externalCertificateTrust;
      externalCertificateTrust = null;
      gooseServeLease = gooseServeLeases.createExternal(
        acpWebSocketUrlFromHttpBase(externalBaseUrl, serverSecret),
        serverSecret,
        leaseCertificateTrust ? async () => leaseCertificateTrust.release() : undefined
      );
    } catch (error) {
      externalCertificateTrust?.release();
      log.error('External ACP backend is misconfigured', error);
      const canDisableExternalBackend = externalBackend.source === 'settings';
      const response = dialog.showMessageBoxSync({
        type: 'error',
        title: 'External Backend Misconfigured',
        message: 'The external backend URL is invalid.',
        detail: errorMessage(error),
        buttons: canDisableExternalBackend
          ? ['Disable External Backend & Retry', 'Quit']
          : ['Quit'],
        defaultId: 0,
        cancelId: canDisableExternalBackend ? 1 : 0,
      });

      if (canDisableExternalBackend && response === 0) {
        updateSettings((s) => {
          if (s.externalGoosed) {
            s.externalGoosed.enabled = false;
          }
        });
        return createChat(app, options);
      }

      app.quit();
      return;
    }
  } else {
    const localCertificateTrust = trustBackendCertificate('127.0.0.1', null);

    let gooseServeResult: Awaited<ReturnType<typeof startGooseServe>>;
    try {
      gooseServeResult = await startGooseServe({
        serverSecret,
        dir: workingDir,
        tls: true,
        env: {
          GOOSE_PATH_ROOT: appConfig.GOOSE_PATH_ROOT as string | undefined,
        },
        isPackaged: app.isPackaged,
        resourcesPath: app.isPackaged ? process.resourcesPath : undefined,
        logger: log,
        diagnosticsDir: STARTUP_LOGS_DIR,
        readinessFetch: net.fetch as unknown as typeof globalThis.fetch,
      });
      if (!gooseServeResult.certFingerprint) {
        await gooseServeResult.cleanup();
        throw new Error(
          'goose serve started with TLS but did not return a certificate fingerprint'
        );
      }

      const localCertFingerprint = normalizeFingerprint(gooseServeResult.certFingerprint);
      if (
        localCertificateTrust.trust.fingerprint &&
        localCertificateTrust.trust.fingerprint !== localCertFingerprint
      ) {
        await gooseServeResult.cleanup();
        throw new Error('goose serve TLS certificate fingerprint did not match readiness probe');
      }
      localCertificateTrust.trust.fingerprint = localCertFingerprint;
    } catch (error) {
      localCertificateTrust.release();
      log.error('goose serve failed to start', error);
      dialog.showMessageBoxSync({
        type: 'error',
        title: 'Goose Failed to Start',
        message: 'The backend server failed to start.',
        detail: [
          'Backend: goose serve',
          'Readiness check: HTTPS GET /status',
          `Startup error:\n${errorMessage(error)}`,
        ].join('\n\n'),
        buttons: ['OK'],
      });
      app.quit();
      return;
    }

    workingDir = gooseServeResult.workingDir;
    const cleanupGooseServe = gooseServeResult.cleanup;
    gooseServeResult.cleanup = async () => {
      try {
        await cleanupGooseServe();
      } finally {
        localCertificateTrust.release();
      }
    };
    gooseServeLease = gooseServeLeases.create(gooseServeResult, serverSecret);
  }

  const cleanupUnregisteredGooseServeLease = async () => {
    if (!gooseServeLease) {
      return;
    }

    const lease = gooseServeLease;
    gooseServeLease = null;
    await gooseServeLeases.cleanupLease(lease);
  };

  let mainWindowState: ReturnType<typeof windowStateKeeper>;
  let mainWindow: BrowserWindow;
  try {
    mainWindowState = windowStateKeeper({
      defaultWidth: 940,
      defaultHeight: 800,
    });

    mainWindow = new BrowserWindow({
      show: false,
      titleBarStyle: process.platform === 'darwin' ? 'hidden' : 'default',
      trafficLightPosition: process.platform === 'darwin' ? { x: 20, y: 16 } : undefined,
      vibrancy: process.platform === 'darwin' ? 'window' : undefined,
      frame: process.platform !== 'darwin',
      // windowStateKeeper persists the outer window bounds (getBounds), so the
      // window must be restored by outer bounds too. With useContentSize the saved
      // outer height is reapplied as the content height, growing the window by the
      // frame height on every launch on framed platforms (#9363).
      x: mainWindowState.x,
      y: mainWindowState.y,
      width: mainWindowState.width,
      height: mainWindowState.height,
      minWidth: 480,
      minHeight: 400,
      resizable: true,
      icon: path.join(__dirname, '../images/icon.icns'),
      webPreferences: {
        spellcheck: settings.spellcheckEnabled ?? true,
        preload: path.join(__dirname, 'preload.js'),
        webSecurity: true,
        nodeIntegration: false,
        contextIsolation: true,
        additionalArguments: [
          JSON.stringify({
            ...appConfig,
            GOOSE_LOCALE: getConfiguredGooseLocale(),
            GOOSE_WORKING_DIR: workingDir,
            REQUEST_DIR: dir,
            GOOSE_VERSION: version,
            recipeDeeplink: recipeDeeplink,
            recipeId: recipeId,
            recipeParameters: recipeParameters,
            scheduledJobId: scheduledJobId,
            SECURITY_ML_MODEL_MAPPING: process.env.SECURITY_ML_MODEL_MAPPING,
            SECURITY_PROMPT_ENABLED_OVERRIDE: process.env.SECURITY_PROMPT_ENABLED_OVERRIDE,
            SECURITY_COMMAND_CLASSIFIER_ENABLED_OVERRIDE:
              process.env.SECURITY_COMMAND_CLASSIFIER_ENABLED_OVERRIDE,
          }),
        ],
        partition: 'persist:goose',
      },
    });
  } catch (error) {
    await cleanupUnregisteredGooseServeLease();
    throw error;
  }

  if (gooseServeLease) {
    const lease = gooseServeLease;
    mainWindow.once('closed', () => {
      void gooseServeLeases.releaseWindow(mainWindow.id);
    });
    gooseServeLeases.attachWindow(mainWindow.id, lease);
    gooseServeLease = null;
  }

  if (!app.isPackaged) {
    installExtension(REACT_DEVELOPER_TOOLS, {
      loadExtensionOptions: { allowFileAccess: true },
      session: mainWindow.webContents.session,
    })
      .then(() => log.info('added react dev tools'))
      .catch((err) => log.info('failed to install react dev tools:', err));
  }

  // Let windowStateKeeper manage the window
  mainWindowState.manage(mainWindow);

  mainWindow.webContents.session.setSpellCheckerLanguages(['en-US', 'en-GB']);
  mainWindow.webContents.on('context-menu', (_event, params) => {
    const menu = new Menu();
    const hasSpellingSuggestions = params.dictionarySuggestions.length > 0 || params.misspelledWord;

    if (hasSpellingSuggestions) {
      for (const suggestion of params.dictionarySuggestions) {
        menu.append(
          new MenuItem({
            label: suggestion,
            click: () => mainWindow.webContents.replaceMisspelling(suggestion),
          })
        );
      }

      if (params.misspelledWord) {
        menu.append(
          new MenuItem({
            label: menuT('Add to dictionary'),
            click: () =>
              mainWindow.webContents.session.addWordToSpellCheckerDictionary(params.misspelledWord),
          })
        );
      }

      if (params.selectionText) {
        menu.append(new MenuItem({ type: 'separator' }));
      }
    }
    if (params.selectionText) {
      menu.append(
        new MenuItem({
          label: menuT('Cut'),
          accelerator: 'CmdOrCtrl+X',
          role: 'cut',
        })
      );
      menu.append(
        new MenuItem({
          label: menuT('Copy'),
          accelerator: 'CmdOrCtrl+C',
          role: 'copy',
        })
      );
    }

    // Only show paste in editable fields (text inputs)
    if (params.isEditable) {
      menu.append(
        new MenuItem({
          label: menuT('Paste'),
          accelerator: 'CmdOrCtrl+V',
          role: 'paste',
        })
      );
    }

    if (menu.items.length > 0) {
      menu.popup();
    }
  });

  // Handle new window creation for links (fallback for any links not handled by onClick)
  mainWindow.webContents.setWindowOpenHandler(({ url }) => {
    try {
      const protocol = new URL(url).protocol;
      if (BLOCKED_PROTOCOLS.includes(protocol)) {
        return { action: 'deny' };
      }
    } catch {
      return { action: 'deny' };
    }

    shell.openExternal(url);
    return { action: 'deny' };
  });

  // Handle new-window events (alternative approach for external links)
  // Use type assertion for non-standard Electron event
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  mainWindow.webContents.on('new-window' as any, function (event: any, url: string) {
    event.preventDefault();
    try {
      const protocol = new URL(url).protocol;
      if (BLOCKED_PROTOCOLS.includes(protocol)) {
        return;
      }
    } catch {
      return;
    }
    shell.openExternal(url);
  });

  const windowId = mainWindow.id;
  const url = getAppUrl();

  let appPath = '/';
  const routeMap: Record<string, string> = {
    chat: '/',
    pair: '/pair',
    settings: '/settings',
    sessions: '/sessions',
    schedules: '/schedules',
    recipes: '/recipes',
    skills: '/skills',
    permission: '/permission',
    ConfigureProviders: '/configure-providers',
  };

  if (viewType) {
    appPath = routeMap[viewType] || '/';
  }
  if (
    appPath === '/' &&
    (recipeDeeplink !== undefined || recipeId !== undefined || initialMessage)
  ) {
    appPath = '/pair';
  }

  let searchParams = new URLSearchParams();
  if (resumeSessionId) {
    searchParams.set('resumeSessionId', resumeSessionId);
    if (appPath === '/') {
      appPath = '/pair';
    }
  }

  // Goose's react app uses HashRouter, so the path + search params follow a #/
  url.hash = `${appPath}?${searchParams.toString()}`;
  let formattedUrl = formatUrl(url);
  log.info('Opening URL: ', formattedUrl);
  mainWindow.once('ready-to-show', () => {
    if (!mainWindow.isDestroyed()) {
      mainWindow.show();
    }
  });
  mainWindow.loadURL(formattedUrl);

  // If we have an initial message, store it to send after React is ready
  if (initialMessage) {
    pendingInitialMessages.set(mainWindow.id, initialMessage);
    if (initialMessageNoAutoSubmit) {
      pendingInitialMessageNoAutoSubmit.add(mainWindow.id);
    }
  }

  // Set up local keyboard shortcuts that only work when the window is focused
  mainWindow.webContents.on('before-input-event', (event, input) => {
    if (input.key === 'r' && input.meta) {
      mainWindow.reload();
      event.preventDefault();
    }

    if (input.key === 'i' && input.alt && input.meta) {
      mainWindow.webContents.openDevTools();
      event.preventDefault();
    }
  });

  mainWindow.on('app-command', (e, cmd) => {
    if (cmd === 'browser-backward') {
      mainWindow.webContents.send('mouse-back-button-clicked');
      e.preventDefault();
    }
  });

  const broadcastFullScreenState = () => {
    if (!mainWindow.isDestroyed()) {
      mainWindow.webContents.send('fullscreen-change', mainWindow.isFullScreen());
    }
  };
  mainWindow.on('enter-full-screen', broadcastFullScreenState);
  mainWindow.on('leave-full-screen', broadcastFullScreenState);

  // Handle mouse back button (button 3)
  // Use type assertion for non-standard Electron event
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  mainWindow.webContents.on('mouse-up' as any, function (_event: any, mouseButton: number) {
    // MouseButton 3 is the back button.
    if (mouseButton === 3) {
      mainWindow.webContents.send('mouse-back-button-clicked');
    }
  });

  windowMap.set(windowId, mainWindow);

  // Handle window closure
  mainWindow.on('closed', () => {
    windowMap.delete(windowId);

    pendingInitialMessages.delete(windowId);
    pendingDeepLinks.delete(windowId);
    reactReadyWindows.delete(windowId);

    if (windowPowerSaveBlockers.has(windowId)) {
      const blockerId = windowPowerSaveBlockers.get(windowId)!;
      try {
        powerSaveBlocker.stop(blockerId);
        console.log(
          `[Main] Stopped power save blocker ${blockerId} for closing window ${windowId}`
        );
      } catch (error) {
        console.error(
          `[Main] Failed to stop power save blocker ${blockerId} for window ${windowId}:`,
          error
        );
      }
      windowPowerSaveBlockers.delete(windowId);
    }
  });
  return mainWindow;
};

let activeLauncherWindow: BrowserWindow | null = null;

const createLauncher = () => {
  if (activeLauncherWindow && !activeLauncherWindow.isDestroyed()) {
    activeLauncherWindow.focus();
    return activeLauncherWindow;
  }

  const launcherWindow = new BrowserWindow({
    width: 600,
    height: 80,
    frame: false,
    transparent: process.platform === 'darwin',
    backgroundColor: process.platform === 'darwin' ? '#00000000' : '#ffffff',
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      nodeIntegration: false,
      contextIsolation: true,
      additionalArguments: [
        JSON.stringify({
          ...appConfig,
          GOOSE_LOCALE: getConfiguredGooseLocale(),
        }),
      ],
      partition: 'persist:goose',
    },
    skipTaskbar: true,
    alwaysOnTop: true,
    resizable: false,
    movable: true,
    minimizable: false,
    maximizable: false,
    fullscreenable: false,
    hasShadow: true,
    vibrancy: process.platform === 'darwin' ? 'window' : undefined,
  });

  // Center on screen
  const primaryDisplay = screen.getPrimaryDisplay();
  const { width, height } = primaryDisplay.workAreaSize;
  const windowBounds = launcherWindow.getBounds();

  launcherWindow.setPosition(
    Math.round(width / 2 - windowBounds.width / 2),
    Math.round(height / 3 - windowBounds.height / 2)
  );

  // Load launcher window content
  const url = getAppUrl();

  url.hash = '/launcher';
  launcherWindow.loadURL(formatUrl(url));
  activeLauncherWindow = launcherWindow;

  launcherWindow.on('closed', () => {
    reactReadyWindows.delete(launcherWindow.id);
    activeLauncherWindow = null;
  });

  // Destroy window when it loses focus
  launcherWindow.on('blur', () => {
    launcherWindow.destroy();
  });

  // Also destroy on escape key
  launcherWindow.webContents.on('before-input-event', (event, input) => {
    if (input.key === 'Escape') {
      launcherWindow.destroy();
      event.preventDefault();
    }
  });

  return launcherWindow;
};

// Track tray instance
let tray: Tray | null = null;

const destroyTray = () => {
  if (tray) {
    tray.destroy();
    tray = null;
  }
};

const disableTray = () => {
  updateSettings((s) => {
    s.showMenuBarIcon = false;
  });
};

const createTray = () => {
  destroyTray();

  const possiblePaths = [
    path.join(process.resourcesPath, 'images', 'iconTemplate.png'),
    path.join(process.cwd(), 'src', 'images', 'iconTemplate.png'),
    path.join(__dirname, '..', 'images', 'iconTemplate.png'),
    path.join(__dirname, 'images', 'iconTemplate.png'),
    path.join(process.cwd(), 'images', 'iconTemplate.png'),
  ];

  const iconPath = possiblePaths.find((p) => fsSync.existsSync(p));

  if (!iconPath) {
    console.warn('[Main] Tray icon not found. App will continue without system tray.');
    disableTray();
    return;
  }

  try {
    tray = new Tray(iconPath);
    setTrayRef(tray);
    updateTrayMenu(getUpdateAvailable());

    if (process.platform === 'win32') {
      tray.on('click', showWindow);
    }
  } catch (error) {
    console.error('[Main] Tray creation failed. App will continue without system tray.', error);
    disableTray();
    tray = null;
  }
};

const showWindow = async () => {
  const windows = BrowserWindow.getAllWindows();

  if (windows.length === 0) {
    log.info('No windows are open, creating a new one...');
    const recentDirs = loadRecentDirs();
    const openDir = recentDirs.length > 0 ? recentDirs[0] : null;
    await createChat(app, { dir: openDir || undefined });
    return;
  }

  const initialOffsetX = 30;
  const initialOffsetY = 30;

  // Iterate over all windows
  windows.forEach((win, index) => {
    const currentBounds = win.getBounds();
    const newX = currentBounds.x + initialOffsetX * index;
    const newY = currentBounds.y + initialOffsetY * index;

    win.setBounds({
      x: newX,
      y: newY,
      width: currentBounds.width,
      height: currentBounds.height,
    });

    if (!win.isVisible()) {
      win.show();
    }

    win.focus();
  });
};

const buildRecentFilesMenu = () => {
  const recentDirs = loadRecentDirs();
  return recentDirs.map((dir) => ({
    label: dir,
    click: async () => {
      await createChat(app, { dir });
    },
  }));
};

const openDirectoryDialog = async (): Promise<OpenDialogReturnValue> => {
  // Get the current working directory from the focused window
  let defaultPath: string | undefined;
  const currentWindow = BrowserWindow.getFocusedWindow();

  if (currentWindow) {
    try {
      const currentWorkingDir = await currentWindow.webContents.executeJavaScript(
        `window.appConfig ? window.appConfig.get('GOOSE_WORKING_DIR') : null`
      );

      if (currentWorkingDir && typeof currentWorkingDir === 'string') {
        // Verify the directory exists before using it as default
        try {
          const stats = fsSync.lstatSync(currentWorkingDir);
          if (stats.isDirectory()) {
            defaultPath = currentWorkingDir;
          }
        } catch (error) {
          if (error && typeof error === 'object' && 'code' in error) {
            const fsError = error as { code?: string; message?: string };
            if (
              fsError.code === 'ENOENT' ||
              fsError.code === 'EACCES' ||
              fsError.code === 'EPERM'
            ) {
              console.warn(
                `Current working directory not accessible (${fsError.code}): ${currentWorkingDir}, falling back to home directory`
              );
              defaultPath = os.homedir();
            } else {
              console.warn(
                `Unexpected filesystem error (${fsError.code}) for directory ${currentWorkingDir}:`,
                fsError.message
              );
              defaultPath = os.homedir();
            }
          } else {
            console.warn(`Unexpected error checking directory ${currentWorkingDir}:`, error);
            defaultPath = os.homedir();
          }
        }
      }
    } catch (error) {
      console.warn('Failed to get current working directory from window:', error);
    }
  }

  if (!defaultPath) {
    defaultPath = os.homedir();
  }

  const result = (await dialog.showOpenDialog({
    properties: ['openFile', 'openDirectory', 'createDirectory'],
    defaultPath: defaultPath,
  })) as unknown as OpenDialogReturnValue;

  if (!result.canceled && result.filePaths.length > 0) {
    const selectedPath = result.filePaths[0];

    // If a file was selected, use its parent directory
    let dirToAdd = selectedPath;
    try {
      const stats = fsSync.lstatSync(selectedPath);

      // Reject symlinks for security
      if (stats.isSymbolicLink()) {
        console.warn(`Selected path is a symlink, using parent directory for security`);
        dirToAdd = path.dirname(selectedPath);
      } else if (stats.isFile()) {
        dirToAdd = path.dirname(selectedPath);
      }
    } catch {
      console.warn(`Could not stat selected path, using parent directory`);
      dirToAdd = path.dirname(selectedPath); // Fallback to parent directory
    }

    addRecentDir(dirToAdd);

    let deeplinkData: RecipeDeeplinkData | undefined = undefined;
    if (windowDeeplinkURL) {
      deeplinkData = parseRecipeDeeplink(windowDeeplinkURL);
    }
    await createChat(app, {
      dir: dirToAdd,
      recipeDeeplink: deeplinkData?.config,
      recipeParameters: deeplinkData?.parameters,
    });
  }
  return result;
};

interface RecipeDeeplinkData {
  config: string;
  parameters?: Record<string, string>;
}

function parseRecipeDeeplink(url: string): RecipeDeeplinkData | undefined {
  const parsedUrl = new URL(url);
  let recipeDeeplink = parsedUrl.searchParams.get('config');
  if (recipeDeeplink && !url.includes(recipeDeeplink)) {
    // URLSearchParams decodes + as space, which can break encoded configs
    // Parse raw query to preserve "+" characters in values like config
    const search = parsedUrl.search || '';
    const configMatch = search.match(/(?:[?&])config=([^&]*)/);
    let recipeDeeplinkTmp = configMatch ? configMatch[1] : null;
    if (recipeDeeplinkTmp) {
      try {
        recipeDeeplink = decodeURIComponent(recipeDeeplinkTmp);
      } catch (error) {
        console.error('[Main] parseRecipeDeeplink - Failed to decode:', errorMessage(error));
        return undefined;
      }
    }
  }
  if (!recipeDeeplink) {
    return undefined;
  }

  // Extract all query parameters except 'config' and 'scheduledJob' as recipe parameters
  // Use raw query string parsing to preserve '+' characters (consistent with config handling)
  const parameters: Record<string, string> = {};
  const search = parsedUrl.search || '';
  const paramMatches = search.matchAll(/[?&]([^=&]+)=([^&]*)/g);

  for (const match of paramMatches) {
    const key = match[1];
    const rawValue = match[2];

    if (key !== 'config' && key !== 'scheduledJob') {
      try {
        parameters[key] = decodeURIComponent(rawValue);
      } catch {
        // If decoding fails, use raw value
        parameters[key] = rawValue;
      }
    }
  }

  return {
    config: recipeDeeplink,
    parameters: Object.keys(parameters).length > 0 ? parameters : undefined,
  };
}

// Global error handler
const handleFatalError = (error: Error) => {
  const windows = BrowserWindow.getAllWindows();
  windows.forEach((win) => {
    win.webContents.send('fatal-error', error.message || 'An unexpected error occurred');
  });
};

process.on('uncaughtException', (error) => {
  console.error('Uncaught Exception:', formatErrorForLogging(error));
  handleFatalError(error);
});

process.on('unhandledRejection', (error) => {
  console.error('Unhandled Rejection:', formatErrorForLogging(error));
  handleFatalError(error instanceof Error ? error : new Error(String(error)));
});

ipcMain.on('react-ready', (event) => {
  log.info('React ready event received');

  // Get the window that sent the react-ready event
  const window = BrowserWindow.fromWebContents(event.sender);
  const windowId = window?.id;

  if (windowId !== undefined) {
    reactReadyWindows.add(windowId);
  }

  // Send any pending initial message for this window
  if (windowId && pendingInitialMessages.has(windowId)) {
    const initialMessage = pendingInitialMessages.get(windowId)!;
    const noAutoSubmit = pendingInitialMessageNoAutoSubmit.has(windowId);
    log.info('Sending pending initial message to window:', initialMessage);
    window.webContents.send('set-initial-message', initialMessage, { noAutoSubmit });
    pendingInitialMessages.delete(windowId);
    pendingInitialMessageNoAutoSubmit.delete(windowId);
  }

  if (windowId && pendingDeepLinks.has(windowId) && window) {
    const deepLinkUrl = pendingDeepLinks.get(windowId)!;
    pendingDeepLinks.delete(windowId);
    log.info('Processing pending deep link for window:', windowId);
    try {
      const parsedUrl = new URL(deepLinkUrl);
      if (parsedUrl.hostname === 'extension') {
        window.webContents.send('add-extension', deepLinkUrl);
      } else if (parsedUrl.hostname === 'sessions') {
        sendOpenSharedSession(window, deepLinkUrl);
      }
    } catch (error) {
      log.error('Error processing pending deep link:', error);
    }
  }
});

ipcMain.handle('open-external', async (_event, url: string) => {
  const parsedUrl = new URL(url);

  if (BLOCKED_PROTOCOLS.includes(parsedUrl.protocol)) {
    console.warn(`[Main] Blocked dangerous protocol: ${parsedUrl.protocol}`);
    return;
  }

  await shell.openExternal(url);
});

ipcMain.handle('directory-chooser', async () => {
  return dialog.showOpenDialog({
    properties: ['openDirectory', 'createDirectory'],
    defaultPath: os.homedir(),
  });
});

ipcMain.handle('add-recent-dir', (_event, dir: string) => {
  if (dir) {
    addRecentDir(dir);
  }
});

ipcMain.handle('list-recent-dirs', () => {
  return loadRecentDirs();
});

ipcMain.handle('list-git-worktree-dirs', async (_event, dir: string) => {
  return await listGitWorktreeDirs(dir);
});

// LM Studio's OWN live per-node state — the ground truth for "is this node generating RIGHT NOW", which the
// REST /api/v0/models does NOT expose (it only reports loaded/not-loaded). `lms ps --json` carries the real
// status per model. Returned as { identifier: 'generating' | 'processingPrompt' | 'idle' }. Empty on any error
// (lms missing, no server, parse fail) so the caller degrades to the digest-only view. The renderer can't shell
// this itself; it goes through main.
ipcMain.handle('fleet-status', async (): Promise<Record<string, string>> => {
  return await new Promise<Record<string, string>>((resolve) => {
    const home = process.env.HOME || os.homedir();
    const lmsHome = `${home}/.lmstudio/bin/lms`;
    const bin = fsSync.existsSync(lmsHome) ? lmsHome : 'lms';
    execFile(bin, ['ps', '--json'], { timeout: 4000 }, (error, stdout) => {
      if (error) {
        resolve({});
        return;
      }
      try {
        const arr = JSON.parse(stdout) as Array<{ identifier?: string; status?: string }>;
        const map: Record<string, string> = {};
        for (const m of arr) {
          if (m.identifier && m.status) map[m.identifier] = m.status;
        }
        resolve(map);
      } catch {
        resolve({});
      }
    });
  });
});

ipcMain.handle('get-setting', (_event, key: SettingKey) => {
  const settings = getSettings();
  return settings[key];
});

// Valid setting keys for runtime validation
const validSettingKeys: Set<string> = new Set([
  'showMenuBarIcon',
  'showDockIcon',
  'enableWakelock',
  'enableNotifications',
  'spellcheckEnabled',
  'externalGoosed',
  'globalShortcut',
  'keyboardShortcuts',
  'theme',
  'useSystemTheme',
  'language',
  'responseStyle',
  'showPricing',
  'seenAnnouncementIds',
  'disableAutoDownload',
  'edition',
]);

ipcMain.handle('set-setting', (_event, key: SettingKey, value: unknown) => {
  // Validate key at runtime to prevent prototype pollution
  if (!validSettingKeys.has(key)) {
    console.error(`Invalid setting key rejected: ${key}`);
    return;
  }

  if (key === 'language' && !isValidLanguageSetting(value)) {
    console.error(`Invalid language setting rejected: ${String(value)}`);
    return;
  }

  const settings = getSettings();
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  (settings as any)[key] = value;
  fsSync.writeFileSync(SETTINGS_FILE, JSON.stringify(settings, null, 2));

  if (key === 'language') {
    appConfig.GOOSE_LOCALE = getConfiguredGooseLocale();
  }

  // Re-register shortcuts if keyboard shortcuts changed
  if (key === 'keyboardShortcuts') {
    registerGlobalShortcuts();
  }

  if (key === 'disableAutoDownload') {
    setAutoDownloadDisabled(value as boolean);
  }
});

ipcMain.handle('get-secret-key', (event) => {
  const windowId = BrowserWindow.fromWebContents(event.sender)?.id;
  if (!windowId) {
    return null;
  }
  return gooseServeLeases.getSecretKey(windowId) ?? null;
});

ipcMain.handle('get-acp-url', async (event) => {
  const windowId = BrowserWindow.fromWebContents(event.sender)?.id;
  if (!windowId) {
    return null;
  }
  return gooseServeLeases.getAcpUrl(windowId) ?? null;
});

// Handle menu bar icon visibility
ipcMain.handle('set-menu-bar-icon', async (_event, show: boolean) => {
  updateSettings((s) => {
    s.showMenuBarIcon = show;
  });

  if (show) {
    createTray();
  } else {
    destroyTray();
  }
  return true;
});

ipcMain.handle('get-menu-bar-icon-state', () => {
  try {
    const settings = getSettings();
    return settings.showMenuBarIcon ?? true;
  } catch (error) {
    console.error('Error getting menu bar icon state:', error);
    return true;
  }
});

// Handle dock icon visibility (macOS only)
ipcMain.handle('set-dock-icon', async (_event, show: boolean) => {
  if (process.platform !== 'darwin') return false;

  const settings = getSettings();
  updateSettings((s) => {
    s.showDockIcon = show;
  });

  if (show) {
    app.dock?.show();
  } else {
    // Only hide the dock if we have a menu bar icon to maintain accessibility
    if (settings.showMenuBarIcon) {
      app.dock?.hide();
      setTimeout(() => {
        focusWindow();
      }, 50);
    }
  }
  return true;
});

ipcMain.handle('get-dock-icon-state', () => {
  try {
    if (process.platform !== 'darwin') return true;
    const settings = getSettings();
    return settings.showDockIcon ?? true;
  } catch (error) {
    console.error('Error getting dock icon state:', error);
    return true;
  }
});

// Handle opening system notifications preferences
ipcMain.handle('open-notifications-settings', async () => {
  try {
    if (process.platform === 'darwin') {
      spawn('open', ['x-apple.systempreferences:com.apple.preference.notifications']);
      return true;
    } else if (process.platform === 'win32') {
      // Windows: Open notification settings in Settings app
      spawn('ms-settings:notifications', { shell: true });
      return true;
    } else if (process.platform === 'linux') {
      // Linux: Try different desktop environments
      function canSpawn(cmd: string): boolean {
        try {
          execFileSync('which', [cmd], { stdio: 'ignore' });
          return true;
        } catch {
          return false;
        }
      }

      // GNOME
      if (canSpawn('gnome-control-center')) {
        spawn('gnome-control-center', ['notifications']);
        return true;
      }

      // KDE Plasma
      if (canSpawn('systemsettings5')) {
        spawn('systemsettings5', ['kcm_notifications']);
        return true;
      }

      // XFCE
      if (canSpawn('xfce4-settings-manager')) {
        spawn('xfce4-settings-manager', ['--socket-id=notifications']);
        return true;
      }

      console.warn('Could not find a suitable settings application for Linux');
      return false;
    } else {
      console.warn(
        `Opening notification settings is not supported on platform: ${process.platform}`
      );
      return false;
    }
  } catch (error) {
    console.error('Error opening notification settings:', error);
    return false;
  }
});

// Handle wakelock setting
ipcMain.handle('set-wakelock', async (_event, enable: boolean) => {
  updateSettings((s) => {
    s.enableWakelock = enable;
  });

  // Stop all existing power save blockers when disabling the setting
  if (!enable) {
    for (const [windowId, blockerId] of windowPowerSaveBlockers.entries()) {
      try {
        powerSaveBlocker.stop(blockerId);
        console.log(
          `[Main] Stopped power save blocker ${blockerId} for window ${windowId} due to wakelock setting disabled`
        );
      } catch (error) {
        console.error(
          `[Main] Failed to stop power save blocker ${blockerId} for window ${windowId}:`,
          error
        );
      }
    }
    windowPowerSaveBlockers.clear();
  }

  return true;
});

ipcMain.handle('get-wakelock-state', () => {
  try {
    const settings = getSettings();
    return settings.enableWakelock ?? false;
  } catch (error) {
    console.error('Error getting wakelock state:', error);
    return false;
  }
});

ipcMain.handle('set-spellcheck', async (_event, enable: boolean) => {
  updateSettings((s) => {
    s.spellcheckEnabled = enable;
  });
  return true;
});

ipcMain.handle('get-spellcheck-state', () => {
  try {
    const settings = getSettings();
    return settings.spellcheckEnabled ?? true;
  } catch (error) {
    console.error('Error getting spellcheck state:', error);
    return true;
  }
});

ipcMain.handle('is-any-window-focused', () => {
  return BrowserWindow.getFocusedWindow() !== null;
});

ipcMain.handle('get-is-fullscreen', (event) => {
  const win = BrowserWindow.fromWebContents(event.sender);
  return win?.isFullScreen() ?? false;
});

// Add file/directory selection handler
ipcMain.handle('select-file-or-directory', async (_event, defaultPath?: string) => {
  const dialogOptions: OpenDialogOptions = {
    properties: process.platform === 'darwin' ? ['openFile', 'openDirectory'] : ['openFile'],
  };

  // Set default path if provided
  if (defaultPath) {
    // Expand tilde to home directory
    const expandedPath = expandTilde(defaultPath);

    // Check if the path exists
    try {
      const stats = await fs.stat(expandedPath);
      if (stats.isDirectory()) {
        dialogOptions.defaultPath = expandedPath;
      } else {
        dialogOptions.defaultPath = path.dirname(expandedPath);
      }
      // eslint-disable-next-line @typescript-eslint/no-unused-vars
    } catch (error) {
      // If path doesn't exist, fall back to home directory and log error
      console.error(`Default path does not exist: ${expandedPath}, falling back to home directory`);
      dialogOptions.defaultPath = os.homedir();
    }
  }

  const result = (await dialog.showOpenDialog(dialogOptions)) as unknown as OpenDialogReturnValue;

  if (!result.canceled && result.filePaths.length > 0) {
    return result.filePaths[0];
  }
  return null;
});

// Native picker tailored for session imports: shows hidden files (so users can
// reach `~/.claude/projects/...` or `~/.pi/agent/sessions/...`), filters for
// .json/.jsonl, and returns the file's contents inline so the renderer doesn't
// need a separate read step.
ipcMain.handle('select-import-session-file', async () => {
  const result = (await dialog.showOpenDialog({
    title: 'Import session',
    defaultPath: os.homedir(),
    properties: ['openFile', 'showHiddenFiles'],
    filters: [
      { name: 'Session files', extensions: ['json', 'jsonl'] },
      { name: 'All files', extensions: ['*'] },
    ],
  })) as unknown as OpenDialogReturnValue;

  if (result.canceled || result.filePaths.length === 0) {
    return null;
  }
  const filePath = result.filePaths[0];
  try {
    const contents = await fs.readFile(filePath, 'utf8');
    return { filePath, contents };
  } catch (err) {
    return { filePath, contents: '', error: errorMessage(err) };
  }
});

// Goose Local Edition — read the latest swarm RUN for a working directory so the desktop can render the
// per-node turn loops live. A `goose swarm run` writes a structured event stream to
// <workingDir>/.swarm/run-<id>.jsonl and a per-turn digest per worker to <workingDir>/.swarm/activity/<task>.json.
// This returns the newest run's parsed events + the activity digests; the renderer folds them into node lanes.
ipcMain.handle('read-swarm-run', async (_event, workingDir: string) => {
  try {
    if (!workingDir || typeof workingDir !== 'string') return null;
    const swarmDir = path.join(expandTilde(workingDir), '.swarm');
    const entries = await fs.readdir(swarmDir).catch(() => [] as string[]);
    const runFiles = entries.filter((f) => f.startsWith('run-') && f.endsWith('.jsonl'));
    if (runFiles.length === 0) return null;

    const withMtime = await Promise.all(
      runFiles.map(async (f) => {
        const m = await fs
          .stat(path.join(swarmDir, f))
          .then((s) => s.mtimeMs)
          .catch(() => 0);
        return { f, m };
      })
    );
    withMtime.sort((a, b) => b.m - a.m);
    const runFile = withMtime[0].f;
    const mtime = withMtime[0].m;

    const raw = await fs.readFile(path.join(swarmDir, runFile), 'utf8').catch(() => '');
    const events = raw
      .split('\n')
      .filter((l) => l.trim().length > 0)
      .map((l) => {
        try {
          return JSON.parse(l) as Record<string, unknown>;
        } catch {
          return null; // tolerate a partial last line while the run is writing
        }
      })
      .filter((e): e is Record<string, unknown> => e !== null);

    const activityDir = path.join(swarmDir, 'activity');
    const actEntries = await fs.readdir(activityDir).catch(() => [] as string[]);
    const activity: Record<string, unknown> = {};
    // A worker rewrites its activity digest every turn but the run log only gets a line on task
    // events, so the freshest activity mtime is the real liveness signal — fold it into `mtime` so a
    // long-running task is not mistaken for stalled, and a killed run correctly goes stale.
    let freshest = mtime;
    await Promise.all(
      actEntries
        .filter((f) => f.endsWith('.json'))
        .map(async (f) => {
          try {
            const p = path.join(activityDir, f);
            const st = await fs.stat(p);
            if (st.mtimeMs > freshest) freshest = st.mtimeMs;
            const c = await fs.readFile(p, 'utf8');
            activity[f.replace(/\.json$/, '')] = JSON.parse(c);
          } catch {
            /* a digest mid-write — skip it this poll */
          }
        })
    );

    // Clarify handshake: when planner confidence is below the ask floor the swarm writes
    // clarify-questions.json and BLOCKS on clarify-answers.json. Surface it so the run panel can prompt the
    // user; `pending` = questions exist AND no answers written yet. answerPath is where the panel writes back.
    type ConfBreakdown = {
      final: number;
      agreement: number;
      agreementReason: string;
      specClarity: number;
      specClarityReason: string;
      productSpecified: boolean;
      openDecisions: string[];
    };
    let clarify: {
      pending: boolean;
      questions: Array<{ question: string; options: string[]; resolves?: string }>;
      planConfidence?: number;
      confidence?: ConfBreakdown | null;
      answerPath: string;
    } | null = null;
    try {
      const qraw = await fs.readFile(path.join(swarmDir, 'clarify-questions.json'), 'utf8');
      const q = JSON.parse(qraw) as {
        questions?: unknown;
        plan_confidence?: unknown;
        plan_confidence_breakdown?: unknown;
      };
      const questions = (Array.isArray(q.questions) ? q.questions : [])
        .map((it) => {
          if (typeof it === 'string') return { question: it, options: [] as string[] };
          const o = it as { question?: unknown; options?: unknown; resolves?: unknown };
          return {
            question: String(o.question ?? ''),
            options: Array.isArray(o.options) ? o.options.map(String) : [],
            resolves: typeof o.resolves === 'string' && o.resolves.length ? o.resolves : undefined,
          };
        })
        .filter((x) => x.question.length > 0);
      if (questions.length > 0) {
        const answered = await fs
          .stat(path.join(swarmDir, 'clarify-answers.json'))
          .then(() => true)
          .catch(() => false);
        // Parse the additive confidence breakdown (null on runs that predate it).
        let confidence: ConfBreakdown | null = null;
        const bd = q.plan_confidence_breakdown;
        if (bd && typeof bd === 'object') {
          const b = bd as Record<string, unknown>;
          const agreement = typeof b.agreement === 'number' ? b.agreement : null;
          const specClarity = typeof b.spec_clarity === 'number' ? b.spec_clarity : null;
          const final =
            typeof b.final === 'number'
              ? b.final
              : agreement != null && specClarity != null
                ? Math.min(agreement, specClarity)
                : null;
          if (final != null) {
            confidence = {
              final,
              agreement: agreement ?? final,
              agreementReason: typeof b.agreement_reason === 'string' ? b.agreement_reason : '',
              specClarity: specClarity ?? final,
              specClarityReason:
                typeof b.spec_clarity_reason === 'string' ? b.spec_clarity_reason : '',
              productSpecified: b.product_specified === true,
              openDecisions: Array.isArray(b.open_decisions) ? b.open_decisions.map(String) : [],
            };
          }
        }
        clarify = {
          pending: !answered,
          questions,
          planConfidence: typeof q.plan_confidence === 'number' ? q.plan_confidence : undefined,
          confidence,
          answerPath: path.join(swarmDir, 'clarify-answers.json'),
        };
      }
    } catch {
      /* no clarify gate this run */
    }

    // Liveness heartbeat (`.swarm/heartbeat`, touched every ~5s while the engine runs). Kept SEPARATE from
    // `mtime` (which reflects real task activity, for the "Xs ago" label): a fresh heartbeat means the engine
    // is alive even mid-long-tool-call, so the panel can flag "stopped" within seconds of a kill without
    // false-positives. Null when the run predates heartbeats — the panel then falls back to mtime staleness.
    let heartbeat: number | null = null;
    try {
      heartbeat = (await fs.stat(path.join(swarmDir, 'heartbeat'))).mtimeMs;
    } catch {
      /* no heartbeat file (older run) */
    }

    // Pause sentinel (.swarm/pause): its existence means the user REQUESTED a hold. This is the optimistic
    // "Pausing…" signal; the engine-truth "Held" comes from the run_paused event in `events`. A request is not
    // a fact — the panel must show "Held" only off the event, never off this stat alone.
    let pauseRequested = false;
    try {
      await fs.stat(path.join(swarmDir, 'pause'));
      pauseRequested = true;
    } catch {
      /* no pause sentinel -> not paused */
    }

    return {
      runId: runFile.replace(/^run-/, '').replace(/\.jsonl$/, ''),
      mtime: freshest,
      heartbeat,
      events,
      activity,
      clarify,
      pauseRequested,
    };
  } catch {
    return null;
  }
});

ipcMain.handle('check-ollama', async () => {
  try {
    return new Promise((resolve) => {
      // Run `ps` and filter for "ollama"
      const ps = spawn('ps', ['aux']);
      const grep = spawn('grep', ['-iw', '[o]llama']);

      let output = '';
      let errorOutput = '';

      // Pipe ps output to grep
      ps.stdout.pipe(grep.stdin);

      grep.stdout.on('data', (data) => {
        output += data.toString();
      });

      grep.stderr.on('data', (data) => {
        errorOutput += data.toString();
      });

      grep.on('close', (code) => {
        if (code !== null && code !== 0 && code !== 1) {
          // grep returns 1 when no matches found
          console.error('Error executing grep command:', errorOutput);
          return resolve(false);
        }

        const trimmedOutput = output.trim();

        const isRunning = trimmedOutput.length > 0;
        resolve(isRunning);
      });

      ps.on('error', (error) => {
        console.error('Error executing ps command:', error);
        resolve(false);
      });

      grep.on('error', (error) => {
        console.error('Error executing grep command:', error);
        resolve(false);
      });

      // Close ps stdin when done
      ps.stdout.on('end', () => {
        grep.stdin.end();
      });
    });
  } catch (err) {
    console.error('Error checking for Ollama:', err);
    return false;
  }
});

ipcMain.handle('read-file', async (_event, filePath) => {
  try {
    const expandedPath = expandTilde(filePath);
    if (process.platform === 'win32') {
      const buffer = await fs.readFile(expandedPath);
      return { file: buffer.toString('utf8'), filePath: expandedPath, error: null, found: true };
    }
    // Non-Windows: keep previous behavior via cat for parity
    return await new Promise((resolve) => {
      const cat = spawn('cat', [expandedPath]);
      let output = '';
      let errorOutput = '';

      cat.stdout.on('data', (data) => {
        output += data.toString();
      });

      cat.stderr.on('data', (data) => {
        errorOutput += data.toString();
      });

      cat.on('close', (code) => {
        if (code !== 0) {
          resolve({ file: '', filePath: expandedPath, error: errorOutput || null, found: false });
          return;
        }
        resolve({ file: output, filePath: expandedPath, error: null, found: true });
      });

      cat.on('error', (error) => {
        console.error('Error reading file:', error);
        resolve({ file: '', filePath: expandedPath, error, found: false });
      });
    });
  } catch (error) {
    console.error('Error reading file:', error);
    return { file: '', filePath: expandTilde(filePath), error, found: false };
  }
});

/**
 * Queue a note for a swarm build that is already running.
 *
 * A DEDICATED handler, not the generic write-file, for one reason: `fs.writeFile` does not create parent
 * directories, and NOTHING creates `.swarm/inbox/`. The engine only ever READS it (read_user_notes does a
 * read_dir and returns empty on Err), and the only create_dir_all calls for it in the whole codebase are
 * inside its own unit tests. So the first note a user ever sent would have silently failed. Widening the
 * generic write-file to mkdir -p would change behaviour for every other caller; this does it only here.
 *
 * The filename IS the ordering — read_user_notes sorts on it, so epoch-ms gives chronology for free, and
 * a unique name per note means two notes can never clobber each other.
 */
ipcMain.handle('swarm-add-note', async (_event, workingDir: string, text: string) => {
  try {
    const t = String(text || '').trim();
    if (!t) return false;
    const inbox = path.join(expandTilde(workingDir), '.swarm', 'inbox');
    await fs.mkdir(inbox, { recursive: true });
    await fs.writeFile(path.join(inbox, `${Date.now()}.json`), JSON.stringify({ text: t }, null, 2), {
      encoding: 'utf8',
    });
    return true;
  } catch (error) {
    console.error('Error queueing swarm note:', error);
    return false;
  }
});

// In-process PAUSE: create/remove the <workingDir>/.swarm/pause sentinel. The scheduler holds at the next task
// boundary while it exists and resumes (re-running nothing) when it is removed. Best-effort like swarm-add-note.
ipcMain.handle('swarm-set-paused', async (_event, workingDir: string, paused: boolean) => {
  try {
    const f = path.join(expandTilde(workingDir), '.swarm', 'pause');
    if (paused) {
      await fs.mkdir(path.dirname(f), { recursive: true });
      await fs.writeFile(f, '', { encoding: 'utf8' });
    } else {
      await fs.rm(f, { force: true });
    }
    return true;
  } catch (error) {
    console.error('Error setting swarm pause:', error);
    return false;
  }
});

ipcMain.handle('write-file', async (_event, filePath, content) => {
  try {
    // Expand tilde to home directory
    const expandedPath = expandTilde(filePath);
    await fs.writeFile(expandedPath, content, { encoding: 'utf8' });
    return true;
  } catch (error) {
    console.error('Error writing to file:', error);
    return false;
  }
});

// Enhanced file operations
ipcMain.handle('ensure-directory', async (_event, dirPath) => {
  try {
    // Expand tilde to home directory
    const expandedPath = expandTilde(dirPath);

    await fs.mkdir(expandedPath, { recursive: true });
    return true;
  } catch (error) {
    console.error('Error creating directory:', error);
    return false;
  }
});

ipcMain.handle('list-files', async (_event, dirPath, extension) => {
  try {
    // Expand tilde to home directory
    const expandedPath = expandTilde(dirPath);

    const files = await fs.readdir(expandedPath);
    if (extension) {
      return files.filter((file) => file.endsWith(extension));
    }
    return files;
  } catch (error) {
    console.error('Error listing files:', error);
    return [];
  }
});

// Recursively copy a directory (used by the Claude Code import tool to relocate a skill directory,
// preserving its supporting files — docs/, scripts/, templates/ — which the sources create API drops).
ipcMain.handle(
  'copy-dir',
  async (_event, src: string, dest: string, opts?: { overwrite?: boolean }) => {
    try {
      const s = expandTilde(src);
      const d = expandTilde(dest);
      // Refusing an existing dir is right for a FIRST import (symmetric with the create path, which errors on
      // a name collision) and WRONG for a re-import, which is the whole reason the button gets pressed twice.
      //
      // MEASURED: this refusal silently froze every imported skill at its first-import snapshot. Twelve days
      // on, atlassian-community-leanzero was 10,506 bytes in goose against 23,210 in Claude Code, missing 186
      // files including the ones the SKILL.md instructs the agent to run. Re-importing changed nothing and
      // reported "16 already present", which reads as "nothing to do" rather than "your updates were dropped".
      // An import that cannot import an update is not a skip; it is a silent stale mirror.
      if (fsSync.existsSync(d) && !opts?.overwrite) {
        return { ok: false, error: `destination already exists: ${d}` };
      }
      // force+no errorOnExist so an update REPLACES the files it carries. Files only in the destination are
      // left alone: cp does not prune, so anything goose wrote that Claude Code does not know about survives.
      //
      // Resolve the skill dir itself, but ONLY the top level.
      //
      // A Claude skill is often a SYMLINK into a real project — six of Mihai's point at
      // ~/Projects/skill-jira-forge/. cp defaults to dereference:false, so it copies the LINK, and a
      // re-import then tries to replace a real destination directory with a symlink:
      // ERR_FS_CP_NON_DIR_TO_DIR, "cannot overwrite directory with non-directory". That was the red X on
      // exactly those six rows.
      //
      // The tempting fix — dereference:true — is WRONG and measurably worse. It follows EVERY link in the
      // tree, including the ones npm/pnpm litter through node_modules: the source inflated from 952 files to
      // 3082 and then died halfway with ERR_FS_CP_EINVAL ("src and dest cannot be the same") on a
      // self-referential node_modules link, leaving a PARTIAL copy behind. A half-written skill that throws
      // is worse than the stale one it replaced.
      //
      // realpath resolves the top-level link only, so both sides of the copy are real directories.
      //
      // The filter then skips every link INSIDE the tree. Leaving them in fails outright: this skill has
      // scripts/browser/node_modules -> ~/Projects/forge-live-harness/node_modules on BOTH sides, and cp
      // resolves the destination link, sees the same real path on each end and refuses with
      // "cannot copy X to a subdirectory of self" (ERR_FS_CP_EINVAL) — killing the whole import over a
      // dependency folder that is not skill content and that goose has no business owning a copy of.
      // Skipping links leaves any existing one exactly as it was.
      const realSrc = await fs.realpath(s);
      await fs.cp(realSrc, d, {
        recursive: true,
        force: Boolean(opts?.overwrite),
        errorOnExist: !opts?.overwrite,
        filter: async (from) => {
          const st = await fs.lstat(from).catch(() => null);
          return !!st && !st.isSymbolicLink();
        },
      });
      return { ok: true };
    } catch (error) {
      console.error('Error copying directory:', error);
      return { ok: false, error: error instanceof Error ? error.message : String(error) };
    }
  }
);

// Compare a Claude Code skill dir against its imported copy, so the import can tell the user what is actually
// out of date instead of calling a stale mirror "already present". Returns file counts, never file contents.
ipcMain.handle('skill-drift', async (_event, src: string, dest: string) => {
  try {
    const s = expandTilde(src);
    const d = expandTilde(dest);
    if (!fsSync.existsSync(d)) return { ok: true, exists: false, added: 0, changed: 0 };
    // The walk must mirror EXACTLY what copy-dir will do, or the badge promises something the button cannot
    // deliver: resolve the top-level link (realpath, below) and then do NOT follow links inside the tree.
    // Following them walks the whole of node_modules — and back out again, since pnpm's links point outside
    // the skill — which is both enormous and, on a self-referential link, unbounded.
    const walk = async (root: string, base = ''): Promise<string[]> => {
      const out: string[] = [];
      for (const e of await fs.readdir(path.join(root, base), { withFileTypes: true })) {
        const rel = base ? `${base}/${e.name}` : e.name;
        if (e.isDirectory()) out.push(...(await walk(root, rel)));
        else if (e.isFile()) out.push(rel);
      }
      return out;
    };
    const realSrc = await fs.realpath(s);
    const srcFiles = await walk(realSrc);
    let added = 0;
    let changed = 0;
    for (const rel of srcFiles) {
      const dp = path.join(d, rel);
      if (!fsSync.existsSync(dp)) {
        added += 1;
        continue;
      }
      // size+mtime, not a hash: this runs over ~950 files per skill on every scan, and a byte-level diff is
      // not needed to answer "is the copy behind?".
      const [a, b] = [fsSync.statSync(path.join(s, rel)), fsSync.statSync(dp)];
      if (a.size !== b.size || a.mtimeMs > b.mtimeMs + 1000) changed += 1;
    }
    return { ok: true, exists: true, added, changed };
  } catch (error) {
    return { ok: false, error: error instanceof Error ? error.message : String(error) };
  }
});

// Run `goose import claude-code <flags>` (memory/hints from ~/.claude/CLAUDE.md + auto-memory notes, or MCP
// from ~/.claude.json). --yes is MANDATORY: with no TTY the importer otherwise prints a preview and returns
// without importing. Reuses the live binary locator so it works in a packaged app.
ipcMain.handle('import-claude-code', async (_event, args: string[]) => {
  try {
    const goosePath = findGooseBinaryPath({
      isPackaged: app.isPackaged,
      resourcesPath: app.isPackaged ? process.resourcesPath : undefined,
    });
    const pathKey = process.platform === 'win32' ? 'Path' : 'PATH';
    const env = {
      ...process.env,
      [pathKey]: `${path.dirname(goosePath)}${path.delimiter}${process.env[pathKey] ?? ''}`,
    };
    return await new Promise((resolve) => {
      execFile(
        goosePath,
        ['import', 'claude-code', ...args, '--apply', '--yes'],
        { env, timeout: 120000 },
        (err, stdout, stderr) => {
          resolve({
            ok: !err,
            stdout: String(stdout ?? ''),
            stderr: String(stderr ?? ''),
            error: err ? err.message : null,
          });
        }
      );
    });
  } catch (error) {
    return { ok: false, stdout: '', stderr: '', error: error instanceof Error ? error.message : String(error) };
  }
});

ipcMain.handle('show-message-box', async (_event, options) => {
  return dialog.showMessageBox(options);
});

ipcMain.handle('show-save-dialog', async (_event, options) => {
  return dialog.showSaveDialog(options);
});

ipcMain.handle('get-allowed-extensions', async () => {
  return await getAllowList();
});

const createNewWindow = async (app: App, dir?: string | null) => {
  const recentDirs = loadRecentDirs();
  const openDir = dir || (recentDirs.length > 0 ? recentDirs[0] : undefined);
  return await createChat(app, { dir: openDir });
};

const focusWindow = () => {
  const windows = BrowserWindow.getAllWindows();
  if (windows.length > 0) {
    windows.forEach((win) => {
      win.show();
    });
    windows[windows.length - 1].webContents.send('focus-input');
  } else {
    createNewWindow(app);
  }
};

const registerGlobalShortcuts = () => {
  globalShortcut.unregisterAll();

  const settings = getSettings();
  const shortcuts = getKeyboardShortcuts(settings);

  if (shortcuts.focusWindow) {
    try {
      globalShortcut.register(shortcuts.focusWindow, () => {
        focusWindow();
      });
    } catch (e) {
      console.error('Error registering focus window hotkey:', e);
    }
  }

  if (shortcuts.quickLauncher) {
    try {
      globalShortcut.register(shortcuts.quickLauncher, () => {
        createLauncher();
      });
    } catch (e) {
      console.error('Error registering launcher hotkey:', e);
    }
  }
};

async function appMain() {
  await configureProxy();

  // Ensure Windows shims are available before any MCP processes are spawned
  await ensureWinShims();

  registerUpdateIpcHandlers();

  // Handle microphone permission requests
  session.defaultSession.setPermissionRequestHandler((_webContents, permission, callback) => {
    console.log('Permission requested:', permission);
    // Allow microphone and media access
    if (permission === 'media') {
      callback(true);
    } else {
      // Default behavior for other permissions
      callback(true);
    }
  });

  // Add CSP headers to all sessions, recomputed on every response so external
  // backend settings take effect without restarting the app.
  session.defaultSession.webRequest.onHeadersReceived((details, callback) => {
    const currentSettings = getSettings();
    callback({
      responseHeaders: {
        ...details.responseHeaders,
        'Content-Security-Policy': buildCSP(getExternalBackendForCsp(currentSettings)),
      },
    });
  });

  // Migrate old settings format if needed (one-time migration)
  const settings = getSettings();
  if (!settings.keyboardShortcuts && settings.globalShortcut !== undefined) {
    updateSettings((s) => {
      s.keyboardShortcuts = getKeyboardShortcuts(s);
      delete s.globalShortcut;
    });
  }

  // Register global shortcuts based on settings
  registerGlobalShortcuts();

  session.defaultSession.webRequest.onBeforeSendHeaders((details, callback) => {
    details.requestHeaders['Origin'] = 'http://localhost:5173';
    callback({ cancel: false, requestHeaders: details.requestHeaders });
  });

  if (settings.showMenuBarIcon) {
    createTray();
  }

  if (process.platform === 'darwin' && !settings.showDockIcon && settings.showMenuBarIcon) {
    app.dock?.hide();
  }

  const { dirPath } = parseArgs();

  if (!openUrlHandledLaunch) {
    await createNewWindow(app, dirPath);
  } else {
    log.info('[Main] Skipping window creation in appMain - open-url already handled launch');
  }

  // Setup auto-updater AFTER window is created and displayed (with delay to avoid blocking)
  setTimeout(() => {
    if (shouldSetupUpdater()) {
      log.info('Setting up auto-updater after window creation...');
      try {
        const settings = getSettings();
        if (settings.disableAutoDownload) {
          setAutoDownloadDisabled(true);
        }
        setupAutoUpdater();
      } catch (error) {
        log.error('Error setting up auto-updater:', error);
      }
    }
  }, 2000);

  if (process.platform === 'darwin') {
    const dockMenu = Menu.buildFromTemplate([
      {
        label: menuT('New Window'),
        click: () => {
          createNewWindow(app);
        },
      },
    ]);
    app.dock?.setMenu(dockMenu);
  }

  const menu = Menu.getApplicationMenu();

  const shortcuts = getKeyboardShortcuts(settings);

  const appMenu = menu?.items.find((item) => item.label === 'Goose');
  if (appMenu?.submenu) {
    appMenu.submenu.insert(1, new MenuItem({ type: 'separator' }));
    if (shortcuts.settings) {
      appMenu.submenu.insert(
        1,
        new MenuItem({
          label: menuT('Settings'),
          accelerator: shortcuts.settings,
          click() {
            const focusedWindow = BrowserWindow.getFocusedWindow();
            if (focusedWindow) focusedWindow.webContents.send('set-view', 'settings');
          },
        })
      );
    }
    appMenu.submenu.insert(1, new MenuItem({ type: 'separator' }));
  }

  const editMenu = menu?.items.find((item) => item.label === 'Edit');
  if (editMenu?.submenu) {
    const selectAllIndex = editMenu.submenu.items.findIndex((item) => item.label === 'Select All');

    const findSubmenu = Menu.buildFromTemplate([
      {
        label: menuT('Find…'),
        accelerator: shortcuts.find || undefined,
        click() {
          const focusedWindow = BrowserWindow.getFocusedWindow();
          if (focusedWindow) focusedWindow.webContents.send('find-command');
        },
      },
      {
        label: menuT('Find Next'),
        accelerator: shortcuts.findNext || undefined,
        click() {
          const focusedWindow = BrowserWindow.getFocusedWindow();
          if (focusedWindow) focusedWindow.webContents.send('find-next');
        },
      },
      {
        label: menuT('Find Previous'),
        accelerator: shortcuts.findPrevious || undefined,
        click() {
          const focusedWindow = BrowserWindow.getFocusedWindow();
          if (focusedWindow) focusedWindow.webContents.send('find-previous');
        },
      },
      {
        label: menuT('Use Selection for Find'),
        accelerator: process.platform === 'darwin' ? 'Command+E' : undefined,
        click() {
          const focusedWindow = BrowserWindow.getFocusedWindow();
          if (focusedWindow) focusedWindow.webContents.send('use-selection-find');
        },
        visible: process.platform === 'darwin', // Only show on Mac
      },
    ]);

    editMenu.submenu.insert(
      selectAllIndex + 1,
      new MenuItem({
        label: menuT('Find'),
        submenu: findSubmenu,
      })
    );
  }

  const fileMenu = menu?.items.find((item) => item.label === 'File');

  if (fileMenu?.submenu) {
    // Use a counter to track the actual insertion index
    let menuIndex = 0;

    if (shortcuts.newChat) {
      fileMenu.submenu.insert(
        menuIndex++,
        new MenuItem({
          label: menuT('New Chat'),
          accelerator: shortcuts.newChat,
          click() {
            const focusedWindow = BrowserWindow.getFocusedWindow();
            if (focusedWindow) focusedWindow.webContents.send('set-view', '');
          },
        })
      );
    }

    if (shortcuts.newChatWindow) {
      fileMenu.submenu.insert(
        menuIndex++,
        new MenuItem({
          label: menuT('New Chat Window'),
          accelerator: shortcuts.newChatWindow,
          click() {
            ipcMain.emit('create-chat-window');
          },
        })
      );
    }

    if (shortcuts.openDirectory) {
      fileMenu.submenu.insert(
        menuIndex++,
        new MenuItem({
          label: menuT('Open Directory...'),
          accelerator: shortcuts.openDirectory,
          click: () => openDirectoryDialog(),
        })
      );
    }

    const recentFilesSubmenu = buildRecentFilesMenu();
    if (recentFilesSubmenu.length > 0) {
      fileMenu.submenu.insert(
        menuIndex++,
        new MenuItem({
          label: menuT('Recent Directories'),
          submenu: recentFilesSubmenu,
        })
      );
    }

    fileMenu.submenu.insert(menuIndex++, new MenuItem({ type: 'separator' }));

    if (shortcuts.focusWindow) {
      fileMenu.submenu.append(
        new MenuItem({
          label: menuT('Focus Goose Window'),
          accelerator: shortcuts.focusWindow,
          click() {
            focusWindow();
          },
        })
      );
    }

    if (shortcuts.quickLauncher) {
      fileMenu.submenu.append(
        new MenuItem({
          label: menuT('Quick Launcher'),
          accelerator: shortcuts.quickLauncher,
          click() {
            createLauncher();
          },
        })
      );
    }
  }

  if (menu) {
    let windowMenu = menu.items.find((item) => item.label === 'Window');

    if (!windowMenu) {
      windowMenu = new MenuItem({
        label: menuT('Window'),
        submenu: Menu.buildFromTemplate([]),
      });

      const helpMenuIndex = menu.items.findIndex((item) => item.label === 'Help');
      if (helpMenuIndex >= 0) {
        menu.items.splice(helpMenuIndex, 0, windowMenu);
      } else {
        menu.items.push(windowMenu);
      }
    }

    if (windowMenu.submenu) {
      if (shortcuts.alwaysOnTop) {
        windowMenu.submenu.append(
          new MenuItem({
            label: menuT('Always on Top'),
            type: 'checkbox',
            accelerator: shortcuts.alwaysOnTop,
            click(menuItem) {
              const focusedWindow = BrowserWindow.getFocusedWindow();
              if (focusedWindow) {
                const isAlwaysOnTop = menuItem.checked;

                if (process.platform === 'darwin') {
                  focusedWindow.setAlwaysOnTop(isAlwaysOnTop, 'floating');
                } else {
                  focusedWindow.setAlwaysOnTop(isAlwaysOnTop);
                }

                console.log(
                  `[Main] Set always-on-top to ${isAlwaysOnTop} for window ${focusedWindow.id}`
                );
              }
            },
          })
        );
      }
    }

    const viewMenu = menu.items.find((item) => item.label === 'View');
    if (viewMenu?.submenu && shortcuts.toggleNavigation) {
      viewMenu.submenu.append(new MenuItem({ type: 'separator' }));
      viewMenu.submenu.append(
        new MenuItem({
          label: menuT('Toggle Navigation'),
          accelerator: shortcuts.toggleNavigation,
          click() {
            const focusedWindow = BrowserWindow.getFocusedWindow();
            if (focusedWindow) {
              focusedWindow.webContents.send('toggle-navigation');
            }
          },
        })
      );
    }
  }

  // on macOS, the topbar is hidden
  if (menu && process.platform !== 'darwin') {
    let helpMenu = menu.items.find((item) => item.label === 'Help');

    // If Help menu doesn't exist, create it and add it to the menu
    if (!helpMenu) {
      helpMenu = new MenuItem({
        label: menuT('Help'),
        submenu: Menu.buildFromTemplate([]), // Start with an empty submenu
      });
      // Find a reasonable place to insert the Help menu, usually near the end
      const insertIndex = menu.items.length > 0 ? menu.items.length - 1 : 0;
      menu.items.splice(insertIndex, 0, helpMenu);
    }

    // Ensure the Help menu has a submenu before appending
    if (helpMenu.submenu) {
      // Add a separator before the About item if the submenu is not empty
      if (helpMenu.submenu.items.length > 0) {
        helpMenu.submenu.append(new MenuItem({ type: 'separator' }));
      }

      // Create the About Goose menu item with a submenu
      const aboutGooseMenuItem = new MenuItem({
        label: menuT('About Goose'),
        submenu: Menu.buildFromTemplate([]), // Start with an empty submenu for About
      });

      // Add the Version menu item (display only) to the About Goose submenu
      if (aboutGooseMenuItem.submenu) {
        aboutGooseMenuItem.submenu.append(
          new MenuItem({
            label: `Version ${version || app.getVersion()}`,
            enabled: false,
          })
        );
      }

      helpMenu.submenu.append(aboutGooseMenuItem);
    }
  }

  if (menu) {
    // Translate labels (including Electron's default top-level entries
    // File/Edit/View/Window/Help and submenu items populated by roles) before
    // installing the menu. Called last so the lookups above that match on the
    // English labels still succeed.
    translateMenuLabels(menu.items);
    Menu.setApplicationMenu(menu);
  }

  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      createNewWindow(app);
    }
  });

  ipcMain.on('create-chat-window', (event, options = {}) => {
    const { query, dir, resumeSessionId, viewType, recipeId } = options;

    let resolvedDir = dir;
    if (!resolvedDir?.trim()) {
      const recentDirs = loadRecentDirs();
      resolvedDir = recentDirs.length > 0 ? recentDirs[0] : undefined;
    }

    const isFromLauncher = query && !resumeSessionId && !viewType && !recipeId;

    if (isFromLauncher) {
      const senderWindow = BrowserWindow.fromWebContents(event.sender);
      const launcherWindowId = senderWindow?.id;
      const allWindows = BrowserWindow.getAllWindows();

      const existingWindows = allWindows.filter(
        (win) => !win.isDestroyed() && win.id !== launcherWindowId
      );

      if (existingWindows.length > 0) {
        const targetWindow = existingWindows[0];
        targetWindow.show();
        targetWindow.focus();
        targetWindow.webContents.send('set-initial-message', query);
        return;
      }
    }

    createChat(app, {
      initialMessage: query,
      dir: resolvedDir,
      resumeSessionId,
      viewType,
      recipeId,
    });
  });

  ipcMain.on('close-window', (event) => {
    const window = BrowserWindow.fromWebContents(event.sender);
    if (window && !window.isDestroyed()) {
      window.close();
    }
  });

  ipcMain.on('notify', (event, data) => {
    try {
      // Validate notification data
      if (!data || typeof data !== 'object') {
        console.error('Invalid notification data');
        return;
      }

      // Validate title and body
      if (typeof data.title !== 'string' || typeof data.body !== 'string') {
        console.error('Invalid notification title or body');
        return;
      }

      // Limit the length of title and body
      const MAX_LENGTH = 1000;
      if (data.title.length > MAX_LENGTH || data.body.length > MAX_LENGTH) {
        console.error('Notification title or body too long');
        return;
      }

      // Remove any HTML tags for security
      const sanitizeText = (text: string) => text.replace(/<[^>]*>/g, '');

      const notification = new Notification({
        title: sanitizeText(data.title),
        body: sanitizeText(data.body),
      });

      // Add click handler to focus the window
      notification.on('click', () => {
        const window = BrowserWindow.fromWebContents(event.sender);
        if (window) {
          if (window.isMinimized()) {
            window.restore();
          }
          window.show();
          window.focus();
        }
      });

      notification.show();
    } catch (error) {
      console.error('Error showing notification:', error);
    }
  });

  ipcMain.on('logInfo', (_event, info) => {
    try {
      // Validate log info
      if (info === undefined || info === null) {
        console.error('Invalid log info: undefined or null');
        return;
      }

      // Convert to string if not already
      const logMessage = String(info);

      // Limit log message length
      const MAX_LENGTH = 10000; // 10KB limit
      if (logMessage.length > MAX_LENGTH) {
        console.error('Log message too long');
        return;
      }

      // Log the sanitized message
      log.info('from renderer:', logMessage);
    } catch (error) {
      console.error('Error logging info:', error);
    }
  });

  ipcMain.on('broadcast-theme-change', (event, themeData) => {
    const senderWindow = BrowserWindow.fromWebContents(event.sender);
    const allWindows = BrowserWindow.getAllWindows();

    allWindows.forEach((window) => {
      if (window.id !== senderWindow?.id) {
        window.webContents.send('theme-changed', themeData);
      }
    });
  });

  ipcMain.on('reload-app', (event) => {
    // Get the window that sent the event
    const window = BrowserWindow.fromWebContents(event.sender);
    if (window) {
      window.reload();
    }
  });

  ipcMain.on('open-in-chrome', (_event, url) => {
    try {
      // Validate URL
      const parsedUrl = new URL(url);

      // Only allow http and https protocols for browser URLs
      if (!WEB_PROTOCOLS.includes(parsedUrl.protocol)) {
        console.error('Invalid URL protocol. Only HTTP and HTTPS are allowed.');
        return;
      }

      // On macOS, use the 'open' command with Chrome
      if (process.platform === 'darwin') {
        spawn('open', ['-a', 'Google Chrome', url]);
      } else if (process.platform === 'win32') {
        // On Windows, start is built-in command of cmd.exe
        spawn('cmd.exe', ['/c', 'start', '', 'chrome', url]);
      } else {
        // On Linux, use xdg-open with chrome
        spawn('xdg-open', [url]);
      }
    } catch (error) {
      console.error('Error opening URL in browser:', error);
    }
  });

  // Handle app restart
  ipcMain.on('restart-app', () => {
    app.relaunch();
    app.exit(0);
  });

  // Handler for getting app version
  ipcMain.on('get-app-version', (event) => {
    event.returnValue = app.getVersion();
  });

  ipcMain.on('get-app-locale', (event) => {
    event.returnValue = getConfiguredGooseLocale();
  });

  ipcMain.handle('open-directory-in-explorer', async (_event, path: string) => {
    try {
      return !!(await shell.openPath(path));
    } catch (error) {
      console.error('Error opening directory in explorer:', error);
      return false;
    }
  });

  // Reveal-and-SELECT a file/dir in Finder (vs openPath, which LAUNCHES it in its default app — the wrong
  // verb for a source file). Used by the swarm activity log's right-click "Reveal in Finder".
  ipcMain.handle('reveal-in-finder', async (_event, path: string) => {
    try {
      shell.showItemInFolder(path);
      return true;
    } catch (error) {
      console.error('Error revealing in finder:', error);
      return false;
    }
  });

  // Read goose's stored memories for the Memories view. Global memories live in ~/.config/goose/memory/ as one
  // .txt per category; the optional local dir is <workingDir>/.goose/memory/. Each file is split into entries on
  // blank lines, and an entry's leading "# ..." line is its space-separated tags (see goose-mcp memory/mod.rs).
  ipcMain.handle('list-memories', async (_event, workingDir?: string) => {
    const parseFile = (filePath: string, category: string, scope: 'global' | 'local') => {
      const out: Array<{
        id: string;
        category: string;
        scope: 'global' | 'local';
        tags: string[];
        content: string;
        updatedAt: number;
      }> = [];
      let mtime = 0;
      try {
        mtime = fsSync.statSync(filePath).mtimeMs;
      } catch {
        mtime = 0;
      }
      const raw = fsSync.readFileSync(filePath, 'utf8');
      let idx = 0;
      for (const block of raw.split('\n\n')) {
        const trimmed = block.trim();
        if (!trimmed) continue;
        const lines = trimmed.split('\n');
        let tags: string[] = [];
        let bodyLines = lines;
        if (lines[0].startsWith('#')) {
          tags = lines[0].slice(1).trim().split(/\s+/).filter(Boolean);
          bodyLines = lines.slice(1);
        }
        const content = bodyLines.join('\n').trim();
        if (!content) continue;
        out.push({ id: `${scope}:${category}:${idx++}`, category, scope, tags, content, updatedAt: mtime });
      }
      return out;
    };
    const readDir = (dir: string, scope: 'global' | 'local') => {
      const acc: ReturnType<typeof parseFile> = [];
      let names: string[] = [];
      try {
        names = fsSync.readdirSync(dir);
      } catch {
        return acc;
      }
      for (const name of names) {
        if (!name.endsWith('.txt')) continue;
        try {
          acc.push(...parseFile(path.join(dir, name), name.replace(/\.txt$/, ''), scope));
        } catch (error) {
          console.error('Error reading memory file', name, error);
        }
      }
      return acc;
    };
    const all = [...readDir(path.join(os.homedir(), '.config', 'goose', 'memory'), 'global')];
    if (workingDir) {
      all.push(...readDir(path.join(workingDir, '.goose', 'memory'), 'local'));
    }
    all.sort((a, b) => b.updatedAt - a.updatedAt);
    return all;
  });

  // Resolve a memory file's path from its scope (global = ~/.config/goose/memory, local = <wd>/.goose/memory)
  // and split a stored entry-block into its leading "# tags" line + body (mirrors the list-memories parse).
  const memoryFilePath = (scope: 'global' | 'local', category: string, workingDir?: string) =>
    scope === 'local' && workingDir
      ? path.join(workingDir, '.goose', 'memory', `${category}.txt`)
      : path.join(os.homedir(), '.config', 'goose', 'memory', `${category}.txt`);
  const splitMemoryBlock = (block: string) => {
    const lines = block.split('\n');
    return lines[0].startsWith('#')
      ? { tagsLine: lines[0], body: lines.slice(1).join('\n').trim() }
      : { tagsLine: '', body: lines.join('\n').trim() };
  };

  // Edit ONE memory entry in place, preserving its tags line. The entry is matched by its exact (pre-edit) body
  // — the same body list-memories returned — so the edit targets the block the user opened. Throws if the entry
  // is not found (it changed on disk since the list was read).
  ipcMain.handle(
    'edit-memory',
    async (
      _event,
      args: {
        scope: 'global' | 'local';
        category: string;
        oldContent: string;
        newContent: string;
        workingDir?: string;
      }
    ) => {
      const fp = memoryFilePath(args.scope, args.category, args.workingDir);
      const raw = fsSync.readFileSync(fp, 'utf8');
      let found = false;
      const out = raw
        .split('\n\n')
        .map((block) => {
          const trimmed = block.trim();
          if (!trimmed) return '';
          const { tagsLine, body } = splitMemoryBlock(trimmed);
          if (!found && body === args.oldContent) {
            found = true;
            return (tagsLine ? tagsLine + '\n' : '') + args.newContent.trim();
          }
          return trimmed;
        })
        .filter((b) => b.length > 0);
      if (!found) throw new Error('Memory entry not found — it may have changed on disk. Reopen and retry.');
      fsSync.writeFileSync(fp, out.join('\n\n') + '\n');
      return true;
    }
  );

  // Delete ONE memory entry (matched by exact body). If it was the file's last entry, remove the file so the
  // category disappears rather than lingering as an empty file.
  ipcMain.handle(
    'delete-memory',
    async (
      _event,
      args: { scope: 'global' | 'local'; category: string; content: string; workingDir?: string }
    ) => {
      const fp = memoryFilePath(args.scope, args.category, args.workingDir);
      const raw = fsSync.readFileSync(fp, 'utf8');
      const kept = raw
        .split('\n\n')
        .map((b) => b.trim())
        .filter((b) => b.length > 0 && splitMemoryBlock(b).body !== args.content);
      if (kept.length === 0) {
        fsSync.rmSync(fp, { force: true });
      } else {
        fsSync.writeFileSync(fp, kept.join('\n\n') + '\n');
      }
      return true;
    }
  );

  ipcMain.handle('launch-app', async (event, gooseApp: GooseApp) => {
    try {
      if (isRetiredGooseChatApp(gooseApp)) {
        throw new Error('This built-in Chat app is no longer supported.');
      }

      const launchingWindow = BrowserWindow.fromWebContents(event.sender);
      if (!launchingWindow) {
        throw new Error('Could not find launching window');
      }

      const launchingWindowId = launchingWindow.id;
      const launchingGooseServeLease = gooseServeLeases.get(launchingWindowId);
      if (!launchingGooseServeLease) {
        throw new Error('No backend lease found for launching window');
      }

      const workingDir = app.getPath('home');
      const appWindow = new BrowserWindow({
        title: formatAppName(gooseApp.name),
        width: gooseApp.width ?? 800,
        height: gooseApp.height ?? 600,
        resizable: gooseApp.resizable ?? true,
        useContentSize: true,
        webPreferences: {
          preload: path.join(__dirname, 'preload.js'),
          nodeIntegration: false,
          contextIsolation: true,
          webSecurity: true,
          additionalArguments: [
            JSON.stringify({
              ...appConfig,
              GOOSE_LOCALE: getConfiguredGooseLocale(),
              GOOSE_WORKING_DIR: workingDir,
              GOOSE_VERSION: version,
            }),
          ],
          partition: 'persist:goose',
        },
      });

      gooseServeLeases.attachWindow(appWindow.id, launchingGooseServeLease);

      appWindows.set(gooseApp.name, appWindow);

      appWindow.on('closed', () => {
        void gooseServeLeases.releaseWindow(appWindow.id);
        appWindows.delete(gooseApp.name);
      });

      const extensionName = gooseApp.mcpServers?.[0] ?? '';

      const url = getAppUrl();

      const searchParams = new URLSearchParams();
      searchParams.set('resourceUri', gooseApp.uri);
      searchParams.set('extensionName', extensionName);
      searchParams.set('appName', gooseApp.name);
      searchParams.set('workingDir', workingDir);

      url.hash = `/standalone-app?${searchParams.toString()}`;
      await appWindow.loadURL(formatUrl(url));
      appWindow.show();
    } catch (error) {
      console.error('Failed to launch app:', error);
      throw error;
    }
  });

  ipcMain.handle('refresh-app', async (_event, gooseApp: GooseApp) => {
    try {
      const appWindow = appWindows.get(gooseApp.name);
      if (!appWindow || appWindow.isDestroyed()) {
        console.log(`App window for '${gooseApp.name}' not found or destroyed, skipping refresh`);
        return;
      }

      // Bring to front first
      if (appWindow.isMinimized()) {
        appWindow.restore();
      }
      appWindow.show();
      appWindow.focus();

      // Then reload
      await appWindow.webContents.reload();
    } catch (error) {
      console.error('Failed to refresh app:', error);
      throw error;
    }
  });

  ipcMain.handle('close-app', async (_event, appName: string) => {
    try {
      const appWindow = appWindows.get(appName);
      if (!appWindow || appWindow.isDestroyed()) {
        console.log(`App window for '${appName}' not found or destroyed, skipping close`);
        return;
      }

      appWindow.close();
    } catch (error) {
      console.error('Failed to close app:', error);
      throw error;
    }
  });
}

app.whenReady().then(async () => {
  try {
    await appMain();
  } catch (error) {
    dialog.showErrorBox('Goose Error', `Failed to create main window: ${error}`);
    app.quit();
  }
});

async function getAllowList(): Promise<string[]> {
  if (!process.env.GOOSE_ALLOWLIST) {
    return [];
  }

  const response = await fetch(process.env.GOOSE_ALLOWLIST);

  if (!response.ok) {
    throw new Error(
      `Failed to fetch allowed extensions: ${response.status} ${response.statusText}`
    );
  }

  // Parse the YAML content
  const yamlContent = await response.text();
  const parsedYaml = yaml.parse(yamlContent);

  // Extract the commands from the extensions array
  if (parsedYaml && parsedYaml.extensions && Array.isArray(parsedYaml.extensions)) {
    const commands = parsedYaml.extensions.map(
      (ext: { id: string; command: string }) => ext.command
    );
    console.log(`Fetched ${commands.length} allowed extension commands`);
    return commands;
  } else {
    console.error('Invalid YAML structure:', parsedYaml);
    return [];
  }
}

app.on('will-quit', async () => {
  const gooseServeLeaseCount = gooseServeLeases.activeLeaseCount();
  if (gooseServeLeaseCount > 0) {
    log.info(`App quitting, cleaning up ${gooseServeLeaseCount} backend lease(s)`);
    await gooseServeLeases.cleanupAll();
  }

  for (const [windowId, blockerId] of windowPowerSaveBlockers.entries()) {
    try {
      powerSaveBlocker.stop(blockerId);
      console.log(
        `[Main] Stopped power save blocker ${blockerId} for window ${windowId} during app quit`
      );
    } catch (error) {
      console.error(
        `[Main] Failed to stop power save blocker ${blockerId} for window ${windowId}:`,
        error
      );
    }
  }
  windowPowerSaveBlockers.clear();

  globalShortcut.unregisterAll();
});

app.on('window-all-closed', () => {
  // Only quit if we're not on macOS or don't have a tray icon
  if (process.platform !== 'darwin' || !tray) {
    app.quit();
  }
});
