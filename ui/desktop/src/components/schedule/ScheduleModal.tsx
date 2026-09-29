import React, { useState, useEffect, FormEvent, useCallback } from 'react';
import type { ScheduledJobDto } from '@aaif/goose-sdk';
import { Card } from '../ui/card';
import { Button } from '../ui/button';
import { Input } from '../ui/input';
import { CronPicker } from './CronPicker';
import { Recipe, parseDeeplink, parseRecipeFromFile } from '../../recipe';
import { getStorageDirectory } from '../../recipe/recipe_management';
import { getInitialWorkingDir } from '../../utils/workingDir';
import ClockIcon from '../../assets/clock-icon.svg';
import { defineMessages, useIntl } from '../../i18n';

const i18n = defineMessages({
  editSchedule: { id: 'scheduleModal.editSchedule', defaultMessage: 'Edit Schedule' },
  createNewSchedule: {
    id: 'scheduleModal.createNewSchedule',
    defaultMessage: 'Create New Schedule',
  },
  nameLabel: { id: 'scheduleModal.nameLabel', defaultMessage: 'Name:' },
  namePlaceholder: {
    id: 'scheduleModal.namePlaceholder',
    defaultMessage: 'e.g., daily-summary-job',
  },
  sourceLabel: { id: 'scheduleModal.sourceLabel', defaultMessage: 'Source:' },
  yaml: { id: 'scheduleModal.yaml', defaultMessage: 'YAML' },
  deepLink: { id: 'scheduleModal.deepLink', defaultMessage: 'Deep link' },
  browseYaml: { id: 'scheduleModal.browseYaml', defaultMessage: 'Browse for YAML file...' },
  selected: { id: 'scheduleModal.selected', defaultMessage: 'Selected: {path}' },
  deepLinkPlaceholder: {
    id: 'scheduleModal.deepLinkPlaceholder',
    defaultMessage: 'Paste goose://recipe link here...',
  },
  recipeParsed: { id: 'scheduleModal.recipeParsed', defaultMessage: 'Recipe parsed successfully' },
  recipeTitle: { id: 'scheduleModal.recipeTitle', defaultMessage: 'Title: {title}' },
  recipeDescription: {
    id: 'scheduleModal.recipeDescription',
    defaultMessage: 'Description: {description}',
  },
  scheduleLabel: { id: 'scheduleModal.scheduleLabel', defaultMessage: 'Schedule:' },
  cancel: { id: 'scheduleModal.cancel', defaultMessage: 'Cancel' },
  updating: { id: 'scheduleModal.updating', defaultMessage: 'Updating...' },
  creating: { id: 'scheduleModal.creating', defaultMessage: 'Creating...' },
  updateSchedule: { id: 'scheduleModal.updateSchedule', defaultMessage: 'Update Schedule' },
  createSchedule: { id: 'scheduleModal.createSchedule', defaultMessage: 'Create Schedule' },
  invalidDeepLink: {
    id: 'scheduleModal.invalidDeepLink',
    defaultMessage: 'Invalid deep link. Please use a goose://recipe link.',
  },
  failedReadFile: {
    id: 'scheduleModal.failedReadFile',
    defaultMessage: 'Failed to read the selected file.',
  },
  failedParseRecipe: {
    id: 'scheduleModal.failedParseRecipe',
    defaultMessage: 'Failed to parse recipe from file.',
  },
  invalidFileType: {
    id: 'scheduleModal.invalidFileType',
    defaultMessage: 'Invalid file type: Please select a YAML file (.yaml or .yml)',
  },
  scheduleIdRequired: {
    id: 'scheduleModal.scheduleIdRequired',
    defaultMessage: 'Schedule ID is required.',
  },
  provideValidRecipe: {
    id: 'scheduleModal.provideValidRecipe',
    defaultMessage: 'Please provide a valid recipe source.',
  },
  folderLabel: { id: 'scheduleModal.folderLabel', defaultMessage: 'Runs in folder:' },
  chooseFolder: { id: 'scheduleModal.chooseFolder', defaultMessage: 'Choose folder...' },
  noFolder: {
    id: 'scheduleModal.noFolder',
    defaultMessage: 'No folder yet. This schedule does not run until you choose one.',
  },
  folderRequired: {
    id: 'scheduleModal.folderRequired',
    defaultMessage: 'Choose the folder this schedule runs in.',
  },
});

