/** Issue #22: normalized runtime events の endpoint 別 projection。 */
import { create } from 'zustand';
import type {
  RuntimeEventEnvelope,
  RuntimeEventKind,
  RuntimeLifecycleState
} from '../../../types/agent-runtime';

export const RUNTIME_PROJECTION_HISTORY_LIMIT = 200;

export interface RuntimeSequenceGap {
  from: number;
  to: number;
}

export interface RuntimeProjectionError {
  code: string;
  message: string;
  recoverable: boolean;
}

export interface RuntimeToolUse {
  toolName: string;
  callId: string | null;
  status: string;
  detail: string | null;
}

export interface RuntimeUsage {
  inputTokens: number;
  cachedInputTokens: number;
  outputTokens: number;
}

export interface RuntimeApprovalRequest {
  requestId: string;
  method: string;
  reason: string | null;
  command: string | null;
  cwd: string | null;
}

export interface RuntimeEndpointProjection {
  endpointId: string;
  lastSequence: number;
  lastKind: RuntimeEventKind | null;
  lifecycle: RuntimeLifecycleState | null;
  currentMessage: string;
  completedMessages: string[];
  /** Consecutive message-delta payloads are merged into one chunk. */
  deltaChunks: string[];
  errors: RuntimeProjectionError[];
  diagnostics: string[];
  toolUses: RuntimeToolUse[];
  diffs: string[];
  usage: RuntimeUsage[];
  /** Endpoint failure/dispose can leave entries stale; Phase 5 Approval Center must discard
   * pending approvals when lifecycle becomes `failed` or `exited`. */
  approvalRequests: RuntimeApprovalRequest[];
  /** History caps で先頭から破棄した entry の累計。 */
  truncatedCount: number;
  missingSequences: RuntimeSequenceGap[];
  outOfOrderCount: number;
}

interface RuntimeProjectionState {
  byEndpoint: Record<string, RuntimeEndpointProjection>;
  projectEvent: (event: RuntimeEventEnvelope) => void;
  clearEndpoint: (endpointId: string) => void;
  clear: () => void;
}

function emptyProjection(endpointId: string): RuntimeEndpointProjection {
  return {
    endpointId,
    lastSequence: 0,
    lastKind: null,
    lifecycle: null,
    currentMessage: '',
    completedMessages: [],
    deltaChunks: [],
    errors: [],
    diagnostics: [],
    toolUses: [],
    diffs: [],
    usage: [],
    approvalRequests: [],
    truncatedCount: 0,
    missingSequences: [],
    outOfOrderCount: 0
  };
}

function appendCapped<T>(items: T[], value: T): { items: T[]; truncated: number } {
  const next = [...items, value];
  const truncated = Math.max(0, next.length - RUNTIME_PROJECTION_HISTORY_LIMIT);
  return {
    items: truncated > 0 ? next.slice(truncated) : next,
    truncated
  };
}

function appendDelta(
  projection: RuntimeEndpointProjection,
  delta: string
): Pick<
  RuntimeEndpointProjection,
  'currentMessage' | 'deltaChunks' | 'truncatedCount'
> {
  const deltaChunks = [...projection.deltaChunks];
  let truncated = 0;
  if (projection.lastKind === 'messageDelta' && deltaChunks.length > 0) {
    deltaChunks[deltaChunks.length - 1] += delta;
  } else {
    const capped = appendCapped(deltaChunks, delta);
    deltaChunks.splice(0, deltaChunks.length, ...capped.items);
    truncated = capped.truncated;
  }
  return {
    currentMessage: projection.currentMessage + delta,
    deltaChunks,
    truncatedCount: projection.truncatedCount + truncated
  };
}

