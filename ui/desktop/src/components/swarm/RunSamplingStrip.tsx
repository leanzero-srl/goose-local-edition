import { useCallback, useEffect, useState } from 'react';
import SamplingKnobs from './SamplingKnobs';
import { useSaveSamplingDefaults } from './useSamplingDefaults';
import {
  hasAnySampling,
  loadSamplingDefaults,
  sanitizeSampling,
  type SamplingSettings,
} from './sampling';

/**
 * The normal-run sampling strip (BaseChat, above the swarm run panel). WYSIWYG: the values shown
 * are exactly what the NEXT `goose swarm run` in this working directory launches with — every edit
 * lands in <workingDir>/.swarm/run-sampling.json, which the swarm provider reads at spawn and
 * threads onto the engine child as GOOSE_SWARM_* env (env beats config, so a set knob overrides
 * the Settings default for that run only; a cleared knob falls back to config, then model default).
 *
 * Resolution order on mount: the file (a pending per-run override survives navigation), else the
 * shared defaults (`swarmSamplingDefaults`), which are then seeded into the file so what is shown
 * is what will ride. While a run is live the strip is read-only on the file the run launched with.
 */
export default function RunSamplingStrip({
  workingDir,
  active,
  className = '',
}: {
  workingDir?: string;
  active: boolean;
  className?: string;
}) {
  const [values, setValues] = useState<SamplingSettings | null>(null);
  const saveDefaults = useSaveSamplingDefaults();

  useEffect(() => {
    if (!workingDir) return;
    let alive = true;
    void (async () => {
      let fromFile: SamplingSettings = {};
      try {
        fromFile = sanitizeSampling(await window.electron.swarmGetSampling?.(workingDir));
      } catch {
        // unreadable file = no overrides
      }
      if (!alive) return;
      if (hasAnySampling(fromFile)) {
        setValues(fromFile);
      } else if (active) {
        // The live run launched with no per-run overrides — show that truth, not this
        // mount's defaults.
        setValues({});
      } else {
        const defaults = loadSamplingDefaults();
        setValues(defaults);
        if (hasAnySampling(defaults)) {
          void window.electron.swarmSetSampling?.(workingDir, defaults);
        }
      }
    })();
    return () => {
      alive = false;
    };
    // Re-resolve when the dir changes or a run starts/ends — not on every values edit.
  }, [workingDir, active]);

  const onChange = useCallback(
    (next: SamplingSettings) => {
      setValues(next);
      if (workingDir) void window.electron.swarmSetSampling?.(workingDir, next);
    },
    [workingDir]
  );

  if (!workingDir || values === null) return null;

  return (
    <SamplingKnobs
      value={values}
      onChange={onChange}
      active={active}
      onSaveDefaults={() => saveDefaults(values)}
      className={className}
    />
  );
}
