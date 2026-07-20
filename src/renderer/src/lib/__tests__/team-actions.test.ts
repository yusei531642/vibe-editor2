import { describe, expect, it, vi } from 'vitest';
import type { Api } from '../tauri-api';
import type { TeamAgentProjection } from '../team-projection';
import { dispatchTeamAgentAction, respondAndResolveTeamApproval } from '../team-actions';

function agent(backend: 'native' | 'pty'): TeamAgentProjection {
  return {
    agentId: 'worker-1', cardId: 'card-1', title: 'Worker', roleProfileId: 'programmer',
    status: 'running', recruit: null, task: null,
    endpoint: {
      teamId: 'team-1', agentId: 'worker-1', endpointId: `${backend}-1`, backend,
      sessionId: null, taskIds: [], live: true, provider: backend, restoreState: 'live'
    },
    runtime: null, changedFiles: [], latestTool: null, latestDiff: null, latestUsage: null,
    approvals: [], worktree: null
  };
}

function apiMock(): Api {
  return {
    agentRuntime: {
      steer: vi.fn().mockResolvedValue({ endpointId: 'native-1' }),
      interrupt: vi.fn().mockResolvedValue({ endpointId: 'native-1' }),
      stop: vi.fn().mockResolvedValue({ endpointId: 'native-1' }),
      respondApproval: vi.fn().mockResolvedValue({ endpointId: 'native-1' })
    },
    team: { memberCommand: vi.fn().mockResolvedValue({ action: 'send', affectedAgentIds: [] }) }
  } as unknown as Api;
}

describe('team card action dispatch', () => {
  it('routes native steer, interrupt and stop through agent_runtime commands', async () => {
    const api = apiMock();
    const native = agent('native');
    await dispatchTeamAgentAction(api, 'team-1', native, native.agentId, 'steer', ' continue ');
    await dispatchTeamAgentAction(api, 'team-1', native, native.agentId, 'interrupt');
    await dispatchTeamAgentAction(api, 'team-1', native, native.agentId, 'stop');
    expect(api.agentRuntime.steer).toHaveBeenCalledWith({ endpointId: 'native-1', input: 'continue' });
    expect(api.agentRuntime.interrupt).toHaveBeenCalledWith('native-1');
    expect(api.agentRuntime.stop).toHaveBeenCalledWith('native-1');
  });

  it('routes PTY steer/control and all dismissals through authorized TeamHub commands', async () => {
    const api = apiMock();
    const pty = agent('pty');
    await dispatchTeamAgentAction(api, 'team-1', pty, pty.agentId, 'steer', 'continue');
    await dispatchTeamAgentAction(api, 'team-1', pty, pty.agentId, 'interrupt');
    await dispatchTeamAgentAction(api, 'team-1', pty, pty.agentId, 'stop');
    await dispatchTeamAgentAction(api, 'team-1', pty, pty.agentId, 'dismiss');
    expect(api.team.memberCommand).toHaveBeenNthCalledWith(1, {
      teamId: 'team-1', command: { action: 'send', agentId: 'worker-1', message: 'continue' }
    });
    expect(api.team.memberCommand).toHaveBeenNthCalledWith(2, {
      teamId: 'team-1', command: { action: 'interrupt', agentId: 'worker-1' }
    });
    expect(api.team.memberCommand).toHaveBeenNthCalledWith(3, {
      teamId: 'team-1', command: { action: 'stop', agentId: 'worker-1' }
    });
    expect(api.team.memberCommand).toHaveBeenNthCalledWith(4, {
      teamId: 'team-1', command: { action: 'dismiss', agentId: 'worker-1' }
    });
  });

  it('removes approval after the runtime response succeeds', async () => {
    const api = apiMock();
    const resolve = vi.fn();
    await respondAndResolveTeamApproval(
      api,
      'team-1',
      'worker-1',
      'native-1',
      'approval-1',
      'acceptForSession',
      resolve
    );
    expect(api.team.memberCommand).toHaveBeenCalledWith({
      teamId: 'team-1',
      command: {
        action: 'respondApproval',
        agentId: 'worker-1',
        requestId: 'approval-1',
        decision: 'acceptForSession'
      }
    });
    expect(api.agentRuntime.respondApproval).not.toHaveBeenCalled();
    expect(resolve).toHaveBeenCalledWith('native-1', 'approval-1');
  });

  it('removes a stale approval while preserving the runtime response error', async () => {
    const api = apiMock();
    const resolve = vi.fn();
    vi.mocked(api.team.memberCommand).mockRejectedValueOnce(new Error('transport failed'));
    await expect(
      respondAndResolveTeamApproval(
        api,
        'team-1',
        'worker-1',
        'native-1',
        'approval-1',
        'decline',
        resolve
      )
    ).rejects.toThrow('transport failed');
    expect(resolve).toHaveBeenCalledWith('native-1', 'approval-1');
  });
});
