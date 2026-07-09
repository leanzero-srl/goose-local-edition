import React, { useState, useEffect, FormEvent, useCallback } from 'react';
import type { CreateScheduleRequest_unstable } from '@aaif/goose-sdk';
import { Card } from '../ui/card';
import { Button } from '../ui/button';
import { Input } from '../ui/input';
import { CronPicker } from '../schedule/CronPicker';
import { Recipe, parseDeeplink, parseRecipeFromFile } from '../../recipe';
import { getStorageDirectory } from '../../recipe/recipe_management';
import { Repeat } from 'lucide-react';
import { defineMessages, useIntl } from '../../i18n';

const i18n = defineMessages({
  createNewLoop: { id: 'loopModal.createNewLoop', defaultMessage: 'Create New Loop' },
  subtitle: {
    id: 'loopModal.subtitle',
    defaultMessage: 'A loop runs a recipe repeatedly until a stop check passes or the iteration cap is reached.',
  },
  nameLabel: { id: 'loopModal.nameLabel', defaultMessage: 'Name:' },
  namePlaceholder: { id: 'loopModal.namePlaceholder', defaultMessage: 'e.g., nightly-refine-loop' },
  sourceLabel: { id: 'loopModal.sourceLabel', defaultMessage: 'Recipe:' },
  yaml: { id: 'loopModal.yaml', defaultMessage: 'YAML' },
  deepLink: { id: 'loopModal.deepLink', defaultMessage: 'Deep link' },
  browseYaml: { id: 'loopModal.browseYaml', defaultMessage: 'Browse for YAML file...' },
  selected: { id: 'loopModal.selected', defaultMessage: 'Selected: {path}' },
  deepLinkPlaceholder: { id: 'loopModal.deepLinkPlaceholder', defaultMessage: 'Paste goose://recipe link here...' },
  recipeParsed: { id: 'loopModal.recipeParsed', defaultMessage: 'Recipe parsed successfully' },
  recipeTitle: { id: 'loopModal.recipeTitle', defaultMessage: 'Title: {title}' },
  recipeDescription: { id: 'loopModal.recipeDescription', defaultMessage: 'Description: {description}' },
  scheduleLabel: { id: 'loopModal.scheduleLabel', defaultMessage: 'Schedule:' },
  maxIterationsLabel: { id: 'loopModal.maxIterationsLabel', defaultMessage: 'Max iterations:' },
  maxIterationsHelp: {
    id: 'loopModal.maxIterationsHelp',
    defaultMessage: 'Hard cap on iterations regardless of the stop check.',
  },
  stopCheckLabel: { id: 'loopModal.stopCheckLabel', defaultMessage: 'Stop check command:' },
  stopCheckPlaceholder: { id: 'loopModal.stopCheckPlaceholder', defaultMessage: 'e.g., test -f done.flag' },
  stopCheckHelp: {
    id: 'loopModal.stopCheckHelp',
    defaultMessage: 'Optional shell command run after each iteration; exit 0 stops the loop.',
  },
  stateArtifactLabel: { id: 'loopModal.stateArtifactLabel', defaultMessage: 'State artifact:' },
  stateArtifactPlaceholder: { id: 'loopModal.stateArtifactPlaceholder', defaultMessage: 'e.g., state.md' },
  stateArtifactHelp: {
    id: 'loopModal.stateArtifactHelp',
    defaultMessage: 'Optional file the recipe writes each iteration; its contents carry into the next.',
  },
  cancel: { id: 'loopModal.cancel', defaultMessage: 'Cancel' },
  creating: { id: 'loopModal.creating', defaultMessage: 'Creating...' },
  createLoop: { id: 'loopModal.createLoop', defaultMessage: 'Create Loop' },
  invalidDeepLink: { id: 'loopModal.invalidDeepLink', defaultMessage: 'Invalid deep link. Please use a goose://recipe link.' },
  failedReadFile: { id: 'loopModal.failedReadFile', defaultMessage: 'Failed to read the selected file.' },
  failedParseRecipe: { id: 'loopModal.failedParseRecipe', defaultMessage: 'Failed to parse recipe from file.' },
  invalidFileType: { id: 'loopModal.invalidFileType', defaultMessage: 'Invalid file type: Please select a YAML file (.yaml or .yml)' },
  loopIdRequired: { id: 'loopModal.loopIdRequired', defaultMessage: 'Loop name is required.' },
  provideValidRecipe: { id: 'loopModal.provideValidRecipe', defaultMessage: 'Please provide a valid recipe source.' },
  maxIterationsRequired: { id: 'loopModal.maxIterationsRequired', defaultMessage: 'Max iterations must be at least 1.' },
});

