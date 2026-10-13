import { useId, useRef, useState } from 'react';
import { Loader2, Upload } from 'lucide-react';
import { defineMessages, useIntl } from '../../i18n';
import type { BenchSeriesSnapshot } from '../../benchSeries';
import { publishedIsCurrent } from '../../benchRescore';
import { Button, Chip, Panel, TYPE, WEIGHT, TNUM, FOCUS, RADIUS, SURFACE, cx } from '../lz';
import type { BenchSession } from './bridge';
import { seriesSessionKey, seriesSessions } from './seriesPresentation';

const messages = defineMessages({
  checking: { id: 'forgeSeries.checking', defaultMessage: 'Checking the two builds…' },
  noResponse: { id: 'forgeSeries.noResponse', defaultMessage: 'No publication response' },
  title: { id: 'forgeSeries.title', defaultMessage: 'Two independent builds' },
  build: { id: 'forgeSeries.build', defaultMessage: 'Build {index} of 2' },
  preparing: { id: 'forgeSeries.preparing', defaultMessage: 'Preparing build {index} of 2' },
  running: { id: 'forgeSeries.running', defaultMessage: 'Build {index} of 2 is running' },
  between: { id: 'forgeSeries.between', defaultMessage: 'Build 1 is complete. Preparing build 2.' },
  finished: { id: 'forgeSeries.finished', defaultMessage: 'Both builds are complete' },
  failed: { id: 'forgeSeries.failed', defaultMessage: 'The two-build sequence did not finish' },
  cancelled: {
    id: 'forgeSeries.cancelled',
    defaultMessage: 'The two-build sequence was cancelled',
  },
  interrupted: {
    id: 'forgeSeries.interrupted',
    defaultMessage:
      'The app ended before the two-build sequence finished. Saved evidence is preserved.',
  },
  queued: { id: 'forgeSeries.queued', defaultMessage: 'Waiting to start' },
  scored: { id: 'forgeSeries.scored', defaultMessage: 'Scored' },
  skipped: { id: 'forgeSeries.skipped', defaultMessage: 'Did not start' },
  childFailed: { id: 'forgeSeries.childFailed', defaultMessage: 'Did not finish' },
  childCancelled: { id: 'forgeSeries.childCancelled', defaultMessage: 'Cancelled' },
  view: { id: 'forgeSeries.view', defaultMessage: 'Open build {index} evidence' },
  median: { id: 'forgeSeries.median', defaultMessage: 'Local median {median} · range {min}–{max}' },
  provisional: {
    id: 'forgeSeries.provisional',
    defaultMessage: 'One scored build is provisional. A two-build median is not available.',
  },
  noScore: { id: 'forgeSeries.noScore', defaultMessage: 'No scored build is available yet.' },
  settings: {
    id: 'forgeSeries.settings',
    defaultMessage:
      'The app runs two fresh builds in sequence with the same model, task and settings. Each keeps its own score and evidence.',
  },
  publishTitle: { id: 'forgeSeries.publishTitle', defaultMessage: 'Publication title' },
  publish: { id: 'forgeSeries.publish', defaultMessage: 'Publish 2 builds' },
  continuePublish: { id: 'forgeSeries.continuePublish', defaultMessage: 'Publish remaining build' },
  publishing: { id: 'forgeSeries.publishing', defaultMessage: 'Publishing build {index} of 2…' },
  live: { id: 'forgeSeries.live', defaultMessage: 'Live on leanzero.net' },
  waitingPublish: {
    id: 'forgeSeries.waitingPublish',
    defaultMessage:
      'Publishing waits for two complete, comparable builds and their native evidence.',
  },
});

function currentPublishableSeries(
  current: BenchSeriesSnapshot | null,
  expected: BenchSeriesSnapshot
): boolean {
  return (
    current != null &&
    current.id === expected.id &&
    current.provider === expected.provider &&
    current.model === expected.model &&
    current.scorerVersion === expected.scorerVersion &&
    current.inputHash === expected.inputHash &&
    !current.running &&
    current.stage === 'finished' &&
    current.summary.count === 2 &&
    !current.summary.provisional &&
    current.builds.every(
      (build, index) =>
        build.runId === expected.builds[index].runId &&
        build.index === expected.builds[index].index &&
        build.score === expected.builds[index].score &&
        build.comparisonHash === expected.builds[index].comparisonHash
    )
  );
}

