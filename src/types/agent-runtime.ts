/**
 * Issue #21 (Phase 0): agent runtime backend の選択と capability 診断の共有型。
 * Rust 側 `src-tauri/src/agent_runtime/mod.rs` / `src-tauri/src/commands/agent_runtime.rs`
 * の serde 出力 (camelCase) と同期する。Phase 1 以降の Runtime 契約 / Event Envelope 型も
 * このファイルへ集約する (shared.ts の file-size ratchet を圧迫しないため)。
 */

export type AgentRuntimeBackend = 'auto' | 'native' | 'pty';

export type AgentRuntimeCapability =
  | 'ptyExecution'
  | 'nativeProcessExecution'
  | 'structuredEventStream'
  | 'cooperativeCancellation';

export type AgentRuntimeSelectionReason =
  | 'explicitPty'
  | 'explicitNativeAvailable'
  | 'nativeCapabilitiesUnavailable'
  | 'autoNativeCapabilitiesAvailable'
  | 'autoPtyFallback';

export interface AgentRuntimeDiagnostics {
  requestedBackend: AgentRuntimeBackend;
  selectedBackend: Exclude<AgentRuntimeBackend, 'auto'>;
  reason: AgentRuntimeSelectionReason;
  capabilities: AgentRuntimeCapability[];
}

export type {
  RuntimeEventEnvelope,
  RuntimeEventKind,
  RuntimeEventPayload,
  RuntimeLifecycleState
} from './generated/runtime-events';

export interface RegisterPtyRuntimeEndpointRequest {
  endpointId: string;
  sessionId: string;
}

export interface RuntimeTurnRequest {
  endpointId: string;
  input: string;
  submit: boolean;
}

export interface RuntimeEndpointResult {
  endpointId: string;
}
