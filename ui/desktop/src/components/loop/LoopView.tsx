import React, { useState, useEffect, useMemo } from 'react';
import type { ScheduledJobDto } from '@aaif/goose-sdk';
import { acpListSchedules, acpCreateSchedule, acpDeleteSchedule } from '../../acp/schedules';
import { ScrollArea } from '../ui/scroll-area';
import { Card } from '../ui/card';
import { Button } from '../ui/button';
import { ConfirmationModal } from '../ui/ConfirmationModal';
import { TrashIcon } from '../icons/TrashIcon';
import { Plus, RefreshCw, Repeat, CircleDotDashed, Hash, Terminal, FileText } from 'lucide-react';
import { LoopModal, NewLoopPayload } from './LoopModal';
import { toastSuccess } from '../../toasts';
import cronstrue from 'cronstrue';
import { formatToLocalDateWithTimezone } from '../../utils/date';
import { errorMessage } from '../../utils/conversionUtils';
import { MainPanelLayout } from '../Layout/MainPanelLayout';
import { defineMessages, useIntl } from '../../i18n';

const i18n = defineMessages({
  loop: { id: 'loopView.loop', defaultMessage: 'Loop' },
  description: {
    id: 'loopView.description',
    defaultMessage:
      'Loops run a recipe repeatedly on a schedule until a stop check passes or the iteration cap is reached.',
  },
  refreshing: { id: 'loopView.refreshing', defaultMessage: 'Refreshing...' },
  refresh: { id: 'loopView.refresh', defaultMessage: 'Refresh' },
  createLoop: { id: 'loopView.createLoop', defaultMessage: 'Create Loop' },
  errorPrefix: { id: 'loopView.errorPrefix', defaultMessage: 'Error: {error}' },
  noLoops: { id: 'loopView.noLoops', defaultMessage: 'No loops yet' },
  lastRun: { id: 'loopView.lastRun', defaultMessage: 'Last run: {date}' },
  maxIterations: { id: 'loopView.maxIterations', defaultMessage: '{count} max iterations' },
  stopCheck: { id: 'loopView.stopCheck', defaultMessage: 'Stop check: {command}' },
  stateArtifact: { id: 'loopView.stateArtifact', defaultMessage: 'State: {file}' },
  confirmDelete: {
    id: 'loopView.confirmDelete',
    defaultMessage: 'Remove loop "{id}"? The recipe will be kept.',
  },
  deleteTitle: {
    id: 'loopView.deleteTitle',
    defaultMessage: 'Remove loop',
  },
  deleteConfirm: {
    id: 'loopView.deleteConfirm',
    defaultMessage: 'Remove',
  },
  loopCreated: { id: 'loopView.loopCreated', defaultMessage: 'Loop Created' },
  loopCreatedMsg: { id: 'loopView.loopCreatedMsg', defaultMessage: 'Successfully created loop "{id}"' },
});

