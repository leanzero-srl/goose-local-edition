import {
  FLEET_PROBE_TIMEOUT_MS,
  lmStudioApiToken,
  probeFleetModels,
  type FetchLike,
  type FleetProbeResult,
} from './fleetProbe';

/**
 * The body of main's `fleet-probe` IPC handler, kept out of main.ts so it can run under a fake fetch.
 * It carries LM Studio's API token from `LMSTUDIO_API_KEY` in main's environment (utils/fleetProbe.ts
 * `lmStudioApiToken`), the key the engine's own probes and chat path read first. There is no other source a desktop process can reach: goose's secret store
 * answers every read from the renderer MASKED (acp/server/config.rs `on_config_read` → `mask_secret`
 * for `is_secret`; goosed's /config/read the same), so a token that lives only in the store leaves the
 * probe bare and the server's 401 is the typed `http` error naming the key — never `unreachable`, and
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
