/**
 * Where the OFFICIAL OpenAI tile's requests really go. The engine's `openai` provider still honours
 * OPENAI_BASE_URL / OPENAI_HOST / OPENAI_BASE_PATH (crates/goose/src/providers/openai_def.rs
 * `resolve_base_url`), so a value saved there by an older build or `goose configure` silently turns
 * the "OpenAI" tile into some other server. The tile shows only the key; this module reads those
 * saved values and names every one that points away from api.openai.com, so the tile can say so
 * loudly instead of ignoring it. Any other OpenAI-speaking server belongs on an OpenAI-compatible
 * endpoint of its own.
 */

/** The official API — what the tile resets the endpoint fields to (the engine's own defaults:
 *  `resolve_base_url`'s fallback host and `OPEN_AI_DEFAULT_BASE_PATH`). */
export const OPENAI_OFFICIAL = {
  OPENAI_HOST: 'https://api.openai.com',
  OPENAI_BASE_URL: 'https://api.openai.com/v1',
  OPENAI_BASE_PATH: 'v1/chat/completions',
} as const;

type EndpointKey = keyof typeof OPENAI_OFFICIAL;
const ENDPOINT_KEYS = Object.keys(OPENAI_OFFICIAL) as EndpointKey[];

export interface SavedField {
  key: string;
  value?: string | null;
}

export interface EndpointOverride {
  key: EndpointKey;
  value: string;
}

/** The engine's `is_direct_openai_host`: the hostname is exactly api.openai.com or a regional
 *  `*.api.openai.com`. Anything unparseable is not the official API. */
export function isOfficialOpenAiHost(url: string): boolean {
  try {
    const host = new URL(url).hostname.toLowerCase();
    return host === 'api.openai.com' || host.endsWith('.api.openai.com');
  } catch {
    return false;
  }
}

function isOfficial(key: EndpointKey, value: string): boolean {
  if (key === 'OPENAI_BASE_PATH') {
    return value.replace(/^\/+/, '') === OPENAI_OFFICIAL.OPENAI_BASE_PATH;
  }
  return isOfficialOpenAiHost(value);
}

/** Every saved endpoint field that sends the official tile somewhere other than the official API,
 *  with the value verbatim. Empty = the tile really is OpenAI. */
export function openAiEndpointOverrides(saved: readonly SavedField[]): EndpointOverride[] {
  const overrides: EndpointOverride[] = [];
  for (const key of ENDPOINT_KEYS) {
    const value = saved.find((field) => field.key === key)?.value?.trim();
    if (value && !isOfficial(key, value)) overrides.push({ key, value });
  }
  return overrides;
}

/** The fields a Connect on the official tile writes so the key is proven against api.openai.com and
 *  chat goes there afterwards: each overridden field set back to the official value. (The engine's
 *  save refuses an empty value, so a field is reset, never blanked.) */
export function officialEndpointReset(
  overrides: readonly EndpointOverride[]
): { key: string; value: string }[] {
  return overrides.map(({ key }) => ({ key, value: OPENAI_OFFICIAL[key] }));
}
