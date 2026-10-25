import { useEffect, useState } from 'react';
import { defineMessages, useIntl } from '../../../i18n';
import { Switch } from '../../ui/switch';
import { Input } from '../../ui/input';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '../../ui/card';
import { acpReadConfig, acpUpsertConfig } from '../../../acp/config';
import { errorMessage } from '../../../utils/conversionUtils';
import {
  LINK_ALLOW_REMOTE_APP_QUIT_KEY,
  MEMFREE_ENABLED_KEY,
  MEMFREE_SHOW_APPS_MIN_MB_DEFAULT,
  MEMFREE_SHOW_APPS_MIN_MB_KEY,
  boolOnFrom,
  memfreeEnabledFrom,
  minMbFrom,
} from '../../memfree/memfree';
import { forgetMemfreeEnabled } from '../../memfree/useMemfreeEnabled';

const i18n = defineMessages({
  title: { id: 'memfreeSettings.title', defaultMessage: 'Free up memory' },
  description: {
    id: 'memfreeSettings.description',
    defaultMessage:
      'When a local model does not fit, goose shows what holds the memory and can free its own share.',
  },
  enabled: { id: 'memfreeSettings.enabled', defaultMessage: 'Offer “Free up memory”' },
  enabledDesc: {
    id: 'memfreeSettings.enabledDesc',
    defaultMessage:
      'Shows the button on a model that is short of memory and on each Mac’s memory bars. On by default.',
  },
  minMb: { id: 'memfreeSettings.minMb', defaultMessage: 'Smallest app to list (MB)' },
  minMbDesc: {
    id: 'memfreeSettings.minMbDesc',
    defaultMessage: 'Apps using less memory than this are left out of the list. Default {mb} MB.',
  },
  remoteQuit: {
    id: 'memfreeSettings.remoteQuit',
    defaultMessage: 'Let linked Macs quit apps on this Mac',
  },
  remoteQuitDesc: {
    id: 'memfreeSettings.remoteQuitDesc',
    defaultMessage:
      'Another Mac on LeanZero Link may ask this Mac to quit an app, always after a confirmation there. Off by default: they can still free goose’s own memory here.',
  },
  readFailed: {
    id: 'memfreeSettings.readFailed',
    defaultMessage: 'Could not read these settings: {reason}',
  },
  saveFailed: {
    id: 'memfreeSettings.saveFailed',
    defaultMessage: 'Could not save this setting: {reason}',
  },
});

interface MemfreeSettingsState {
  enabled: boolean;
  minMb: string;
  remoteQuit: boolean;
}

/**
 * Settings › App: the "Free up memory" tunables, each a goose config key the engine reads
 * (GOOSE_MEMFREE_ENABLED on by default, GOOSE_MEMFREE_SHOW_APPS_MIN_MB 200,
 * GOOSE_LINK_ALLOW_REMOTE_APP_QUIT off by default).
 */
export function MemfreeSettings() {
  const intl = useIntl();
  const [state, setState] = useState<MemfreeSettingsState | null>(null);
  const [problem, setProblem] = useState<{ kind: 'read' | 'save'; reason: string } | null>(null);

  useEffect(() => {
    let alive = true;
    Promise.all([
      acpReadConfig(MEMFREE_ENABLED_KEY, false),
      acpReadConfig(MEMFREE_SHOW_APPS_MIN_MB_KEY, false),
      acpReadConfig(LINK_ALLOW_REMOTE_APP_QUIT_KEY, false),
    ]).then(
      ([enabled, minMb, remoteQuit]) => {
        if (!alive) return;
        setState({
          enabled: memfreeEnabledFrom(enabled),
          minMb: String(minMbFrom(minMb)),
          remoteQuit: boolOnFrom(remoteQuit),
        });
      },
      (error: unknown) => {
        if (alive) setProblem({ kind: 'read', reason: errorMessage(error, String(error)) });
      }
    );
    return () => {
      alive = false;
    };
  }, []);

  const save = async (key: string, value: unknown, next: MemfreeSettingsState) => {
    const before = state;
    setState(next);
    setProblem(null);
    try {
      await acpUpsertConfig(key, value, false);
      if (key === MEMFREE_ENABLED_KEY) forgetMemfreeEnabled();
    } catch (error) {
      setState(before);
      setProblem({ kind: 'save', reason: errorMessage(error, String(error)) });
    }
  };

  const commitMinMb = () => {
    if (!state) return;
    const mb = minMbFrom(state.minMb.trim() === '' ? NaN : state.minMb);
    void save(MEMFREE_SHOW_APPS_MIN_MB_KEY, mb, { ...state, minMb: String(mb) });
  };

  return (
    <Card className="rounded-lg" data-testid="memfree-settings">
      <CardHeader className="pb-0">
        <CardTitle>{intl.formatMessage(i18n.title)}</CardTitle>
        <CardDescription>{intl.formatMessage(i18n.description)}</CardDescription>
      </CardHeader>
      <CardContent className="pt-4 space-y-4 px-4">
        {problem && (
          <p role="alert" className="text-xs text-text-danger max-w-md">
            {intl.formatMessage(problem.kind === 'read' ? i18n.readFailed : i18n.saveFailed, {
              reason: problem.reason,
            })}
          </p>
        )}
        <div className="flex items-center justify-between gap-4">
          <div>
            <h3 className="text-text-primary text-xs">{intl.formatMessage(i18n.enabled)}</h3>
            <p className="text-xs text-text-secondary max-w-md mt-[2px]">
              {intl.formatMessage(i18n.enabledDesc)}
            </p>
          </div>
          <Switch
            data-testid="memfree-enabled-switch"
            checked={state?.enabled ?? true}
            disabled={state == null}
            onCheckedChange={(checked: boolean) =>
              state && void save(MEMFREE_ENABLED_KEY, checked, { ...state, enabled: checked })
            }
            variant="mono"
          />
        </div>
        <div className="flex items-center justify-between gap-4">
          <div>
            <h3 className="text-text-primary text-xs">{intl.formatMessage(i18n.minMb)}</h3>
            <p className="text-xs text-text-secondary max-w-md mt-[2px]">
              {intl.formatMessage(i18n.minMbDesc, { mb: MEMFREE_SHOW_APPS_MIN_MB_DEFAULT })}
            </p>
          </div>
          <Input
            data-testid="memfree-min-mb-input"
            aria-label={intl.formatMessage(i18n.minMb)}
            className="w-24"
            inputMode="numeric"
            value={state?.minMb ?? ''}
            disabled={state == null}
            onChange={(e) => state && setState({ ...state, minMb: e.target.value })}
            onBlur={commitMinMb}
            onKeyDown={(e) => {
              if (e.key === 'Enter') commitMinMb();
            }}
          />
        </div>
        <div className="flex items-center justify-between gap-4">
          <div>
            <h3 className="text-text-primary text-xs">{intl.formatMessage(i18n.remoteQuit)}</h3>
            <p className="text-xs text-text-secondary max-w-md mt-[2px]">
              {intl.formatMessage(i18n.remoteQuitDesc)}
            </p>
          </div>
          <Switch
            data-testid="memfree-remote-quit-switch"
            checked={state?.remoteQuit ?? false}
            disabled={state == null}
            onCheckedChange={(checked: boolean) =>
              state &&
              void save(LINK_ALLOW_REMOTE_APP_QUIT_KEY, checked, { ...state, remoteQuit: checked })
            }
            variant="mono"
          />
        </div>
      </CardContent>
    </Card>
  );
}
