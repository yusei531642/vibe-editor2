import { useEffect, useMemo } from 'react';
import type { RuntimeProvider } from '../../../../../../types/agent-runtime';
import { useRuntimeStore } from '../../../../stores/runtime';
import type { TerminalRuntimeStatus } from '../../../../lib/terminal-status';
import type { AgentPayload } from './types';

export function isNativeRuntimeProvider(
  provider?: RuntimeProvider
): provider is Extract<RuntimeProvider, 'claude-native' | 'codex-native'> {
  return provider === 'claude-native' || provider === 'codex-native';
}

export function NativeRuntimeConnector({
  cardId,
  payload,
  systemPrompt,
  initialMessage,
  setCardPayload,
  onStatus
}: {
  cardId: string;
  payload: AgentPayload;
  systemPrompt?: string;
  initialMessage?: string;
  setCardPayload: (id: string, patch: Record<string, unknown>) => void;
  onStatus: (status: TerminalRuntimeStatus | null) => void;
}): JSX.Element | null {
  const provider = payload.runtimeProvider;
  const endpointId = useMemo(
    () => (payload.agentId ? `native-${payload.agentId}` : null),
    [payload.agentId]
  );

  useEffect(() => {
    if (!endpointId || !payload.agentId || !payload.teamId) return;
    if (!isNativeRuntimeProvider(provider)) return;
    let disposed = false;
    let registered = false;
    let unsubscribe: (() => void) | null = null;
    const start = async (): Promise<void> => {
      onStatus({ kind: 'starting', command: provider });
      unsubscribe = await window.api.agentRuntime.onEventReady(
        endpointId,
        useRuntimeStore.getState().projectEvent
      );
      if (disposed) {
        unsubscribe();
        return;
      }
      if (provider === 'claude-native') {
        await window.api.agentRuntime.reconnectClaude({
          endpointId,
          teamId: payload.teamId,
          agentId: payload.agentId,
          systemPrompt: systemPrompt ?? null,
          session: { mode: 'start' }
        });
      } else {
        await window.api.agentRuntime.reconnectCodex({
          endpointId,
          teamId: payload.teamId,
          agentId: payload.agentId,
          cwd: null,
          thread: { mode: 'start' }
        });
      }
      registered = true;
      if (disposed) {
        await window.api.agentRuntime.dispose(endpointId);
        return;
      }
      const bootstrap = initialMessage?.trim() ||
        (provider === 'codex-native' ? systemPrompt?.trim() : '') ||
        'Start your assigned team role and read pending TeamHub messages.';
      await window.api.agentRuntime.spawnTurn({
        endpointId,
        input: bootstrap,
        submit: true
      });
      onStatus({ kind: 'running', command: provider });
    };
    void start().catch((error) => {
      if (disposed) return;
      const message = error instanceof Error ? error.message : String(error);
      onStatus({ kind: 'spawn_failed', command: provider, error: message });
      const fallbackFrom = provider as Extract<
        RuntimeProvider,
        'codex-native' | 'claude-native'
      >;
      // Runtime availability can change after Rust policy selection. Preserve the failure
      // explicitly on the card while activating the compatibility PTY path.
      setCardPayload(cardId, { runtimeProvider: 'pty', fallbackFrom });
      if (registered) void window.api.agentRuntime.dispose(endpointId).catch(() => undefined);
    });
    return () => {
      disposed = true;
      unsubscribe?.();
      if (registered) void window.api.agentRuntime.dispose(endpointId).catch(() => undefined);
    };
  }, [
    cardId,
    endpointId,
    initialMessage,
    onStatus,
    payload.agentId,
    payload.teamId,
    provider,
    setCardPayload,
    systemPrompt
  ]);

  if (!isNativeRuntimeProvider(provider)) return null;
  return (
    <div className="canvas-agent-native-runtime" data-provider={provider} role="status">
      {provider === 'claude-native' ? 'Claude native runtime' : 'Codex native runtime'}
    </div>
  );
}
