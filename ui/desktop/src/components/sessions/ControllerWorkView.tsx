import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { ArrowLeft } from 'lucide-react';
import {
  workKey,
  type CatalogEntry,
  type CommandAction,
  type HostDescriptor,
  type WorkCommand,
} from '../../acp/controllerTypes';
import { defineMessages, useIntl } from '../../i18n';
import { Button } from '../ui/button';
import { useControllerWork } from './useControllerWork';
import ControllerInput from './ControllerInput';
import { transcriptMessage } from './controllerReplay';
import { objectValue } from './controllerForm';
import type { Message } from '../../types/message';

const messages = defineMessages({
  back: { id: 'controller.work.back', defaultMessage: 'All linked sessions' },
  owner: { id: 'controller.work.owner', defaultMessage: 'Runs on {name}' },
  load: { id: 'controller.work.load', defaultMessage: 'Loading from the owner Mac…' },
  reload: { id: 'controller.work.reload', defaultMessage: 'Reload owner state' },
  refreshLog: { id: 'controller.work.refreshLog', defaultMessage: 'Refresh log' },
  loadOlder: { id: 'controller.work.loadOlder', defaultMessage: 'Load older output' },
  loadingOlder: { id: 'controller.work.loadingOlder', defaultMessage: 'Loading older output…' },
  historyPaused: {
    id: 'controller.work.historyPaused',
    defaultMessage: 'Automatic log refresh is paused while viewing older output.',
  },
  noOutput: { id: 'controller.work.noOutput', defaultMessage: 'No log output yet.' },
  unavailable: {
    id: 'controller.work.unavailable',
    defaultMessage:
      'The owner is unavailable. Saved content may be out of date; controls are disabled.',
  },
  deleted: {
    id: 'controller.work.deleted',
    defaultMessage: 'This work was deleted or is no longer shared by its owner.',
  },
  send: { id: 'controller.work.send', defaultMessage: 'Send' },
  queue: { id: 'controller.work.queue', defaultMessage: 'Queue next message' },
  steer: { id: 'controller.work.steer', defaultMessage: 'Steer current run' },
  stop: { id: 'controller.work.stop', defaultMessage: 'Stop current run' },
  note: { id: 'controller.work.note', defaultMessage: 'Add note' },
  pause: { id: 'controller.work.pause', defaultMessage: 'Pause' },
  resume: { id: 'controller.work.resume', defaultMessage: 'Resume' },
  tick: { id: 'controller.work.tick', defaultMessage: 'Run a tick' },
  message: { id: 'controller.work.message', defaultMessage: 'Message for this session on {name}' },
  uncertain: {
    id: 'controller.work.uncertain',
    defaultMessage: 'Acknowledgement not received. Check the receipt or retry this same command.',
  },
  check: { id: 'controller.work.check', defaultMessage: 'Check receipt' },
  retry: { id: 'controller.work.retry', defaultMessage: 'Retry same command' },
  noMessages: { id: 'controller.work.noMessages', defaultMessage: 'No visible messages yet.' },
  user: { id: 'controller.work.user', defaultMessage: 'You' },
  assistant: { id: 'controller.work.assistant', defaultMessage: 'Goose' },
  content: { id: 'controller.work.content', defaultMessage: 'Message details' },
  accepted: { id: 'controller.receipt.accepted', defaultMessage: 'Accepted by the owner' },
  waiting: { id: 'controller.receipt.waiting', defaultMessage: 'Queued for the next turn' },
  dispatching: {
    id: 'controller.receipt.dispatching',
    defaultMessage: 'The owner is delivering this command',
  },
  delivered: { id: 'controller.receipt.delivered', defaultMessage: 'Delivered' },
  rejected: { id: 'controller.receipt.rejected', defaultMessage: 'Rejected by the owner' },
  outcome_unknown: {
    id: 'controller.receipt.outcomeUnknown',
    defaultMessage:
      'The owner cannot confirm the outcome. Check the session before trying another command.',
  },
});

/** Remote paths are never handed to local file/image tools. Only inline pixels can render here. */
function ControllerMessage({ message }: { message: Message }) {
  const intl = useIntl();
  return (
    <article className="p-4 rounded-xl border border-border-primary bg-background-secondary space-y-3">
      <div className="flex gap-3 justify-between text-sm text-text-secondary">
        <span className="font-semibold text-text-primary">
          {intl.formatMessage(message.role === 'user' ? messages.user : messages.assistant)}
        </span>
        <time dateTime={new Date(message.created * 1000).toISOString()}>
          {new Date(message.created * 1000).toLocaleString()}
        </time>
      </div>
      {message.content.map((part, index) => {
        if (part.type === 'text')
          return (
            <p
              key={`${message.id}:text:${index}`}
              className="whitespace-pre-wrap break-words text-sm"
            >
              {part.text}
            </p>
          );
        if (part.type === 'image' && /^image\/(png|jpeg|gif|webp)$/.test(part.mimeType))
          return (
            <img
              key={`${message.id}:image:${index}`}
              src={`data:${part.mimeType};base64,${part.data}`}
              alt=""
              className="max-w-full max-h-96 object-contain"
            />
          );
        const value = objectValue(part);
        return (
          <details key={`${message.id}:content:${index}`} className="text-sm">
            <summary className="cursor-pointer">
              {typeof value?.type === 'string' ? value.type : intl.formatMessage(messages.content)}
            </summary>
            <pre className="whitespace-pre-wrap break-all text-xs max-h-96 overflow-auto mt-2">
              {JSON.stringify(part, null, 2)}
            </pre>
          </details>
        );
      })}
    </article>
  );
}