const LoopCard: React.FC<{
  job: ScheduledJobDto;
  onDelete: (id: string) => void;
  actionInProgress: boolean;
}> = ({ job, onDelete, actionInProgress }) => {
  const intl = useIntl();
  const loopConfig = job.loopConfig;

  let readableCron: string;
  try {
    readableCron = cronstrue.toString(job.cron);
  } catch {
    readableCron = job.cron;
  }

  const formattedLastRun = formatToLocalDateWithTimezone(job.lastRun);

  return (
    <Card className="py-3 px-4 mb-2 bg-background-primary border border-border-primary hover:bg-background-secondary transition-all duration-150">
      <div className="flex justify-between items-start gap-4">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2 mb-1">
            <Repeat className="h-4 w-4 text-blue-600 shrink-0" />
            <h3 className="text-base truncate max-w-[50vw]" title={job.id}>
              {job.id}
            </h3>
          </div>
          <p className="text-text-secondary text-sm mb-2 line-clamp-2" title={readableCron}>
            {readableCron}
          </p>
          <div className="flex flex-wrap items-center gap-2 mb-2">
            {loopConfig && (
              <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-semibold bg-blue-600 text-white">
                <Hash className="w-3 h-3" />
                {intl.formatMessage(i18n.maxIterations, { count: loopConfig.maxIterations })}
              </span>
            )}
            {loopConfig?.stopCheckCommand && (
              <span
                className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-semibold bg-purple-600 text-white max-w-full"
                title={loopConfig.stopCheckCommand}
              >
                <Terminal className="w-3 h-3 shrink-0" />
                <span className="truncate">
                  {intl.formatMessage(i18n.stopCheck, { command: loopConfig.stopCheckCommand })}
                </span>
              </span>
            )}
            {loopConfig?.stateArtifact && (
              <span
                className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-semibold bg-teal-600 text-white max-w-full"
                title={loopConfig.stateArtifact}
              >
                <FileText className="w-3 h-3 shrink-0" />
                <span className="truncate">
                  {intl.formatMessage(i18n.stateArtifact, { file: loopConfig.stateArtifact })}
                </span>
              </span>
            )}
          </div>
          <div className="flex items-center text-xs text-text-secondary">
            <span>{intl.formatMessage(i18n.lastRun, { date: formattedLastRun })}</span>
          </div>
        </div>

        <div className="flex items-center gap-2 shrink-0">
          <Button
            onClick={(e) => {
              e.stopPropagation();
              onDelete(job.id);
            }}
            disabled={actionInProgress}
            variant="ghost"
            size="sm"
            className="h-8 text-red-500 hover:text-red-600 hover:bg-red-50 dark:hover:bg-red-900/20"
          >
            <TrashIcon className="w-4 h-4" />
          </Button>
        </div>
      </div>
    </Card>
  );
};

