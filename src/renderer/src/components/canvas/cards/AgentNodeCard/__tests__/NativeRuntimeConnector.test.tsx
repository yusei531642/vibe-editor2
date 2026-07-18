import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentPayload } from '../types';
import { NativeRuntimeConnector } from '../NativeRuntimeConnector';

vi.mock('../../../../../lib/i18n', () => ({
  useT: () => (key: string) => key
}));

const runtime = vi.hoisted(() => ({
  dispose: vi.fn().mockResolvedValue(undefined),
  onEventReady: vi.fn().mockResolvedValue(vi.fn()),
  reconnectClaude: vi.fn().mockResolvedValue({ endpointId: 'native-agent-1' }),
  reconnectCodex: vi.fn().mockResolvedValue({ endpointId: 'native-agent-1' }),
  spawnTurn: vi.fn().mockResolvedValue({ endpointId: 'native-agent-1' })
}));

function payload(overrides: Partial<AgentPayload> = {}): AgentPayload {
  return {
    agentId: 'agent-1',
    teamId: 'team-1',
    runtimeProvider: 'claude-native',
    runtimeModel: 'claude-fable-5',
    runtimeEffort: 'high',
    runtimePermission: 'workspace',
    ...overrides
  };
}

describe('NativeRuntimeConnector', () => {
  let originalApi: typeof window.api;

  beforeEach(() => {
    originalApi = window.api;
    const currentApi = window.api ?? {};
    window.api = {
      ...currentApi,
      agentRuntime: { ...currentApi.agentRuntime, ...runtime }
    } as typeof window.api;
    for (const mock of Object.values(runtime)) mock.mockClear();
  });

  afterEach(() => {
    cleanup();
    window.api = originalApi;
  });

  it('model・effort・permission の変更では endpoint と初期指示を再起動しない', async () => {
    const onStatus = vi.fn();
    const view = render(
      <NativeRuntimeConnector
        cardId="card-1"
        payload={payload()}
        initialMessage="最初の指示"
        onStatus={onStatus}
      />
    );
    await waitFor(() => expect(runtime.spawnTurn).toHaveBeenCalledTimes(1));
    expect(runtime.spawnTurn).toHaveBeenCalledWith(expect.objectContaining({ input: '最初の指示' }));

    view.rerender(
      <NativeRuntimeConnector
        cardId="card-1"
        payload={payload({
          runtimeModel: 'claude-opus-4-6',
          runtimeEffort: 'max',
          runtimePermission: 'full'
        })}
        initialMessage="最初の指示"
        onStatus={onStatus}
      />
    );

    await Promise.resolve();
    expect(runtime.reconnectClaude).toHaveBeenCalledTimes(1);
    expect(runtime.spawnTurn).toHaveBeenCalledTimes(1);
    expect(runtime.dispose).not.toHaveBeenCalled();
  });

  it('native 登録失敗後も GUI から再接続できる', async () => {
    runtime.reconnectClaude.mockRejectedValueOnce(new Error('sidecar unavailable'));
    const onStatus = vi.fn();
    render(
      <NativeRuntimeConnector
        cardId="card-1"
        payload={payload()}
        initialMessage="最初の指示"
        onStatus={onStatus}
      />
    );
    const retry = await screen.findByRole('button', { name: 'v2.team.card.reconnect' });
    expect(onStatus).toHaveBeenCalledWith(expect.objectContaining({ kind: 'spawn_failed' }));

    fireEvent.click(retry);

    await waitFor(() => expect(runtime.reconnectClaude).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(runtime.spawnTurn).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole('button', { name: 'v2.team.card.reconnect' })).not.toBeInTheDocument();
  });
});
