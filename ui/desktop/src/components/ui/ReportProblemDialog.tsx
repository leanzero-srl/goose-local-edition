import { useId, useState, type ReactNode } from 'react';
import { Bug, Mail, Send, X } from 'lucide-react';
import { OverlayDialog, OverlayDialogTitle } from './OverlayDialog';
import { Discord } from '../icons';
import { Button, Checkbox, FOCUS, RADIUS, SURFACE, TONE_FILL, TYPE, cx } from '../lz';
import { defineMessages, useIntl } from '../../i18n';
import { getDiagnosticsReport } from '../../acp/diagnostics';
import { LEANZERO_DISCORD_INVITE_URL, LEANZERO_SUPPORT_EMAIL } from '../../branding';
import {
  REPORT_MAX_CHARS,
  REPORT_MIN_CHARS,
  mailtoUrl,
  systemDetailsText,
  validateReport,
  type ReportProblemKind,
  type ReportVia,
} from '../../utils/problemReport';

const i18n = defineMessages({
  reportProblem: {
    id: 'diagnosticsModal.reportProblem',
    defaultMessage: 'Report a problem',
  },
  goesTo: {
    id: 'reportProblem.goesTo',
    defaultMessage: 'Your report goes to {email}.',
  },
  close: { id: 'reportProblem.close', defaultMessage: 'Close' },
  whatWentWrong: { id: 'reportProblem.whatWentWrong', defaultMessage: 'What went wrong?' },
  whatWentWrongHint: {
    id: 'reportProblem.whatWentWrongHint',
    defaultMessage: 'What you were doing, what you expected, and what happened instead.',
  },
  email: { id: 'reportProblem.email', defaultMessage: 'Your email' },
  emailHint: {
    id: 'reportProblem.emailHint',
    defaultMessage:
      'LeanZero replies to this address. Needed to send from here; optional for your mail app.',
  },
  attach: { id: 'reportProblem.attach', defaultMessage: 'Attach system details' },
  attachHint: {
    id: 'reportProblem.attachHint',
    defaultMessage:
      'App and engine versions, OS, provider, model and extension names. Never your chats, logs, settings or keys.',
  },
  previewTitle: {
    id: 'reportProblem.previewTitle',
    defaultMessage: 'Exactly what will be attached:',
  },
  previewLoading: {
    id: 'reportProblem.previewLoading',
    defaultMessage: 'Reading system details…',
  },
  previewFailed: {
    id: 'reportProblem.previewFailed',
    defaultMessage: 'Could not read system details ({error}). Nothing will be attached.',
  },
  descriptionShort: {
    id: 'reportProblem.descriptionShort',
    defaultMessage: 'Describe the problem in at least {min} characters.',
  },
  descriptionLong: {
    id: 'reportProblem.descriptionLong',
    defaultMessage: 'That is over {max} characters. Please shorten it.',
  },
  emailMissing: {
    id: 'reportProblem.emailMissing',
    defaultMessage: 'Add your email so LeanZero can reply, or send it from your mail app.',
  },
  emailInvalid: {
    id: 'reportProblem.emailInvalid',
    defaultMessage: 'That email address does not look right.',
  },
  sendFailed: {
    id: 'reportProblem.sendFailed',
    defaultMessage:
      'It did not go through: {reason}. Your report is still here. Try again, or send it from your mail app.',
  },
  mailFailed: {
    id: 'reportProblem.mailFailed',
    defaultMessage: 'Could not open your mail app: {reason}.',
  },
  send: { id: 'reportProblem.send', defaultMessage: 'Send' },
  useMailApp: { id: 'reportProblem.useMailApp', defaultMessage: 'Use my mail app' },
  joinDiscord: { id: 'reportProblem.joinDiscord', defaultMessage: 'Join the LeanZero Discord' },
  sentTitle: { id: 'reportProblem.sentTitle', defaultMessage: 'Report sent' },
  sentBody: {
    id: 'reportProblem.sentBody',
    defaultMessage:
      'LeanZero has your report and will reply to {email}. leanzero.net also emails you a receipt.',
  },
  mailedTitle: { id: 'reportProblem.mailedTitle', defaultMessage: 'Opened in your mail app' },
  mailedBody: {
    id: 'reportProblem.mailedBody',
    defaultMessage:
      'Your report is ready in a new email to {email}. It is not sent until you send it from there.',
  },
  done: { id: 'reportProblem.done', defaultMessage: 'Done' },
});

/**
 * The action's ONE name (Q-9): the composer's button, its tooltip and this dialog's title all read
 * it, so the bug icon can no longer say "Generate diagnostics bundle" and open "Report a Problem".
 */
export const reportProblemMessage = i18n.reportProblem;

const FIELD = cx(
  'w-full border border-lz-border-strong bg-lz-surface px-2.5 py-1.5 text-lz-body text-lz-ink placeholder:text-lz-ink-3',
  RADIUS.control,
  FOCUS
);

