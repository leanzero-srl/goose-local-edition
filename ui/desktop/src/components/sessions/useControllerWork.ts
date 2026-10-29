import { useCallback, useEffect, useRef, useState } from 'react';
import {
  controllerChanges,
  controllerCommand,
  controllerCommandNotAdmitted,
  controllerErrorText,
  controllerHistory,
  controllerHistoryLatest,
  controllerSnapshotRequired,
  controllerOpen,
  controllerPendingCommands,
  controllerReceipt,
  type PendingCommand,
} from '../../acp/controller';
import {
  workKey,
  type CatalogEntry,
  type CommandAction,
  type WorkCommand,
} from '../../acp/controllerTypes';
import {
  addHistory,
  applyWorkFrame,
  refreshWorkSnapshot,
  workSnapshot,
  type ControllerWorkState,
} from './controllerReplay';

export function useControllerWork(selected: CatalogEntry) {
  const [state, setState] = useState<ControllerWorkState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [historyReset, setHistoryReset] = useState(0);
  const [commandNotice, setCommandNotice] = useState<string | null>(null);
  const [unacceptedCommands, setUnacceptedCommands] = useState<WorkCommand[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadingOlder, setLoadingOlder] = useState(false);
  const [historyPaused, setHistoryPaused] = useState(false);
  const [sending, setSending] = useState(false);
  const [commands, setCommands] = useState<Array<PendingCommand & { error?: string }>>([]);
  const [reload, setReload] = useState(0);
  const current = useRef<ControllerWorkState | null>(null);
  const recoveryAttempted = useRef(false);
  const resetHistory = useRef(false);
  const commandBusy = useRef(false);
  const alive = useRef(false);
  const loadOlderRequest = useRef<(() => Promise<void>) | null>(null);
  const identity = workKey(selected.work);

  useEffect(() => {
    setUnacceptedCommands([]);
    setCommandNotice(null);
    setHistoryReset(0);
    recoveryAttempted.current = false;
  }, [identity, selected.store_generation]);

  useEffect(() => {
    alive.current = true;
    let cancelled = false;
    let replayStopped = false;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let paused = false;
    let olderBusy = false;
    const externalLog = selected.work.kind !== 'chat';
    const historyTokens = new Set<string>();
    const previous = current.current;
    const keepChatHistory =
      !resetHistory.current &&
      !externalLog &&
      previous &&
      !previous.deleted &&
      workKey(previous.entry.work) === identity &&
      previous.entry.store_generation === selected.store_generation;
    if (keepChatHistory && previous.nextHistoryPageToken)
      historyTokens.add(previous.nextHistoryPageToken);
    resetHistory.current = false;
    if (!keepChatHistory) {
      current.current = null;
      setState(null);
    }
    setLoading(true);
    setLoadingOlder(false);
    setHistoryPaused(false);
    setError(null);
    setCommands([]);
    const publish = (next: ControllerWorkState) => {
      if (cancelled) return;
      current.current = next;
      setState(next);
    };
    const preserveHistory = (next: ControllerWorkState): ControllerWorkState => {
      const previous = current.current;
      if (!externalLog || !paused || !previous) return next;
      return {
        ...next,
        items: previous.items,
        partialIds: previous.partialIds,
        historyRevision: previous.historyRevision,
        nextHistoryPageToken: previous.nextHistoryPageToken,
      };
    };
    const restoreCommands = async () => {
      const pending = await controllerPendingCommands(selected.work);
      const refreshed = await Promise.all(
        pending.commands.map(async (item) => {
          if (
            item.receipt &&
            ['delivered', 'rejected', 'outcome_unknown'].includes(item.receipt.state)
          )
            return item;
          try {
            const receipt = await controllerReceipt(
              item.command.work.owner_node_id,
              item.command.command_id
            );
            if (
              receipt.command_id !== item.command.command_id ||
              workKey(receipt.work) !== workKey(item.command.work)
            )
              throw new Error('The owner returned a receipt for another command.');
            return { ...item, receipt };
          } catch (failure) {
            return { ...item, error: controllerErrorText(failure) };
          }
        })
      );
      if (!cancelled) setCommands(refreshed);
    };
    const load = async (replayed?: ControllerWorkState) => {
      let next = workSnapshot(await controllerOpen(selected.work, selected.store_generation));
      if (
        workKey(next.entry.work) !== identity ||
        next.entry.store_generation !== selected.store_generation
      )
        throw new Error('The owner returned a different work identity.');
      if (cancelled) return;
      const previous =
        current.current && (!replayed || current.current.sequence >= replayed.sequence)
          ? current.current
          : replayed;
      if (!externalLog && previous && !previous.deleted && previous.historyRevision !== null) {
        next = refreshWorkSnapshot(previous, next);
      } else if (!externalLog || !paused) {
        const page = externalLog
          ? await controllerHistory(selected.work, selected.store_generation, null)
          : await controllerHistoryLatest(selected.work, selected.store_generation, next.cursor);
        if (!externalLog && page.revision !== next.snapshotRevision)
          throw new Error('History did not match the selected work snapshot. Reload this work.');
        next = addHistory(next, page);
        if (!paused) {
          historyTokens.clear();
          if (page.next_page_token) historyTokens.add(page.next_page_token);
        }
      }
      if (!cancelled) {
        publish(preserveHistory(next));
        setLoading(false);
      }
    };
    loadOlderRequest.current = async () => {
      const token = current.current?.nextHistoryPageToken;
      if (!token || olderBusy || current.current?.deleted) return;
      paused = externalLog;
      olderBusy = true;
      setHistoryPaused(externalLog);
      setLoadingOlder(true);
      try {
        const page = await controllerHistory(selected.work, selected.store_generation, token);
        if (cancelled || !current.current || current.current.deleted) return;
        if (page.next_page_token && historyTokens.has(page.next_page_token))
          throw new Error('History pagination did not advance.');
        const next = addHistory(current.current, page);
        recoveryAttempted.current = false;
        if (page.next_page_token) historyTokens.add(page.next_page_token);
        publish(next);
        setError(null);
      } catch (failure) {
        if (!cancelled) {
          if (!externalLog && !recoveryAttempted.current && controllerSnapshotRequired(failure)) {
            resetHistory.current = true;
            recoveryAttempted.current = true;
            setHistoryReset((value) => value + 1);
            setReload((value) => value + 1);
          } else setError(controllerErrorText(failure));
        }
      } finally {
        olderBusy = false;
        if (!cancelled) setLoadingOlder(false);
      }
    };
    const poll = async () => {
      try {
        const before = current.current;
        if (!before || before.deleted) return;
        let next = before;
        let more: boolean;
        do {
          if (cancelled) return;
          const previousCursor = next.cursor;
          const batch = await controllerChanges(
            selected.work,
            selected.store_generation,
            next.cursor
          );
          if (cancelled) return;
          // Paging may have completed during the replay request. Keep its history and cursor.
          if (current.current && current.current.sequence >= next.sequence) next = current.current;
          else next = preserveHistory(next);
          for (const frame of batch.frames) next = applyWorkFrame(next, frame);
          if (!batch.resume_cursor || (batch.has_more && batch.resume_cursor === previousCursor))
            throw new Error('Work replay did not advance.');
          recoveryAttempted.current = false;
          next = { ...next, cursor: batch.resume_cursor };
          // Older-page requests must merge onto every consumed replay batch, including while
          // the next batch is still in flight.
          publish(next);
          more = batch.has_more;
        } while (more && !next.refreshSnapshot && !next.deleted);
        if (next.refreshSnapshot) await load(next);
        await restoreCommands();
        if (!cancelled) setError(null);
      } catch (failure) {
        if (!cancelled) {
          if (!externalLog && !recoveryAttempted.current && controllerSnapshotRequired(failure)) {
            resetHistory.current = true;
            recoveryAttempted.current = true;
            setHistoryReset((value) => value + 1);
            setReload((value) => value + 1);
          } else {
            if (controllerSnapshotRequired(failure)) replayStopped = true;
            setError(controllerErrorText(failure));
          }
        }
      } finally {
        if (!cancelled && !replayStopped && !current.current?.deleted)
          timer = setTimeout(() => void poll(), 1000);
      }
    };
    void (async () => {
      try {
        await load();
        await restoreCommands();
        if (!cancelled) await poll();
      } catch (failure) {
        if (!cancelled) {
          setError(controllerErrorText(failure));
          setLoading(false);
        }
      }
    })();
    return () => {
      cancelled = true;
      alive.current = false;
      loadOlderRequest.current = null;
      if (timer) clearTimeout(timer);
    };
    // A catalog revision refresh must not restart selected transcript history.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [identity, selected.store_generation, reload]);

  const submitExact = useCallback(async (command: WorkCommand) => {
    if (commandBusy.current) throw new Error('A command is already being submitted.');
    commandBusy.current = true;
    setCommandNotice(null);
    setSending(true);
    setCommands((items) => [
      ...items.filter((item) => item.command.command_id !== command.command_id),
      { command, receipt: null },
    ]);
    try {
      const receipt = await controllerCommand(command);
      if (
        receipt.command_id !== command.command_id ||
        workKey(receipt.work) !== workKey(command.work)
      )
        throw new Error('The owner returned a receipt for another command.');
      if (alive.current) {
        setCommands((items) => [
          ...items.filter((item) => item.command.command_id !== command.command_id),
          { command, receipt },
        ]);
        setError(null);
      }
      if (receipt.state === 'rejected' || receipt.state === 'outcome_unknown')
        throw new Error(receipt.reason ?? receipt.state);
    } catch (failure) {
      if (controllerCommandNotAdmitted(failure)) {
        const detail =
          'Not sent: the session changed before the owner accepted this action. Review the refreshed session and try again.';
        if (
          alive.current &&
          current.current &&
          workKey(current.current.entry.work) === workKey(command.work) &&
          current.current.entry.store_generation === command.expected_store_generation
        ) {
          setCommands((items) =>
            items.filter((item) => item.command.command_id !== command.command_id)
          );
          setCommandNotice(detail);
          setUnacceptedCommands((items) => [
            ...items.filter((item) => item.command_id !== command.command_id),
            command,
          ]);
          setLoading(true);
          setReload((value) => value + 1);
        }
        throw new Error(detail);
      }
      const detail = controllerErrorText(failure);
      if (alive.current) setError(detail);
      throw new Error(detail);
    } finally {
      commandBusy.current = false;
      if (alive.current) setSending(false);
    }
  }, []);
  const send = useCallback(
    async (action: CommandAction) => {
      const latest = current.current;
      if (!latest || latest.deleted || latest.refreshSnapshot)
        throw new Error('Reload the owner state before sending a command.');
      if (!latest.entry.capabilities.includes(action.action))
        throw new Error('The owner does not allow this action.');
      await submitExact({
        ...action,
        command_id: window.crypto.randomUUID(),
        work: latest.entry.work,
        expected_store_generation: latest.entry.store_generation,
        expected_revision: latest.entry.revision,
        expected_run_id: latest.entry.current_run_id,
      });
    },
    [submitExact]
  );
  const checkReceipt = useCallback(async (pending: PendingCommand) => {
    try {
      const receipt = await controllerReceipt(
        pending.command.work.owner_node_id,
        pending.command.command_id
      );
      if (
        receipt.command_id !== pending.command.command_id ||
        workKey(receipt.work) !== workKey(pending.command.work)
      )
        throw new Error('The owner returned a receipt for another command.');
      if (alive.current)
        setCommands((items) =>
          items.map((item) =>
            item.command.command_id === receipt.command_id ? { ...item, receipt } : item
          )
        );
    } catch (failure) {
      if (alive.current) setError(controllerErrorText(failure));
    }
  }, []);
  return {
    state,
    error,
    commandNotice,
    unacceptedCommands: unacceptedCommands.filter(
      (command) =>
        workKey(command.work) === identity &&
        command.expected_store_generation === selected.store_generation
    ),
    loading,
    loadingOlder,
    historyPaused,
    historyReset,
    sending,
    commands,
    send,
    retry: submitExact,
    checkReceipt,
    loadOlder: () => loadOlderRequest.current?.(),
    reload: () => setReload((value) => value + 1),
  };
}
