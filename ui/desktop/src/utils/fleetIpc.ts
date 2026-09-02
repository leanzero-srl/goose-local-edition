import {
  FLEET_CHAT_TIMEOUT_MS,
  FLEET_PROBE_TIMEOUT_MS,
  lmStudioApiToken,
  postFleetChat,
  probeFleetModels,
  type FetchLike,
  type FleetChatResult,
  type FleetProbeResult,
} from './fleetProbe';

/**
 * The bodies of main's `fleet-probe` and `fleet-chat` IPC handlers, kept out of main.ts so both can run
 * under a fake fetch. Both carry LM Studio's API token from the SAME source: `LMSTUDIO_API_KEY` in
 * main's environment (utils/fleetProbe.ts `lmStudioApiToken`), the key the engine's own probes and
 * chat path read first. There is no other source a desktop process can reach: goose's secret store
 * answers every read from the renderer MASKED (acp/server/config.rs `on_config_read` → `mask_secret`
 * for `is_secret`; goosed's /config/read the same), so a token that lives only in the store leaves both
 * calls bare and the server's 401 is the typed `http` error naming the key — never `unreachable`, and
 * never a masked string sent as a credential. The token is read per call and never logged.
 */
export type FleetTokenSource = () => string | null;

export function fleetProbeHandler(
  fetchImpl: FetchLike,
  token: FleetTokenSource = () => lmStudioApiToken()
): (_event: unknown, endpoint: unknown) => Promise<FleetProbeResult> {
  return (_event, endpoint) =>
    probeFleetModels(
      typeof endpoint === 'string' ? endpoint : '',
      fetchImpl,
      FLEET_PROBE_TIMEOUT_MS,
      token()
    );
}

export function fleetChatHandler(
  fetchImpl: FetchLike,
  token: FleetTokenSource = () => lmStudioApiToken()
): (_event: unknown, endpoint: unknown, body: unknown) => Promise<FleetChatResult> {
  return (_event, endpoint, body) =>
    postFleetChat(
      typeof endpoint === 'string' ? endpoint : '',
      body,
      fetchImpl,
      FLEET_CHAT_TIMEOUT_MS,
      token()
    );
}
