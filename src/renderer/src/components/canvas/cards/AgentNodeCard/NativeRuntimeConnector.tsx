import { useEffect, useMemo, useRef } from 'react';
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
  onStatus
}: {
  cardId: string;
  payload: AgentPayload;
  systemPrompt?: string;
  initialMessage?: string;
  onStatus: (status: TerminalRuntimeStatus | null) => void;
}): JSX.Element | null {
  const provider = payload.runtimeProvider;
  const endpointId = useMemo(
    () => (payload.agentId ? `native-${payload.agentId}` : null),
    [payload.agentId]
  );
  const runtimeIdentity = `${cardId}:${provider ?? ''}:${payload.teamId ?? ''}:${payload.agentId ?? ''}`;
  // Team membership updates regenerate the prompt. They must not dispose an active session.
  const systemPromptRef = useRef(systemPrompt);
  systemPromptRef.current = systemPrompt;
  const initialMessageRef = useRef(initialMessage);
  initialMessageRef.current = initialMessage;
  const onStatusRef = useRef(onStatus);
  onStatusRef.current = onStatus;
  const runtimeOptionsRef = useRef({
    model: payload.runtimeModel ?? null,
    effort: payload.runtimeEffort ?? null,
    permission: payload.runtimePermission ?? 'workspace'
  });
  runtimeOptionsRef.current = {
    model: payload.runtimeModel ?? null,
    effort: payload.runtimeEffort ?? null,
    permission: payload.runtimePermission ?? 'workspace'
  };
  const bootstrappedIdentityRef = useRef<string | null>(null);

  useEffect(() => {
    if (!endpointId || !payload.agentId || !payload.teamId) return;
    if (!isNativeRuntimeProvider(provider)) return;
    let disposed = false;
    let registered = false;
    let unsubscribe: (() => void) | null = null;
    const start = async (): Promise<void> => {
      onStatusRef.current({ kind: 'starting', command: provider });
      unsubscribe = await window.api.agentRuntime.onEventReady(
        endpointId,
        useRuntimeStore.getState().projectEvent
      );
      if (disposed) {
        unsubscribe();
        return;
      }
      const runtimeOptions = runtimeOptionsRef.current;
      if (provider === 'claude-native') {
        await window.api.agentRuntime.reconnectClaude({
          endpointId,
          teamId: payload.teamId,
          agentId: payload.agentId,
          systemPrompt: systemPromptRef.current ?? null,
          model: runtimeOptions.model,
          effort: runtimeOptions.effort,
          permission: runtimeOptions.permission,
          session: { mode: 'start' }
        });
      } else {
        await window.api.agentRuntime.reconnectCodex({
          endpointId,
          teamId: payload.teamId,
          agentId: payload.agentId,
          cwd: null,
          model: runtimeOptions.model,
          permission: runtimeOptions.permission,
          thread: { mode: 'start' }
        });
      }
      registered = true;
      if (disposed) {
        await window.api.agentRuntime.dispose(endpointId);
        return;
      }
      const bootstrap = (bootstrappedIdentityRef.current === runtimeIdentity
        ? ''
        : initialMessageRef.current?.trim()) ||
        (provider === 'codex-native' ? systemPromptRef.current?.trim() : '') ||
        'Start your assigned team role and read pending TeamHub messages.';
      await window.api.agentRuntime.spawnTurn({
        endpointId,
        input: bootstrap,
        submit: true,
        model: runtimeOptions.model,
        effort: runtimeOptions.effort,
        permission: runtimeOptions.permission
      });
      bootstrappedIdentityRef.current = runtimeIdentity;
      onStatusRef.current({ kind: 'running', command: provider });
    };
    void start().catch((error) => {
      if (disposed) return;
      const message = error instanceof Error ? error.message : String(error);
      onStatusRef.current({ kind: 'spawn_failed', command: provider, error: message });
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
    payload.agentId,
    payload.teamId,
    provider,
    runtimeIdentity
  ]);

  return null;
}
