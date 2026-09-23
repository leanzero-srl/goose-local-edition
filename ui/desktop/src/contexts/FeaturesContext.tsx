import { createContext, useContext, useEffect, useState, useMemo } from 'react';
import { getAcpFeatureCapabilities } from '../acp/capabilities';

interface FeaturesContextValue {
  localInference: boolean;
  mlxEngine: boolean;
  /** The distributed MLX engine (one model across several Macs) — absent on older backends. */
  mlxDistributed: boolean;
  leanzeroLink: boolean;
  isLoading: boolean;
}

const FeaturesContext = createContext<FeaturesContextValue | null>(null);

export function FeaturesProvider({ children }: { children: React.ReactNode }) {
  const [localInference, setLocalInference] = useState(false);
  const [mlxEngine, setMlxEngine] = useState(false);
  const [mlxDistributed, setMlxDistributed] = useState(false);
  const [leanzeroLink, setLeanzeroLink] = useState(false);
  const [isLoading, setIsLoading] = useState(true);

  useEffect(() => {
    (async () => {
      try {
        const capabilities = await getAcpFeatureCapabilities();
        setLocalInference(capabilities.localInference);
        setMlxEngine(capabilities.mlxEngine ?? false);
        setMlxDistributed(capabilities.mlxDistributed ?? false);
        setLeanzeroLink(capabilities.leanzeroLink ?? false);
      } catch (error) {
        console.warn('[FeaturesContext] Failed to fetch features:', error);
      } finally {
        setIsLoading(false);
      }
    })();
  }, []);

  const value = useMemo<FeaturesContextValue>(
    () => ({
      localInference,
      mlxEngine,
      mlxDistributed,
      leanzeroLink,
      isLoading,
    }),
    [localInference, mlxEngine, mlxDistributed, leanzeroLink, isLoading]
  );

  return <FeaturesContext.Provider value={value}>{children}</FeaturesContext.Provider>;
}

export function useFeatures(): FeaturesContextValue {
  const context = useContext(FeaturesContext);
  if (!context) {
    throw new Error('useFeatures must be used within a FeaturesProvider');
  }
  return context;
}
