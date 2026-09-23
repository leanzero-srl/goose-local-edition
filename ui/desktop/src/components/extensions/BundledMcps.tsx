import { nameToKey } from '../settings/extensions/utils';
import { inspectConfigExtension } from '../../acp/extensions';
import { useEffect, useState } from 'react';
import { BookOpen, Check, FolderOpen, Globe, Plug, Save } from 'lucide-react';
import { Button, Panel, Segmented } from '../lz';
import { useConfig, type FixedExtensionEntry } from '../ConfigContext';
import type { McpSetupResult } from '../../types/mcpSetup';
import { McpCapabilities } from './McpCapabilities';

import {
  hostedSettings,
  mergeMcpSettings,
  savedCredential,
  setupLinks,
  validateFolder,
  validateLocalIntegrations,
  type BundledMcp,
  type SetupMode,
} from './leanZeroSetup';
export { BUNDLED_NAMES, mergeMcpSettings } from './leanZeroSetup';
const input =
  'w-full rounded-lg border border-lz-border-strong bg-lz-surface px-3 py-2 text-sm text-lz-ink focus:outline-none focus:ring-2 focus:ring-lz-accent';

function ServerSetup({ entry, saved }: { entry: BundledMcp; saved?: FixedExtensionEntry }) {
  const { addExtension, upsert, setExtensionEnabled } = useConfig();
  const web = entry.name === 'LeanZero Web Search';
  const [mode, setMode] = useState<SetupMode>(
    saved?.type === 'streamable_http' ? 'hosted' : 'local'
  );
  const [loading, setLoading] = useState(Boolean(saved));
  const [loadFailed, setLoadFailed] = useState(false);
  const [query, setQuery] = useState('');
  const [probeOutput, setProbeOutput] = useState('');
  const [values, setValues] = useState<Record<string, string>>({});
  const [dirty, setDirty] = useState(false);
  const [status, setStatus] = useState('');
  const [result, setResult] = useState<McpSetupResult | null>(null);
  const [source, setSource] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (dirty) return;
    let cancelled = false;
    setLoading(Boolean(saved));
    setLoadFailed(false);
    setValues({});
    setMode(saved?.type === 'streamable_http' ? 'hosted' : 'local');
    if (!saved) return;
    if (saved.type === 'streamable_http') {
      setValues({ ENDPOINT: saved.uri, HOSTED_OUTPUT_DIR: saved.headers?.['X-Output-Dir'] ?? '' });
      setLoading(false);
      return;
    }
    inspectConfigExtension(entry.name, undefined, true)
      .then((data) => {
        if (!cancelled) setValues(data.settings);
      })
      .catch(() => {
        if (!cancelled) {
          setLoadFailed(true);
          setError(
            'Saved settings could not be loaded. Reopen this page after reconnecting to Goose.'
          );
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
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
    setProbeOutput('');
  };
  const config = () => {
    for (const key of ['OUTPUT_DIR', 'DOC_OUTPUT_DIR', 'CRAWL_CACHE_DIR']) {
      const folder = values[key]?.trim();
      if (folder) validateFolder(folder);
    }
    if (!values[web ? 'OUTPUT_DIR' : 'DOC_OUTPUT_DIR']?.trim()) {
      throw new Error('Choose an output folder so files are saved somewhere you can find them.');
    }
    validateLocalIntegrations(web, values, saved);
    return mergeMcpSettings(entry, saved, values);
  };
  const hasKey = (key: string) =>
    key in values ? Boolean(values[key].trim()) : savedCredential(saved, mode, web, key);
  const switchMode = (next: SetupMode) => {
    setMode(next);
    setValues(next === 'hosted' ? { ENDPOINT: setupLinks(web).endpoint } : {});
    setDirty(true);
    setStatus('');
    setError('');
    setResult(null);
    setProbeOutput('');
  };
  const perform = async (action: () => Promise<void>) => {
    if (busy) return;
    setBusy(true);
    setError('');
    setStatus('');
    setProbeOutput('');
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
          secret && savedCredential(saved, mode, web, key)
            ? 'Saved · leave unchanged or enter a replacement'
            : undefined
        }
        value={values[key] ?? ''}
        onChange={(e) => update(key, e.target.value)}
      />
      <span className="block text-xs leading-relaxed text-lz-ink-2">{hint}</span>
      {secret && savedCredential(saved, mode, web, key) && (
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
      <fieldset
        disabled={busy || loading || loadFailed}
        aria-busy={busy}
        className="min-w-0 space-y-5"
      >
        <Segmented
          aria-label={`${entry.name} location`}
          value={mode}
          onChange={switchMode}
          options={[
            { value: 'local', label: 'On this computer' },
            { value: 'hosted', label: 'LeanZero hosted' },
          ]}
        />
        <p className="text-sm leading-relaxed text-lz-ink-2">
          {mode === 'local'
            ? 'Included with Goose. No LeanZero access token is needed. Files stay on this computer; web and vision requests go to their providers.'
            : 'Hosted demo for evaluation. Requests and files are processed on the server. It cannot read files from this computer by their local paths.'}
        </p>
        <a
          className="text-sm text-lz-accent underline"
          href={`${setupLinks(web).guide}${mode === 'hosted' ? '#get-key' : ''}`}
          target="_blank"
          rel="noreferrer"
        >
          {mode === 'hosted'
            ? `Get a ${web ? 'Web Search' : 'Documents'} access token`
            : 'Setup guide'}
        </a>
        {mode === 'hosted' && (
          <>
            {field(
              'ENDPOINT',
              'MCP endpoint',
              'Use the HTTPS endpoint supplied with your access token.'
            )}
            {field(
              'ACCESS_TOKEN',
              `${web ? 'Web Search' : 'Documents'} access token`,
              'Required. Each service issues its own token; a Serper or vision key is not an access token.',
              true
            )}
            {field(
              'HOSTED_OUTPUT_DIR',
              'Hosted folder name',
              'Optional subfolder on the server, not a folder on this computer. Use letters, numbers, hyphens or underscores.'
            )}
          </>
        )}
        {web &&
          field(
            'SERPER_API_KEY',
            'Search API key',
            'Serper key for web searches. You can collect a known page without a key.',
            true
          )}
        {mode === 'local' && (
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
                  if (!picked.canceled && picked.filePaths[0])
                    update(folderKey, picked.filePaths[0]);
                })
              }
            >
              Choose
            </Button>
          </div>
        )}
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
                {mode === 'local' &&
                  field(
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
                {mode === 'local' &&
                  field(
                    'Z_AI_BASE_URL',
                    'Vision API base URL',
                    'Set the endpoint for your vision provider. Without an override the server uses BigModel; a Z.AI key needs its matching Z.AI endpoint.'
                  )}
                {mode === 'local' &&
                  field(
                    'Z_AI_VISION_MODEL',
                    'Vision model',
                    'Optional: model identifier supported by your vision endpoint.'
                  )}
                {mode === 'local' &&
                  field(
                    'WEB_SEARCH_MCP_URL',
                    'Web research MCP URL',
                    'Fact checking calls a hosted Web Search service even in local mode. Enter the endpoint supplied with its token.'
                  )}
                {mode === 'local' &&
                  field(
                    'WEB_SEARCH_BEARER',
                    'Web research access token',
                    'Required for fact checking: a separate Web Search access token, not the Documents token.',
                    true
                  )}
                {mode === 'local' &&
                  field(
                    'SERPER_API_KEY',
                    'Fact-check search API key',
                    'Serper key used by the web research service to verify document claims.',
                    true
                  )}
              </>
            )}
          </div>
        </details>
        <section
          className="rounded-lg bg-lz-surface-2 p-3 text-sm space-y-2"
          aria-label="Feature requirements"
        >
          <p className="font-medium">Before you use it</p>
          {web ? (
            <>
              <p>Page extraction: no Serper key needed.</p>
              <p>
                Web search:{' '}
                {hasKey('SERPER_API_KEY')
                  ? 'key configured; run a search below to verify it.'
                  : 'needs your Serper key. Get one at serper.dev.'}
              </p>
            </>
          ) : (
            <>
              <p>Document creation and text reading: no vision key needed.</p>
              <p>
                Scanned pages and images:{' '}
                {hasKey('Z_AI_API_KEY')
                  ? 'vision key configured; OCR has not been tested.'
                  : 'need a vision API key.'}
              </p>
              <p>
                {mode === 'local'
                  ? 'Fact checking needs a Web Search endpoint, its access token and a Serper key.'
                  : 'Fact checking needs a separate Web Search token and Serper key as tool inputs. The Documents token does not authorize Web Search.'}
              </p>
            </>
          )}
          {mode === 'hosted' && (
            <p>
              Created files and cached sources stay on the server. Read them through the MCP tools
              or use a returned download link. Choose local mode for files on this computer.
            </p>
          )}
        </section>
        <div className="flex flex-wrap gap-2">
          <Button
            variant="primary"
            icon={<Save />}
            onClick={() =>
              perform(async () => {
                if (mode === 'hosted') {
                  const hosted = hostedSettings(entry, saved, values);
                  for (const [key, value] of Object.entries(hosted.secrets))
                    await upsert(key, value, true);
                  await addExtension(entry.name, hosted.config, saved?.enabled ?? true);
                } else await addExtension(entry.name, config(), saved?.enabled ?? true);
                setValues({});
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
                  `Connected · ${discovered.tools.length} tools discovered. Tool execution and provider credentials have not been tested.`
                );
              })
            }
          >
            Test connection
          </Button>
          {saved && (
            <Button
              disabled={dirty}
              onClick={() =>
                perform(async () => {
                  await setExtensionEnabled(
                    saved.configKey ?? nameToKey(entry.name),
                    !saved.enabled
                  );
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
        {web && mode === 'local' && (
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
        {web && (
          <section className="space-y-3 border-t border-lz-border pt-5">
            <h3 className="text-sm font-semibold">Try a real search</h3>
            <p className="text-xs text-lz-ink-2">
              Runs your query through the saved MCP and uses your Serper search allowance. Review
              the returned sources; tool discovery alone does not verify search.
            </p>
            <label className="block text-sm">
              Search query
              <input
                className={`${input} mt-1.5`}
                value={query}
                onChange={(e) => setQuery(e.target.value)}
              />
            </label>
            <Button
              disabled={!saved || dirty || !query.trim() || !hasKey('SERPER_API_KEY')}
              onClick={() =>
                perform(async () => {
                  const checked = await inspectConfigExtension(
                    entry.name,
                    undefined,
                    false,
                    query.trim()
                  );
                  setProbeOutput(checked.probeOutput ?? 'No output returned.');
                  setStatus(
                    'Search returned. Review its results below; an empty result does not verify the key.'
                  );
                })
              }
            >
              Run test search
            </Button>
          </section>
        )}
        {probeOutput && (
          <pre className="max-h-72 overflow-auto whitespace-pre-wrap break-words rounded-lg bg-lz-surface-2 p-3 text-xs">
            {probeOutput}
          </pre>
        )}
        {loading && <p role="status">Loading saved settings…</p>}
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
        Choose where each server runs, add your own credentials, then verify the features you need.
        Local tools are included; hosted access tokens are issued separately.
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
