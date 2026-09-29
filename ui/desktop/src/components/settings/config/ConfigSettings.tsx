import { useState, useEffect, useMemo } from 'react';
import { Input } from '../../ui/input';
import { Button } from '../../ui/button';
import { useConfig } from '../../ConfigContext';
import { cn } from '../../../utils';
import { Save, RotateCcw, FileText, Settings } from 'lucide-react';
import { toastSuccess, toastError } from '../../../toasts';
import { getUiNames, providerPrefixes } from '../../../utils/configUtils';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '../../ui/card';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '../../ui/dialog';
import { errorMessage } from '../../../utils/conversionUtils';
import { defineMessages, useIntl } from '../../../i18n';

const i18n = defineMessages({
  title: {
    id: 'configSettings.title',
    defaultMessage: 'Configuration',
  },
  description: {
    id: 'configSettings.description',
    defaultMessage: 'Edit your goose configuration settings',
  },
  descriptionWithProvider: {
    id: 'configSettings.descriptionWithProvider',
    defaultMessage: 'Edit your goose configuration settings (current settings for {provider})',
  },
  editConfiguration: {
    id: 'configSettings.editConfiguration',
    defaultMessage: 'Edit Configuration',
  },
  configurationEditor: {
    id: 'configSettings.configurationEditor',
    defaultMessage: 'Configuration Editor',
  },
  noSettings: {
    id: 'configSettings.noSettings',
    defaultMessage: 'No configuration settings found.',
  },
  enterValue: {
    id: 'configSettings.enterValue',
    defaultMessage: 'Enter {name}',
  },
  saving: {
    id: 'configSettings.saving',
    defaultMessage: 'Saving...',
  },
  resetChanges: {
    id: 'configSettings.resetChanges',
    defaultMessage: 'Reset Changes',
  },
  done: {
    id: 'configSettings.done',
    defaultMessage: 'Done',
  },
  configUpdated: {
    id: 'configSettings.configUpdated',
    defaultMessage: 'Configuration Updated',
  },
  configUpdatedMsg: {
    id: 'configSettings.configUpdatedMsg',
    defaultMessage: 'Successfully saved "{name}"',
  },
  saveFailed: {
    id: 'configSettings.saveFailed',
    defaultMessage: 'Save Failed',
  },
  saveFailedMsg: {
    id: 'configSettings.saveFailedMsg',
    defaultMessage: 'Failed to save "{name}"',
  },
  configReset: {
    id: 'configSettings.configReset',
    defaultMessage: 'Configuration Reset',
  },
  configResetMsg: {
    id: 'configSettings.configResetMsg',
    defaultMessage: 'All changes have been reverted',
  },
  structuredReadOnly: {
    id: 'configSettings.structuredReadOnly',
    defaultMessage:
      'A list or group of settings, shown read-only so a save here can never flatten it. Edit it in config.yaml.',
  },
  notANumber: {
    id: 'configSettings.notANumber',
    defaultMessage: '"{name}" must be a number, so it was not saved.',
  },
  notABoolean: {
    id: 'configSettings.notABoolean',
    defaultMessage: '"{name}" must be true or false, so it was not saved.',
  },
});

type FieldKind = 'text' | 'number' | 'boolean' | 'structured';

/**
 * Q-473: the config holds more than strings. `String(value || '')` showed "[object Object]" for
 * a nested value (and blanked `false` and `0`), and a save wrote that text over the real value.
 * Each value keeps its kind: scalars are edited as text and saved back as their own type; lists
 * and groups are shown as their JSON and are never saved from here.
 */
export function describeConfigValue(value: unknown): { kind: FieldKind; text: string } {
  if (value === null || value === undefined) return { kind: 'text', text: '' };
  if (typeof value === 'number') return { kind: 'number', text: String(value) };
  if (typeof value === 'boolean') return { kind: 'boolean', text: String(value) };
  if (typeof value === 'object')
    return { kind: 'structured', text: JSON.stringify(value, null, 2) };
  return { kind: 'text', text: String(value) };
}