type Details =
  | { state: 'idle' }
  | { state: 'loading' }
  | { state: 'ready'; text: string }
  | { state: 'failed'; error: string };

type Phase = 'editing' | 'sent' | 'mailed';

function Field({
  id,
  label,
  hint,
  children,
}: {
  id: string;
  label: string;
  hint: string;
  children: ReactNode;
}) {
  return (
    <div>
      <label htmlFor={id} className={cx('mb-1 block', TYPE.body, 'font-lz-medium')}>
        {label}
      </label>
      <div id={`${id}-hint`} className={cx('mb-1.5', TYPE.meta)}>
        {hint}
      </div>
      {children}
    </div>
  );
}

export interface ReportProblemDialogProps {
  isOpen: boolean;
  onClose: () => void;
  /**
   * The chat the report was opened from, when there is one. The attached details come from the
   * engine's SUMMARY diagnostics, which read no session (goose session/diagnostics.rs reads the
   * session only at `full`), so Settings — with no chat — attaches the same six lines.
   */
  sessionId?: string;
}

/**
 * Report a problem (Q-192, owner): "a small form that sends an email to office@leanzero.net,
 * alternatively lets the user join the LeanZero Discord". What went wrong, a reply address, an
 * opt-in for system details that previews EXACTLY the text it attaches, then SEND (main posts to
 * leanzero.net's contact endpoint) or the person's own mail app (mailto:). Replaces the old dialog
 * that downloaded a full diagnostics JSON — chats, logs and config included — or filed on GitHub.
 */
