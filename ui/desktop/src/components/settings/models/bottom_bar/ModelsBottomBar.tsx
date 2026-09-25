import { Sliders, Bot, LoaderCircle, Settings, BookOpen, ExternalLink, X, Cpu } from 'lucide-react';
import React, { useEffect, useMemo, useRef, useState } from 'react';
import { useModelAndProvider } from '../../../ModelAndProviderContext';
import { useFeatures } from '../../../../contexts/FeaturesContext';
import { useMlxEngineStatusPoll } from '../../../leanzero-swarm/useMlxEngineStatus';
import { MLX_ENTRY_LABEL, MLX_PROVIDER_ID } from '../leanzeroSelectorPolicy';
import { LeanZero } from '../../../icons';
import { LEANZERO_DOCS_URL, LEANZERO_WEBSITE_URL, SWARM_PROVIDER_ID } from '../../../../branding';
import { SwitchModelModal } from '../subcomponents/SwitchModelModal';
import { View } from '../../../../utils/navigationUtils';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '../../../ui/dropdown-menu';
import { getProviderMetadata } from '../modelInterface';
import { getModelDisplayName } from '../predefinedModelsUtils';

import { ModelSettingsPanel } from '../../localInference/ModelSettingsPanel';
import { ScrollArea } from '../../../ui/scroll-area';
import {
  Button as StudioButton,
  MOTION,
  StatusDot,
  SURFACE,
  TNUM,
  TYPE,
  WEIGHT,
  cx,
  type EnginePhase,
} from '../../../lz';
import { defineMessages, useIntl } from '../../../../i18n';
import type { Message } from '../../../../types/message';
import type { ChatServedBy } from '../../../chatServedBy/chatServedBy';
import { shortModelName } from '../../../noNodeNotice/mlxMount';
import { compactTokens } from '../../../leanzero-swarm/mlxLiveStats';

const i18n = defineMessages({
  selectModel: {
    id: 'modelsBottomBar.selectModel',
    defaultMessage: 'Select Model',
  },
  currentModel: {
    id: 'modelsBottomBar.currentModel',
    defaultMessage: 'Current model',
  },
  loadingModel: {
    id: 'modelsBottomBar.loadingModel',
    defaultMessage: 'Loading model...',
  },
  changeModel: {
    id: 'modelsBottomBar.changeModel',
    defaultMessage: 'Change Model',
  },
  changeProvider: {
    id: 'modelsBottomBar.changeProvider',
    defaultMessage: 'Change Provider',
  },
  useCloudInstead: {
    id: 'modelsBottomBar.useCloudInstead',
    defaultMessage: 'Use a cloud provider instead',
  },
  useCloudInsteadHint: {
    id: 'modelsBottomBar.useCloudInsteadHint',
    defaultMessage: 'Leaves {model} for this chat',
  },
  swarmDocs: {
    id: 'modelsBottomBar.swarmDocs',
    defaultMessage: 'Documentation',
  },
  leanzeroWebsite: {
    id: 'modelsBottomBar.leanzeroWebsite',
    defaultMessage: 'LeanZero website',
  },
  localModelSettings: {
    id: 'modelsBottomBar.localModelSettings',
    defaultMessage: 'Local Model Settings',
  },
  localModelSettingsTitle: {
    id: 'modelsBottomBar.localModelSettingsTitle',
    defaultMessage: 'Local Model Settings — {modelName}',
  },
  resolvedModel: {
    id: 'modelsBottomBar.resolvedModel',
    defaultMessage: 'Resolved model',
  },
  close: {
    id: 'modelsBottomBar.close',
    defaultMessage: 'Close',
  },
  servedChip: {
    id: 'modelsBottomBar.servedChip',
    defaultMessage: '{model} · {where}',
  },
  servedChipNotRunning: {
    id: 'modelsBottomBar.servedChipNotRunning',
    defaultMessage: '{model} · not running',
  },
  servedWhere: {
    id: 'modelsBottomBar.servedWhere',
    defaultMessage: '{phase} on {where}',
  },
  servedWhereForeign: {
    id: 'modelsBottomBar.servedWhereForeign',
    defaultMessage: '{phase} on {where} · run by another window',
  },
  servedNotRunning: {
    id: 'modelsBottomBar.servedNotRunning',
    defaultMessage: 'Not running — start it from the Engine',
  },
  servedContext: {
    id: 'modelsBottomBar.servedContext',
    defaultMessage: '{tokens}-token context',
  },
  servedContextSplit: {
    id: 'modelsBottomBar.servedContextSplit',
    defaultMessage:
      '{tokens} context on this split — sized from the memory free when it started; restart it to grow',
  },
  openEngine: {
    id: 'modelsBottomBar.openEngine',
    defaultMessage: 'Open Engine',
  },
  openEngineHint: {
    id: 'modelsBottomBar.openEngineHint',
    defaultMessage: 'Change the model or the Mac it runs on',
  },
  phaseUnloaded: { id: 'modelsBottomBar.phase.unloaded', defaultMessage: 'Not loaded' },
  phaseIdle: { id: 'modelsBottomBar.phase.idle', defaultMessage: 'Idle' },
  phaseLoading: { id: 'modelsBottomBar.phase.loading', defaultMessage: 'Loading' },
  phaseReading: { id: 'modelsBottomBar.phase.reading', defaultMessage: 'Reading a prompt' },
  phaseWriting: { id: 'modelsBottomBar.phase.writing', defaultMessage: 'Writing' },
  phaseHeld: { id: 'modelsBottomBar.phase.held', defaultMessage: 'Queued' },
  phaseFailed: { id: 'modelsBottomBar.phase.failed', defaultMessage: 'Failed' },
  phaseUnknown: { id: 'modelsBottomBar.phase.unknown', defaultMessage: 'State unknown' },
  phaseReconnecting: { id: 'modelsBottomBar.phase.reconnecting', defaultMessage: 'Reconnecting' },
});