const LoopView: React.FC = () => {
  const intl = useIntl();
  const [jobs, setJobs] = useState<ScheduledJobDto[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [apiError, setApiError] = useState<string | null>(null);
  const [submitApiError, setSubmitApiError] = useState<string | null>(null);
  const [isModalOpen, setIsModalOpen] = useState(false);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [actionsInProgress, setActionsInProgress] = useState<Set<string>>(new Set());
  const [pendingDeleteId, setPendingDeleteId] = useState<string | null>(null);

  const loops = useMemo(() => jobs.filter((job) => job.loopConfig != null), [jobs]);

  const fetchLoops = async () => {
    setIsLoading(true);
    setApiError(null);
    try {
      const fetched = await acpListSchedules();
      setJobs(fetched);
    } catch (error) {
      console.error('Failed to fetch loops:', error);
      setApiError(errorMessage(error, 'An unknown error occurred while fetching loops.'));
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    fetchLoops();
  }, []);

  useEffect(() => {
    if (actionsInProgress.size > 0) return;

    const intervalId = setInterval(() => {
      if (!isRefreshing && !isLoading && !isSubmitting) {
        fetchLoops();
      }
    }, 15000);

    return () => clearInterval(intervalId);
  }, [isRefreshing, isLoading, isSubmitting, actionsInProgress.size]);

  const handleRefresh = async () => {
    setIsRefreshing(true);
    try {
      await fetchLoops();
    } finally {
      setIsRefreshing(false);
    }
  };

  const handleModalSubmit = async (payload: NewLoopPayload) => {
    setIsSubmitting(true);
    setSubmitApiError(null);
    try {
      await acpCreateSchedule(payload);
      toastSuccess({
        title: intl.formatMessage(i18n.loopCreated),
        msg: intl.formatMessage(i18n.loopCreatedMsg, { id: payload.id }),
      });
      await fetchLoops();
      setIsModalOpen(false);
    } catch (error) {
      console.error('Failed to save loop:', error);
      setSubmitApiError(errorMessage(error, 'Unknown error saving loop.'));
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleDeleteLoop = async (id: string) => {
    setActionsInProgress((prev) => new Set(prev).add(id));
    setApiError(null);

    try {
      await acpDeleteSchedule(id);
      await fetchLoops();
    } catch (error) {
      console.error(`Failed to remove loop "${id}":`, error);
      setApiError(errorMessage(error, `Unknown error removing loop "${id}".`));
    } finally {
      setActionsInProgress((prev) => {
        const newSet = new Set(prev);
        newSet.delete(id);
        return newSet;
      });
    }
  };

  return (
    <>
      <MainPanelLayout>
        <div className="flex-1 flex flex-col min-h-0">
          <div className="bg-background-primary px-8 pb-8 pt-16">
            <div className="flex flex-col page-transition">
              <div className="flex justify-between items-center mb-1">
                <h1 className="text-4xl font-light">{intl.formatMessage(i18n.loop)}</h1>
                <div className="flex gap-2">
                  <Button
                    onClick={handleRefresh}
                    disabled={isRefreshing || isLoading}
                    variant="outline"
                    size="sm"
                    className="flex items-center gap-2"
                  >
                    <RefreshCw className={`h-4 w-4 ${isRefreshing ? 'animate-spin' : ''}`} />
                    {isRefreshing ? intl.formatMessage(i18n.refreshing) : intl.formatMessage(i18n.refresh)}
                  </Button>
                  <Button
                    onClick={() => {
                      setSubmitApiError(null);
                      setIsModalOpen(true);
                    }}
                    size="sm"
                    className="flex items-center gap-2"
                  >
                    <Plus className="h-4 w-4" />
                    {intl.formatMessage(i18n.createLoop)}
                  </Button>
                </div>
              </div>
              <p className="text-sm text-text-secondary mb-1">{intl.formatMessage(i18n.description)}</p>
            </div>
          </div>

          <div className="flex-1 min-h-0 relative px-8">
            <ScrollArea className="h-full">
              <div className="h-full relative">
                {apiError && (
                  <div className="mb-4 p-4 bg-background-danger border border-border-danger rounded-md">
                    <p className="text-text-danger text-sm">
                      {intl.formatMessage(i18n.errorPrefix, { error: apiError })}
                    </p>
                  </div>
                )}

                {isLoading && loops.length === 0 && (
                  <div className="flex justify-center items-center py-12">
                    <div className="animate-spin rounded-full h-8 w-8 border-t-2 border-b-2"></div>
                  </div>
                )}

                {!isLoading && !apiError && loops.length === 0 && (
                  <div className="flex flex-col pt-4 pb-12">
                    <CircleDotDashed className="h-5 w-5 text-text-secondary mb-3.5" />
                    <p className="text-base text-text-secondary font-light mb-2">
                      {intl.formatMessage(i18n.noLoops)}
                    </p>
                  </div>
                )}

                {!isLoading && loops.length > 0 && (
                  <div className="space-y-2 pb-8">
                    {loops.map((job) => (
                      <LoopCard
                        key={job.id}
                        job={job}
                        onDelete={(id) => setPendingDeleteId(id)}
                        actionInProgress={actionsInProgress.has(job.id) || isSubmitting}
                      />
                    ))}
                  </div>
                )}
              </div>
            </ScrollArea>
          </div>
        </div>
      </MainPanelLayout>

      <LoopModal
        isOpen={isModalOpen}
        onClose={() => {
          setIsModalOpen(false);
          setSubmitApiError(null);
        }}
        onSubmit={handleModalSubmit}
        isLoadingExternally={isSubmitting}
        apiErrorExternally={submitApiError}
      />

      <ConfirmationModal
        isOpen={pendingDeleteId !== null}
        title={intl.formatMessage(i18n.deleteTitle)}
        message={
          pendingDeleteId
            ? intl.formatMessage(i18n.confirmDelete, { id: pendingDeleteId })
            : ''
        }
        confirmLabel={intl.formatMessage(i18n.deleteConfirm)}
        onConfirm={() => {
          const id = pendingDeleteId;
          setPendingDeleteId(null);
          if (id) void handleDeleteLoop(id);
        }}
        onCancel={() => setPendingDeleteId(null)}
      />
    </>
  );
};

export default LoopView;
