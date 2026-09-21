import { useCallback, useEffect, useState } from 'react';
import { Switch } from '../../ui/switch';
import { Input } from '../../ui/input';
import { useConfig } from '../../ConfigContext';
import { defineMessages, useIntl } from '../../../i18n';

const i18n = defineMessages({
  proposalsTitle: {
    id: 'memoryProposals.title',
    defaultMessage: 'Ask before saving memories',
  },
  proposalsDescription: {
    id: 'memoryProposals.description',
    defaultMessage:
      'At the end of a turn goose judges how it went — well or badly — and, when something is worth remembering, shows a card under the reply. Nothing is saved until you click Save.',
  },
  modelTitle: {
    id: 'memoryProposals.modelTitle',
    defaultMessage: 'Assessment model',
  },
  modelDescription: {
    id: 'memoryProposals.modelDescription',
    defaultMessage:
      "The model that judges the turn. Leave empty to use the session's own model; set a small fast model to keep the judgement cheap.",
  },
  modelPlaceholder: {
    id: 'memoryProposals.modelPlaceholder',
    defaultMessage: "session's model",
  },
});

export const MEMORY_PROPOSALS_KEY = 'GOOSE_MEMORY_PROPOSALS';
export const ASSESSMENT_MODEL_KEY = 'GOOSE_ASSESSMENT_MODEL';

/** The engine's default is ON (the owner's ask); an absent key reads as enabled. */
export function proposalsEnabledFrom(value: unknown): boolean {
  return value == null ? true : value === true || value === 'true';
}

export const MemoryProposalsSection = () => {
  const intl = useIntl();
  const { read, upsert, remove } = useConfig();
  const [enabled, setEnabled] = useState(true);
  const [model, setModel] = useState('');

  const load = useCallback(async () => {
    try {
      setEnabled(proposalsEnabledFrom(await read(MEMORY_PROPOSALS_KEY, false)));
      const stored = await read(ASSESSMENT_MODEL_KEY, false);
      setModel(typeof stored === 'string' ? stored : '');
    } catch (error) {
      console.error('Error reading memory proposal settings:', error);
    }
  }, [read]);

  useEffect(() => {
    void load();
  }, [load]);

  const handleToggle = async (checked: boolean) => {
    setEnabled(checked);
    try {
      await upsert(MEMORY_PROPOSALS_KEY, checked, false);
    } catch (error) {
      console.error('Error updating memory proposals:', error);
    }
  };

  const commitModel = async () => {
    const next = model.trim();
    try {
      if (next) {
        await upsert(ASSESSMENT_MODEL_KEY, next, false);
      } else {
        await remove(ASSESSMENT_MODEL_KEY, false);
      }
    } catch (error) {
      console.error('Error updating assessment model:', error);
    }
  };

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between py-2 px-2 hover:bg-background-secondary rounded-lg transition-all">
        <div>
          <h3 className="text-text-primary">{intl.formatMessage(i18n.proposalsTitle)}</h3>
          <p className="text-xs text-text-secondary max-w-md mt-[2px]">
            {intl.formatMessage(i18n.proposalsDescription)}
          </p>
        </div>
        <Switch
          checked={enabled}
          onCheckedChange={handleToggle}
          variant="mono"
          data-testid="memory-proposals-switch"
        />
      </div>
      <div className="flex items-center justify-between gap-4 py-2 px-2 hover:bg-background-secondary rounded-lg transition-all">
        <div>
          <h3 className="text-text-primary">{intl.formatMessage(i18n.modelTitle)}</h3>
          <p className="text-xs text-text-secondary max-w-md mt-[2px]">
            {intl.formatMessage(i18n.modelDescription)}
          </p>
        </div>
        <Input
          className="w-56"
          value={model}
          placeholder={intl.formatMessage(i18n.modelPlaceholder)}
          onChange={(e) => setModel(e.target.value)}
          onBlur={() => void commitModel()}
          onKeyDown={(e) => {
            if (e.key === 'Enter') void commitModel();
          }}
          data-testid="assessment-model-input"
        />
      </div>
    </div>
  );
};
