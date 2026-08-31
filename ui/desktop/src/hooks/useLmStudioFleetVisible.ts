import { useEffect, useState } from 'react';

/** Dispatched by the Settings > App toggle so every mounted consumer re-reads at once. */
export const LMSTUDIO_FLEET_SETTING_CHANGED = 'lmStudioFleetSettingChanged';

/**
 * Pass E (owner): LM Studio fleet surfaces are LEGACY — every LM Studio-sourced row/panel in
 * session and benchmark UI hides behind the 'showLmStudioFleet' setting, DEFAULT FALSE. The
 * discovery code (useFleet/useFleetStatus) is untouched; consumers gate on this hook. Starts
 * false (hidden) until the setting proves otherwise — the default posture, never a flash of rows.
 */
export function useLmStudioFleetVisible(): boolean {
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    let alive = true;
    const load = () => {
      void window.electron
        .getSetting('showLmStudioFleet')
        .then((v) => {
          if (alive) setVisible(v === true);
        })
        .catch(() => {
          if (alive) setVisible(false);
        });
    };
    load();
    window.addEventListener(LMSTUDIO_FLEET_SETTING_CHANGED, load);
    return () => {
      alive = false;
      window.removeEventListener(LMSTUDIO_FLEET_SETTING_CHANGED, load);
    };
  }, []);

  return visible;
}