export default function ControllerWorkView({
  entry,
  host,
  stale,
  onBack,
}: {
  entry: CatalogEntry;
  host: HostDescriptor;
  stale: boolean;
  onBack: () => void;
}) {
  const intl = useIntl();
  const work = useControllerWork(entry);
  const [text, setText] = useState('');
  const recoveredIds = useRef(new Set<string>());
  const identity = workKey(entry.work);
  useEffect(() => {
    setText('');
    recoveredIds.current.clear();
  }, [identity, entry.store_generation]);
  useEffect(() => {
    for (const command of work.unacceptedCommands) {
      if (recoveredIds.current.has(command.command_id)) continue;
      recoveredIds.current.add(command.command_id);
      const recovered = commandText(command);
      if (recovered !== null) setText((draft) => (draft === '' ? recovered : draft));
    }
  }, [work.unacceptedCommands]);
  const transcript = useRef<HTMLDivElement>(null);
  const olderScroll = useRef<{ height: number; top: number } | null>(null);
  const current = work.state?.entry ?? entry;
  const externalLog = current.work.kind !== 'chat';
  useLayoutEffect(() => {
    if (work.loadingOlder || !olderScroll.current || !transcript.current) return;
    const { height, top } = olderScroll.current;
    transcript.current.scrollTop = top + transcript.current.scrollHeight - height;
    olderScroll.current = null;
  }, [work.state?.items, work.loadingOlder]);
  const refresh = () => {
    olderScroll.current = null;
    work.reload();
  };
  const loadOlder = () => {
    if (transcript.current)
      olderScroll.current = {
        height: transcript.current.scrollHeight,
        top: transcript.current.scrollTop,
      };
    void work.loadOlder();
  };
  const disabled =
    stale ||
    host.availability !== 'online' ||
    work.loading ||
    work.sending ||
    Boolean(work.error) ||
    !work.state ||
    work.state.deleted;
  const unresolved = work.commands.some(
    (item) => !item.receipt || item.receipt.state === 'outcome_unknown'
  );
  const supports = (action: CommandAction['action']) => current.capabilities.includes(action);
  const execute = async (action: CommandAction, clear = false) => {
    try {
      await work.send(action);
      if (clear) setText('');
    } catch {
      /* The hook retains the command and displays its failure. */
    }
  };
  return (
    <div className="h-full min-h-0 flex flex-col">
      <header className="p-4 space-y-2 border-b border-border-primary">
        <Button variant="ghost" onClick={onBack}>
          <ArrowLeft className="h-4 w-4 mr-2" />
          {intl.formatMessage(messages.back)}
        </Button>
        <h2 className="text-xl font-semibold break-words">{current.title}</h2>
        <p className="text-sm font-semibold">
          {intl.formatMessage(messages.owner, { name: host.name })}
        </p>
        <p className="text-xs text-text-secondary break-all">
          {current.working_dir} · {current.work.kind} · {current.state}
        </p>
        {externalLog && !work.state?.deleted && (
          <div className="flex flex-wrap items-center gap-3">
            <Button
              variant="outline"
              disabled={work.loading || stale || host.availability !== 'online'}
              onClick={refresh}
            >
              {intl.formatMessage(messages.refreshLog)}
            </Button>
            {work.historyPaused && (
              <p role="status" className="text-sm text-text-secondary">
                {intl.formatMessage(messages.historyPaused)}
              </p>
            )}
          </div>
        )}
        {(stale || host.availability !== 'online') && (
          <p role="status" className="text-text-danger text-sm">
            {intl.formatMessage(messages.unavailable)}
          </p>
        )}
        {work.error && (
          <div role="alert" className="text-text-danger text-sm space-y-2">
            <p>{work.error}</p>
            <Button variant="outline" onClick={refresh}>
              {intl.formatMessage(messages.reload)}
            </Button>
          </div>
        )}
        {work.commandNotice && (
          <p role="status" className="text-sm">
            {work.commandNotice}
          </p>
        )}
      </header>
      <div
        ref={transcript}
        className="flex-1 min-h-0 overflow-auto p-4 space-y-4"
        aria-live={work.historyPaused ? 'off' : 'polite'}
      >
        {work.loading && <p>{intl.formatMessage(messages.load)}</p>}
        {work.state?.deleted && <p>{intl.formatMessage(messages.deleted)}</p>}
        {externalLog && work.state?.nextHistoryPageToken && !work.state.deleted && (
          <Button
            variant="outline"
            disabled={work.loadingOlder || stale || host.availability !== 'online'}
            onClick={loadOlder}
          >
            {intl.formatMessage(work.loadingOlder ? messages.loadingOlder : messages.loadOlder)}
          </Button>
        )}
        {work.state?.items.map((item) => (
          <ControllerMessage key={item.message_id} message={transcriptMessage(item)} />
        ))}
        {!work.loading &&
          !work.error &&
          work.state &&
          work.state.items.length === 0 &&
          !work.state.deleted &&
          !work.state.nextHistoryPageToken && (
            <p className="text-text-secondary">
              {intl.formatMessage(externalLog ? messages.noOutput : messages.noMessages)}
            </p>
          )}
        {work.state?.pendingInputs.map((input) => (
          <ControllerInput
            key={`${input.input_id}:${input.run_id}`}
            input={input}
            disabled={
              Boolean(disabled) ||
              !supports('answer_input') ||
              unresolved ||
              work.commands.some(
                (item) =>
                  item.command.action === 'answer_input' &&
                  item.command.payload.input_id === input.input_id &&
                  item.receipt?.state !== 'rejected'
              )
            }
            onAnswer={async (option, value) => {
              await work.send({
                action: 'answer_input',
                payload: { input_id: input.input_id, option_id: option.option_id, value },
              });
            }}
          />
        ))}
        {work.commands.map((item) => (
          <div
            key={item.command.command_id}
            className="border border-border-primary rounded-lg p-3 text-sm space-y-2"
            role="status"
          >
            <p>
              {item.command.action} ·{' '}
              {item.receipt
                ? intl.formatMessage(messages[item.receipt.state])
                : intl.formatMessage(messages.uncertain)}
            </p>
            {item.receipt?.reason && <p>{item.receipt.reason}</p>}
            {item.error && <p className="text-text-danger">{item.error}</p>}
            {(!item.receipt || !['delivered', 'rejected'].includes(item.receipt.state)) && (
              <Button
                variant="outline"
                disabled={stale || work.sending}
                onClick={() => void work.checkReceipt(item)}
              >
                {intl.formatMessage(messages.check)}
              </Button>
            )}
            {!item.receipt && (
              <Button
                variant="outline"
                disabled={stale || work.sending}
                onClick={() => void work.retry(item.command).catch(() => undefined)}
              >
                {intl.formatMessage(messages.retry)}
              </Button>
            )}
          </div>
        ))}
        {work.unacceptedCommands.map((command) => (
          <details key={command.command_id} className="text-sm" open>
            <summary>
              {command.action} · {intl.formatMessage(messages.rejected)} ·{' '}
              {intl.formatMessage(messages.content)}
            </summary>
            <pre className="whitespace-pre-wrap break-all">
              {commandText(command) ?? JSON.stringify(command.payload, null, 2)}
            </pre>
          </details>
        ))}
      </div>
      <footer className="p-4 border-t border-border-primary space-y-3">
        {(supports('send') || supports('steer') || supports('note')) && (
          <textarea
            className="w-full min-h-20 resize-y rounded-lg border border-border-primary bg-background-primary p-3 text-sm"
            aria-label={intl.formatMessage(messages.message, { name: host.name })}
            value={text}
            onChange={(event) => setText(event.target.value)}
            disabled={Boolean(disabled)}
          />
        )}
        <div className="flex flex-wrap gap-2">
          {supports('send') && (
            <Button
              disabled={Boolean(disabled) || unresolved || !text.trim()}
              onClick={() =>
                void execute(
                  { action: 'send', payload: { content: [{ type: 'text', text }] } },
                  true
                )
              }
            >
              {intl.formatMessage(current.current_run_id ? messages.queue : messages.send)}
            </Button>
          )}
          {supports('steer') && current.current_run_id && (
            <Button
              variant="outline"
              disabled={Boolean(disabled) || unresolved || !text.trim()}
              onClick={() =>
                void execute(
                  { action: 'steer', payload: { content: [{ type: 'text', text }] } },
                  true
                )
              }
            >
              {intl.formatMessage(messages.steer)}
            </Button>
          )}
          {supports('note') && (
            <Button
              variant="outline"
              disabled={Boolean(disabled) || unresolved || !text.trim()}
              onClick={() => void execute({ action: 'note', payload: { text } }, true)}
            >
              {intl.formatMessage(messages.note)}
            </Button>
          )}
          {(['stop', 'pause', 'resume', 'tick'] as const).filter(supports).map((action) => (
            <Button
              key={action}
              variant="outline"
              disabled={Boolean(disabled) || (action === 'stop' && !current.current_run_id)}
              onClick={() => void execute({ action, payload: {} })}
            >
              {intl.formatMessage(messages[action])}
            </Button>
          ))}
        </div>
      </footer>
    </div>
  );
}

function commandText(command: WorkCommand): string | null {
  if (command.action === 'note') return command.payload.text;
  if (
    (command.action === 'send' || command.action === 'steer') &&
    command.payload.content.length === 1 &&
    command.payload.content[0].type === 'text'
  ) {
    return command.payload.content[0].text;
  }
  return null;
}