export type NewLoopPayload = CreateScheduleRequest_unstable & {
  recipe: Recipe;
};

interface LoopModalProps {
  isOpen: boolean;
  onClose: () => void;
  onSubmit: (payload: NewLoopPayload) => Promise<void>;
  isLoadingExternally: boolean;
  apiErrorExternally: string | null;
}

type SourceType = 'file' | 'deeplink';

const modalLabelClassName = 'block text-sm font-medium text-text-primary mb-1';
const helpTextClassName = 'mt-1 text-xs text-text-secondary';

export const LoopModal: React.FC<LoopModalProps> = ({
  isOpen,
  onClose,
  onSubmit,
  isLoadingExternally,
  apiErrorExternally,
}) => {
  const intl = useIntl();

  const [loopId, setLoopId] = useState<string>('');
  const [sourceType, setSourceType] = useState<SourceType>('file');
  const [recipeSourcePath, setRecipeSourcePath] = useState<string>('');
  const [deepLinkInput, setDeepLinkInput] = useState<string>('');
  const [parsedRecipe, setParsedRecipe] = useState<Recipe | null>(null);
  const [cronExpression, setCronExpression] = useState<string>('0 0 14 * * *');
  const [maxIterations, setMaxIterations] = useState<string>('10');
  const [stopCheckCommand, setStopCheckCommand] = useState<string>('');
  const [stateArtifact, setStateArtifact] = useState<string>('');
  const [internalValidationError, setInternalValidationError] = useState<string | null>(null);
  const [isValid, setIsValid] = useState(true);

  const setLoopIdFromTitle = (title: string) => {
    const cleanId = title
      .toLowerCase()
      .replace(/[^a-z0-9-]/g, '-')
      .replace(/-+/g, '-');
    setLoopId(cleanId);
  };

  const handleDeepLinkChange = useCallback(
    async (value: string) => {
      setDeepLinkInput(value);
      setInternalValidationError(null);

      if (value.trim()) {
        try {
          const recipe = await parseDeeplink(value.trim());
          if (!recipe) throw new Error();
          setParsedRecipe(recipe);
          if (recipe.title) {
            setLoopIdFromTitle(recipe.title);
          }
        } catch {
          setParsedRecipe(null);
          setInternalValidationError(intl.formatMessage(i18n.invalidDeepLink));
        }
      } else {
        setParsedRecipe(null);
      }
    },
    [intl]
  );

  useEffect(() => {
    if (isOpen) {
      setLoopId('');
      setSourceType('file');
      setRecipeSourcePath('');
      setDeepLinkInput('');
      setParsedRecipe(null);
      setCronExpression('0 0 14 * * *');
      setMaxIterations('10');
      setStopCheckCommand('');
      setStateArtifact('');
      setInternalValidationError(null);
    }
  }, [isOpen]);

  const handleBrowseFile = async () => {
    const defaultPath = getStorageDirectory(true);
    const filePath = await window.electron.selectFileOrDirectory(defaultPath);
    if (filePath) {
      if (filePath.endsWith('.yaml') || filePath.endsWith('.yml')) {
        setRecipeSourcePath(filePath);
        setInternalValidationError(null);

        try {
          const fileResponse = await window.electron.readFile(filePath);
          if (!fileResponse.found || fileResponse.error) {
            throw new Error(intl.formatMessage(i18n.failedReadFile));
          }
          const recipe = await parseRecipeFromFile(fileResponse.file);
          if (!recipe) {
            throw new Error(intl.formatMessage(i18n.failedParseRecipe));
          }
          setParsedRecipe(recipe);
          if (recipe.title) {
            setLoopIdFromTitle(recipe.title);
          }
        } catch (e) {
          setParsedRecipe(null);
          setInternalValidationError(
            e instanceof Error ? e.message : intl.formatMessage(i18n.failedParseRecipe)
          );
        }
      } else {
        setInternalValidationError(intl.formatMessage(i18n.invalidFileType));
      }
    }
  };

  const handleLocalSubmit = async (event: FormEvent) => {
    event.preventDefault();
    setInternalValidationError(null);

    if (!loopId.trim()) {
      setInternalValidationError(intl.formatMessage(i18n.loopIdRequired));
      return;
    }

    if (!parsedRecipe) {
      setInternalValidationError(intl.formatMessage(i18n.provideValidRecipe));
      return;
    }

    const iterations = Number.parseInt(maxIterations, 10);
    if (!Number.isFinite(iterations) || iterations < 1) {
      setInternalValidationError(intl.formatMessage(i18n.maxIterationsRequired));
      return;
    }

    const trimmedStopCheck = stopCheckCommand.trim();
    const trimmedStateArtifact = stateArtifact.trim();

    const newLoopPayload: NewLoopPayload = {
      id: loopId.trim(),
      recipe: parsedRecipe,
      cron: cronExpression,
      loop_config: {
        maxIterations: iterations,
        stopCheckCommand: trimmedStopCheck ? trimmedStopCheck : null,
        stateArtifact: trimmedStateArtifact ? trimmedStateArtifact : null,
      },
    };

    await onSubmit(newLoopPayload);
  };

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 bg-black/50 z-40 flex items-center justify-center p-4">
      <Card className="w-full max-w-md bg-background-primary shadow-xl rounded-3xl z-50 flex flex-col max-h-[90vh] overflow-hidden">
        <div className="px-8 pt-6 pb-4 flex-shrink-0">
          <div className="flex items-center gap-3">
            <div className="flex h-8 w-8 items-center justify-center rounded-full bg-blue-600 text-white">
              <Repeat className="h-4 w-4" />
            </div>
            <div className="flex-1">
              <h2 className="text-base font-semibold text-text-primary">
                {intl.formatMessage(i18n.createNewLoop)}
              </h2>
              <p className="text-sm text-text-secondary">{intl.formatMessage(i18n.subtitle)}</p>
            </div>
          </div>
        </div>

        <form
          id="loop-form"
          onSubmit={handleLocalSubmit}
          className="px-8 py-4 space-y-4 flex-grow overflow-y-auto"
        >
          {apiErrorExternally && (
            <p className="text-text-danger text-sm mb-3 p-2 border border-border-danger rounded-md">
              {apiErrorExternally}
            </p>
          )}
          {internalValidationError && (
            <p className="text-text-danger text-sm mb-3 p-2 border border-border-danger rounded-md">
              {internalValidationError}
            </p>
          )}

          <div>
            <label htmlFor="loopId-modal" className={modalLabelClassName}>
              {intl.formatMessage(i18n.nameLabel)} <span className="text-red-500">*</span>
            </label>
            <Input
              type="text"
              id="loopId-modal"
              value={loopId}
              onChange={(e) => setLoopId(e.target.value)}
              placeholder={intl.formatMessage(i18n.namePlaceholder)}
              required
            />
          </div>

          <div>
            <label className={modalLabelClassName}>
              {intl.formatMessage(i18n.sourceLabel)} <span className="text-red-500">*</span>
            </label>
            <div className="space-y-2">
              <div className="flex bg-gray-100 dark:bg-gray-700 rounded-full p-1">
                <button
                  type="button"
                  onClick={() => setSourceType('file')}
                  className={`flex-1 px-4 py-2 text-sm font-medium rounded-full transition-all ${
                    sourceType === 'file'
                      ? 'bg-white dark:bg-gray-800 text-gray-900 dark:text-white shadow-sm'
                      : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white'
                  }`}
                >
                  {intl.formatMessage(i18n.yaml)}
                </button>
                <button
                  type="button"
                  onClick={() => setSourceType('deeplink')}
                  className={`flex-1 px-4 py-2 text-sm font-medium rounded-full transition-all ${
                    sourceType === 'deeplink'
                      ? 'bg-white dark:bg-gray-800 text-gray-900 dark:text-white shadow-sm'
                      : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white'
                  }`}
                >
                  {intl.formatMessage(i18n.deepLink)}
                </button>
              </div>

              {sourceType === 'file' && (
                <div>
                  <Button
                    type="button"
                    variant="outline"
                    onClick={handleBrowseFile}
                    className="w-full justify-center rounded-full"
                  >
                    {intl.formatMessage(i18n.browseYaml)}
                  </Button>
                  {recipeSourcePath && (
                    <p className="mt-2 text-xs text-gray-500 dark:text-gray-400 italic">
                      {intl.formatMessage(i18n.selected, { path: recipeSourcePath })}
                    </p>
                  )}
                </div>
              )}

              {sourceType === 'deeplink' && (
                <div>
                  <Input
                    type="text"
                    value={deepLinkInput}
                    onChange={(e) => handleDeepLinkChange(e.target.value)}
                    placeholder={intl.formatMessage(i18n.deepLinkPlaceholder)}
                    className="rounded-full"
                  />
                  {parsedRecipe && (
                    <div className="mt-2 p-2 bg-green-600 rounded-md">
                      <p className="text-xs text-white font-semibold">
                        ✓ {intl.formatMessage(i18n.recipeParsed)}
                      </p>
                      <p className="text-xs text-white/90">
                        {intl.formatMessage(i18n.recipeTitle, { title: parsedRecipe.title })}
                      </p>
                      <p className="text-xs text-green-600 dark:text-green-400">
                        {intl.formatMessage(i18n.recipeDescription, {
                          description: parsedRecipe.description,
                        })}
                      </p>
                    </div>
                  )}
                </div>
              )}
            </div>
          </div>

          <div>
            <label className={modalLabelClassName}>{intl.formatMessage(i18n.scheduleLabel)}</label>
            <CronPicker schedule={null} onChange={setCronExpression} isValid={setIsValid} />
          </div>

          <div>
            <label htmlFor="loop-max-iterations" className={modalLabelClassName}>
              {intl.formatMessage(i18n.maxIterationsLabel)} <span className="text-red-500">*</span>
            </label>
            <Input
              type="number"
              id="loop-max-iterations"
              min={1}
              step={1}
              value={maxIterations}
              onChange={(e) => setMaxIterations(e.target.value)}
              required
            />
            <p className={helpTextClassName}>{intl.formatMessage(i18n.maxIterationsHelp)}</p>
          </div>

          <div>
            <label htmlFor="loop-stop-check" className={modalLabelClassName}>
              {intl.formatMessage(i18n.stopCheckLabel)}
            </label>
            <Input
              type="text"
              id="loop-stop-check"
              value={stopCheckCommand}
              onChange={(e) => setStopCheckCommand(e.target.value)}
              placeholder={intl.formatMessage(i18n.stopCheckPlaceholder)}
            />
            <p className={helpTextClassName}>{intl.formatMessage(i18n.stopCheckHelp)}</p>
          </div>

          <div>
            <label htmlFor="loop-state-artifact" className={modalLabelClassName}>
              {intl.formatMessage(i18n.stateArtifactLabel)}
            </label>
            <Input
              type="text"
              id="loop-state-artifact"
              value={stateArtifact}
              onChange={(e) => setStateArtifact(e.target.value)}
              placeholder={intl.formatMessage(i18n.stateArtifactPlaceholder)}
            />
            <p className={helpTextClassName}>{intl.formatMessage(i18n.stateArtifactHelp)}</p>
          </div>
        </form>

        <div className="flex gap-2 px-8 py-4 border-t border-border-primary">
          <Button
            type="button"
            variant="ghost"
            onClick={onClose}
            disabled={isLoadingExternally}
            className="flex-1 text-gray-400 hover:bg-gray-50 dark:hover:bg-gray-800"
          >
            {intl.formatMessage(i18n.cancel)}
          </Button>
          <Button type="submit" form="loop-form" disabled={isLoadingExternally || !isValid} className="flex-1">
            {isLoadingExternally
              ? intl.formatMessage(i18n.creating)
              : intl.formatMessage(i18n.createLoop)}
          </Button>
        </div>
      </Card>
    </div>
  );
};
