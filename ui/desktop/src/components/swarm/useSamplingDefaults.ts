import { useCallback } from 'react';
import { useConfig } from '../ConfigContext';
import { sanitizeSampling, saveSamplingDefaults, type SamplingSettings } from './sampling';

/**
 * "Save as defaults" for the run strips — ONE default set, kept in both stores: localStorage
 * `swarmSamplingDefaults` (what every run window prefills from, synchronously) and the swarm
 * config's sampling fields (what the engine falls back to when a run sends no override, and what
 * a headless `goose swarm run` uses). The Settings panel's "Sampling defaults" section writes the
 * same two stores, so saving from a run window and saving from Settings are the same act.
 */
export function useSaveSamplingDefaults(): (s: SamplingSettings) => void {
  const { read, upsert } = useConfig();
  return useCallback(
    (s: SamplingSettings) => {
      const clean = sanitizeSampling(s);
      saveSamplingDefaults(clean);
      void (async () => {
        try {
          const raw = ((await read('swarm', false)) ?? {}) as Record<string, unknown>;
          await upsert(
            'swarm',
            {
              ...raw,
              temperature: clean.temperature ?? null,
              top_p: clean.topP ?? null,
              top_k: clean.topK ?? null,
              min_p: clean.minP ?? null,
              repeat_penalty: clean.repeatPenalty ?? null,
            },
            false
          );
        } catch {
          // config write is best-effort — localStorage already holds the defaults
        }
      })();
    },
    [read, upsert]
  );
}
