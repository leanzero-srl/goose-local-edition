import { useEffect, useState } from 'react';
import { defineMessages, useIntl } from '../../../i18n';
import { Switch } from '../../ui/switch';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '../../ui/card';
import { acpReadConfig, acpUpsertConfig } from '../../../acp/config';
import { errorMessage } from '../../../utils/conversionUtils';

const i18n = defineMessages({
  title: { id: 'mlxRestoreSettings.title', defaultMessage: 'Local model at launch' },
  description: {
    id: 'mlxRestoreSettings.description',
    defaultMessage:
      'Whether opening goose brings back the local model that served before. Off by default.',
  },
  restore: {
    id: 'mlxRestoreSettings.restore',
    defaultMessage: 'Restore the local model at launch',
  },
  restoreDesc: {
    id: 'mlxRestoreSettings.restoreDesc',
    defaultMessage:
      'When goose starts, bring back the model you last started and did not stop: on this Mac, on another Mac over LeanZero Link, or split across Macs. Off: goose starts no model by itself; start one from Providers when you want it.',
  },
  readFailed: {
    id: 'mlxRestoreSettings.readFailed',
    defaultMessage: 'Could not read this setting: {reason}',
  },
  saveFailed: {
    id: 'mlxRestoreSettings.saveFailed',
    defaultMessage: 'Could not save this setting: {reason}',
  },
});

/** goose config.yaml key; the engine reads the same key (`mlx_engine.rs` RESTORE_AT_LAUNCH_KEY). */
export const RESTORE_AT_LAUNCH_KEY = 'GOOSE_MLX_RESTORE_AT_LAUNCH';

/**
 * Off unless the owner turned it on, read the way goose reads a boolean (a quoted "True" or
 * " TRUE " is on there too, so the switch must show it on): an absent key, or anything else, is off.
 */
export function restoreAtLaunchFrom(value: unknown): boolean {
  return value === true || (typeof value === 'string' && value.trim().toLowerCase() === 'true');
}

/**
 * Settings › App: whether a launch brings back what served chat before the app quit. Off by
 * default — the launch then starts nothing and shows no "Restoring…" or "Restore failed" anywhere;
 * goose keeps recording what the owner starts, so turning it on later works.
 */
export function MlxRestoreSettings() {
  const intl = useIntl();
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [problem, setProblem] = useState<{ kind: 'read' | 'save'; reason: string } | null>(null);

  useEffect(() => {
    let alive = true;
    acpReadConfig(RESTORE_AT_LAUNCH_KEY, false).then(
      (value) => {
        if (alive) setEnabled(restoreAtLaunchFrom(value));
      },
      (error: unknown) => {
        if (alive) setProblem({ kind: 'read', reason: errorMessage(error, String(error)) });
      }
    );
    return () => {
      alive = false;
    };
  }, []);

  const handleToggle = async (checked: boolean) => {
    const before = enabled;
    setEnabled(checked);
    setProblem(null);
    try {
      await acpUpsertConfig(RESTORE_AT_LAUNCH_KEY, checked, false);
    } catch (error) {
      setEnabled(before);
      setProblem({ kind: 'save', reason: errorMessage(error, String(error)) });
    }
  };

  return (
    <Card className="rounded-lg" data-testid="mlx-restore-settings">
      <CardHeader className="pb-0">
        <CardTitle>{intl.formatMessage(i18n.title)}</CardTitle>
        <CardDescription>{intl.formatMessage(i18n.description)}</CardDescription>
      </CardHeader>
      <CardContent className="pt-4 space-y-4 px-4">
        <div className="flex items-center justify-between gap-4">
          <div>
            <h3 className="text-text-primary text-xs">{intl.formatMessage(i18n.restore)}</h3>
            <p className="text-xs text-text-secondary max-w-md mt-[2px]">
              {intl.formatMessage(i18n.restoreDesc)}
            </p>
            {problem && (
              <p role="alert" className="text-xs text-text-danger max-w-md mt-[2px]">
                {intl.formatMessage(problem.kind === 'read' ? i18n.readFailed : i18n.saveFailed, {
                  reason: problem.reason,
                })}
              </p>
            )}
          </div>
          <Switch
            data-testid="mlx-restore-at-launch-switch"
            checked={enabled ?? false}
            disabled={enabled == null}
            onCheckedChange={(checked: boolean) => void handleToggle(checked)}
            variant="mono"
          />
        </div>
      </CardContent>
    </Card>
  );
}
