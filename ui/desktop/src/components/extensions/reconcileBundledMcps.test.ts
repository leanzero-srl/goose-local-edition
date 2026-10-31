import { describe, expect, it, vi } from 'vitest';
import {
  planBundledMcpAdditions,
  planBundledMcpCorrections,
  reconcileBundledMcps,
  type BundledMcp,
  type BundledMcpRecord,
} from './reconcileBundledMcps';
import type { FixedExtensionEntry } from '../ConfigContext';

// The installed app's own bundle, as main.ts reports it.
const APP = '/Applications/Goose Swarm.app/Contents/Resources';
const BROWSER = `${APP}/bundled-mcps/browser/chrome-headless-shell/mac_arm-153/chrome-headless-shell`;
const managedEnvKeys = [
  'MCP_CLIENT_TYPE',
  'PUPPETEER_EXECUTABLE_PATH',
  'LEANZERO_BROWSER_EXECUTABLE',
  'ELECTRON_RUN_AS_NODE',
];
const bundleEnvs = {
  MCP_CLIENT_TYPE: 'agent',
  PUPPETEER_EXECUTABLE_PATH: BROWSER,
  LEANZERO_BROWSER_EXECUTABLE: BROWSER,
};
const bundled = (id: string, name: string, entry: string): BundledMcp => ({
  name,
  description: `${name} (this build)`,
  type: 'stdio',
  cmd: `${APP}/bin/node`,
  args: [`${APP}/bundled-mcps/${id}/${entry}`],
  envs: bundleEnvs,
  timeout: 300,
  bundleEntry: `bundled-mcps/${id}/${entry}`,
  managedEnvKeys,
  packaged: true,
});
const webSearch = bundled('leanzero-web-search', 'LeanZero Web Search', 'dist/index.js');
const documents = bundled('leanzero-documents', 'LeanZero Documents', 'src/index.js');

// ~/.config/goose/config.yaml as measured on 2026-09-26 (Q-118): web search written by a dev
// run, documents written by the installed app with none of the bundle's env keys.
const DEV = '/Users/mihaiperdum/Projects/goose/ui';
const savedWebSearch: FixedExtensionEntry = {
  type: 'stdio',
  name: 'LeanZero Web Search',
  description:
    'Search the web and extract pages. Configure SERPER_API_KEY for search; page extraction works without a key.',
  cmd: `${DEV}/node_modules/electron/dist/Electron.app/Contents/MacOS/Electron`,
  args: [`${DEV}/desktop/bundled-mcps/leanzero-web-search/dist/index.js`],
  env_keys: [
    'MCP_CLIENT_TYPE',
    'ELECTRON_RUN_AS_NODE',
    'PUPPETEER_EXECUTABLE_PATH',
    'LEANZERO_BROWSER_EXECUTABLE',
    'OUTPUT_DIR',
  ],
  timeout: 300,
  enabled: true,
  configKey: 'leanzerowebsearch',
};
const savedDocuments: FixedExtensionEntry = {
  type: 'stdio',
  name: 'LeanZero Documents',
  description: 'Read, create and edit PDF, Word, Excel and PowerPoint documents locally.',
  cmd: `${APP}/bin/node`,
  args: [`${APP}/bundled-mcps/leanzero-documents/src/index.js`],
  env_keys: [],
  timeout: 300,
  enabled: true,
  configKey: 'leanzerodocuments',
};
const userFetch: FixedExtensionEntry = {
  type: 'stdio',
  name: 'fetch',
  description: 'user added',
  cmd: 'uvx',
  args: ['mcp-server-fetch'],
  enabled: true,
};

const recordOf = (...entries: BundledMcp[]): BundledMcpRecord =>
  Object.fromEntries(
    entries.map((e) => [
      e.name,
      { cmd: e.cmd, args: e.args, description: e.description, envs: e.envs },
    ])
  );