const PHASE_WORD: Record<EnginePhase, (typeof i18n)['phaseIdle']> = {
  unloaded: i18n.phaseUnloaded,
  idle: i18n.phaseIdle,
  loading: i18n.phaseLoading,
  reading: i18n.phaseReading,
  writing: i18n.phaseWriting,
  held: i18n.phaseHeld,
  failed: i18n.phaseFailed,
};

interface ModelsBottomBarProps {
  sessionId: string | null;
  dropdownRef: React.RefObject<HTMLDivElement>;
  setView: (view: View) => void;
  sessionModel?: string | null;
  sessionProvider?: string | null;
  latestInference?: Message['metadata']['inference'] | null;
  onModelChanged: (override: { model: string; provider: string }) => void;
  sessionLoaded?: boolean;
  /**
   * Where chat goes (`deriveChatServedBy`, computed once by the composer). When it names a model,
   * the chip names THAT model and its Mac — never the provider id "swarm" (Q-5, Q-12).
   */
  served?: ChatServedBy | null;
}

export default function ModelsBottomBar({
  sessionId,
  dropdownRef,
  setView,
  sessionModel,
  sessionProvider,
  latestInference,
  onModelChanged,
  sessionLoaded,
  served = null,
}: ModelsBottomBarProps) {
  // ChatInput owns the override state and passes effective model/provider as sessionModel/sessionProvider.
  // Fall back to config defaults when no session-specific model is available.
  const {
    currentModel: configModel,
    currentProvider: configProvider,
    changeModel,
  } = useModelAndProvider();
  const currentModel = sessionModel ?? configModel;
  const currentProvider = sessionProvider ?? configProvider;
  const isSwarm = currentProvider === SWARM_PROVIDER_ID;

  // Session sync for the Leanzero MLX entry: ONLY when the session already rides the engine.
  // A remount that changes the served id updates the session to it via the SAME changeModel
  // path the selector uses. A session on a cloud provider is never yanked onto the engine,
  // and an engine that is mounting/stopped/failed never touches the session model.
  const { mlxEngine: mlxCapability } = useFeatures();
  const isMlxSession = currentProvider === MLX_PROVIDER_ID;
  const { status: mlxStatus } = useMlxEngineStatusPoll(mlxCapability && isMlxSession);
  const mlxSyncAttemptRef = useRef<string | null>(null);
  const mlxServedModelId = mlxStatus?.state === 'running' ? mlxStatus.servedModelId : undefined;
  useEffect(() => {
    if (!mlxCapability || !isMlxSession) return;
    if (!mlxServedModelId || mlxServedModelId === currentModel) return;
    // One attempt per (session, served id): a failed change surfaces its own toast and must
    // not retry on every poll tick; success flips currentModel and ends the divergence.
    const attemptKey = `${sessionId ?? 'none'}|${mlxServedModelId}`;
    if (mlxSyncAttemptRef.current === attemptKey) return;
    mlxSyncAttemptRef.current = attemptKey;
    void (async () => {
      const ok = await changeModel(sessionId, {
        name: mlxServedModelId,
        provider: MLX_PROVIDER_ID,
        subtext: MLX_ENTRY_LABEL,
      });
      if (ok) onModelChanged({ model: mlxServedModelId, provider: MLX_PROVIDER_ID });
    })();
  }, [
    mlxCapability,
    isMlxSession,
    mlxServedModelId,
    currentModel,
    sessionId,
    changeModel,
    onModelChanged,
  ]);

  const intl = useIntl();
  const [displayProvider, setDisplayProvider] = useState<string | null>(null);
  const [displayModelName, setDisplayModelName] = useState<string>(
    intl.formatMessage(i18n.selectModel)
  );
  const [isAddModelModalOpen, setIsAddModelModalOpen] = useState(false);
  const [isLocalModelSettingsOpen, setIsLocalModelSettingsOpen] = useState(false);
  const [providerDefaultModel, setProviderDefaultModel] = useState<string | null>(null);

  // Show a visible loading placeholder while session metadata is still being fetched,
  // rather than flashing the config default or leaving the footer blank.
  const isModelLoading = Boolean(sessionId && !sessionLoaded);
  const displayModel = currentModel || providerDefaultModel || displayModelName;
  const resolvedModel = latestInference?.resolvedModel ?? null;
  const shouldShowResolvedModel = Boolean(
    !isModelLoading &&
    resolvedModel &&
    latestInference?.provider === currentProvider &&
    latestInference?.requestedModel === currentModel &&
    resolvedModel !== currentModel
  );
  const loadingModelLabel = intl.formatMessage(i18n.loadingModel);
  const triggerLabel = isModelLoading ? loadingModelLabel : displayModel;
  const menuModelLabel = isModelLoading ? loadingModelLabel : displayModelName;

  useEffect(() => {
    if (!currentProvider) return;
    getProviderMetadata(currentProvider)
      .then((metadata) => {
        setDisplayProvider(metadata.display_name || currentProvider);
      })
      .catch(() => {
        setDisplayProvider(currentProvider);
      });
  }, [currentProvider, currentModel]);

  // Fetch provider default model when provider changes and no current model
  useEffect(() => {
    if (currentProvider && !currentModel) {
      (async () => {
        try {
          const metadata = await getProviderMetadata(currentProvider);
          setProviderDefaultModel(metadata.default_model);
        } catch (error) {
          console.error('Failed to get provider default model:', error);
          setProviderDefaultModel(null);
        }
      })();
    } else if (currentModel) {
      setProviderDefaultModel(null);
    }
  }, [currentProvider, currentModel]);

  useEffect(() => {
    if (!currentModel) return;
    setDisplayModelName(getModelDisplayName(currentModel));
  }, [currentModel]);

  const resolvedDisplayModelName = useMemo(
    () => (resolvedModel ? getModelDisplayName(resolvedModel) : null),
    [resolvedModel]
  );

  const handleModelSelected = (model: string, provider: string) => {
    onModelChanged({ model, provider });
  };

  // The MLX engine that serves this chat, as the one derivation names it. `where` is empty only
  // when nothing is named — the chip then keeps the provider's own label.
  const servedModel = !isModelLoading && served?.model ? shortModelName(served.model) : null;
  const servedWhere =
    served && served.where.length > 0
      ? intl.formatList(served.where, { type: 'conjunction' })
      : null;
  const servedRunning = served != null && served.engine !== 'none';
  // The amber of a Mac that stopped answering is not "Loading" — it is named for what it is.
  const phaseWord =
    served?.readiness.kind === 'reconnecting'
      ? intl.formatMessage(i18n.phaseReconnecting)
      : served?.phase
        ? intl.formatMessage(PHASE_WORD[served.phase])
        : intl.formatMessage(i18n.phaseUnknown);
  const chipLabel =
    servedModel == null
      ? null
      : servedRunning && servedWhere
        ? intl.formatMessage(i18n.servedChip, { model: servedModel, where: servedWhere })
        : intl.formatMessage(i18n.servedChipNotRunning, { model: servedModel });

  return (
    <div className="relative flex items-center" ref={dropdownRef}>
      <DropdownMenu>
        <DropdownMenuTrigger
          className={cx(
            'flex min-w-0 max-w-[180px] items-center text-lz-ink-3 hover:cursor-pointer hover:text-lz-ink md:max-w-[200px] lg:max-w-[380px]',
            MOTION
          )}
        >
          <div className="flex items-center truncate max-w-[130px] md:max-w-[200px] lg:max-w-[360px] min-w-0">
            {chipLabel != null && served?.phase ? (
              <>
                <StatusDot phase={served.phase} label={phaseWord} className="mr-1.5" />
                {/* The dot's word, on screen — not only its aria-label (Q-56). */}
                <span
                  data-testid="model-chip-phase"
                  className="mr-1.5 shrink-0 text-lz-meta font-lz-semibold"
                >
                  {phaseWord}
                </span>
              </>
            ) : (
              <Bot className="mr-1 h-4 w-4 flex-shrink-0" />
            )}
            {chipLabel != null ? (
              <span
                data-testid="model-chip-served"
                data-engine={served?.engine}
                title={served?.model ?? undefined}
                className="truncate text-lz-meta"
              >
                {chipLabel}
              </span>
            ) : isModelLoading ? (
              <span
                data-testid="model-loading-state"
                className="inline-flex items-center gap-1 truncate text-lz-meta"
              >
                <LoaderCircle className="h-3 w-3 animate-spin flex-shrink-0" />
                <span className="truncate">{triggerLabel}</span>
              </span>
            ) : (
              <span className="truncate text-lz-meta">{triggerLabel}</span>
            )}
          </div>
        </DropdownMenuTrigger>
        <DropdownMenuContent side="top" align="center" className="w-64 text-sm">
          <h6 className={cx('mt-2 ml-2', TYPE.meta)}>{intl.formatMessage(i18n.currentModel)}</h6>
          {servedModel != null && served ? (
            <div
              data-testid="model-menu-served"
              className={cx('mx-2 mb-2 flex flex-col gap-0.5 border-b pb-2', SURFACE.hairline)}
            >
              <p className={cx('break-all', TYPE.body, WEIGHT.semibold)} title={served.model ?? ''}>
                {servedModel}
              </p>
              <p className={cx('flex items-center gap-1.5', TYPE.meta)}>
                {served.phase && <StatusDot phase={served.phase} label={phaseWord} />}
                {servedRunning && servedWhere
                  ? intl.formatMessage(
                      served.foreign ? i18n.servedWhereForeign : i18n.servedWhere,
                      { phase: phaseWord, where: servedWhere }
                    )
                  : intl.formatMessage(i18n.servedNotRunning)}
              </p>
              {served.contextWindow != null && (
                <p data-testid="model-menu-context" className={cx(TYPE.meta, TNUM)}>
                  {served.contextFromFreeMemory
                    ? intl.formatMessage(i18n.servedContextSplit, {
                        tokens: compactTokens(served.contextWindow),
                      })
                    : intl.formatMessage(i18n.servedContext, {
                        tokens: served.contextWindow.toLocaleString(),
                      })}
                </p>
              )}
            </div>
          ) : (
            <p
              className={cx(
                'mx-2 mb-2 flex items-center justify-between border-b pb-2',
                TYPE.body,
                SURFACE.hairline
              )}
            >
              {menuModelLabel}
              {!isModelLoading && displayProvider && ` — ${displayProvider}`}
            </p>
          )}
          {shouldShowResolvedModel && resolvedDisplayModelName && (
            <div className={cx('mx-2 mb-2 border-b pb-2', SURFACE.hairline)}>
              <h6 className={TYPE.meta}>{intl.formatMessage(i18n.resolvedModel)}</h6>
              <p className="truncate text-lz-meta text-lz-ink" title={resolvedModel ?? undefined}>
                {resolvedDisplayModelName}
              </p>
            </div>
          )}
          {servedModel != null && (
            <DropdownMenuItem
              data-testid="model-menu-open-engine"
              onClick={() => setView('mlxEngine')}
            >
              <span className="flex min-w-0 flex-col">
                <span>{intl.formatMessage(i18n.openEngine)}</span>
                <span className={TYPE.meta}>{intl.formatMessage(i18n.openEngineHint)}</span>
              </span>
              <Cpu className="ml-auto h-4 w-4 shrink-0" />
            </DropdownMenuItem>
          )}
          <DropdownMenuItem
            data-testid="model-menu-switch"
            onClick={() => setIsAddModelModalOpen(true)}
          >
            {/* While the chip names the model on your Macs, "Change Provider" opened a picker whose
                swarm row names no model and contradicts the chip (Q-41): the one thing that
                picker still does from here is leave for a cloud provider, so it says that. */}
            {servedModel != null ? (
              <span className="flex min-w-0 flex-col">
                <span>{intl.formatMessage(i18n.useCloudInstead)}</span>
                <span className={TYPE.meta}>
                  {intl.formatMessage(i18n.useCloudInsteadHint, { model: servedModel })}
                </span>
              </span>
            ) : (
              <span>{intl.formatMessage(isSwarm ? i18n.changeProvider : i18n.changeModel)}</span>
            )}
            <Sliders className="ml-auto h-4 w-4 shrink-0 rotate-90" />
          </DropdownMenuItem>
          {isSwarm && (
            <>
              <DropdownMenuItem onClick={() => window.electron.openExternal(LEANZERO_DOCS_URL)}>
                <span>{intl.formatMessage(i18n.swarmDocs)}</span>
                <BookOpen className="ml-auto h-4 w-4" />
              </DropdownMenuItem>
              <DropdownMenuItem onClick={() => window.electron.openExternal(LEANZERO_WEBSITE_URL)}>
                <span className="flex items-center gap-1.5">
                  <LeanZero className="h-4 w-4" />
                  {intl.formatMessage(i18n.leanzeroWebsite)}
                </span>
                <ExternalLink className="ml-auto h-4 w-4" />
              </DropdownMenuItem>
            </>
          )}
          {currentProvider === 'local' && currentModel && (
            <DropdownMenuItem onClick={() => setIsLocalModelSettingsOpen(true)}>
              <span>{intl.formatMessage(i18n.localModelSettings)}</span>
              <Settings className="ml-auto h-4 w-4" />
            </DropdownMenuItem>
          )}
        </DropdownMenuContent>
      </DropdownMenu>

      {isAddModelModalOpen ? (
        <SwitchModelModal
          sessionId={sessionId}
          setView={setView}
          onClose={() => setIsAddModelModalOpen(false)}
          sessionModel={currentModel}
          sessionProvider={currentProvider}
          onModelSelected={(model, provider) => handleModelSelected(model, provider)}
        />
      ) : null}

      {isLocalModelSettingsOpen && currentModel && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
          <div className={cx(SURFACE.overlay, 'flex max-h-[80vh] w-[480px] flex-col')}>
            <div
              className={cx(
                'flex items-center justify-between border-b px-4 py-3',
                SURFACE.hairline
              )}
            >
              <h3 className={cx(TYPE.body, WEIGHT.semibold)}>
                {intl.formatMessage(i18n.localModelSettingsTitle, {
                  modelName: getModelDisplayName(currentModel),
                })}
              </h3>
              <StudioButton
                variant="ghost"
                size="sm"
                iconOnly
                aria-label={intl.formatMessage(i18n.close)}
                icon={<X />}
                onClick={() => setIsLocalModelSettingsOpen(false)}
              />
            </div>
            <ScrollArea className="flex-1 px-4 py-3 overflow-y-auto max-h-[calc(80vh-52px)]">
              <ModelSettingsPanel modelId={currentModel} />
            </ScrollArea>
          </div>
        </div>
      )}
    </div>
  );
}
