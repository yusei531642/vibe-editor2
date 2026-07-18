// This file is generated from src-tauri/src/agent_runtime/event.rs via ts-rs.
// Run `npm run generate:runtime-event-types` after changing runtime event wire types.

export type RuntimeEventKind = "messageDelta" | "messageComplete" | "toolUse" | "diff" | "usage" | "approvalRequest" | "lifecycle" | "error" | "diagnostic" | "turnComplete";

export type RuntimeLifecycleState = "spawning" | "ready" | "exited" | "failed";

export type RuntimeEventPayload = { "type": "messageDelta", delta: string, } | { "type": "messageComplete", message: string, } | { "type": "toolUse", toolName: string, callId: string | null, status: string, detail: string | null, } | { "type": "diff", diff: string, } | { "type": "usage", inputTokens: number, cachedInputTokens: number, outputTokens: number, } | { "type": "approvalRequest", requestId: string, method: string, reason: string | null, command: string | null, cwd: string | null, } | { "type": "lifecycle", state: RuntimeLifecycleState, detail: string | null, } | { "type": "error", code: string, message: string, recoverable: boolean, } | { "type": "diagnostic", message: string, } | { "type": "turnComplete", interrupted: boolean, };

export type RuntimeEventEnvelope = { endpointId: string, 
/**
 * endpoint registration unit. sequence is monotonic only within this epoch.
 */
epoch: number, 
/**
 * JSON/JS renderer では number として扱う。endpoint ごとの process-local counter なので
 * JavaScript の safe integer 上限へ到達する前に session lifetime が終わる。
 */
sequence: number, kind: RuntimeEventKind, payload: RuntimeEventPayload, timestamp: string, };
