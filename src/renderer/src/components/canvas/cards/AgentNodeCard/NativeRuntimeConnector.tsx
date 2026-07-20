import { useEffect, useMemo, useRef, useState } from 'react';
import type { RuntimeProvider } from '../../../../../../types/agent-runtime';
import { useV2RuntimeCatalog } from '../../../../lib/hooks/use-v2-runtime-catalog';
import { useT } from '../../../../lib/i18n';
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
  onStatus,
  setCardPayload
}: {
  cardId: string;
  payload: AgentPayload;
  systemPrompt?: string;
  initialMessage?: string;
  onStatus: (status: TerminalRuntimeStatus | null) => void;
  setCardPayload?: (patch: Partial<AgentPayload>) => void;
}): JSX.Element | null {
  const t = useT();
  const [failure, setFailure] = useState<string | null>(null);
  const [retryNonce, setRetryNonce] = useState(0);
  const provider = payload.runtimeProvider;
  const engine = provider === 'codex-native' ? 'codex' : 'claude';
  const catalog = useV2RuntimeCatalog(engine, isNativeRuntimeProvider(provider));
  const resolvedModel = payload.runtimeModel ?? catalog.models[0]?.id ?? null;
  const resolvedModelOption = catalog.models.find((option) => option.id === resolvedModel);
  const resolvedEffort = payload.runtimeEffort
    ?? resolvedModelOption?.defaultEffort
    ?? resolvedModelOption?.supportedEfforts[0]
    ?? null;
  const waitingForInitialCatalog = (
    !payload.runtimeModel && catalog.models.length === 0 && !catalog.error
  );
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
    model: resolvedModel,
    effort: resolvedEffort,
    permission: 'workspace' as const
  });
  runtimeOptionsRef.current = {
    model: resolvedModel,
    effort: resolvedEffort,
    permission: 'workspace'
  };
  const bootstrappedIdentityRef = useRef<string | null>(null);

  useEffect(() => {
    if (!resolvedModel) return;
    const patch: Partial<AgentPayload> = {};
    if (!payload.runtimeModel) patch.runtimeModel = resolvedModel;
    if (!payload.runtimeEffort && resolvedEffort) patch.runtimeEffort = resolvedEffort;
    if (Object.keys(patch).length > 0) setCardPayload?.(patch);
  }, [payload.runtimeEffort, payload.runtimeModel, resolvedEffort, resolvedModel, setCardPayload]);

  useEffect(() => {
    if (!endpointId || !payload.agentId || !payload.teamId) return;
    if (!isNativeRuntimeProvider(provider)) return;
    if (waitingForInitialCatalog) return;
    let disposed = false;
    let registered = false;
    let unsubscribe: (() => void) | null = null;
    const start = async (): Promise<void> => {
      setFailure(null);
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
      if (bootstrappedIdentityRef.current === runtimeIdentity) {
        onStatusRef.current({ kind: 'running', command: provider });
        return;
      }
      const bootstrap = initialMessageRef.current?.trim() ||
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
      setFailure(message);
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
    retryNonce,
    runtimeIdentity,
    waitingForInitialCatalog
  ]);

  return failure ? (
    <button
      type="button"
      className="team-chat-runtime-retry"
      title={failure}
      onClick={() => setRetryNonce((current) => current + 1)}
    >
      {t('v2.team.card.reconnect')}
    </button>
  ) : null;
}
