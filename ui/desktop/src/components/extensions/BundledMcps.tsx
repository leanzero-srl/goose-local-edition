import { inspectConfigExtension } from '../../acp/extensions';
import { useEffect, useState } from 'react';
import { BookOpen, Check, FolderOpen, Globe, Plug, Save } from 'lucide-react';
import { Button, Panel } from '../lz';
import { useConfig, type FixedExtensionEntry } from '../ConfigContext';
import type { McpSetupResult } from '../../types/mcpSetup';
import { McpCapabilities } from './McpCapabilities';

type BundledMcp = Awaited<ReturnType<typeof window.electron.bundledMcps>>[number];
const input =
  'w-full rounded-lg border border-lz-border-strong bg-lz-surface px-3 py-2 text-sm text-lz-ink focus:outline-none focus:ring-2 focus:ring-lz-accent';

export const BUNDLED_NAMES = ['LeanZero Web Search', 'LeanZero Documents'];

export function mergeMcpSettings(
  entry: BundledMcp,
  saved: FixedExtensionEntry | undefined,
  values: Record<string, string>
) {
  const envs = {
    ...('envs' in (saved ?? {}) ? (saved as { envs: Record<string, string> }).envs : {}),
    ...entry.envs,
  };
  for (const [key, value] of Object.entries(values)) {
    if (value.trim()) envs[key] = value.trim();
    else delete envs[key];
  }
  const envKeys = (saved && 'env_keys' in saved ? (saved.env_keys ?? []) : []).filter(
    (key) => !(key in values) || Boolean(values[key].trim())
  );
  return {
    ...entry,
    ...saved,
    env_keys: envKeys,
    type: 'stdio' as const,
    cmd: entry.cmd,
    args: entry.args,
    envs,
  };
}

