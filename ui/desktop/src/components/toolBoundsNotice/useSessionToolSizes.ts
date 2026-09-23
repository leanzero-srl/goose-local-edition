import { useCallback, useEffect, useRef, useState } from 'react';
import { listTools } from '../../acp/permissions';
import { getSessionExtensions } from '../../acp/session-extensions';
import { errorMessage } from '../../utils/conversionUtils';
import { measureTools, type ToolGroupSize } from './toolSchemaBounds';

export interface ExtensionToolSize extends ToolGroupSize {
  /** The session extension's name — what the per-session toggle removes. */
  name: string;
}

export type SessionToolSizes =
  | { state: 'idle' }
  | { state: 'loading' }
  | { state: 'failed'; error: string }
  | {
      state: 'ready';
      /** Largest first, by the engine's byte measure. */
      extensions: ExtensionToolSize[];
      /** Tools the session sends that no session extension owns (goose's own platform tools). */
      unowned: ToolGroupSize;
      total: ToolGroupSize;
    };

/**
 * The session's tools, grouped by the extension that owns them and measured in the engine's
 * units. The grouping is the SERVER's: `toolsList_unstable` filtered by extension name applies the
 * same owner rule the agent uses (extension_manager.rs `filter_tools`), so a tool is never
 * attributed by guessing at its name prefix. One call per session extension, plus one for the whole
 * list to catch tools no extension owns.
 */
export function useSessionToolSizes(sessionId: string, armed: boolean) {
  const [sizes, setSizes] = useState<SessionToolSizes>({ state: 'idle' });
  const generation = useRef(0);

  const load = useCallback(async () => {
    const mine = ++generation.current;
    setSizes({ state: 'loading' });
    try {
      const extensions = await getSessionExtensions(sessionId);
      const [all, ...perExtension] = await Promise.all([
        listTools(sessionId),
        ...extensions.map((extension) => listTools(sessionId, extension.name)),
      ]);
      const owned = new Set(perExtension.flat().map((tool) => tool.name));
      const rows = extensions
        .map((extension, i) => ({ name: extension.name, ...measureTools(perExtension[i]) }))
        .sort((a, b) => b.bytes - a.bytes || b.tools - a.tools);
      if (mine !== generation.current) return;
      setSizes({
        state: 'ready',
        extensions: rows,
        unowned: measureTools(all.filter((tool) => !owned.has(tool.name))),
        total: measureTools(all),
      });
    } catch (error) {
      if (mine !== generation.current) return;
      setSizes({ state: 'failed', error: errorMessage(error) });
    }
  }, [sessionId]);

  useEffect(() => {
    if (!armed) return;
    void load();
    const inFlight = generation;
    return () => {
      inFlight.current++;
    };
  }, [armed, load]);

  return { sizes, reload: load };
}