export function ForgeBuildSeries({
  series,
  sessions,
  onOpenBuild,
  onPublished,
  disabled,
}: {
  series: BenchSeriesSnapshot;
  sessions: readonly BenchSession[];
  onOpenBuild: (runKey: string) => void;
  onPublished: () => Promise<void>;
  disabled: boolean;
}) {
  const intl = useIntl();
  const titleId = useId();
  const [title, setTitle] = useState('');
  const publicationInFlight = useRef(false);
  const [publishing, setPublishing] = useState(false);
  const [publishingBuild, setPublishingBuild] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [accepted, setAccepted] = useState<
    Record<string, { url: string | null; score?: number; rescoredAt?: string }>
  >({});
  const children = seriesSessions(series.id, sessions);
  const isPosted = (session: BenchSession) => {
    const receipt = accepted[seriesSessionKey(session)];
    return (
      (receipt != null &&
        receipt.score === session.score &&
        receipt.rescoredAt === session.rescoredAt) ||
      (session.published != null && publishedIsCurrent(session.published, session.rescoredAt))
    );
  };
  const posted = children.filter(isPosted).length;
  const canPublish =
    !disabled &&
    !series.running &&
    series.stage === 'finished' &&
    series.summary.count === 2 &&
    !series.summary.provisional &&
    children.length === 2 &&
    children.every(
      (child) => child.outcome === 'finished' && (child.publishable || isPosted(child))
    );
  const stageMessage =
    series.stage === 'between-builds'
      ? messages.between
      : series.stage === 'running'
        ? messages.running
        : messages[series.stage];
  const publish = async () => {
    if (!canPublish || !title.trim() || publicationInFlight.current) return;
    publicationInFlight.current = true;
    setPublishing(true);
    setError(null);
    try {
      // A partial acceptance survives a second-build refusal; the next attempt reads the durable index.
      const [fresh, currentSeries] = await Promise.all([
        window.electron.benchmarkSessions(),
        window.electron.benchmarkSeries(series.id),
      ]);
      if (!currentPublishableSeries(currentSeries, series))
        throw new Error(intl.formatMessage(messages.waitingPublish));
      const currentChildren = seriesSessions(series.id, fresh.sessions);
      if (
        currentChildren.length !== 2 ||
        currentChildren.some(
          (child) =>
            child.outcome !== 'finished' ||
            child.score !==
              series.builds.find((build) => build.index === child.buildIndex)?.score ||
            (!child.publishable && !isPosted(child)) ||
            !series.builds.some(
              (build) => build.runId === child.runId && build.index === child.buildIndex
            )
        )
      )
        throw new Error(intl.formatMessage(messages.waitingPublish));
      for (const child of currentChildren) {
        if (isPosted(child)) continue;
        if (!currentPublishableSeries(await window.electron.benchmarkSeries(series.id), series))
          throw new Error(intl.formatMessage(messages.waitingPublish));
        const runKey = seriesSessionKey(child);
        setPublishingBuild(child.buildIndex ?? null);
        const result = await window.electron.benchmarkPublish({ title: title.trim(), runKey });
        if (!result?.ok)
          throw new Error(
            result?.error ?? result?.message ?? intl.formatMessage(messages.noResponse)
          );
        setAccepted((previous) => ({
          ...previous,
          [runKey]: { url: result.url ?? null, score: child.score, rescoredAt: child.rescoredAt },
        }));
        await onPublished();
      }
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
      await onPublished();
    } finally {
      publicationInFlight.current = false;
      setPublishing(false);
      setPublishingBuild(null);
    }
  };
  return (
    <Panel title={intl.formatMessage(messages.title)}>
      <div
        data-testid="forge-build-series"
        data-series-id={series.id}
        className="flex flex-col gap-3"
      >
        <div className="flex flex-wrap items-center gap-3">
          <span className={cx(TYPE.h2, 'break-all')}>{series.model}</span>
          <Chip tone={series.running ? 'accent' : series.stage === 'finished' ? 'ok' : 'warn'}>
            {intl.formatMessage(stageMessage, { index: series.currentBuild })}
          </Chip>
        </div>
        <p className={TYPE.bodyMuted}>{intl.formatMessage(messages.settings)}</p>
        <div className="grid gap-3 sm:grid-cols-2">
          {series.builds.map((build) => {
            const session = children.find((child) => child.buildIndex === build.index);
            const words =
              build.outcome === 'running'
                ? messages.running
                : build.outcome === 'failed'
                  ? messages.childFailed
                  : build.outcome === 'cancelled'
                    ? messages.childCancelled
                    : messages[build.outcome];
            const url =
              session && isPosted(session)
                ? session.published && publishedIsCurrent(session.published, session.rescoredAt)
                  ? session.published.url
                  : accepted[seriesSessionKey(session)]?.url
                : null;
            return (
              <div
                key={build.runId}
                data-testid={`forge-series-build-${build.index}`}
                className={cx('flex flex-col gap-2 p-3', SURFACE.outline, RADIUS.control)}
              >
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <span className={cx(TYPE.body, WEIGHT.semibold)}>
                    {intl.formatMessage(messages.build, { index: build.index })}
                  </span>
                  <span className={cx(TYPE.body, TNUM)}>
                    {build.outcome === 'scored' && typeof build.score === 'number'
                      ? build.score.toFixed(4)
                      : intl.formatMessage(words, { index: build.index })}
                  </span>
                </div>
                {build.error && (
                  <p className="text-lz-err text-lz-body break-words">{build.error}</p>
                )}
                {session && (
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => onOpenBuild(seriesSessionKey(session))}
                  >
                    {intl.formatMessage(messages.view, { index: build.index })}
                  </Button>
                )}
                {session &&
                  isPosted(session) &&
                  (url ? (
                    <a
                      href={url.startsWith('/') ? `https://leanzero.net${url}` : url}
                      target="_blank"
                      rel="noreferrer"
                      className={cx(TYPE.body, FOCUS, 'text-lz-accent underline')}
                    >
                      {intl.formatMessage(messages.live)}
                    </a>
                  ) : (
                    <span className={TYPE.body}>{intl.formatMessage(messages.live)}</span>
                  ))}
              </div>
            );
          })}
        </div>
        <p data-testid="forge-series-summary" className={cx(TYPE.body, WEIGHT.semibold, TNUM)}>
          {series.summary.count === 2 &&
          !series.summary.provisional &&
          series.summary.median != null &&
          series.summary.min != null &&
          series.summary.max != null
            ? intl.formatMessage(messages.median, {
                median: series.summary.median.toFixed(4),
                min: series.summary.min.toFixed(4),
                max: series.summary.max.toFixed(4),
              })
            : intl.formatMessage(
                series.summary.count === 1 ? messages.provisional : messages.noScore
              )}
        </p>
        {series.summary.reason && <p className={TYPE.bodyMuted}>{series.summary.reason}</p>}
        {posted < 2 ? (
          <div className="flex flex-wrap items-end gap-3">
            <div className="w-full max-w-sm">
              <label htmlFor={titleId} className={cx('mb-1 block', TYPE.meta)}>
                {intl.formatMessage(messages.publishTitle)}
              </label>
              <input
                id={titleId}
                value={title}
                onChange={(event) => setTitle(event.target.value.slice(0, 80))}
                maxLength={80}
                disabled={publishing}
                placeholder={series.model}
                className={cx(
                  'h-8 w-full bg-lz-surface px-3 text-lz-body text-lz-ink',
                  SURFACE.outline,
                  RADIUS.control,
                  FOCUS
                )}
              />
            </div>
            <Button
              icon={publishing ? <Loader2 className="animate-spin" /> : <Upload />}
              onClick={() => void publish()}
              disabled={!canPublish || !title.trim() || publishing}
            >
              {intl.formatMessage(
                publishingBuild != null
                  ? messages.publishing
                  : publishing
                    ? messages.checking
                    : posted === 1
                      ? messages.continuePublish
                      : messages.publish,
                { index: publishingBuild }
              )}
            </Button>
          </div>
        ) : null}
        {!canPublish && posted < 2 && (
          <p className={TYPE.meta}>{intl.formatMessage(messages.waitingPublish)}</p>
        )}
        {error && (
          <p role="alert" className="text-lz-err text-lz-body break-words">
            {error}
          </p>
        )}
      </div>
    </Panel>
  );
}
