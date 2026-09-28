import { OctagonAlert, RotateCcw } from 'lucide-react';
import { defineMessages, useIntl } from '../../i18n';
import type { ProviderErrorNotice as Notice } from '../../types/message';
import { Button, Disclosure, SPACE, SURFACE, TONE_FILL, TYPE, WEIGHT, cx } from '../lz';
import { AGENT_ERROR_WRAP } from '../linkDropNotice/parseLinkDrop';

const i18n = defineMessages({
  headline: {
    id: 'providerErrorNotice.headline',
    defaultMessage:
      '{class, select, request {The model’s server refused this request} auth {The provider did not accept goose’s credentials} rate_limit {The provider is limiting how often goose can ask} server {The model’s server failed while answering} network {goose lost the connection to the model} context_length {The conversation is too long for this model} not_implemented {The model does not support this request} endpoint_not_found {The model’s server has no endpoint for this request} execution {goose could not send this request} other {The model returned an error}}',
  },
  transient: {
    id: 'providerErrorNotice.transient',
    defaultMessage: 'This can pass on its own — send it again in a moment.',
  },
  permanent: {
    id: 'providerErrorNotice.permanent',
    defaultMessage:
      'Sending the same request again will fail the same way until its cause is fixed.',
  },
  retry: { id: 'providerErrorNotice.retry', defaultMessage: 'Retry' },
  details: { id: 'providerErrorNotice.details', defaultMessage: 'Details' },
});

/**
 * The model's answer and the provider error after it (Q-302): the agent loop appends its error text
 * to whatever the model had written (agents/agent.rs — "Ran into this error: <detail>." in the
 * generic arm, "<detail>" in the network arm), and the same error rides the message's metadata
 * as `detail`. The answer is the text before that error, found by the error's own words; text
 * that does not carry them is kept whole, never cut on a guess.
 */
export function splitProviderErrorAnswer(text: string, notice: Notice): string {
  const at = text.lastIndexOf(notice.detail);
  if (at < 0) return text;
  const before = text.slice(0, at);
  return before.endsWith(AGENT_ERROR_WRAP) ? before.slice(0, -AGENT_ERROR_WRAP.length) : before;
}

/**
 * A turn that ended on a provider error, in the error colour and in plain words: what went wrong
 * by the error's class, the serving engine's own sentence, retry advice only when the class is
 * one a resend can outlive (`ProviderError::is_transient`), and the endpoint and body behind
 * Details (Q-302 — the chat had shown "Ran into this error: … 404 at http://…: {json}. Please
 * retry…" in grey for a refusal no retry could change).
 */
export default function ProviderErrorNotice({
  notice,
  live,
  retryText,
  onRetry,
}: {
  notice: Notice;
  /** This is the chat's latest message: a retry sends the next turn. */
  live: boolean;
  retryText: string | null;
  onRetry: (text: string) => void;
}) {
  const intl = useIntl();
  return (
    <div
      role="alert"
      data-testid="provider-error-notice"
      data-class={notice.class}
      data-transient={notice.transient}
      className={cx(SURFACE.card, 'mt-2 flex flex-col overflow-hidden')}
    >
      <div className={cx(TONE_FILL.err, 'flex items-start gap-2.5 px-lz-card py-2.5')}>
        <OctagonAlert aria-hidden className="mt-0.5 size-5 shrink-0" />
        <p
          data-testid="provider-error-headline"
          className={cx('min-w-0 flex-1 text-lz-body', WEIGHT.semibold)}
        >
          {intl.formatMessage(i18n.headline, { class: notice.class })}
        </p>
      </div>
      <div className={cx(SPACE.card, 'flex flex-col gap-3')}>
        <p data-testid="provider-error-said" className={cx(TYPE.body, 'break-words')}>
          {notice.said}
        </p>
        <p data-testid="provider-error-advice" className={TYPE.bodyMuted}>
          {intl.formatMessage(notice.transient ? i18n.transient : i18n.permanent)}
        </p>
        {notice.transient && live && retryText != null && (
          <div className="flex flex-wrap items-center gap-2">
            <Button
              variant="primary"
              size="sm"
              icon={<RotateCcw />}
              data-testid="provider-error-retry"
              onClick={() => onRetry(retryText)}
            >
              {intl.formatMessage(i18n.retry)}
            </Button>
          </div>
        )}
        <Disclosure
          variant="plain"
          testId="provider-error-details"
          title={intl.formatMessage(i18n.details)}
        >
          <p data-testid="provider-error-detail" className={cx(TYPE.meta, 'break-words font-mono')}>
            {notice.detail}
          </p>
        </Disclosure>
      </div>
    </div>
  );
}