export function ReportProblemDialog({ isOpen, onClose, sessionId }: ReportProblemDialogProps) {
  const intl = useIntl();
  const ids = useId();
  const descriptionId = `${ids}-description`;
  const emailId = `${ids}-email`;
  const [description, setDescription] = useState('');
  const [email, setEmail] = useState('');
  const [attach, setAttach] = useState(false);
  const [details, setDetails] = useState<Details>({ state: 'idle' });
  const [problem, setProblem] = useState<ReportProblemKind | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [phase, setPhase] = useState<Phase>('editing');

  const problemText = (kind: ReportProblemKind): string => {
    switch (kind) {
      case 'description-short':
        return intl.formatMessage(i18n.descriptionShort, { min: REPORT_MIN_CHARS });
      case 'description-long':
        return intl.formatMessage(i18n.descriptionLong, { max: REPORT_MAX_CHARS });
      case 'email-missing':
        return intl.formatMessage(i18n.emailMissing);
      case 'email-invalid':
        return intl.formatMessage(i18n.emailInvalid);
    }
  };

  const toggleAttach = async (next: boolean) => {
    setAttach(next);
    if (!next || details.state === 'ready' || details.state === 'loading') return;
    setDetails({ state: 'loading' });
    try {
      // Summary level: the engine reads no session for it (see the prop's note), so no chat is needed.
      const report = await getDiagnosticsReport(sessionId ?? '', 'summary');
      setDetails({
        state: 'ready',
        text: systemDetailsText(report.system, window.electron.getVersion()),
      });
    } catch (err) {
      setAttach(false);
      setDetails({ state: 'failed', error: err instanceof Error ? err.message : String(err) });
    }
  };

  const attachment = attach && details.state === 'ready' ? details.text : null;
  const detailsPending = attach && details.state === 'loading';

  const blocked = (via: ReportVia): boolean => {
    const kind = validateReport({ description, email }, via);
    setProblem(kind);
    setFailure(null);
    return kind != null;
  };

  const send = async () => {
    if (blocked('send')) return;
    const result = await window.electron.sendProblemReport({
      description,
      email,
      attachment,
      desktopVersion: window.electron.getVersion(),
    });
    if (result.ok) {
      setPhase('sent');
      return;
    }
    const reason =
      result.reason === 'http' && result.status != null
        ? `${result.message} (HTTP ${result.status})`
        : result.message;
    setFailure(intl.formatMessage(i18n.sendFailed, { reason }));
  };

  const openMailApp = async () => {
    if (blocked('mail')) return;
    try {
      await window.electron.openExternal(mailtoUrl(description, email, attachment));
      setPhase('mailed');
    } catch (err) {
      setFailure(
        intl.formatMessage(i18n.mailFailed, {
          reason: err instanceof Error ? err.message : String(err),
        })
      );
    }
  };

  const joinDiscord = () => window.electron.openExternal(LEANZERO_DISCORD_INVITE_URL);

  const discordButton = (
    <Button
      variant="ghost"
      icon={<Discord />}
      onClick={joinDiscord}
      data-testid="report-problem-discord"
    >
      {intl.formatMessage(i18n.joinDiscord)}
    </Button>
  );

  const header = (
    <div className="mb-4 flex items-start gap-3">
      <span
        aria-hidden
        className={cx(
          'flex size-8 shrink-0 items-center justify-center [&_svg]:size-4',
          RADIUS.control,
          TONE_FILL.accent
        )}
      >
        <Bug />
      </span>
      <div className="min-w-0 flex-1">
        <OverlayDialogTitle asChild>
          <h2 className={TYPE.h2}>{intl.formatMessage(i18n.reportProblem)}</h2>
        </OverlayDialogTitle>
        <p className={cx('mt-0.5', TYPE.meta)}>
          {intl.formatMessage(i18n.goesTo, { email: LEANZERO_SUPPORT_EMAIL })}
        </p>
      </div>
      <Button
        variant="ghost"
        size="sm"
        iconOnly
        icon={<X />}
        aria-label={intl.formatMessage(i18n.close)}
        onClick={onClose}
      />
    </div>
  );

  const confirmation = (title: string, body: string, testId: string) => (
    <div data-testid={testId} role="status">
      <p className={cx(TYPE.body, 'font-lz-semibold')}>{title}</p>
      <p className={cx('mt-1', TYPE.bodyMuted)}>{body}</p>
      <div className="mt-5 flex items-center justify-between gap-2">
        {discordButton}
        <Button variant="primary" onClick={onClose}>
          {intl.formatMessage(i18n.done)}
        </Button>
      </div>
    </div>
  );

  return (
    <OverlayDialog
      open={isOpen}
      onClose={onClose}
      panelClassName={cx('w-[32rem] p-5', SURFACE.overlay)}
    >
      <div data-testid="report-problem-dialog">
        {header}
        {phase === 'sent' &&
          confirmation(
            intl.formatMessage(i18n.sentTitle),
            intl.formatMessage(i18n.sentBody, { email: email.trim() }),
            'report-problem-sent'
          )}
        {phase === 'mailed' &&
          confirmation(
            intl.formatMessage(i18n.mailedTitle),
            intl.formatMessage(i18n.mailedBody, { email: LEANZERO_SUPPORT_EMAIL }),
            'report-problem-mailed'
          )}
        {phase === 'editing' && (
          <div className="flex flex-col gap-4">
            <Field
              id={descriptionId}
              label={intl.formatMessage(i18n.whatWentWrong)}
              hint={intl.formatMessage(i18n.whatWentWrongHint)}
            >
              <textarea
                id={descriptionId}
                aria-describedby={`${descriptionId}-hint`}
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                maxLength={REPORT_MAX_CHARS}
                rows={5}
                className={cx(FIELD, 'resize-y')}
                autoFocus
              />
            </Field>
            <Field
              id={emailId}
              label={intl.formatMessage(i18n.email)}
              hint={intl.formatMessage(i18n.emailHint)}
            >
              <input
                id={emailId}
                type="email"
                autoComplete="email"
                aria-describedby={`${emailId}-hint`}
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                className={FIELD}
              />
            </Field>
            <div>
              <Checkbox
                checked={attach}
                onChange={(next) => void toggleAttach(next)}
                label={intl.formatMessage(i18n.attach)}
                description={intl.formatMessage(i18n.attachHint)}
                testId="report-problem-attach"
              />
              {attach && (
                <div
                  data-testid="report-problem-preview"
                  className={cx('mt-2 border p-3', RADIUS.control, SURFACE.hairline, SURFACE.inset)}
                >
                  {details.state === 'ready' ? (
                    <>
                      <p className={cx('mb-1.5', TYPE.meta)}>
                        {intl.formatMessage(i18n.previewTitle)}
                      </p>
                      <pre
                        data-testid="report-problem-preview-text"
                        className={cx(TYPE.mono, 'whitespace-pre-wrap break-words')}
                      >
                        {details.text}
                      </pre>
                    </>
                  ) : (
                    <p className={TYPE.meta}>{intl.formatMessage(i18n.previewLoading)}</p>
                  )}
                </div>
              )}
              {!attach && details.state === 'failed' && (
                <p role="alert" className="mt-2 text-lz-meta text-lz-err">
                  {intl.formatMessage(i18n.previewFailed, { error: details.error })}
                </p>
              )}
            </div>
            {(problem != null || failure != null) && (
              <p
                role="alert"
                data-testid="report-problem-error"
                className="text-lz-body text-lz-err"
              >
                {failure ?? (problem != null ? problemText(problem) : null)}
              </p>
            )}
            <div className="flex flex-wrap items-center justify-between gap-2">
              {discordButton}
              <div className="flex items-center gap-2">
                <Button
                  variant="secondary"
                  icon={<Mail />}
                  onClick={openMailApp}
                  disabled={detailsPending}
                >
                  {intl.formatMessage(i18n.useMailApp)}
                </Button>
                <Button variant="primary" icon={<Send />} onClick={send} disabled={detailsPending}>
                  {intl.formatMessage(i18n.send)}
                </Button>
              </div>
            </div>
          </div>
        )}
      </div>
    </OverlayDialog>
  );
}
