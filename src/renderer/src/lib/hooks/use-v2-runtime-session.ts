import { useCallback, useEffect, useRef, useState } from 'react';
import type {
  RuntimeApprovalDecision,
  RuntimeEngine,
  RuntimeEventEnvelope,
  RuntimePermission
} from '../../../../types/agent-runtime';

export interface V2PendingApproval {
  endpointId: string;
  requestId: string;
  method: string;
  reason: string | null;
}

interface RuntimeCallbacks {
  onDelta: (delta: string, engine: RuntimeEngine) => void;
  onComplete: (message: string, engine: RuntimeEngine) => void;
  onError: (message: string, engine: RuntimeEngine) => void;
}

interface EndpointBinding {
  endpointId: string;
  engine: RuntimeEngine;
  unsubscribe: () => void;
}

export function useV2RuntimeSession(callbacks: RuntimeCallbacks): {
  running: boolean;
  pendingApproval: V2PendingApproval | null;
  send: (request: {
    input: string;
    engine: RuntimeEngine;
    model: string;
    effort: string;
    permission: RuntimePermission;
  }) => Promise<void>;
  stop: () => Promise<void>;
  reset: () => Promise<void>;
  respondApproval: (decision: RuntimeApprovalDecision) => Promise<void>;
} {
  const callbacksRef = useRef(callbacks);
  callbacksRef.current = callbacks;
  const bindingRef = useRef<EndpointBinding | null>(null);
  const [running, setRunning] = useState(false);
  const [pendingApproval, setPendingApproval] = useState<V2PendingApproval | null>(null);

  const handleEvent = useCallback((event: RuntimeEventEnvelope): void => {
    const binding = bindingRef.current;
    if (!binding || binding.endpointId !== event.endpointId) return;
    const engine = binding.engine;
    switch (event.payload.type) {
      case 'messageDelta':
        callbacksRef.current.onDelta(event.payload.delta, engine);
        break;
      case 'messageComplete':
        callbacksRef.current.onComplete(event.payload.message, engine);
        break;
      case 'approvalRequest':
        setPendingApproval({
          endpointId: event.endpointId,
          requestId: event.payload.requestId,
          method: event.payload.method,
          reason: event.payload.reason
        });
        break;
      case 'turnComplete':
        setRunning(false);
        setPendingApproval(null);
        break;
      case 'error':
        callbacksRef.current.onError(event.payload.message, engine);
        setRunning(false);
        break;
      case 'lifecycle':
        if (event.payload.state === 'failed' || event.payload.state === 'exited') {
          setRunning(false);
          setPendingApproval(null);
        }
        break;
      default:
        break;
    }
  }, []);

  const disposeBinding = useCallback(async (): Promise<void> => {
    const binding = bindingRef.current;
    bindingRef.current = null;
    binding?.unsubscribe();
    setPendingApproval(null);
    setRunning(false);
    if (binding) {
      await window.api.agentRuntime.dispose(binding.endpointId).catch(() => undefined);
    }
  }, []);

  const ensureBinding = useCallback(async (
    engine: RuntimeEngine,
    model: string,
    effort: string,
    permission: RuntimePermission
  ): Promise<EndpointBinding> => {
    if (bindingRef.current?.engine === engine) return bindingRef.current;
    await disposeBinding();
    const endpointId = `v2-${engine}-${crypto.randomUUID()}`;
    const unsubscribe = await window.api.agentRuntime.onEventReady(endpointId, handleEvent);
    const binding = { endpointId, engine, unsubscribe };
    bindingRef.current = binding;
    try {
      if (engine === 'claude') {
        await window.api.agentRuntime.registerClaudeEndpoint({
          endpointId,
          teamId: null,
          agentId: null,
          systemPrompt: null,
          model: model || null,
          effort: effort || null,
          permission,
          session: { mode: 'start' }
        });
      } else {
        await window.api.agentRuntime.registerCodexEndpoint({
          endpointId,
          teamId: null,
          agentId: null,
          cwd: null,
          model: model || null,
          permission,
          thread: { mode: 'start' }
        });
      }
      return binding;
    } catch (error) {
      if (bindingRef.current?.endpointId === endpointId) bindingRef.current = null;
      unsubscribe();
      throw error;
    }
  }, [disposeBinding, handleEvent]);

  const send = useCallback(async ({
    input,
    engine,
    model,
    effort,
    permission
  }: {
    input: string;
    engine: RuntimeEngine;
    model: string;
    effort: string;
    permission: RuntimePermission;
  }): Promise<void> => {
    setPendingApproval(null);
    try {
      const binding = await ensureBinding(engine, model, effort, permission);
      setRunning(true);
      await window.api.agentRuntime.spawnTurn({
        endpointId: binding.endpointId,
        input,
        submit: true,
        model: model || null,
        effort: effort || null,
        permission
      });
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      callbacksRef.current.onError(message, engine);
      setRunning(false);
      throw error;
    }
  }, [ensureBinding]);

  const stop = useCallback(async (): Promise<void> => {
    const binding = bindingRef.current;
    if (!binding) {
      setRunning(false);
      return;
    }
    await window.api.agentRuntime.interrupt(binding.endpointId);
    setRunning(false);
    setPendingApproval(null);
  }, []);

  const respondApproval = useCallback(async (
    decision: RuntimeApprovalDecision
  ): Promise<void> => {
    const approval = pendingApproval;
    if (!approval) return;
    await window.api.agentRuntime.respondApproval({
      endpointId: approval.endpointId,
      requestId: approval.requestId,
      decision
    });
    setPendingApproval(null);
  }, [pendingApproval]);

  useEffect(() => () => {
    void disposeBinding();
  }, [disposeBinding]);

  return {
    running,
    pendingApproval,
    send,
    stop,
    reset: disposeBinding,
    respondApproval
  };
}
