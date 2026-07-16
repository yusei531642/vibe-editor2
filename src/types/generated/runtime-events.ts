// This file is generated from src-tauri/src/agent_runtime/event.rs via ts-rs.
// Run `npm run generate:runtime-event-types` after changing runtime event wire types.

export type RuntimeEventKind = "messageDelta" | "messageComplete" | "lifecycle" | "error" | "diagnostic";

export type RuntimeLifecycleState = "spawning" | "ready" | "exited" | "failed";

export type RuntimeEventPayload = { "type": "messageDelta", delta: string, } | { "type": "messageComplete", message: string, } | { "type": "lifecycle", state: RuntimeLifecycleState, detail: string | null, } | { "type": "error", code: string, message: string, recoverable: boolean, } | { "type": "diagnostic", message: string, };

export type RuntimeEventEnvelope = { endpointId: string, 
/**
 * JSON/JS renderer では number として扱う。endpoint ごとの process-local counter なので
 * JavaScript の safe integer 上限へ到達する前に session lifetime が終わる。
 */
sequence: number, kind: RuntimeEventKind, payload: RuntimeEventPayload, timestamp: string, };
