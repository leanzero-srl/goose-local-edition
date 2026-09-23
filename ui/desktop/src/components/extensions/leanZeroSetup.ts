import type { ExtensionConfig } from '../../types/extensions';
import type { FixedExtensionEntry } from '../ConfigContext';

export type SetupMode = 'local' | 'hosted';
export type BundledMcp = Awaited<ReturnType<typeof window.electron.bundledMcps>>[number];
export const BUNDLED_NAMES = ['LeanZero Web Search', 'LeanZero Documents'];
export const hostedFields = (web: boolean) => ({
  ACCESS_TOKEN: 'Authorization',
  ...(web
    ? { SERPER_API_KEY: 'X-Serper-Key', GITHUB_TOKEN: 'X-GitHub-Token' }
    : { Z_AI_API_KEY: 'X-ZAI-Key' }),
});
export const secretName = (web: boolean, field: string) =>
  `LEANZERO_HOSTED_${web ? 'WEB' : 'DOCUMENTS'}_${field}`;
export const setupLinks = (web: boolean) => ({
  endpoint: `https://worksmacstudio.tailfc4700.ts.net/${web ? 'websearch' : 'docproc'}/mcp`,
  guide: `https://leanzero.net/portfolio/${web ? 'mcp-web-search' : 'mcp-doc-processor'}`,
});
export function savedCredential(
  saved: FixedExtensionEntry | undefined,
  mode: SetupMode,
  web: boolean,
  field: string
) {
  if (!saved) return false;
  if (mode === 'local') return saved.type === 'stdio' && Boolean(saved.env_keys?.includes(field));
  if (saved.type !== 'streamable_http') return false;
  const header = hostedFields(web)[field as keyof ReturnType<typeof hostedFields>];
  return Boolean(
    header &&
    Object.entries(saved.headers ?? {}).some(
      ([name, value]) => name.toLowerCase() === header.toLowerCase() && value
    )
  );
}
export function validateFolder(folder: string) {
  if (!/^(?:\/|[A-Za-z]:[\\/]|\\\\)/.test(folder)) {
    throw new Error(
      'Choose a folder or enter its full absolute path. Relative paths and ~ are not supported.'
    );
  }
}
export function mergeMcpSettings(
  entry: BundledMcp,
  saved: FixedExtensionEntry | undefined,
  values: Record<string, string>
) {
  const local = saved?.type === 'stdio' ? saved : undefined;
  const envs = { ...local?.envs, ...entry.envs };
  for (const [key, value] of Object.entries(values)) {
    if (value.trim()) envs[key] = value.trim();
    else delete envs[key];
  }
  const envKeys = (local?.env_keys ?? []).filter(
    (key) => !(key in values) || Boolean(values[key].trim())
  );
  return {
    ...entry,
    ...local,
    env_keys: envKeys,
    type: 'stdio' as const,
    cmd: entry.cmd,
    args: entry.args,
    envs,
  };
}
export function hostedSettings(
  entry: BundledMcp,
  saved: FixedExtensionEntry | undefined,
  values: Record<string, string>
) {
  const web = entry.name === BUNDLED_NAMES[0];
  const url = new URL(values.ENDPOINT || setupLinks(web).endpoint);
  if (url.protocol !== 'https:' || url.username || url.password || url.search || url.hash) {
    throw new Error(
      'Use an HTTPS MCP endpoint without credentials, query parameters or a fragment. Enter keys in the protected fields below.'
    );
  }
  const previous = saved?.type === 'streamable_http' ? saved : undefined;
  // Keys saved for one host must never be forwarded when the endpoint changes.
  const sameEndpoint = previous?.uri === url.href;
  const headers: Record<string, string> = sameEndpoint ? { ...previous.headers } : {};
  const secrets: Record<string, string> = {};
  for (const [field, header] of Object.entries(hostedFields(web))) {
    if (!(field in values)) continue;
    for (const name of Object.keys(headers))
      if (name.toLowerCase() === header.toLowerCase()) delete headers[name];
    const value = values[field].trim();
    if (value) {
      if (/[\r\n]/.test(value)) throw new Error('Keys cannot contain line breaks.');
      const key = secretName(web, field);
      secrets[key] = field === 'ACCESS_TOKEN' ? value.replace(/^Bearer\s+/i, '') : value;
      headers[header] = `${field === 'ACCESS_TOKEN' ? 'Bearer ' : ''}\${${key}}`;
    }
  }
  if (
    !Object.entries(headers).some(
      ([name, value]) => name.toLowerCase() === 'authorization' && value
    )
  ) {
    throw new Error(
      'Enter the access token issued for this MCP. When changing the endpoint, enter its token again.'
    );
  }
  const folder =
    values.HOSTED_OUTPUT_DIR?.trim() ??
    (sameEndpoint ? previous?.headers?.['X-Output-Dir'] : undefined);
  if (folder) {
    if (!/^[a-zA-Z0-9_-]+$/.test(folder))
      throw new Error('Use letters, numbers, hyphens or underscores for the hosted folder name.');
    headers['X-Output-Dir'] = folder;
  } else delete headers['X-Output-Dir'];
  const envKeys = [
    ...new Set(
      Object.values(headers).flatMap((value) =>
        [...value.matchAll(/\$\{(\w+)\}/g)].map((match) => match[1])
      )
    ),
  ];
  const config: ExtensionConfig = {
    type: 'streamable_http',
    name: entry.name,
    description: entry.description,
    uri: url.href,
    headers,
    env_keys: envKeys,
    timeout: entry.timeout,
  };
  return { config, secrets };
}

export function validateLocalIntegrations(
  web: boolean,
  values: Record<string, string>,
  saved?: FixedExtensionEntry
) {
  const has = (key: string) =>
    key in values ? Boolean(values[key].trim()) : savedCredential(saved, 'local', web, key);
  if (web) return;
  if (has('Z_AI_API_KEY') && !values.Z_AI_BASE_URL?.trim()) {
    throw new Error(
      'Enter the vision endpoint that matches your API key. Z.AI and BigModel use different endpoints.'
    );
  }
  const factCheck = ['WEB_SEARCH_MCP_URL', 'WEB_SEARCH_BEARER', 'SERPER_API_KEY'];
  if (factCheck.some(has) && !factCheck.every(has)) {
    throw new Error(
      'Fact checking needs all three: Web Search endpoint, Web Search access token and Serper key.'
    );
  }
  for (const key of ['Z_AI_BASE_URL', 'WEB_SEARCH_MCP_URL']) {
    if (!values[key]?.trim()) continue;
    const url = new URL(values[key]);
    if (url.protocol !== 'https:' || url.username || url.password || url.search || url.hash) {
      throw new Error('Use HTTPS service URLs without embedded credentials or query parameters.');
    }
  }
}