// Q-282: every schedule runs in a folder its owner chose — this window's by default. goose serves
// every window from one process, so its own folder is nobody's project.
export interface NewSchedulePayload {
  id: string;
  recipe: Recipe;
  cron: string;
  working_dir: string;
}

export interface ScheduleEditPayload {
  cron: string;
  workingDir: string;
}

interface ScheduleModalProps {
  isOpen: boolean;
  onClose: () => void;
  onSubmit: (payload: NewSchedulePayload | ScheduleEditPayload) => Promise<void>;
  schedule: ScheduledJobDto | null;
  isLoadingExternally: boolean;
  apiErrorExternally: string | null;
  initialDeepLink: string | null;
}

type SourceType = 'file' | 'deeplink';

const modalLabelClassName = 'block text-sm font-medium text-text-primary mb-1';

export const ScheduleModal: React.FC<ScheduleModalProps> = ({
  isOpen,
  onClose,
  onSubmit,
  schedule,
  isLoadingExternally,
  apiErrorExternally,
  initialDeepLink,
}) => {
  const intl = useIntl();
  const isEditMode = !!schedule;

  const [scheduleId, setScheduleId] = useState<string>('');
  const [sourceType, setSourceType] = useState<SourceType>('file');
  const [recipeSourcePath, setRecipeSourcePath] = useState<string>('');
  const [deepLinkInput, setDeepLinkInput] = useState<string>('');
  const [parsedRecipe, setParsedRecipe] = useState<Recipe | null>(null);
  const [cronExpression, setCronExpression] = useState<string>('0 0 14 * * *');
  const [internalValidationError, setInternalValidationError] = useState<string | null>(null);
  const [isValid, setIsValid] = useState(true);
  const [workingDir, setWorkingDir] = useState<string>('');

  const setScheduleIdFromTitle = (title: string) => {
    const cleanId = title
      .toLowerCase()
      .replace(/[^a-z0-9-]/g, '-')
      .replace(/-+/g, '-');
    setScheduleId(cleanId);
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
            setScheduleIdFromTitle(recipe.title);
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
      if (schedule) {
        setScheduleId(schedule.id);
        setCronExpression(schedule.cron);
        setWorkingDir(schedule.workingDir ?? '');
        setInternalValidationError(null);
      } else {
        setWorkingDir(getInitialWorkingDir());
        setScheduleId('');
        setSourceType('file');
        setRecipeSourcePath('');
        setDeepLinkInput('');
        setParsedRecipe(null);
        setCronExpression('0 0 14 * * *');
        setInternalValidationError(null);
        if (initialDeepLink) {
          setSourceType('deeplink');
          handleDeepLinkChange(initialDeepLink);
        }
      }
    }
  }, [isOpen, schedule, initialDeepLink, handleDeepLinkChange]);

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
            setScheduleIdFromTitle(recipe.title);
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

  const handleChooseFolder = async () => {
    const result = await window.electron.directoryChooser(workingDir || undefined);
    if (result.canceled || result.filePaths.length === 0) return;
    setWorkingDir(result.filePaths[0]);
    setInternalValidationError(null);
  };

  const handleLocalSubmit = async (event: FormEvent) => {
    event.preventDefault();
    setInternalValidationError(null);

    if (!workingDir) {
      setInternalValidationError(intl.formatMessage(i18n.folderRequired));
      return;
    }

    if (isEditMode) {
      await onSubmit({ cron: cronExpression, workingDir });
      return;
    }

    if (!scheduleId.trim()) {
      setInternalValidationError(intl.formatMessage(i18n.scheduleIdRequired));
      return;
    }

    if (!parsedRecipe) {
      setInternalValidationError(intl.formatMessage(i18n.provideValidRecipe));
      return;
    }

    const newSchedulePayload: NewSchedulePayload = {
      id: scheduleId.trim(),
      recipe: parsedRecipe,
      cron: cronExpression,
      working_dir: workingDir,
    };

    await onSubmit(newSchedulePayload);
  };

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 bg-black/50 z-40 flex items-center justify-center p-4">
      <Card className="w-full max-w-md bg-background-primary shadow-lz-overlay dark:shadow-lz-overlay-dark rounded-3xl z-50 flex flex-col max-h-[90vh] overflow-hidden">
        <div className="px-8 pt-6 pb-4 flex-shrink-0">
          <div className="flex items-center gap-3">
            <img src={ClockIcon} alt="Clock" className="w-8 h-8" />
            <div className="flex-1">
              <h2 className="text-base font-semibold text-text-primary">
                {isEditMode
                  ? intl.formatMessage(i18n.editSchedule)
                  : intl.formatMessage(i18n.createNewSchedule)}
              </h2>
              {isEditMode && <p className="text-sm text-text-secondary">{schedule.id}</p>}
            </div>
          </div>
        </div>

        <form
          id="schedule-form"
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

          {!isEditMode && (
            <>
              <div>
                <label htmlFor="scheduleId-modal" className={modalLabelClassName}>
                  {intl.formatMessage(i18n.nameLabel)} <span className="text-text-danger">*</span>
                </label>
                <Input
                  type="text"
                  id="scheduleId-modal"
                  value={scheduleId}
                  onChange={(e) => setScheduleId(e.target.value)}
                  placeholder={intl.formatMessage(i18n.namePlaceholder)}
                  required
                />
              </div>

              <div>
                <label className={modalLabelClassName}>
                  {intl.formatMessage(i18n.sourceLabel)} <span className="text-text-danger">*</span>
                </label>
                <div className="space-y-2">
                  <div className="flex bg-gray-100 dark:bg-gray-700 rounded-full p-1">
                    <button
                      type="button"
                      onClick={() => setSourceType('file')}
                      className={`flex-1 px-4 py-2 text-sm font-medium rounded-full transition-all ${
                        sourceType === 'file'
                          ? 'bg-white dark:bg-gray-800 text-gray-900 dark:text-white'
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
                          ? 'bg-white dark:bg-gray-800 text-gray-900 dark:text-white'
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
                        <div className="mt-2 p-2 bg-lz-ok-solid rounded-md">
                          <p className="text-xs text-white font-medium">
                            ✓ {intl.formatMessage(i18n.recipeParsed)}
                          </p>
                          <p className="text-xs text-white">
                            {intl.formatMessage(i18n.recipeTitle, { title: parsedRecipe.title })}
                          </p>
                          <p className="text-xs text-white">
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
            </>
          )}

          <div>
            <label className={modalLabelClassName}>
              {intl.formatMessage(i18n.folderLabel)} <span className="text-text-danger">*</span>
            </label>
            <div className="flex items-center gap-2">
              {workingDir ? (
                <p
                  className="flex-1 min-w-0 truncate text-sm font-mono text-text-primary"
                  title={workingDir}
                  data-testid="schedule-folder"
                >
                  {workingDir}
                </p>
              ) : (
                <p
                  className="flex-1 min-w-0 text-sm font-medium text-text-danger"
                  data-testid="schedule-no-folder"
                >
                  {intl.formatMessage(i18n.noFolder)}
                </p>
              )}
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={handleChooseFolder}
                className="shrink-0 rounded-full"
              >
                {intl.formatMessage(i18n.chooseFolder)}
              </Button>
            </div>
          </div>

          <div>
            <label className={modalLabelClassName}>{intl.formatMessage(i18n.scheduleLabel)}</label>
            <CronPicker schedule={schedule} onChange={setCronExpression} isValid={setIsValid} />
          </div>
        </form>

        <div className="flex gap-2 px-8 py-4 border-t border-border-primary">
          <Button
            type="button"
            variant="ghost"
            onClick={onClose}
            disabled={isLoadingExternally}
            className="flex-1 text-text-primary hover:bg-background-tertiary"
          >
            {intl.formatMessage(i18n.cancel)}
          </Button>
          <Button
            type="submit"
            form="schedule-form"
            disabled={isLoadingExternally || !isValid}
            className="flex-1"
          >
            {isLoadingExternally
              ? isEditMode
                ? intl.formatMessage(i18n.updating)
                : intl.formatMessage(i18n.creating)
              : isEditMode
                ? intl.formatMessage(i18n.updateSchedule)
                : intl.formatMessage(i18n.createSchedule)}
          </Button>
        </div>
      </Card>
    </div>
  );
};
