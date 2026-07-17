/**
 * Issue #21 (Phase 0): agent runtime backend の選択と capability 診断の共有型。
 * Rust 側 `src-tauri/src/agent_runtime/mod.rs` / `src-tauri/src/commands/agent_runtime.rs`
 * の serde 出力 (camelCase) と同期する。Phase 1 以降の Runtime 契約 / Event Envelope 型も
 * このファイルへ集約する (shared.ts の file-size ratchet を圧迫しないため)。
 */

export type AgentRuntimeBackend = 'auto' | 'native' | 'pty';
export type RuntimeProvider = 'codex-native' | 'claude-native' | 'pty' | 'api';

export type AgentRuntimeCapability =
  | 'ptyExecution'
  | 'nativeProcessExecution'
  | 'structuredEventStream'
  | 'cooperativeCancellation'
  | 'sessionResume'
  | 'sessionFork'
  | 'turnSteering'
  | 'approvalResponses';

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
  providers: Array<{
    provider: RuntimeProvider;
    available: boolean;
    capabilities: AgentRuntimeCapability[];
  }>;
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

export type CodexThreadAction =
  | { mode: 'start' }
  | { mode: 'resume'; threadId: string }
  | { mode: 'fork'; threadId: string };

/**
 * DESIGN.md "Runtime boundary": renderer は endpoint 意図のみを渡す。
 * codex 実行コマンドは settings.json、control socket は Rust 側 daemon 検出が正本で、
 * renderer から raw path / argv は受け付けない。cwd は project authority 照合を通る。
 */
export interface RegisterCodexRuntimeEndpointRequest {
  endpointId: string;
  teamId?: string | null;
  agentId?: string | null;
  cwd?: string | null;
  thread: CodexThreadAction;
}

export type ClaudeSessionAction =
  | { mode: 'start' }
  | { mode: 'resume'; sessionId: string }
  | { mode: 'fork'; sessionId: string };

export interface RegisterClaudeRuntimeEndpointRequest {
  endpointId: string;
  teamId?: string | null;
  agentId?: string | null;
  systemPrompt?: string | null;
  session: ClaudeSessionAction;
}

export interface RuntimeSteerRequest {
  endpointId: string;
  input: string;
}

export type RuntimeApprovalDecision =
  | 'accept'
  | 'acceptForSession'
  | 'decline'
  | 'cancel';

export interface RuntimeApprovalResponseRequest {
  endpointId: string;
  requestId: string;
  decision: RuntimeApprovalDecision;
}

export interface RuntimeEndpointResult {
  endpointId: string;
}

export interface CodexRuntimeEndpointResult extends RuntimeEndpointResult {
  threadId: string;
}

export interface ClaudeRuntimeEndpointResult extends RuntimeEndpointResult {
  sessionId?: string | null;
}
