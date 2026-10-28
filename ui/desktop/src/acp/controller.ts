import { getAcpClient } from './acpConnection';
import type {
  CatalogEntry,
  CommandReceipt,
  HistoryPage,
  HostDescriptor,
  WorkCommand,
  WorkFrame,
  WorkRef,
  WorkSnapshot,
} from './controllerTypes';

export interface ControllerStatus {
  state: 'signed_out' | 'ready' | 'unavailable';
  device: {
    device_id: string;
    name: string;
    platform: string;
    role: 'controller';
    revision: number;
    scopes: string[];
    created_at: string;
    revoked_at: string | null;
  } | null;
  error: string | null;
}
export interface ControllerHostCatalog {
  host: HostDescriptor;
  entries: CatalogEntry[];
  stale: boolean;
  error: string | null;
}
export interface ControllerCatalog {
  hosts: ControllerHostCatalog[];
  updated_at: string | null;
}
export interface WorkChanges {
  frames: WorkFrame[];
  resume_cursor: string;
  has_more: boolean;
}
async function call<T>(method: string, params: Record<string, unknown> = {}): Promise<T> {
  return (await (
    await getAcpClient()
  ).extMethod(`_goose/unstable/controller/${method}`, params)) as T;
}
export const controllerStatus = () => call<ControllerStatus>('status');
export const controllerCatalog = () => call<ControllerCatalog>('catalog');
export const controllerSync = () => call<ControllerCatalog>('sync');
export const controllerOpen = (work: WorkRef, store_generation: string) =>
  call<WorkSnapshot>('openWork', { work, store_generation });
export const controllerHistory = (
  work: WorkRef,
  store_generation: string,
  page_token: string | null
) => call<HistoryPage>('history', { work, store_generation, page_token });
export const controllerChanges = (work: WorkRef, store_generation: string, cursor: string) =>
  call<WorkChanges>('workChanges', { work, store_generation, cursor });
export const controllerCommand = (command: WorkCommand) =>
  call<CommandReceipt>('command', { command });
export const controllerReceipt = (owner_node_id: string, command_id: string) =>
  call<CommandReceipt>('receipt', { owner_node_id, command_id });
export interface PendingCommand {
  command: WorkCommand;
  receipt: CommandReceipt | null;
}
export const controllerPendingCommands = (work: WorkRef) =>
  call<{ commands: PendingCommand[] }>('pendingCommands', { work });

export function controllerCommandNotAdmitted(error: unknown): boolean {
  const detail = (error as { data?: { error?: { status?: unknown; code?: unknown } } } | null)?.data
    ?.error;
  return detail?.status === 409 && detail.code === 'stale_revision';
}

export function controllerErrorText(error: unknown): string {
  const data = (error as { data?: unknown } | null)?.data;
  if (typeof data === 'string') return data;
  if (data && typeof data === 'object') {
    const detail = data as { message?: unknown; error?: { message?: unknown } };
    if (typeof detail.error?.message === 'string') return detail.error.message;
    if (typeof detail.message === 'string') return detail.message;
  }
  return error instanceof Error ? error.message : 'The owner Mac could not be reached.';
}
