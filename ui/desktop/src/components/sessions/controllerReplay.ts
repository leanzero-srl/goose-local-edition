import type {
  CatalogEntry,
  HistoryItem,
  HistoryPage,
  PendingInput,
  WorkFrame,
  WorkSnapshot,
} from '../../acp/controllerTypes';
import { workKey } from '../../acp/controllerTypes';
import type { Message } from '../../types/message';
import { objectValue } from './controllerForm';

export interface ControllerWorkState {
  entry: CatalogEntry;
  items: HistoryItem[];
  partialIds: string[];
  pendingInputs: PendingInput[];
  cursor: string;
  sequence: number;
  historyRevision: number | null;
  nextHistoryPageToken: string | null;
  refreshSnapshot: boolean;
  deleted: boolean;
}
export function transcriptMessage(item: HistoryItem): Message {
  const message = objectValue(item.message);
  if (
    !message ||
    !['user', 'assistant'].includes(String(message.role)) ||
    !Array.isArray(message.content) ||
    typeof message.created !== 'number'
  ) {
    throw new Error('The owner returned an unreadable transcript message.');
  }
  return { ...message, id: item.message_id } as Message;
}
function mergeItems(items: HistoryItem[], added: HistoryItem[]): HistoryItem[] {
  const byId = new Map(items.map((item) => [item.message_id, item]));
  for (const item of added) {
    transcriptMessage(item);
    if (!byId.has(item.message_id) || byId.get(item.message_id)!.revision <= item.revision)
      byId.set(item.message_id, item);
  }
  return [...byId.values()].sort(
    (a, b) => transcriptMessage(a).created - transcriptMessage(b).created
  );
}
function mergeLogItems(items: HistoryItem[], added: HistoryItem[]): HistoryItem[] {
  return mergeItems(items, added)
    .map((item) => {
      const range = /^log_(\d+)_(\d+)$/.exec(item.message_id);
      if (!range) throw new Error('The owner returned a log chunk without its byte range.');
      return { item, offset: Number(range[1]) };
    })
    .sort((a, b) => a.offset - b.offset)
    .map(({ item }) => item);
}
export function workSnapshot(snapshot: WorkSnapshot): ControllerWorkState {
  const { entry, history_watermark: mark } = snapshot;
  if (
    entry.work.owner_node_id !== mark.owner_node_id ||
    entry.store_generation !== mark.store_generation ||
    !snapshot.work_cursor
  )
    throw new Error('The owner returned an inconsistent work snapshot.');
  return {
    entry,
    items: mergeItems([], snapshot.live_messages),
    partialIds: snapshot.live_messages.map((item) => item.message_id),
    pendingInputs: snapshot.pending_inputs,
    cursor: snapshot.work_cursor,
    sequence: mark.revision,
    historyRevision: null,
    nextHistoryPageToken: null,
    refreshSnapshot: false,
    deleted: false,
  };
}
export function addHistory(state: ControllerWorkState, page: HistoryPage): ControllerWorkState {
  if (
    workKey(state.entry.work) !== workKey(page.work) ||
    state.entry.store_generation !== page.store_generation ||
    (state.historyRevision !== null && state.historyRevision !== page.revision)
  )
    throw new Error('History changed identity or watermark between pages. Reload this work.');
  const committed = new Set(page.items.map((item) => item.message_id));
  const durable = state.items.filter((item) => !state.partialIds.includes(item.message_id));
  const partials = state.items.filter((item) => state.partialIds.includes(item.message_id));
  const items =
    state.entry.work.kind === 'chat'
      ? mergeItems(mergeItems(durable, page.items), partials)
      : mergeLogItems(state.items, page.items);
  return {
    ...state,
    // The history pages retain the owner's timestamp/row order. Live chunks come after those
    // rows for timestamp ties, including a user message and its answer in the same second.
    items,
    partialIds: state.partialIds.filter((id) => !committed.has(id)),
    historyRevision: page.revision,
    nextHistoryPageToken: page.next_page_token,
  };
}
export function applyWorkFrame(state: ControllerWorkState, frame: WorkFrame): ControllerWorkState {
  if (
    workKey(frame.work) !== workKey(state.entry.work) ||
    frame.store_generation !== state.entry.store_generation ||
    frame.stream_epoch !== state.entry.store_generation
  )
    throw new Error('This work changed owner or generation. Reload before continuing.');
  if (frame.sequence <= state.sequence) return state;
  if (!frame.resume_cursor) throw new Error('The owner omitted the work replay cursor.');
  const next = {
    ...state,
    cursor: frame.resume_cursor,
    sequence: frame.sequence,
    entry: { ...state.entry, revision: frame.work_revision, current_run_id: frame.run_id },
  };
  const payload = objectValue(frame.payload);
  if (frame.type === 'tombstone')
    return {
      ...next,
      deleted: true,
      items: [],
      pendingInputs: [],
      partialIds: [],
      nextHistoryPageToken: null,
      refreshSnapshot: false,
    };
  // A complete history snapshot is authoritative even for messages absent from it. Applying an
  // earlier upsert/delete can resurrect a deleted message or remove a subsequently renamed one.
  if (
    state.historyRevision !== null &&
    frame.sequence <= state.historyRevision &&
    ['message_upsert', 'message_deleted'].includes(frame.type)
  )
    return next;
  if (frame.type === 'message_upsert') {
    const item = payload?.item as HistoryItem | undefined;
    if (!item?.message_id) throw new Error('A transcript update omitted its message identity.');
    return {
      ...next,
      items: mergeItems(
        state.items.filter((old) => old.message_id !== payload?.previous_message_id),
        [item]
      ),
      partialIds: state.partialIds.filter(
        (id) => id !== item.message_id && id !== payload?.previous_message_id
      ),
    };
  }
  if (frame.type === 'message_deleted')
    return {
      ...next,
      items: state.items.filter((item) => item.message_id !== payload?.message_id),
      partialIds: state.partialIds.filter((id) => id !== payload?.message_id),
    };
  if (frame.type === 'partial_discarded') {
    if (!Array.isArray(payload?.message_ids))
      throw new Error('A partial discard omitted its message identities.');
    const ids = new Set(payload.message_ids);
    return {
      ...next,
      items: state.items.filter(
        (item) => !state.partialIds.includes(item.message_id) || !ids.has(item.message_id)
      ),
      partialIds: state.partialIds.filter((id) => !ids.has(id)),
    };
  }
  if (frame.type === 'partial_replacement') {
    const id = payload?.message_id;
    const revision = payload?.message_revision;
    if (
      typeof id !== 'string' ||
      typeof revision !== 'number' ||
      typeof payload?.offset_bytes !== 'number'
    )
      throw new Error('A partial update omitted its replay position.');
    const old = state.items.find((item) => item.message_id === id);
    if (old && old.revision >= revision) return next;
    // A later durable history page wins over partials replayed from the earlier work snapshot.
    if (old && !state.partialIds.includes(id)) return next;
    const chunk = transcriptMessage({ message_id: id, revision, message: payload.content });
    const before = old ? transcriptMessage(old) : null;
    const textBytes = before
      ? new TextEncoder().encode(
          before.content
            .filter((part) => part.type === 'text')
            .map((part) => part.text)
            .join('')
        ).length
      : 0;
    if (textBytes !== payload.offset_bytes) return { ...next, refreshSnapshot: true };
    const content = [...(before?.content ?? [])];
    for (const part of chunk.content) {
      const last = content[content.length - 1];
      if (part.type === 'text' && last?.type === 'text')
        content[content.length - 1] = { ...last, text: last.text + part.text };
      else content.push(part);
    }
    const item = { message_id: id, revision, message: { ...chunk, content } };
    return {
      ...next,
      items: mergeItems(state.items, [item]),
      partialIds: [...new Set([...state.partialIds, id])],
    };
  }
  const entry = payload?.entry as CatalogEntry | undefined;
  if (entry) {
    if (
      workKey(entry.work) !== workKey(state.entry.work) ||
      entry.store_generation !== state.entry.store_generation
    )
      throw new Error('A state update changed work identity.');
    next.entry = entry;
  }
  if (entry && ['session_changed', 'run_state'].includes(frame.type)) {
    return {
      ...next,
      pendingInputs: entry.current_run_id === state.entry.current_run_id ? state.pendingInputs : [],
    };
  }
  // Inputs contain original operation details only in the owner snapshot. Never retain an old
  // permission while waiting for its new snapshot, and never claim an unknown frame was applied.
  return { ...next, pendingInputs: [], refreshSnapshot: true };
}
