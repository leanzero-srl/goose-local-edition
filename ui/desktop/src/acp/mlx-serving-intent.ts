import type { MlxServingIntentDto } from '@aaif/goose-sdk';
import { getAcpClient } from './acpConnection';

/**
 * What the owner last started serving MLX chat from on this Mac and did not stop
 * (`_goose/unstable/mlxEngine/servingIntent`) — the thing a relaunch brings back. goose writes it
 * on the owner's own starts and removes it on the matching explicit stop; an app quit, an update
 * or a goosed exit never touches it.
 */
export type MlxServingIntent = MlxServingIntentDto;

export interface MlxServingIntentRead {
  /** null = nothing to bring back. */
  intent: MlxServingIntent | null;
  /** The record exists and could not be read — named, never "nothing to restore". */
  error: string | null;
  /**
   * The owner's switch `GOOSE_MLX_RESTORE_AT_LAUNCH` (Settings › App, default off): false = the
   * launch brings nothing back and shows nothing; goose keeps recording starts either way.
   */
  restoreAtLaunch: boolean;
}

/**
 * `atLaunch`: the launch restore's own read — the only one that lets goose forget a record whose
 * model folder is gone (with the switch on). Every other reader gets the record as it stands.
 */
export async function mlxServingIntent(
  options: { atLaunch?: boolean } = {}
): Promise<MlxServingIntentRead> {
  const client = await getAcpClient();
  const params = options.atLaunch ? { atLaunch: true } : {};
  const response = (await client.extMethod('_goose/unstable/mlxEngine/servingIntent', params)) as {
    intent?: MlxServingIntent | null;
    error?: string | null;
    restoreAtLaunch?: boolean;
  };
  return {
    intent: response.intent ?? null,
    error: response.error ?? null,
    restoreAtLaunch: response.restoreAtLaunch === true,
  };
}
