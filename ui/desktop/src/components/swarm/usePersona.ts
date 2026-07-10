import { useCallback, useEffect, useState } from 'react';
import { useConfig } from '../ConfigContext';
import type { Persona } from './PersonaChooser';

/**
 * Persists the Local Edition persona (Coding | Agent) via the same config-key mechanism GOOSE_MODE uses
 * (see settings/mode/ModeSection). 'coding' = interactive build with the fleet; 'agent' = the autonomous
 * implementation that runs a loop with a recipe + skills.
 */
const KEY = 'LEANZERO_PERSONA';

export function usePersona(): { persona: Persona; setPersona: (p: Persona) => void } {
  const { read, upsert } = useConfig();
  const [persona, setPersonaState] = useState<Persona>('coding');

  useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const v = (await read(KEY, false)) as Persona | null;
        if (alive && (v === 'coding' || v === 'agent')) setPersonaState(v);
      } catch {
        /* default to coding */
      }
    })();
    return () => {
      alive = false;
    };
  }, [read]);

  const setPersona = useCallback(
    (p: Persona) => {
      setPersonaState(p);
      void upsert(KEY, p, false).catch(() => {});
    },
    [upsert]
  );

  return { persona, setPersona };
}
