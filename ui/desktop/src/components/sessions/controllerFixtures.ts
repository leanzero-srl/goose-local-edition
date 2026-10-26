import type {
  CatalogEntry,
  HistoryItem,
  HostDescriptor,
  WorkFrame,
  WorkSnapshot,
} from '../../acp/controllerTypes';

export const fixtureHost: HostDescriptor = {
  owner_node_id: 'owner-a',
  name: 'Studio',
  device_role: 'host',
  protocol_version: 1,
  availability: 'online',
  last_seen_at: null,
  capabilities: [
    'catalog.read',
    'history.read',
    'work.send',
    'work.steer',
    'work.stop',
    'input.answer',
  ],
};
export const fixtureEntry: CatalogEntry = {
  work: { owner_node_id: 'owner-a', kind: 'chat', work_id: 'work-a' },
  store_generation: 'generation-a',
  revision: 4,
  title: 'Original session',
  working_dir: '/Users/owner/project',
  state: 'running',
  current_run_id: 'run-a',
  capabilities: ['send', 'steer', 'stop', 'answer_input'],
  parent: null,
  related: [],
  updated_at: '2026-10-26T10:00:00Z',
  last_event: null,
  archived: false,
};
export function fixtureMessage(id = 'message-a', revision = 3, text = 'Saved answer'): HistoryItem {
  return {
    message_id: id,
    revision,
    message: {
      id,
      role: 'assistant',
      created: 1,
      content: [{ type: 'text', text }],
      metadata: { userVisible: true, agentVisible: true },
    },
  };
}
export function fixtureSnapshot(): WorkSnapshot {
  return {
    entry: fixtureEntry,
    live_messages: [],
    pending_inputs: [],
    work_cursor: 'cursor-4',
    history_watermark: { owner_node_id: 'owner-a', store_generation: 'generation-a', revision: 4 },
  };
}
export function fixtureFrame(type: string, sequence: number, payload: unknown): WorkFrame {
  return {
    work: fixtureEntry.work,
    store_generation: 'generation-a',
    stream_epoch: 'generation-a',
    sequence,
    work_revision: sequence,
    run_id: 'run-a',
    type,
    payload,
    resume_cursor: `cursor-${sequence}`,
  };
}