function ServerSetup({ entry, saved }: { entry: BundledMcp; saved?: FixedExtensionEntry }) {
  const { addExtension } = useConfig();
  const web = entry.name === 'LeanZero Web Search';
  const [values, setValues] = useState<Record<string, string>>({});
  const [dirty, setDirty] = useState(false);
  const [status, setStatus] = useState('');
  const [result, setResult] = useState<McpSetupResult | null>(null);
  const [source, setSource] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (!saved || dirty) return;
    let cancelled = false;
    inspectConfigExtension(entry.name, undefined, true)
      .then((data) => {
        if (!cancelled) setValues(data.settings);
      })
      .catch(() => {
        if (!cancelled)
          setError('Saved settings could not be loaded. Reconnect to Goose before editing.');
      });
    return () => {
      cancelled = true;
    };
  }, [entry.name, saved, dirty]);
  const update = (key: string, value: string) => {
    setValues((previous) => ({ ...previous, [key]: value }));
    setDirty(true);
    setStatus('');
    setResult(null);
  };
  const config = () => {
    for (const key of ['OUTPUT_DIR', 'DOC_OUTPUT_DIR', 'CRAWL_CACHE_DIR']) {
      const folder = values[key]?.trim();
      if (folder && !/^(?:\/|[A-Za-z]:[\\/]|\\\\)/.test(folder)) {
        throw new Error(
          'Choose a folder or enter its full absolute path. Relative paths and ~ are not supported.'
        );
      }
    }
    return mergeMcpSettings(entry, saved, values);
  };
  const perform = async (action: () => Promise<void>) => {
    if (busy) return;
    setBusy(true);
    setError('');
    setStatus('');
    try {
      await action();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  const field = (key: string, title: string, hint: string, secret = false) => (
    <label className="block space-y-1.5">
      <span className="text-sm font-medium">{title}</span>
      <input
        className={input}
        type={secret ? 'password' : 'text'}
        autoComplete="off"
        placeholder={
          secret && saved && 'env_keys' in saved && saved.env_keys?.includes(key)
            ? 'Saved · leave unchanged or enter a replacement'
            : undefined
        }
        value={values[key] ?? ''}
        onChange={(e) => update(key, e.target.value)}
      />
      <span className="block text-xs leading-relaxed text-lz-ink-2">{hint}</span>
      {secret && saved && 'env_keys' in saved && saved.env_keys?.includes(key) && (
        <button
          type="button"
          className="text-xs text-lz-err underline"
          onClick={() => update(key, '')}
        >
          Remove saved key on next save
        </button>
      )}
    </label>
  );
  const folderKey = web ? 'OUTPUT_DIR' : 'DOC_OUTPUT_DIR';
  return (
    <article className="min-w-0 rounded-xl border border-lz-border bg-lz-surface p-5">
      <div className="mb-5 flex items-start gap-3">
        <span className="rounded-lg bg-lz-accent p-2.5 text-lz-accent-ink">
          {web ? <Globe size={20} /> : <BookOpen size={20} />}
        </span>
        <div className="flex-1">
          <h2 className="font-semibold">{entry.name}</h2>
          <p className="mt-1 text-sm text-lz-ink-2">
            {web
              ? 'Find reliable sources and collect pages for your research.'
              : 'Read and create PDF, Word, Excel and PowerPoint files.'}
          </p>
        </div>
        <span className="text-xs font-medium text-lz-ink-2">
          {saved ? (saved.enabled ? 'Enabled' : 'Disabled') : 'Not configured'}
        </span>
      </div>
      <fieldset disabled={busy} aria-busy={busy} className="min-w-0 space-y-5">
        {web &&
          field(
            'SERPER_API_KEY',
            'Search API key',
            'Serper key for web searches. You can collect a known page without a key.',
            true
          )}
        <div className="flex items-start gap-2">
          <div className="flex-1">
            {field(
              folderKey,
              web ? 'Research corpus folder' : 'Document output folder',
              web
                ? 'Sources are saved under docs/research-output with provenance, and are available through the server’s list/read cached document tools. Use Choose or enter a full absolute path.'
                : 'New documents are written here. Existing documents are read from paths supplied in chat.'
            )}
          </div>
          <Button
            className="mt-7"
            icon={<FolderOpen />}
            aria-label={`Choose ${entry.name} folder`}
            onClick={() =>
              perform(async () => {
                const picked = await window.electron.directoryChooser(values[folderKey]);
                if (!picked.canceled && picked.filePaths[0]) update(folderKey, picked.filePaths[0]);
              })
            }
          >
            Choose
          </Button>
        </div>
        <details className="rounded-lg border border-lz-border p-3">
          <summary className="cursor-pointer text-sm font-medium">Optional integrations</summary>
          <div className="mt-4 space-y-4">
            {web ? (
              <>
                {field(
                  'GITHUB_TOKEN',
                  'GitHub token',
                  'Optional: authenticated repository access and a higher GitHub request allowance.',
                  true
                )}
                {field(
                  'CRAWL_CACHE_DIR',
                  'Page cache folder',
                  'Optional: a separate location for downloaded page cache.'
                )}
              </>
            ) : (
              <>
                {field(
                  'Z_AI_API_KEY',
                  'Vision API key',
                  'Optional: enables the server’s vision service for image analysis.',
                  true
                )}
                {field(
                  'Z_AI_BASE_URL',
                  'Vision API base URL',
                  'Optional: override the vision service endpoint.'
                )}
                {field(
                  'Z_AI_VISION_MODEL',
                  'Vision model',
                  'Optional: model identifier supported by your vision endpoint.'
                )}
                {field(
                  'WEB_SEARCH_MCP_URL',
                  'Web research MCP URL',
                  'Optional: your HTTP MCP endpoint for document fact checking. Leave blank when not configured.'
                )}
                {field(
                  'WEB_SEARCH_BEARER',
                  'Web research access token',
                  'Required by document fact checking when using the HTTP web research service.',
                  true
                )}
                {field(
                  'SERPER_API_KEY',
                  'Fact-check search API key',
                  'Serper key used by the web research service to verify document claims.',
                  true
                )}
              </>
            )}
          </div>
        </details>
        <div className="flex flex-wrap gap-2">
          <Button
            variant="primary"
            icon={<Save />}
            onClick={() =>
              perform(async () => {
                await addExtension(entry.name, config(), saved?.enabled ?? true);
                setDirty(false);
                setStatus('Settings saved. New chats and agent runs use these settings.');
              })
            }
          >
            {saved ? 'Save settings' : 'Save and enable'}
          </Button>
          <Button
            icon={<Plug />}
            disabled={!saved || dirty}
            onClick={() =>
              perform(async () => {
                const discovered = await inspectConfigExtension(entry.name);
                setResult({ tools: discovered.tools as unknown as McpSetupResult['tools'] });
                setStatus(
                  `Connected · ${discovered.tools.length} tools discovered${dirty ? ' · settings not yet saved' : ''}`
                );
              })
            }
          >
            Test connection
          </Button>
          {saved && (
            <Button
              onClick={() =>
                perform(async () => {
                  await addExtension(entry.name, config(), !saved.enabled);
                  setDirty(false);
                  setStatus(saved.enabled ? 'Disabled for new chats.' : 'Enabled for new chats.');
                })
              }
            >
              {saved.enabled ? 'Disable' : 'Enable'}
            </Button>
          )}
        </div>
        {dirty && (
          <p className="text-xs text-lz-ink-2">
            Save your changes before testing the connection or collecting a source.
          </p>
        )}
        {web && (
          <section className="space-y-3 border-t border-lz-border pt-5">
            <h3 className="text-sm font-semibold">Build your corpus</h3>
            <p className="text-xs leading-relaxed text-lz-ink-2">
              Add a page you want the agent to reference. Collection writes a new Markdown source;
              existing files are kept. Review the extracted content before relying on it.
            </p>
            <label className="block">
              <span className="mb-1.5 block text-sm">Source URL</span>
              <input
                className={input}
                type="url"
                placeholder="https://example.com/reference"
                value={source}
                onChange={(e) => setSource(e.target.value)}
              />
            </label>
            <Button
              icon={<BookOpen />}
              disabled={!saved || dirty || !source.trim() || !values.OUTPUT_DIR}
              onClick={() =>
                perform(async () => {
                  const collected = await inspectConfigExtension(entry.name, source.trim());
                  setResult({
                    tools: collected.tools as unknown as McpSetupResult['tools'],
                    savedFile: collected.savedFile ?? undefined,
                  });
                  setStatus('Source collected.');
                })
              }
            >
              Collect source
            </Button>
          </section>
        )}
        {status && (
          <p role="status" className="flex items-start gap-2 text-sm text-lz-ok">
            <Check size={16} className="mt-0.5 shrink-0" />
            {status}
          </p>
        )}
        {error && (
          <p role="alert" className="text-sm text-lz-err">
            {error}
          </p>
        )}
        {result?.savedFile && (
          <p className="break-all rounded-lg bg-lz-surface-2 p-3 text-xs">
            Saved to {result.savedFile}
          </p>
        )}
        {result && <McpCapabilities tools={result.tools} />}
      </fieldset>
    </article>
  );
}

export function BundledMcps() {
  const { extensionsList } = useConfig();
  const [entries, setEntries] = useState<BundledMcp[]>([]);
  const [error, setError] = useState('');
  useEffect(() => {
    window.electron
      .bundledMcps()
      .then(setEntries)
      .catch((e: unknown) => setError(String(e)));
  }, []);
  return (
    <Panel title="LeanZero tools & sources">
      <p className="mb-5 text-sm text-lz-ink-2">
        Configure your tools, verify the connection, then choose what your agents can use.
      </p>
      {error && (
        <p role="alert" className="text-lz-err">
          Bundled servers unavailable: {error}
        </p>
      )}
      <div className="grid gap-5 xl:grid-cols-2">
        {entries.map((entry) => (
          <ServerSetup
            key={entry.name}
            entry={entry}
            saved={extensionsList.find((item) => item.name === entry.name)}
          />
        ))}
      </div>
    </Panel>
  );
}