function applyPayload(
  projection: RuntimeEndpointProjection,
  event: RuntimeEventEnvelope
): RuntimeEndpointProjection {
  const base = { ...projection, lastSequence: event.sequence, lastKind: event.kind };
  switch (event.payload.type) {
    case 'messageDelta':
      return { ...base, ...appendDelta(projection, event.payload.delta) };
    case 'messageComplete':
      {
        const capped = appendCapped(
          projection.completedMessages,
          event.payload.message
        );
        return {
          ...base,
          currentMessage: '',
          completedMessages: capped.items,
          truncatedCount: projection.truncatedCount + capped.truncated
        };
      }
    case 'toolUse':
      {
        const capped = appendCapped(projection.toolUses, {
          toolName: event.payload.toolName,
          callId: event.payload.callId,
          status: event.payload.status,
          detail: event.payload.detail
        });
        return {
          ...base,
          toolUses: capped.items,
          truncatedCount: projection.truncatedCount + capped.truncated
        };
      }
    case 'diff':
      {
        const capped = appendCapped(projection.diffs, event.payload.diff);
        return {
          ...base,
          diffs: capped.items,
          truncatedCount: projection.truncatedCount + capped.truncated
        };
      }
    case 'usage':
      {
        const capped = appendCapped(projection.usage, {
          inputTokens: event.payload.inputTokens,
          cachedInputTokens: event.payload.cachedInputTokens,
          outputTokens: event.payload.outputTokens
        });
        return {
          ...base,
          usage: capped.items,
          truncatedCount: projection.truncatedCount + capped.truncated
        };
      }
    case 'approvalRequest':
      {
        const capped = appendCapped(projection.approvalRequests, {
          requestId: event.payload.requestId,
          method: event.payload.method,
          reason: event.payload.reason,
          command: event.payload.command,
          cwd: event.payload.cwd
        });
        return {
          ...base,
          approvalRequests: capped.items,
          truncatedCount: projection.truncatedCount + capped.truncated
        };
      }
    case 'lifecycle':
      return { ...base, lifecycle: event.payload.state };
    case 'error':
      {
        const capped = appendCapped(projection.errors, {
          code: event.payload.code,
          message: event.payload.message,
          recoverable: event.payload.recoverable
        });
        return {
          ...base,
          errors: capped.items,
          truncatedCount: projection.truncatedCount + capped.truncated
        };
      }
    case 'diagnostic':
      {
        const capped = appendCapped(projection.diagnostics, event.payload.message);
        return {
          ...base,
          diagnostics: capped.items,
          truncatedCount: projection.truncatedCount + capped.truncated
        };
      }
  }
}

function isSpawning(event: RuntimeEventEnvelope): boolean {
  return event.payload.type === 'lifecycle' && event.payload.state === 'spawning';
}

function project(
  projection: RuntimeEndpointProjection,
  event: RuntimeEventEnvelope
): RuntimeEndpointProjection {
  // Rust 側の sequence counter は「登録 epoch」単位 (detach/dispose で削除、再登録で 1 から)。
  // lifecycle `spawning` は新 epoch の開始なので、巻き戻った sequence を out-of-order として
  // 捨てずに projection を作り直す。
  if (isSpawning(event) && event.sequence <= projection.lastSequence) {
    return applyPayload(emptyProjection(projection.endpointId), event);
  }
  if (event.sequence <= projection.lastSequence) {
    return { ...projection, outOfOrderCount: projection.outOfOrderCount + 1 };
  }

  const expected = projection.lastSequence + 1;
  const missingSequences =
    event.sequence > expected
      ? [...projection.missingSequences, { from: expected, to: event.sequence - 1 }]
      : projection.missingSequences;
  return applyPayload({ ...projection, missingSequences }, event);
}

export const useRuntimeStore = create<RuntimeProjectionState>((set) => ({
  byEndpoint: {},
  projectEvent: (event) =>
    set((state) => {
      const previous = state.byEndpoint[event.endpointId] ?? emptyProjection(event.endpointId);
      const next = project(previous, event);
      return { byEndpoint: { ...state.byEndpoint, [event.endpointId]: next } };
    }),
  clearEndpoint: (endpointId) =>
    set((state) => {
      if (!(endpointId in state.byEndpoint)) return state;
      const byEndpoint = { ...state.byEndpoint };
      delete byEndpoint[endpointId];
      return { byEndpoint };
    }),
  clear: () => set({ byEndpoint: {} })
}));
