import { useEffect, useState } from 'react';
import { CircleSlash, RotateCcw } from 'lucide-react';
import { defineMessages, useIntl } from '../../i18n';
import { useAcpChatSessionSnapshot } from '../../acp/chatSessionStore';
import type { UserInput } from '../../types/message';
import { Button, RADIUS, TONE_FILL, TYPE, cx } from '../lz';
import { useActivityOf, useSessionActivityRead } from '../sessionActivity/sessionActivityStore';
import { resendInputOf, unansweredPromptOf } from './unansweredPrompt';

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
});

const PILL = 'inline-flex h-6 items-center gap-1.5 px-2 text-lz-meta font-lz-semibold';

/**
 * Under the person's last prompt when no turn is answering it (Q-493): the app was killed mid-turn,
 * so the transcript ends on the prompt with no reply, no "stopped", nothing — it read as goose
 * still thinking, or never asked. One honest line and a Resend that sends the same words through
 * the composer's door. Driven by the chat's live state (its prompt call, the engine's run, the
 * engine's busy set), never a timer: the moment a turn starts or streams, the line is gone.
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
  const prompt = snapshot
    ? unansweredPromptOf(snapshot.messages, {
        chatState: snapshot.chatState,
        activePromptAttemptId: snapshot.activePromptAttemptId,
        activeRunId: snapshot.activeRunId,
        pendingCancelPromptAttemptId: snapshot.pendingCancelPromptAttemptId,
        submitError: snapshot.submitError,
        engineRead,
        engineRunning: runningSince !== undefined,
      })
    : null;
  const promptKey = prompt ? (prompt.id ?? `${prompt.created}`) : null;

  // Shown from the commit AFTER the one that found the prompt unanswered: an auto-submit (a resumed
  // or forked chat with shouldStartAgent) fires in the parent's effect of that same commit, and its
  // turn start is batched with this state — so a turn starting on load never flashes the line.
  const [settledKey, setSettledKey] = useState<string | null>(null);
  useEffect(() => {
    setSettledKey(promptKey);
  }, [promptKey]);

  if (!prompt || settledKey !== promptKey) return null;

  const detail = failedAt
    ? failedReason
      ? intl.formatMessage(i18n.failedWithReason, { reason: failedReason })
      : intl.formatMessage(i18n.failed)
    : intl.formatMessage(i18n.appClosed);

  return (
    <div
      role="status"
      data-testid="unanswered-prompt"
      className="flex max-w-2xl flex-wrap items-center gap-2 py-2"
    >
      <span className={cx(PILL, RADIUS.pill, TONE_FILL.stopped)}>
        <CircleSlash aria-hidden className="size-3.5" />
        {intl.formatMessage(i18n.title)}
      </span>
      <span className={TYPE.meta}>{detail}</span>
      <Button
        size="sm"
        variant="secondary"
        icon={<RotateCcw />}
        disabled={sendBlocked}
        onClick={() => onResend(resendInputOf(prompt))}
      >
        {intl.formatMessage(i18n.resend)}
      </Button>
    </div>
  );
}
