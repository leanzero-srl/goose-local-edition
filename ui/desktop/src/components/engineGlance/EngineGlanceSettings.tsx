import { defineMessages, useIntl } from '../../i18n';
import { Switch } from '../ui/switch';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '../ui/card';
import { Button, Segmented } from '../lz';
import { DEFAULT_GLANCE_PREFS, type GlanceDesktopMode } from '../../utils/engineGlance';
import { setGlancePrefs, showDesktopGlanceAgain, useEngineGlance } from './glanceStore';

const i18n = defineMessages({
  title: { id: 'engineGlanceSettings.title', defaultMessage: 'Engine at a glance' },
  description: {
    id: 'engineGlanceSettings.description',
    defaultMessage:
      "The Engine's live card, small: what it is reading or writing, how fast, and which chat it serves. Click it to open the Engine.",
  },
  inApp: { id: 'engineGlanceSettings.inApp', defaultMessage: 'In the sidebar' },
  inAppDesc: {
    id: 'engineGlanceSettings.inAppDesc',
    defaultMessage:
      'At the foot of the sidebar, under your sessions. Hide it from the card itself; it comes back from the row it leaves in its place, or from here.',
  },
  desktop: { id: 'engineGlanceSettings.desktop', defaultMessage: 'Floating on the desktop' },
  desktopDesc: {
    id: 'engineGlanceSettings.desktopDescSession',
    defaultMessage:
      'A small window over your other apps, in a corner you drag it to. It never takes focus, and brings goose forward only when you open something from it. Its close button hides it for the rest of this session.',
  },
  dismissed: {
    id: 'engineGlanceSettings.dismissed',
    defaultMessage: 'You closed it — hidden until you bring it back.',
  },
  showAgain: { id: 'engineGlanceSettings.showAgain', defaultMessage: 'Show it again' },
  off: { id: 'engineGlanceSettings.desktop.off', defaultMessage: 'Off' },
  away: {
    id: 'engineGlanceSettings.desktop.away',
    defaultMessage: 'While goose is in the background',
  },
  busy: { id: 'engineGlanceSettings.desktop.busy', defaultMessage: 'Whenever the engine works' },
});

/**
 * Settings › App: the two engine-glance switches, stored by main (`engine-glance-prefs-set`) — and,
 * while the person has closed the floating window this session, the way to bring it back (Q-426).
 */
export function EngineGlanceSettings() {
  const intl = useIntl();
  const push = useEngineGlance();
  const prefs = push?.prefs ?? DEFAULT_GLANCE_PREFS;
  const options: { value: GlanceDesktopMode; label: string; testId: string }[] = [
    { value: 'off', label: intl.formatMessage(i18n.off), testId: 'engine-glance-desktop-off' },
    { value: 'away', label: intl.formatMessage(i18n.away), testId: 'engine-glance-desktop-away' },
    { value: 'busy', label: intl.formatMessage(i18n.busy), testId: 'engine-glance-desktop-busy' },
  ];
  return (
    <Card className="rounded-lg" data-testid="engine-glance-settings">
      <CardHeader className="pb-0">
        <CardTitle>{intl.formatMessage(i18n.title)}</CardTitle>
        <CardDescription>{intl.formatMessage(i18n.description)}</CardDescription>
      </CardHeader>
      <CardContent className="pt-4 space-y-4 px-4">
        <div className="flex items-center justify-between gap-4">
          <div>
            <h3 className="text-text-primary text-xs">{intl.formatMessage(i18n.inApp)}</h3>
            <p className="text-xs text-text-secondary max-w-md mt-[2px]">
              {intl.formatMessage(i18n.inAppDesc)}
            </p>
          </div>
          <Switch
            data-testid="engine-glance-inapp-switch"
            checked={prefs.inApp}
            disabled={!push}
            onCheckedChange={(inApp: boolean) => void setGlancePrefs({ ...prefs, inApp })}
            variant="mono"
          />
        </div>
        <div className="flex flex-col gap-2">
          <div>
            <h3 className="text-text-primary text-xs">{intl.formatMessage(i18n.desktop)}</h3>
            <p className="text-xs text-text-secondary max-w-md mt-[2px]">
              {intl.formatMessage(i18n.desktopDesc)}
            </p>
          </div>
          <Segmented
            aria-label={intl.formatMessage(i18n.desktop)}
            options={options}
            value={prefs.desktop}
            disabled={!push}
            onChange={(desktop) => void setGlancePrefs({ ...prefs, desktop })}
            className="self-start"
          />
          {push?.desktopDismissed && prefs.desktop !== 'off' && (
            <div
              data-testid="engine-glance-desktop-dismissed"
              className="flex flex-wrap items-center gap-3"
            >
              <p className="text-xs text-text-primary">{intl.formatMessage(i18n.dismissed)}</p>
              <Button
                data-testid="engine-glance-desktop-show-again"
                size="sm"
                onClick={showDesktopGlanceAgain}
              >
                {intl.formatMessage(i18n.showAgain)}
              </Button>
            </div>
          )}
        </div>
      </CardContent>
    </Card>
  );
}