export function parseConfigDraft(
  kind: FieldKind,
  draft: string
): { ok: true; value: unknown } | { ok: false } {
  switch (kind) {
    case 'number': {
      const trimmed = draft.trim();
      const parsed = Number(trimmed);
      return trimmed !== '' && Number.isFinite(parsed)
        ? { ok: true, value: parsed }
        : { ok: false };
    }
    case 'boolean': {
      const lowered = draft.trim().toLowerCase();
      if (lowered === 'true') return { ok: true, value: true };
      if (lowered === 'false') return { ok: true, value: false };
      return { ok: false };
    }
    case 'structured':
      return { ok: false };
    default:
      return { ok: true, value: draft };
  }
}

export default function ConfigSettings() {
  const intl = useIntl();
  const { config, upsert } = useConfig();
  const typedConfig = config as Record<string, unknown>;
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [saving, setSaving] = useState<string | null>(null);
  const [isModalOpen, setIsModalOpen] = useState(false);
  const [originalKeyOrder, setOriginalKeyOrder] = useState<string[]>([]);

  const modifiedKeys = useMemo(
    () =>
      new Set(
        Object.keys(drafts).filter(
          (key) => drafts[key] !== describeConfigValue(typedConfig[key]).text
        )
      ),
    [drafts, typedConfig]
  );

  useEffect(() => {
    setDrafts({});

    // Capture the original key order only on first load or when new keys are added
    const currentKeys = Object.keys(typedConfig);
    setOriginalKeyOrder((prevOrder) => {
      if (prevOrder.length === 0) {
        // First load - capture the initial order
        return currentKeys;
      } else if (currentKeys.length > prevOrder.length) {
        // New keys have been added - add them to the end while preserving existing order
        const newKeys = currentKeys.filter((key) => !prevOrder.includes(key));
        return [...prevOrder, ...newKeys];
      }
      // Don't reorder when keys are just updated/saved - preserve the original order
      return prevOrder;
    });
  }, [typedConfig]);

  const handleChange = (key: string, value: string) => {
    setDrafts((prev) => ({ ...prev, [key]: value }));
  };

  const handleSave = async (key: string) => {
    const draft = drafts[key];
    if (draft === undefined) return;
    const kind = describeConfigValue(typedConfig[key]).kind;
    const parsed = parseConfigDraft(kind, draft);
    if (!parsed.ok) {
      toastError({
        title: intl.formatMessage(i18n.saveFailed),
        msg: intl.formatMessage(kind === 'boolean' ? i18n.notABoolean : i18n.notANumber, {
          name: getUiNames(key),
        }),
      });
      return;
    }
    setSaving(key);
    try {
      await upsert(key, parsed.value, false);
      toastSuccess({
        title: intl.formatMessage(i18n.configUpdated),
        msg: intl.formatMessage(i18n.configUpdatedMsg, { name: getUiNames(key) }),
      });

      setDrafts((prev) => {
        const next = { ...prev };
        delete next[key];
        return next;
      });
    } catch (error) {
      console.error('Failed to save config:', error);
      toastError({
        title: intl.formatMessage(i18n.saveFailed),
        msg: intl.formatMessage(i18n.saveFailedMsg, { name: getUiNames(key) }),
        traceback: errorMessage(error),
      });
    } finally {
      setSaving(null);
    }
  };

  const handleReset = () => {
    setDrafts({});
    toastSuccess({
      title: intl.formatMessage(i18n.configReset),
      msg: intl.formatMessage(i18n.configResetMsg),
    });
  };

  const handleModalClose = (open: boolean) => {
    if (!open) {
      setDrafts({});
    }
    setIsModalOpen(open);
  };

  const currentProvider =
    typeof typedConfig.GOOSE_PROVIDER === 'string' ? typedConfig.GOOSE_PROVIDER : '';

  const configKeys: string[] = useMemo(() => {
    const currentProviderPrefixes = providerPrefixes[currentProvider] || [];
    const allProviderPrefixes = Object.values(providerPrefixes).flat();

    return originalKeyOrder.filter((key) => {
      // skip secrets
      if (key === 'extensions' || key.includes('_KEY') || key.includes('_TOKEN')) {
        return false;
      }

      // Only show provider-specific entries for the current provider
      const providerSpecific = allProviderPrefixes.some((prefix: string) => key.startsWith(prefix));
      if (providerSpecific) {
        return currentProviderPrefixes.some((prefix: string) => key.startsWith(prefix));
      }

      return true;
    });
  }, [originalKeyOrder, currentProvider]);

  return (
    <Card className="rounded-lg">
      <CardHeader className="pb-0">
        <CardTitle className="flex items-center gap-2">
          <FileText className="text-text-primary" size={20} />
          {intl.formatMessage(i18n.title)}
        </CardTitle>
        <CardDescription>
          {currentProvider
            ? intl.formatMessage(i18n.descriptionWithProvider, { provider: currentProvider })
            : intl.formatMessage(i18n.description)}
        </CardDescription>
      </CardHeader>
      <CardContent className="pt-4 px-4">
        <Dialog open={isModalOpen} onOpenChange={handleModalClose}>
          <DialogTrigger asChild>
            <Button className="flex items-center gap-2" variant="secondary" size="sm">
              <Settings className="h-4 w-4" />
              {intl.formatMessage(i18n.editConfiguration)}
            </Button>
          </DialogTrigger>
          <DialogContent className="max-w-4xl max-h-[80vh]">
            <DialogHeader>
              <DialogTitle className="flex items-center gap-2">
                <FileText className="text-text-primary" size={20} />
                {intl.formatMessage(i18n.configurationEditor)}
              </DialogTitle>
              <DialogDescription>
                {currentProvider
                  ? intl.formatMessage(i18n.descriptionWithProvider, { provider: currentProvider })
                  : intl.formatMessage(i18n.description)}
              </DialogDescription>
            </DialogHeader>

            <div className="flex-1 max-h-[60vh] overflow-auto pr-4">
              <div className="space-y-4">
                {configKeys.length === 0 ? (
                  <p className="text-text-secondary">{intl.formatMessage(i18n.noSettings)}</p>
                ) : (
                  configKeys.map((key) => {
                    const field = describeConfigValue(typedConfig[key]);
                    if (field.kind === 'structured') {
                      return (
                        <div
                          key={key}
                          data-testid={`config-structured-${key}`}
                          className="grid grid-cols-[200px_1fr_auto] gap-3 items-start"
                        >
                          <label className="text-sm font-medium text-text-primary" title={key}>
                            {getUiNames(key)}
                          </label>
                          <div className="flex min-w-0 flex-col gap-1.5">
                            <pre className="max-h-48 overflow-auto whitespace-pre-wrap break-all rounded-md border border-border-primary bg-background-secondary px-3 py-2 font-mono text-xs text-text-primary">
                              {field.text}
                            </pre>
                            <p className="text-xs text-text-secondary">
                              {intl.formatMessage(i18n.structuredReadOnly)}
                            </p>
                          </div>
                          <span className="min-w-[60px]" />
                        </div>
                      );
                    }
                    return (
                      <div key={key} className="grid grid-cols-[200px_1fr_auto] gap-3 items-center">
                        <label className="text-sm font-medium text-text-primary" title={key}>
                          {getUiNames(key)}
                        </label>
                        <Input
                          aria-label={getUiNames(key)}
                          value={drafts[key] ?? field.text}
                          onChange={(e) => handleChange(key, e.target.value)}
                          className={cn(
                            'text-text-primary border-border-primary hover:border-border-primary transition-colors',
                            modifiedKeys.has(key) && 'border-lz-accent-line focus:ring-ring'
                          )}
                          placeholder={intl.formatMessage(i18n.enterValue, {
                            name: getUiNames(key),
                          })}
                        />
                        <Button
                          onClick={() => handleSave(key)}
                          disabled={!modifiedKeys.has(key) || saving === key}
                          variant="ghost"
                          size="sm"
                          className="min-w-[60px]"
                        >
                          {saving === key ? (
                            <span className="text-xs">{intl.formatMessage(i18n.saving)}</span>
                          ) : (
                            <Save className="h-4 w-4" />
                          )}
                        </Button>
                      </div>
                    );
                  })
                )}
              </div>
            </div>

            <DialogFooter className="gap-2">
              {modifiedKeys.size > 0 && (
                <Button onClick={handleReset} variant="outline">
                  <RotateCcw className="h-4 w-4 mr-2" />
                  {intl.formatMessage(i18n.resetChanges)}
                </Button>
              )}
              <Button onClick={() => setIsModalOpen(false)} variant="default">
                {intl.formatMessage(i18n.done)}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </CardContent>
    </Card>
  );
}
