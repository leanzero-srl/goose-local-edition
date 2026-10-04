import type {
  OpenRouterHostDto,
  OpenRouterHostProbeResponse_unstable,
  OpenRouterHostsListResponse_unstable,
  OpenRouterPinDto,
} from '@aaif/goose-sdk';
import { getAcpClient } from './acpConnection';

export type OpenRouterHost = OpenRouterHostDto;
export type OpenRouterHostsList = OpenRouterHostsListResponse_unstable;
export type OpenRouterProbe = OpenRouterHostProbeResponse_unstable;
export type OpenRouterPin = OpenRouterPinDto;

/** The routing pin as config.yaml's OPENROUTER_PARAMETERS holds it now. */
export async function acpReadOpenRouterPin(): Promise<OpenRouterPin> {
  const client = await getAcpClient();
  return (await client.goose.openrouterPinRead_unstable({})).pin;
}

/** Pin every OpenRouter request to `tag` with no fallbacks; `null` removes the pin. */
export async function acpSetOpenRouterPin(tag: string | null): Promise<OpenRouterPin> {
  const client = await getAcpClient();
  return (await client.goose.openrouterPinSet_unstable(tag == null ? {} : { tag })).pin;
}

/** The hosts OpenRouter's endpoints listing names for `model` (author/slug). The engine reads the
 *  saved OpenRouter key; it never crosses into the renderer. */
export async function acpListOpenRouterHosts(model: string): Promise<OpenRouterHostsList> {
  const client = await getAcpClient();
  return client.goose.openrouterHostsList_unstable({ model });
}

/** One tool-carrying call to `model` on exactly host `tag`: speed, tool call, finish, or the error. */
export async function acpProbeOpenRouterHost(model: string, tag: string): Promise<OpenRouterProbe> {
  const client = await getAcpClient();
  return client.goose.openrouterHostsProbe_unstable({ model, tag });
}
