import { describe, expect, it } from 'vitest';
import { addHistory, applyWorkFrame, transcriptMessage, workSnapshot } from './controllerReplay';
import { fixtureEntry, fixtureFrame, fixtureMessage, fixtureSnapshot } from './controllerFixtures';

describe('selected controller transcript replay', () => {
  it('does not resurrect absent history or undo renames when replay begins before its watermark', () => {
    let state = addHistory(workSnapshot(fixtureSnapshot()), {
      work: fixtureEntry.work,
      store_generation: 'generation-a',
      revision: 12,
      items: [fixtureMessage('current-name', 11, 'Current')],
      next_page_token: null,
      work_cursor: 'history-12',
    });
    state = applyWorkFrame(
      state,
      fixtureFrame('message_upsert', 6, {
        item: fixtureMessage('deleted-at-10', 6, 'Deleted secret'),
        previous_message_id: null,
      })
    );
    state = applyWorkFrame(
      state,
      fixtureFrame('message_upsert', 7, {
        item: fixtureMessage('older-name', 7),
        previous_message_id: 'current-name',
      })
    );
    state = applyWorkFrame(
      state,
      fixtureFrame('message_deleted', 8, { message_id: 'current-name' })
    );
    expect(state.items.map((item) => item.message_id)).toEqual(['current-name']);
    state = applyWorkFrame(
      state,
      fixtureFrame('message_deleted', 13, { message_id: 'current-name' })
    );
    expect(state.items).toEqual([]);
  });

  it('puts owner history rows before live partials when creation timestamps tie', () => {
    const partial = fixtureMessage('assistant', 4, 'Started answer');
    const user = fixtureMessage('user', 3, 'Question');
    (user.message as { role: string }).role = 'user';
    const state = addHistory(workSnapshot({ ...fixtureSnapshot(), live_messages: [partial] }), {
      work: fixtureEntry.work,
      store_generation: 'generation-a',
      revision: 4,
      items: [user],
      next_page_token: null,
      work_cursor: 'history-4',
    });
    expect(state.items.map((item) => item.message_id)).toEqual(['user', 'assistant']);
  });
  it('reconciles UTF-8 partials, duplicate frames and the final committed message', () => {
    let state = workSnapshot(fixtureSnapshot());
    state = applyWorkFrame(
      state,
      fixtureFrame('partial_replacement', 5, {
        message_id: 'm',
        message_revision: 5,
        offset_bytes: 0,
        content: fixtureMessage('m', 5, '🦆').message,
      })
    );
    const frame = fixtureFrame('partial_replacement', 8, {
      message_id: 'm',
      message_revision: 8,
      offset_bytes: 4,
      content: fixtureMessage('m', 8, ' swimming').message,
    });
    state = applyWorkFrame(state, frame);
    state = applyWorkFrame(state, frame);
    expect(transcriptMessage(state.items[0]).content).toEqual([
      { type: 'text', text: '🦆 swimming' },
    ]);
    state = applyWorkFrame(
      state,
      fixtureFrame('message_upsert', 10, {
        item: fixtureMessage('m', 10, '🦆 swimming.'),
        previous_message_id: null,
      })
    );
    expect(state.items).toHaveLength(1);
    expect(state.partialIds).toEqual([]);
    expect(transcriptMessage(state.items[0]).content).toEqual([
      { type: 'text', text: '🦆 swimming.' },
    ]);
  });
  it('keeps a later history watermark and never overwrites committed text with old partials', () => {
    let state = workSnapshot(fixtureSnapshot());
    state = addHistory(state, {
      work: fixtureEntry.work,
      store_generation: 'generation-a',
      revision: 12,
      items: [fixtureMessage('m', 11, 'Final')],
      next_page_token: null,
      work_cursor: 'history-12',
    });
    state = applyWorkFrame(
      state,
      fixtureFrame('partial_replacement', 6, {
        message_id: 'm',
        message_revision: 6,
        offset_bytes: 0,
        content: fixtureMessage('m', 6, 'Before').message,
      })
    );
    expect(transcriptMessage(state.items[0]).content).toEqual([{ type: 'text', text: 'Final' }]);
    expect(state.cursor).toBe('cursor-6');
    expect(() =>
      addHistory(state, {
        work: fixtureEntry.work,
        store_generation: 'generation-a',
        revision: 13,
        items: [],
        next_page_token: null,
        work_cursor: 'wrong',
      })
    ).toThrow('watermark');
  });
  it('resnapshots a gap, invalidates pending inputs and clears tombstoned content', () => {
    let state = workSnapshot(fixtureSnapshot());
    state = applyWorkFrame(
      state,
      fixtureFrame('partial_replacement', 5, {
        message_id: 'm',
        message_revision: 5,
        offset_bytes: 5,
        content: fixtureMessage().message,
      })
    );
    expect(state.refreshSnapshot).toBe(true);
    state = applyWorkFrame(
      state,
      fixtureFrame('pending_input', 6, {
        entry: { ...fixtureEntry, state: 'waiting_input', revision: 6 },
      })
    );
    expect(state.pendingInputs).toEqual([]);
    expect(state.entry.state).toBe('waiting_input');
    state = applyWorkFrame(state, fixtureFrame('tombstone', 7, null));
    expect(state.deleted).toBe(true);
    expect(state.items).toEqual([]);
    expect(state.refreshSnapshot).toBe(false);
    expect(state.nextHistoryPageToken).toBeNull();
  });
  it('resnapshots external log growth without changing the run phase', () => {
    const entry = { ...fixtureEntry, work: { ...fixtureEntry.work, kind: 'shell_job' as const } };
    const state = addHistory(workSnapshot({ ...fixtureSnapshot(), entry }), {
      work: entry.work,
      store_generation: entry.store_generation,
      revision: 4,
      items: [fixtureMessage('log_100_200', 4, 'Current tail')],
      next_page_token: 'older-100',
      work_cursor: 'cursor-4',
    });
    const changed = applyWorkFrame(state, {
      ...fixtureFrame('log_updated', 5, { entry: { ...entry, revision: 5 } }),
      work: entry.work,
    });
    expect(changed.refreshSnapshot).toBe(true);
    expect(changed.entry.state).toBe('running');
    expect(changed.cursor).toBe('cursor-5');
    expect(changed.items).toEqual(state.items);
    expect(changed.nextHistoryPageToken).toBe('older-100');
  });
  it('refuses cross-owner and changed-generation frames even when work IDs collide', () => {
    const state = workSnapshot(fixtureSnapshot());
    const frame = fixtureFrame('run_state', 5, { entry: fixtureEntry });
    expect(() =>
      applyWorkFrame(state, { ...frame, work: { ...frame.work, owner_node_id: 'owner-b' } })
    ).toThrow('owner or generation');
    expect(() => applyWorkFrame(state, { ...frame, store_generation: 'replacement' })).toThrow(
      'owner or generation'
    );
  });
  it('removes renamed, deleted and discarded message identities without touching committed siblings', () => {
    let state = workSnapshot({
      ...fixtureSnapshot(),
      live_messages: [fixtureMessage('partial', 3)],
    });
    state = addHistory(state, {
      work: fixtureEntry.work,
      store_generation: 'generation-a',
      revision: 4,
      items: [fixtureMessage('old', 3)],
      next_page_token: null,
      work_cursor: 'history',
    });
    state = applyWorkFrame(
      state,
      fixtureFrame('message_upsert', 5, {
        item: fixtureMessage('new', 5),
        previous_message_id: 'old',
      })
    );
    state = applyWorkFrame(
      state,
      fixtureFrame('partial_discarded', 6, { message_ids: ['partial', 'new'] })
    );
    expect(state.items.map((item) => item.message_id)).toEqual(['new']);
    state = applyWorkFrame(state, fixtureFrame('message_deleted', 7, { message_id: 'new' }));
    expect(state.items).toEqual([]);
  });
});
