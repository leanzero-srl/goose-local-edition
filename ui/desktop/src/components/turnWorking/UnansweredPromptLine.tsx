import { useEffect, useState } from 'react';
import { CircleSlash, Play, RotateCcw } from 'lucide-react';
import { defineMessages, useIntl } from '../../i18n';
import { useAcpChatSessionSnapshot } from '../../acp/chatSessionStore';
import type { UserInput } from '../../types/message';
import { Button, RADIUS, TONE_FILL, TYPE, cx } from '../lz';
import { useActivityOf, useSessionActivityRead } from '../sessionActivity/sessionActivityStore';
import { resendInputOf, stoppedTurnOf } from './unansweredPrompt';

const i18n = defineMessages({
  title: {
    id: 'unansweredPrompt.title',
    defaultMessage: 'goose stopped before answering this',
  },
  appClosed: {
    id: 'unansweredPrompt.appClosed',
    defaultMessage:
      'The turn ended without a reply — the app may have closed while goose was working.',
  },
  failed: {
    id: 'unansweredPrompt.failed',
    defaultMessage: 'The turn failed before a reply.',
  },
  failedWithReason: {
    id: 'unansweredPrompt.failedWithReason',
    defaultMessage: 'The turn failed before a reply: {reason}',
  },
  resend: {
    id: 'unansweredPrompt.resend',
    defaultMessage: 'Resend',
  },
  midwayAppClosed: {
    id: 'unansweredPrompt.midwayAppClosed',
    defaultMessage: 'goose stopped mid-way — the app closed while it was working',
  },
  midway: {
    id: 'unansweredPrompt.midway',
    defaultMessage: 'goose stopped mid-way',
  },
  continue: {
    id: 'unansweredPrompt.continue',
    defaultMessage: 'Continue',
  },
  continueMessage: {
    id: 'unansweredPrompt.continueMessage',
    defaultMessage: 'Continue where you left off.',
  },
});

const PILL = 'inline-flex h-6 items-center gap-1.5 px-2 text-lz-meta font-lz-semibold';

/**
 * Under a turn no one is running any more. Q-493: the app was killed before goose replied, so the
 * transcript ends on the prompt — one honest line and a Resend that sends the same words through
 * the composer's door. Q-495: it was killed between tool calls, so the transcript ends on a tool
 * card — the line says goose stopped mid-way, and Continue sends a short "continue" through the
 * same door. Driven by the chat's live state (its prompt call, the engine's run, the engine's busy
 * set), never a timer: the moment a turn starts or streams, the line is gone.
 */
export function UnansweredPromptLine({
  sessionId,
  sendBlocked,
  onResend,
}: {
  sessionId: string;
  sendBlocked: boolean;
  onResend: (input: UserInput) => void;
}) {
  const intl = useIntl();
  const snapshot = useAcpChatSessionSnapshot(sessionId);
  const engineRead = useSessionActivityRead();
  const { runningSince, failedAt, failedReason } = useActivityOf(sessionId);
  const stopped = snapshot
    ? stoppedTurnOf(snapshot.messages, {
        chatState: snapshot.chatState,
        activePromptAttemptId: snapshot.activePromptAttemptId,
        activeRunId: snapshot.activeRunId,
        pendingCancelPromptAttemptId: snapshot.pendingCancelPromptAttemptId,
        submitError: snapshot.submitError,
        engineRead,
        engineRunning: runningSince !== undefined,
      })
    : null;
  const stoppedKey = stopped
    ? `${stopped.kind}:${stopped.message.id ?? stopped.message.created}`
    : null;

  // Shown from the commit AFTER the one that found the turn stopped: an auto-submit (a resumed
  // or forked chat with shouldStartAgent) fires in the parent's effect of that same commit, and its
  // turn start is batched with this state — so a turn starting on load never flashes the line.
  const [settledKey, setSettledKey] = useState<string | null>(null);
  useEffect(() => {
    setSettledKey(stoppedKey);
  }, [stoppedKey]);

  if (!stopped || settledKey !== stoppedKey) return null;

  const failure = failedAt
    ? failedReason
      ? intl.formatMessage(i18n.failedWithReason, { reason: failedReason })
      : intl.formatMessage(i18n.failed)
    : null;
  const midway = stopped.kind === 'midway';
  const title = midway
    ? intl.formatMessage(failure ? i18n.midway : i18n.midwayAppClosed)
    : intl.formatMessage(i18n.title);
  // Mid-way, "the app closed" is already the title; only a failure has more to say.
  const detail = failure ?? (midway ? null : intl.formatMessage(i18n.appClosed));

  return (
    <div
      role="status"
      data-testid="unanswered-prompt"
      data-kind={stopped.kind}
      className="flex max-w-2xl flex-wrap items-center gap-2 py-2"
    >
      <span className={cx(PILL, RADIUS.pill, TONE_FILL.stopped)}>
        <CircleSlash aria-hidden className="size-3.5" />
        {title}
      </span>
      {detail && <span className={TYPE.meta}>{detail}</span>}
      {midway ? (
        <Button
          size="sm"
          variant="secondary"
          icon={<Play />}
          disabled={sendBlocked}
          onClick={() => onResend({ msg: intl.formatMessage(i18n.continueMessage), images: [] })}
        >
          {intl.formatMessage(i18n.continue)}
        </Button>
      ) : (
        <Button
          size="sm"
          variant="secondary"
          icon={<RotateCcw />}
          disabled={sendBlocked}
          onClick={() => onResend(resendInputOf(stopped.message))}
        >
          {intl.formatMessage(i18n.resend)}
        </Button>
      )}
    </div>
  );
}
