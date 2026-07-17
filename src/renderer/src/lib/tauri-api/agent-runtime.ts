// tauri-api/agent-runtime.ts — Issue #21 diagnostics / Issue #22 runtime endpoints.

import { invokeCommand } from './command-error';
import { subscribeEventReady } from '../subscribe-event';
import type {
  AgentRuntimeBackend,
  AgentRuntimeDiagnostics,
  ClaudeRuntimeEndpointResult,
  CodexRuntimeEndpointResult,
  RegisterCodexRuntimeEndpointRequest,
  RegisterClaudeRuntimeEndpointRequest,
  RegisterPtyRuntimeEndpointRequest,
  RuntimeApprovalResponseRequest,
  RuntimeEndpointResult,
  RuntimeEventEnvelope,
  RuntimeSteerRequest,
  RuntimeTurnRequest
} from '../../../../types/agent-runtime';

export const agentRuntime = {
  diagnostics: (backend: AgentRuntimeBackend): Promise<AgentRuntimeDiagnostics> =>
    invokeCommand('agent_runtime_diagnostics', { backend }),

  registerPtyEndpoint: (
    request: RegisterPtyRuntimeEndpointRequest
  ): Promise<RuntimeEndpointResult> =>
    invokeCommand('agent_runtime_register_pty_endpoint', { request }),

  registerCodexEndpoint: (
    request: RegisterCodexRuntimeEndpointRequest
  ): Promise<CodexRuntimeEndpointResult> =>
    invokeCommand('agent_runtime_register_codex_endpoint', { request }),

  registerClaudeEndpoint: (
    request: RegisterClaudeRuntimeEndpointRequest
  ): Promise<ClaudeRuntimeEndpointResult> =>
    invokeCommand('agent_runtime_register_claude_endpoint', { request }),

  reconnectCodex: (
    request: RegisterCodexRuntimeEndpointRequest
  ): Promise<CodexRuntimeEndpointResult> =>
    invokeCommand('agent_runtime_reconnect_codex', { request }),

  reconnectClaude: (
    request: RegisterClaudeRuntimeEndpointRequest
  ): Promise<ClaudeRuntimeEndpointResult> =>
    invokeCommand('agent_runtime_reconnect_claude', { request }),

  spawnTurn: (request: RuntimeTurnRequest): Promise<RuntimeEndpointResult> =>
    invokeCommand('agent_runtime_spawn_turn', { request }),

  write: (endpointId: string, data: string): Promise<RuntimeEndpointResult> =>
    invokeCommand('agent_runtime_write', { endpointId, data }),

  inject: (endpointId: string, data: string): Promise<RuntimeEndpointResult> =>
    invokeCommand('agent_runtime_inject', { endpointId, data }),

  steer: (request: RuntimeSteerRequest): Promise<RuntimeEndpointResult> =>
    invokeCommand('agent_runtime_steer', { request }),

  interrupt: (endpointId: string): Promise<RuntimeEndpointResult> =>
    invokeCommand('agent_runtime_interrupt', { endpointId }),

  respondApproval: (
    request: RuntimeApprovalResponseRequest
  ): Promise<RuntimeEndpointResult> =>
    invokeCommand('agent_runtime_respond_approval', { request }),

  stop: (endpointId: string): Promise<RuntimeEndpointResult> =>
    invokeCommand('agent_runtime_stop', { endpointId }),

  dispose: (endpointId: string): Promise<RuntimeEndpointResult> =>
    invokeCommand('agent_runtime_dispose', { endpointId }),

  /**
   * Client-generated endpointId で register 前に await し、初期 lifecycle を取り逃さない。
   * Issue #285: returned promise の解決直後に caller が disposed flag を再確認し、set 済みなら
   * 返された cleanup を即時に呼ぶこと。await pending 中の dispose は helper 側では検知できない。
   */
  onEventReady: (
    endpointId: string,
    cb: (event: RuntimeEventEnvelope) => void
  ): Promise<() => void> =>
    subscribeEventReady<RuntimeEventEnvelope>(`runtime:event:${endpointId}`, cb)
};
