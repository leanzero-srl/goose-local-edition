import React from 'react';
import { Monitor, Moon, Sun } from 'lucide-react';
import { useTheme, type ThemePreference } from '../../contexts/ThemeContext';
import { Segmented, cx, type SegmentedOption } from '../lz';
import { defineMessages, useIntl } from '../../i18n';

// The same catalog ids as GooseSidebar/ThemeSelector — one set of words for one setting.
const i18n = defineMessages({
  theme: { id: 'themeSelector.theme', defaultMessage: 'Theme' },
  system: { id: 'themeSelector.system', defaultMessage: 'System' },
  light: { id: 'themeSelector.light', defaultMessage: 'Light' },
  dark: { id: 'themeSelector.dark', defaultMessage: 'Dark' },
});

/**
 * The sidebar's theme control: an lz Segmented radiogroup — System | Light | Dark as icons with
 * titles and screen-reader labels — over the ONE theme store (ThemeContext: persisted through the
 * `useSystemTheme` / `theme` settings; every choice is pushed to main's nativeTheme.themeSource and
 * System paints main's shouldUseDarkColors, re-resolved on its 'updated' event). Settings › App's
 * ThemeSelector sets the same value, so the two never disagree. Its own 36px row above Settings, spanning the sidebar as three EQUAL segments.
 *
 * Icons, not words, on the segments — measured, not chosen: at the sidebar's 240px (NAV_WIDTH)
 * the row has 206px of content width, and three icon+label segments need 3 × (20 padding +
 * 14 icon + 6 gap + 43 "System" in Inter Medium 12px) + 10 strip chrome = 259px even before
 * equalising them. The title and the screen-reader label carry each word instead — the
 * icon+title form DESIGN.md › Buttons prescribes where a label would clip.
 */
export const ThemeSwitch: React.FC<{ className?: string }> = ({ className }) => {
  const intl = useIntl();
  const { userThemePreference, setUserThemePreference } = useTheme();
  const words = {
    system: intl.formatMessage(i18n.system),
    light: intl.formatMessage(i18n.light),
    dark: intl.formatMessage(i18n.dark),
  };
  const options: SegmentedOption<ThemePreference>[] = [
    {
      value: 'system',
      icon: <Monitor />,
      label: <span className="sr-only">{words.system}</span>,
      title: words.system,
      testId: 'theme-system',
    },
    {
      value: 'light',
      icon: <Sun />,
      label: <span className="sr-only">{words.light}</span>,
      title: words.light,
      testId: 'theme-light',
    },
    {
      value: 'dark',
      icon: <Moon />,
      label: <span className="sr-only">{words.dark}</span>,
      title: words.dark,
      testId: 'theme-dark',
    },
  ];
  return (
    <Segmented
      aria-label={intl.formatMessage(i18n.theme)}
      options={options}
      value={userThemePreference}
      onChange={setUserThemePreference}
      className={cx(
        'no-drag h-lz-row w-full [&>button]:flex-1 [&>button]:justify-center',
        className
      )}
    />
  );
};
