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

function project(
  projection: RuntimeEndpointProjection,
  event: RuntimeEventEnvelope
): RuntimeEndpointProjection {
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
