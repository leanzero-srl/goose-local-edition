import type { UnreadableConfigFile } from '@aaif/goose-sdk';
import { getAcpClient } from './acpConnection';

export type { UnreadableConfigFile };

export type ConfigReadValue = unknown;

export async function acpReadConfig(
  key: string,
  isSecret: boolean = false
): Promise<ConfigReadValue> {
  const client = await getAcpClient();
  const { value } = await client.goose.configRead_unstable({ key, isSecret });
  if (value == null) {
    return null;
  }
  if (isSecret) {
    return { maskedValue: value as string };
  }
  return value;
}

export async function acpUpsertConfig(
  key: string,
  value: unknown,
  isSecret: boolean = false
): Promise<void> {
  const client = await getAcpClient();
  await client.goose.configUpsert_unstable({ key, value, isSecret });
}

export async function acpRemoveConfig(key: string, isSecret: boolean): Promise<void> {
  const client = await getAcpClient();
  await client.goose.configRemove_unstable({ key, isSecret });
}

export interface ConfigReadAll {
  config: Record<string, unknown>;
  /** Settings files goosed skipped because it could not read them (Q-468). */
  unreadableFiles: UnreadableConfigFile[];
}

export async function acpReadAllConfig(): Promise<ConfigReadAll> {
  const client = await getAcpClient();
  const { config, unreadableFiles } = await client.goose.configReadAll_unstable({});
  return { config, unreadableFiles: unreadableFiles ?? [] };
}

/** Renames an unreadable settings file to `<name>.corrupt-<utc>`; returns where it went. */
export async function acpMoveConfigAside(path: string): Promise<string> {
  const client = await getAcpClient();
  const { movedTo } = await client.goose.configMoveAside_unstable({ path });
  return movedTo;
}
