import { beforeEach, describe, expect, it, vi } from 'vitest';
import { getAcpClient } from '../acpConnection';
import { MlxMountRefusedError, mlxEngineMount, type MlxMountRefusal } from '../mlx-engine';
import { mlxErrorMessage } from '../../components/leanzero-swarm/mlxErrorMessage';

vi.mock('../acpConnection', () => ({
  getAcpClient: vi.fn(),
}));

const REFUSAL: MlxMountRefusal = {
  fit: {
    modelId: 'rapid-mlx/Qwen3.8-Flash-Next-4bit',
    verdict: 'block',
    message: 'needs 47.1 GiB, the budget is 31.2 GiB — short 15.9 GiB',
    shortBytes: 17_072_495_001,
  },
  badge: { kind: 'needsBothMacs' },
};

function answering(response: unknown) {
  const extMethod = vi.fn().mockResolvedValue(response);
  vi.mocked(getAcpClient).mockResolvedValue({ extMethod } as never);
  return extMethod;
}

/**
 * goose answers a gate refusal as a RESULT (`{ refusal }`), not an error. Every caller of
 * `mlxEngineMount` (the Providers view, the tray, the no-node notice) awaited a void, so a refusal
 * read as "mounting started" would be a silent failure; it is thrown with the fit rule's words.
 */
describe('mlxEngineMount — a refusal is never read as a started mount', () => {
  beforeEach(() => vi.clearAllMocks());

  it('mounting started: resolves', async () => {
    const ext = answering({});
    await expect(mlxEngineMount('m')).resolves.toBeUndefined();
    expect(ext).toHaveBeenCalledWith('_goose/unstable/mlxEngine/mount', { modelId: 'm' });
  });

  it('an older goose answering nothing: resolves (it errored on refusal then)', async () => {
    answering(null);
    await expect(mlxEngineMount('m')).resolves.toBeUndefined();
  });

  it('refused: throws the structured refusal, its message the fit rule’s own words', async () => {
    answering({ refusal: REFUSAL });
    const error = await mlxEngineMount('m').catch((e: unknown) => e);
    expect(error).toBeInstanceOf(MlxMountRefusedError);
    expect((error as MlxMountRefusedError).refusal).toEqual(REFUSAL);
    // what every existing catch arm shows
    expect(mlxErrorMessage(error, 'Mount failed.')).toBe(REFUSAL.fit.message);
  });
});
