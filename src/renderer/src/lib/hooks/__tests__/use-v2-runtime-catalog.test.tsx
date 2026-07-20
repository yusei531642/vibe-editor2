import { renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useV2RuntimeCatalog } from '../use-v2-runtime-catalog';

describe('useV2RuntimeCatalog', () => {
  const modelCatalog = vi.fn(async () => ({
    engine: 'codex' as const,
    models: [{
      id: 'gpt-5.4',
      label: 'GPT-5.4',
      description: 'Codex model',
      isDefault: true,
      defaultEffort: 'high',
      supportedEfforts: ['low', 'medium', 'high']
    }]
  }));

  beforeEach(() => {
    vi.clearAllMocks();
    Object.defineProperty(window, 'api', {
      configurable: true,
      value: { agentRuntime: { modelCatalog } }
    });
  });

  it('複数カードの同時取得を engine ごとに1回へ集約する', async () => {
    const first = renderHook(() => useV2RuntimeCatalog('codex'));
    const second = renderHook(() => useV2RuntimeCatalog('codex'));

    await waitFor(() => {
      expect(first.result.current.models).toHaveLength(1);
      expect(second.result.current.models).toHaveLength(1);
    });
    expect(modelCatalog).toHaveBeenCalledTimes(1);
  });
});