describe('the installed app points its bundled MCP servers at its own bundle', () => {
  it('corrects the measured Q-118 config: cmd, script, env paths — and keeps the user settings', async () => {
    const add = vi.fn().mockResolvedValue(undefined);
    const saveRecord = vi.fn().mockResolvedValue(undefined);
    const log = vi.fn();
    const corrections = await reconcileBundledMcps({
      entries: [webSearch, documents],
      extensions: [savedWebSearch, savedDocuments, userFetch],
      record: {},
      add,
      saveRecord,
      log,
    });

    expect(corrections.map((c) => c.name)).toEqual(['LeanZero Web Search', 'LeanZero Documents']);
    expect(add).toHaveBeenCalledTimes(2);
    const [webConfig, webEnabled] = add.mock.calls[0];
    expect(webEnabled).toBe(true);
    expect(webConfig).toMatchObject({
      type: 'stdio',
      name: 'LeanZero Web Search',
      cmd: `${APP}/bin/node`,
      args: [`${APP}/bundled-mcps/leanzero-web-search/dist/index.js`],
      envs: bundleEnvs,
      env_keys: ['OUTPUT_DIR'],
      timeout: 300,
      description: webSearch.description,
    });
    expect(webConfig).not.toHaveProperty('enabled');
    expect(webConfig).not.toHaveProperty('configKey');
    expect(JSON.stringify(add.mock.calls)).not.toContain(DEV);
    expect(add.mock.calls[1][0]).toMatchObject({ envs: bundleEnvs, env_keys: [] });

    expect(saveRecord).toHaveBeenCalledWith(recordOf(webSearch, documents));
    expect(log).toHaveBeenCalledTimes(2);
    const line = log.mock.calls[0][0] as string;
    expect(line).toContain('corrected "LeanZero Web Search"');
    const { before, after } = JSON.parse(line.slice(line.indexOf('{')));
    expect(before.cmd).toBe(savedWebSearch.cmd);
    expect(before.args).toEqual(savedWebSearch.args);
    expect(before.envs).toBe('not recorded (stored as secrets)');
    expect(after.cmd).toBe(`${APP}/bin/node`);
    expect(after.envKeys).toEqual(['OUTPUT_DIR', ...Object.keys(bundleEnvs)]);
  });

  it('the next start finds nothing to correct: no write, no log', async () => {
    const corrected: FixedExtensionEntry = {
      ...savedWebSearch,
      cmd: webSearch.cmd,
      args: webSearch.args,
      description: webSearch.description,
      env_keys: ['OUTPUT_DIR', ...Object.keys(bundleEnvs)],
    };
    const add = vi.fn();
    const saveRecord = vi.fn();
    const log = vi.fn();
    await reconcileBundledMcps({
      entries: [webSearch],
      extensions: [corrected],
      record: recordOf(webSearch),
      add,
      saveRecord,
      log,
    });
    expect(add).not.toHaveBeenCalled();
    expect(saveRecord).not.toHaveBeenCalled();
    expect(log).not.toHaveBeenCalled();
  });

  it('a new browser build in the bundle is corrected even when cmd and script are unchanged', () => {
    const current: FixedExtensionEntry = {
      ...savedWebSearch,
      cmd: webSearch.cmd,
      args: webSearch.args,
      description: webSearch.description,
      env_keys: ['OUTPUT_DIR', ...Object.keys(bundleEnvs)],
    };
    const oldBrowser = `${APP}/bundled-mcps/browser/chrome-headless-shell/mac_arm-150/chrome-headless-shell`;
    const record = recordOf(webSearch);
    record[webSearch.name] = {
      ...record[webSearch.name],
      envs: { ...bundleEnvs, LEANZERO_BROWSER_EXECUTABLE: oldBrowser },
    };
    const [correction] = planBundledMcpCorrections([webSearch], [current], record);
    expect(correction.before.envs).toMatchObject({ LEANZERO_BROWSER_EXECUTABLE: oldBrowser });
    expect(correction.config).toMatchObject({ envs: bundleEnvs });
  });

  it('keeps the enabled flag and a custom timeout', () => {
    const [correction] = planBundledMcpCorrections(
      [webSearch],
      [{ ...savedWebSearch, enabled: false, timeout: 900 }],
      {}
    );
    expect(correction.enabled).toBe(false);
    expect(correction.config).toMatchObject({ timeout: 900 });
  });

  it('never touches a same-named server the user pointed at their own script, or an unconfigured one', () => {
    const own: FixedExtensionEntry = {
      ...savedWebSearch,
      cmd: 'node',
      args: ['/Users/someone/mcp-web-search/dist/index.js'],
    };
    expect(planBundledMcpCorrections([webSearch, documents], [own, userFetch], {})).toEqual([]);
  });

  it('a development run never rewrites the config', () => {
    expect(
      planBundledMcpCorrections([{ ...webSearch, packaged: false }], [savedWebSearch], {})
    ).toEqual([]);
  });
});

