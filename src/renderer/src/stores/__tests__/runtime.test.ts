import { beforeEach, describe, expect, it } from 'vitest';
import type {
  RuntimeEventEnvelope,
  RuntimeEventPayload
} from '../../../../types/agent-runtime';
import {
  RESOLVED_APPROVAL_HISTORY_LIMIT,
  RUNTIME_PROJECTION_HISTORY_LIMIT,
  useRuntimeStore
} from '../runtime';

function event(
  sequence: number,
  payload: RuntimeEventPayload,
  endpointId = 'endpoint-1',
  epoch = 1
): RuntimeEventEnvelope {
  const kind = payload.type;
  return {
    endpointId,
    epoch,
    sequence,
    kind,
    payload,
    timestamp: `2026-07-16T00:00:0${sequence}Z`
  };
}

describe('runtime projection store', () => {
  beforeEach(() => useRuntimeStore.getState().clear());

  it('records missing sequences and ignores out-of-order payloads', () => {
    const store = useRuntimeStore.getState();
    store.projectEvent(
      event(1, { type: 'lifecycle', state: 'spawning', detail: null })
    );
    store.projectEvent(event(3, { type: 'lifecycle', state: 'ready', detail: null }));
    store.projectEvent(
      event(2, { type: 'lifecycle', state: 'failed', detail: 'late event' })
    );

    const projection = useRuntimeStore.getState().byEndpoint['endpoint-1'];
    expect(projection.lastSequence).toBe(3);
    expect(projection.lifecycle).toBe('ready');
    expect(projection.missingSequences).toEqual([{ from: 2, to: 2 }]);
    expect(projection.outOfOrderCount).toBe(1);
  });

  it('coalesces consecutive message deltas and starts a new chunk after another kind', () => {
    const store = useRuntimeStore.getState();
    store.projectEvent(event(1, { type: 'messageDelta', delta: 'hel' }));
    store.projectEvent(event(2, { type: 'messageDelta', delta: 'lo' }));
    store.projectEvent(event(3, { type: 'diagnostic', message: 'boundary' }));
    store.projectEvent(event(4, { type: 'messageDelta', delta: ' world' }));

    const projection = useRuntimeStore.getState().byEndpoint['endpoint-1'];
    expect(projection.currentMessage).toBe('hello world');
    expect(projection.deltaChunks).toEqual(['hello', ' world']);
    expect(projection.diagnostics).toEqual(['boundary']);
  });

  it('caps projection histories and records entries truncated from the front', () => {
    const store = useRuntimeStore.getState();
    let sequence = 1;
    for (let index = 0; index <= RUNTIME_PROJECTION_HISTORY_LIMIT; index++) {
      store.projectEvent(
        event(sequence++, { type: 'messageDelta', delta: `delta-${index}` })
      );
      store.projectEvent(
        event(sequence++, { type: 'lifecycle', state: 'ready', detail: null })
      );
      store.projectEvent(
        event(sequence++, { type: 'messageComplete', message: `complete-${index}` })
      );
      store.projectEvent(
        event(sequence++, {
          type: 'error',
          code: `error-${index}`,
          message: `error message ${index}`,
          recoverable: true
        })
      );
      store.projectEvent(
        event(sequence++, { type: 'diagnostic', message: `diagnostic-${index}` })
      );
    }

    const projection = useRuntimeStore.getState().byEndpoint['endpoint-1'];
    expect(projection.deltaChunks).toHaveLength(RUNTIME_PROJECTION_HISTORY_LIMIT);
    expect(projection.completedMessages).toHaveLength(
      RUNTIME_PROJECTION_HISTORY_LIMIT
    );
    expect(projection.errors).toHaveLength(RUNTIME_PROJECTION_HISTORY_LIMIT);
    expect(projection.diagnostics).toHaveLength(RUNTIME_PROJECTION_HISTORY_LIMIT);
    expect(projection.deltaChunks[0]).toBe('delta-1');
    expect(projection.completedMessages[0]).toBe('complete-1');
    expect(projection.errors[0].code).toBe('error-1');
    expect(projection.diagnostics[0]).toBe('diagnostic-1');
    expect(projection.truncatedCount).toBe(4);
  });

  it('projects native tool diff usage and approval events', () => {
    const store = useRuntimeStore.getState();
    store.projectEvent(event(1, {
      type: 'toolUse',
      toolName: 'commandExecution',
      callId: 'call-1',
      status: 'started',
      detail: '{"command":"git status"}'
    }));
    store.projectEvent(event(2, { type: 'diff', diff: '@@ -1 +1 @@' }));
    store.projectEvent(event(3, {
      type: 'usage',
      inputTokens: 10,
      cachedInputTokens: 4,
      outputTokens: 6
    }));
    store.projectEvent(event(4, {
      type: 'approvalRequest',
      requestId: '7',
      method: 'item/commandExecution/requestApproval',
      reason: 'needs permission',
      command: 'git status',
      cwd: '/tmp/project'
    }));

    const projection = useRuntimeStore.getState().byEndpoint['endpoint-1'];
    expect(projection.toolUses[0].callId).toBe('call-1');
    expect(projection.diffs).toEqual(['@@ -1 +1 @@']);
    expect(projection.usage[0].cachedInputTokens).toBe(4);
    expect(projection.approvalRequests[0].requestId).toBe('7');
  });

  it('resets the projection when a spawning lifecycle event starts a new epoch', () => {
    const store = useRuntimeStore.getState();
    store.projectEvent(event(1, { type: 'lifecycle', state: 'spawning', detail: null }));
    store.projectEvent(event(2, { type: 'messageDelta', delta: 'old-epoch' }));
    store.projectEvent(event(3, { type: 'messageComplete', message: 'old-message' }));

    // detach 後の再登録: Rust 側 counter は 1 から振り直される。
    store.projectEvent(event(1, { type: 'lifecycle', state: 'spawning', detail: null }, 'endpoint-1', 2));

    const projection = useRuntimeStore.getState().byEndpoint['endpoint-1'];
    expect(projection.lastSequence).toBe(1);
    expect(projection.lifecycle).toBe('spawning');
    expect(projection.completedMessages).toHaveLength(0);
    expect(projection.deltaChunks).toHaveLength(0);
    expect(projection.outOfOrderCount).toBe(0);
  });

  it('discards approvals when an endpoint fails or exits', () => {
    const store = useRuntimeStore.getState();
    store.projectEvent(event(1, {
      type: 'approvalRequest',
      requestId: 'stale-1',
      method: 'command/requestApproval',
      reason: null,
      command: 'npm test',
      cwd: null
    }));
    store.projectEvent(event(2, { type: 'lifecycle', state: 'failed', detail: 'crashed' }));
    expect(useRuntimeStore.getState().byEndpoint['endpoint-1'].approvalRequests).toEqual([]);

    store.projectEvent(event(3, {
      type: 'approvalRequest',
      requestId: 'stale-2',
      method: 'command/requestApproval',
      reason: null,
      command: 'npm test',
      cwd: null
    }));
    store.projectEvent(event(4, { type: 'lifecycle', state: 'exited', detail: null }));
    expect(useRuntimeStore.getState().byEndpoint['endpoint-1'].approvalRequests).toEqual([]);
  });

  it('removes a responded approval without disturbing other pending requests', () => {
    const store = useRuntimeStore.getState();
    for (const [sequence, requestId] of [[1, 'one'], [2, 'two']] as const) {
      store.projectEvent(event(sequence, {
        type: 'approvalRequest',
        requestId,
        method: 'command/requestApproval',
        reason: null,
        command: null,
        cwd: null
      }));
    }
    store.resolveApproval('endpoint-1', 'one');
    expect(
      useRuntimeStore.getState().byEndpoint['endpoint-1'].approvalRequests.map(
        (request) => request.requestId
      )
    ).toEqual(['two']);
  });

  it('does not resurrect a resolved approval after endpoint clear and buffer replay', () => {
    const approval = event(1, {
      type: 'approvalRequest',
      requestId: 'resolved-one',
      method: 'command/requestApproval',
      reason: null,
      command: 'npm test',
      cwd: null
    });
    const store = useRuntimeStore.getState();
    store.projectEvent(approval);
    store.resolveApproval('endpoint-1', 'resolved-one');
    store.clearEndpoint('endpoint-1');
    useRuntimeStore.getState().projectEvent(approval);

    expect(
      useRuntimeStore.getState().byEndpoint['endpoint-1'].approvalRequests
    ).toEqual([]);
  });

  it('bounds the persistent resolved approval Set', () => {
    const store = useRuntimeStore.getState();
    for (let index = 0; index <= RESOLVED_APPROVAL_HISTORY_LIMIT; index += 1) {
      store.resolveApproval('endpoint-1', `request-${index}`);
    }
    const resolved = useRuntimeStore.getState().resolvedApprovalRequestIds;
    expect(resolved.size).toBe(RESOLVED_APPROVAL_HISTORY_LIMIT);
    expect(resolved.has('endpoint-1\u0000request-0')).toBe(false);
  });
});
