/** Controller protocol 1. Keep snake_case in sync with leanzero-link/controller_wire.rs. */
export type WorkKind = 'chat' | 'agent_work' | 'benchmark' | 'shell_job';
export interface WorkRef {
  owner_node_id: string;
  kind: WorkKind;
  work_id: string;
}
export type WorkAction =
  | 'send'
  | 'steer'
  | 'stop'
  | 'answer_input'
  | 'note'
  | 'pause'
  | 'resume'
  | 'tick';
export type WorkState =
  | 'idle'
  | 'running'
  | 'waiting_input'
  | 'paused'
  | 'finished'
  | 'failed'
  | 'interrupted'
  | 'deleted';
export interface CatalogEntry {
  work: WorkRef;
  store_generation: string;
  revision: number;
  title: string;
  working_dir: string | null;
  state: WorkState;
  current_run_id: string | null;
  capabilities: WorkAction[];
  parent: WorkRef | null;
  related: WorkRef[];
  updated_at: string;
  last_event: { kind: string; text: string } | null;
  archived: boolean;
}
export interface HostDescriptor {
  owner_node_id: string;
  name: string;
  device_role: 'host';
  protocol_version: number;
  availability: 'online' | 'offline' | 'update_required';
  last_seen_at: string | null;
  capabilities: string[];
}
export interface CatalogCursor {
  owner_node_id: string;
  store_generation: string;
  revision: number;
}
export interface HistoryItem {
  message_id: string;
  revision: number;
  message: unknown;
}
export interface InputOption {
  option_id: string;
  label: string;
  accepts_value: boolean;
}
export interface PendingInput {
  input_id: string;
  run_id: string | null;
  prompt: string;
  options: InputOption[];
  payload:
    | {
        kind: 'permission';
        request_id: string;
        tool_name: string;
        arguments: Record<string, unknown>;
      }
    | { kind: 'form'; schema: unknown }
    | {
        kind: 'question';
        item_id: string;
        why: string;
        recommended_answer: string;
        origin_run_id: string | null;
      };
}
export interface WorkSnapshot {
  live_messages: HistoryItem[];
  work_cursor: string;
  entry: CatalogEntry;
  pending_inputs: PendingInput[];
  history_watermark: CatalogCursor;
}
export interface HistoryPage {
  work_cursor: string;
  work: WorkRef;
  store_generation: string;
  revision: number;
  items: HistoryItem[];
  next_page_token: string | null;
}
export interface WorkFrame {
  resume_cursor: string;
  work: WorkRef;
  store_generation: string;
  stream_epoch: string;
  sequence: number;
  work_revision: number;
  run_id: string | null;
  type: string;
  payload: unknown;
}
export type CommandAction =
  | { action: 'send' | 'steer'; payload: { content: Array<{ type: 'text'; text: string }> } }
  | { action: 'stop' | 'pause' | 'resume' | 'tick'; payload: Record<string, never> }
  | { action: 'note'; payload: { text: string } }
  | {
      action: 'answer_input';
      payload: { input_id: string; option_id: string; value: string | null };
    };
export type WorkCommand = CommandAction & {
  command_id: string;
  work: WorkRef;
  expected_store_generation: string;
  expected_revision: number;
  expected_run_id: string | null;
};
export interface CommandReceipt {
  command_id: string;
  work: WorkRef;
  authenticated_device_id: string;
  revision: number;
  state: 'accepted' | 'waiting' | 'dispatching' | 'delivered' | 'rejected' | 'outcome_unknown';
  run_id: string | null;
  message_id: string | null;
  input_id: string | null;
  checkpoint_id: string | null;
  reason: string | null;
  updated_at: string;
}
export function workKey(work: WorkRef): string {
  return `${work.owner_node_id}/${work.kind}/${work.work_id}`;
}
