import { beforeEach, describe, expect, it } from 'vitest';
import type { RuntimeEventEnvelope, RuntimeEventPayload } from '../../../../types/agent-runtime';
import type { TeamOrchestrationState, TeamProjectionSnapshot } from '../../../../types/shared';
import { useRuntimeStore } from '../../stores/runtime';
import { buildTeamProjection, changedFilesFromDiff } from '../team-projection';

function event(sequence: number, payload: RuntimeEventPayload): RuntimeEventEnvelope {
  return {
    endpointId: 'native-worker-1',
    epoch: 1,
    sequence,
    kind: payload.type,
    payload,
    timestamp: `2026-07-17T00:00:0${sequence}Z`
  };
}

const snapshot: TeamProjectionSnapshot = {
  teamId: 'team-1',
  endpoints: [{
    teamId: 'team-1',
    agentId: 'worker-1',
    endpointId: 'native-worker-1',
    backend: 'native',
    sessionId: 'thread-1',
    taskIds: [1],
    live: true,
    provider: 'codex-native',
    restoreState: 'live'
  }],
  runtimeEvents: [],
  retainedEventCursors: [],
  runtimeDroppedCount: 3
};

const orchestration: TeamOrchestrationState = {
  schemaVersion: 1,
  projectRoot: '/repo',
  teamId: 'team-1',
  tasks: [{
    id: 1,
    assignedTo: 'worker-1',
    description: 'Implement projection',
    status: 'in_progress',
    createdBy: 'leader',
    createdAt: '2026-07-17T00:00:00Z',
    targetPaths: ['src/base.ts']
  }],
  pendingTasks: [],
  workerReports: [],
  humanGate: {},
  nextActions: [],
  handoffEvents: [],
  updatedAt: '2026-07-17T00:00:00Z'
};

describe('team projection integration', () => {
  beforeEach(() => useRuntimeStore.getState().clear());

  it('unifies task, runtime, diff-derived files, usage and approvals per agent', () => {
    const store = useRuntimeStore.getState();
    store.projectEvent(event(1, { type: 'lifecycle', state: 'ready', detail: null }));
    store.projectEvent(event(2, {
      type: 'toolUse', toolName: 'apply_patch', callId: 'c1', status: 'completed', detail: null
    }));
    store.projectEvent(event(3, {
      type: 'diff', diff: 'diff --git a/src/old.ts b/src/new.ts\n+++ b/src/new.ts'
    }));
    store.projectEvent(event(4, {
      type: 'usage', inputTokens: 10, cachedInputTokens: 2, outputTokens: 8
    }));
    store.projectEvent(event(5, { type: 'messageComplete', message: 'Restored response' }));
    store.projectEvent(event(6, {
      type: 'approvalRequest', requestId: 'approve-1', method: 'command/requestApproval',
      reason: 'run tests', command: 'npm test', cwd: '/repo'
    }));

    const projection = buildTeamProjection({
      teamId: 'team-1',
      members: [{
        cardId: 'card-1', agentId: 'worker-1', title: 'Programmer', roleProfileId: 'programmer'
      }],
      snapshot,
      orchestration,
      recruits: [],
      runtimeByEndpoint: useRuntimeStore.getState().byEndpoint,
      worktreeSnapshot: null
    });

    expect(projection.agents[0]).toMatchObject({
      status: 'running',
      task: { id: 1 },
      latestTool: { toolName: 'apply_patch' },
      latestUsage: { outputTokens: 8 },
      changedFiles: ['src/base.ts', 'src/new.ts']
    });
    expect(projection.approvals[0]).toMatchObject({ requestId: 'approve-1', agentId: 'worker-1' });
    expect(projection.activity).toContainEqual(expect.objectContaining({
      kind: 'message', message: 'Restored response'
    }));
    expect(projection.runtimeDroppedCount).toBe(3);
  });

  it('extracts the destination path from unified diffs', () => {
    expect(changedFilesFromDiff('diff --git a/a.ts b/b.ts\n--- a/a.ts\n+++ b/b.ts')).toEqual(['b.ts']);
  });
});
