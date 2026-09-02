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
 * `useSystemTheme` / `theme` settings, System follows the OS through the existing
 * prefers-color-scheme listener). Settings › App's ThemeSelector sets the same value, so the two
 * never disagree. 36px tall like the nav rows it sits beside.
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
      className={cx('no-drag h-lz-row shrink-0', className)}
    />
  );
};