// WR-17b (Gabee round 4, 3.0.137, 2026-10-31): config.yaml had neither bundled server, nothing
// added them, and every search in the chat ended "Tool 'web-search' not found".
describe('the installed app adds its bundled MCP servers the config has never had', () => {
  it('a first run or an upgrade with neither server configured adds both, enabled, and records them', async () => {
    const add = vi.fn().mockResolvedValue(undefined);
    const saveRecord = vi.fn().mockResolvedValue(undefined);
    const log = vi.fn();
    await reconcileBundledMcps({
      entries: [webSearch, documents],
      extensions: [userFetch],
      record: {},
      add,
      saveRecord,
      log,
    });
    expect(add).toHaveBeenCalledTimes(2);
    expect(add.mock.calls.map(([config, enabled]) => [config.name, enabled])).toEqual([
      ['LeanZero Web Search', true],
      ['LeanZero Documents', true],
    ]);
    expect(add.mock.calls[0][0]).toMatchObject({
      type: 'stdio',
      cmd: `${APP}/bin/node`,
      args: [`${APP}/bundled-mcps/leanzero-web-search/dist/index.js`],
      envs: bundleEnvs,
      env_keys: [],
      timeout: 300,
      description: webSearch.description,
    });
    expect(add.mock.calls[0][0]).not.toHaveProperty('enabled');
    expect(saveRecord).toHaveBeenCalledWith(recordOf(webSearch, documents));
    expect(log.mock.calls[0][0]).toContain('added "LeanZero Web Search"');
  });

  it('the start after the addition adds nothing again', async () => {
    const added: FixedExtensionEntry = {
      type: 'stdio',
      name: webSearch.name,
      description: webSearch.description,
      cmd: webSearch.cmd,
      args: webSearch.args,
      env_keys: Object.keys(bundleEnvs),
      timeout: 300,
      enabled: true,
    };
    const add = vi.fn();
    const saveRecord = vi.fn();
    await reconcileBundledMcps({
      entries: [webSearch],
      extensions: [added],
      record: recordOf(webSearch),
      add,
      saveRecord,
      log: vi.fn(),
    });
    expect(add).not.toHaveBeenCalled();
    expect(saveRecord).not.toHaveBeenCalled();
  });

  it('a server the user removed (the app wrote it before) is not added back', () => {
    expect(planBundledMcpAdditions([webSearch, documents], [], recordOf(webSearch))).toEqual([
      expect.objectContaining({ name: 'LeanZero Documents' }),
    ]);
  });

  it('a disabled server stays disabled and is not added twice', () => {
    const disabled = { ...savedDocuments, enabled: false };
    expect(planBundledMcpAdditions([documents], [disabled], {})).toEqual([]);
    expect(planBundledMcpCorrections([documents], [disabled], {})[0].enabled).toBe(false);
  });

  it('the bundle running under another name counts as configured; a development run adds nothing', () => {
    const renamed = { ...savedDocuments, name: 'My documents' };
    expect(planBundledMcpAdditions([documents], [renamed], {})).toEqual([]);
    expect(planBundledMcpAdditions([{ ...webSearch, packaged: false }], [], {})).toEqual([]);
  });
});
