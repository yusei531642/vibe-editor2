import { useEffect, useMemo, useState } from 'react';
import type {
  RuntimeEngine,
  RuntimeModelOption
} from '../../../../types/agent-runtime';
import { defaultRuntimeModel } from '../v2-runtime-controls';

export interface V2RuntimeCatalogState {
  models: RuntimeModelOption[];
  loading: boolean;
  error: string | null;
}

export function useV2RuntimeCatalog(
  engine: RuntimeEngine,
  enabled = true
): V2RuntimeCatalogState {
  const [byEngine, setByEngine] = useState<Partial<Record<RuntimeEngine, RuntimeModelOption[]>>>({});
  const [loadingEngine, setLoadingEngine] = useState<RuntimeEngine | null>(null);
  const [errorByEngine, setErrorByEngine] = useState<Partial<Record<RuntimeEngine, string>>>({});

  useEffect(() => {
    if (!enabled || byEngine[engine]) return;
    let cancelled = false;
    setLoadingEngine(engine);
    void window.api.agentRuntime.modelCatalog(engine).then((catalog) => {
      if (cancelled) return;
      const preferred = defaultRuntimeModel(catalog);
      const models = preferred
        ? [preferred, ...catalog.models.filter((model) => model.id !== preferred.id)]
        : catalog.models;
      setByEngine((current) => ({ ...current, [engine]: models }));
      setErrorByEngine((current) => ({ ...current, [engine]: undefined }));
    }).catch((error) => {
      if (cancelled) return;
      setErrorByEngine((current) => ({
        ...current,
        [engine]: error instanceof Error ? error.message : String(error)
      }));
    }).finally(() => {
      if (!cancelled) setLoadingEngine((current) => current === engine ? null : current);
    });
    return () => {
      cancelled = true;
    };
  }, [byEngine, enabled, engine]);

  return useMemo(() => ({
    models: byEngine[engine] ?? [],
    loading: loadingEngine === engine,
    error: errorByEngine[engine] ?? null
  }), [byEngine, engine, errorByEngine, loadingEngine]);
}
