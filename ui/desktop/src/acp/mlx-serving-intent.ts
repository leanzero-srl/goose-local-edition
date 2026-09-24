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
}

export async function mlxServingIntent(): Promise<MlxServingIntentRead> {
  const client = await getAcpClient();
  const response = (await client.extMethod('_goose/unstable/mlxEngine/servingIntent', {})) as {
    intent?: MlxServingIntent | null;
    error?: string | null;
  };
  return { intent: response.intent ?? null, error: response.error ?? null };
}
