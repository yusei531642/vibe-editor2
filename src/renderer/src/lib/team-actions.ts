import type { RuntimeApprovalDecision } from '../../../types/agent-runtime';
import type { Api } from './tauri-api';
import type { TeamAgentProjection } from './team-projection';

export type TeamAgentAction = 'steer' | 'interrupt' | 'stop' | 'dismiss';

export async function dispatchTeamAgentAction(
  api: Api,
  teamId: string,
  agent: TeamAgentProjection | null,
  agentId: string,
  action: TeamAgentAction,
  input = ''
): Promise<void> {
  if (!agent?.endpoint && action !== 'dismiss') {
    throw new Error('runtime endpoint is not available');
  }
  if (action === 'dismiss') {
    await api.team.memberCommand({ teamId, command: { action: 'dismiss', agentId } });
    return;
  }
  if (action === 'steer') {
    const message = input.trim();
    if (!message) throw new Error('instruction is empty');
    if (agent?.endpoint?.backend === 'native') {
      await api.agentRuntime.steer({ endpointId: agent.endpoint.endpointId, input: message });
    } else {
      await api.team.memberCommand({
        teamId,
        command: { action: 'send', agentId, message }
      });
    }
    return;
  }
  if (agent?.endpoint?.backend === 'native') {
    if (action === 'interrupt') await api.agentRuntime.interrupt(agent.endpoint.endpointId);
    else await api.agentRuntime.stop(agent.endpoint.endpointId);
  } else {
    await api.team.memberCommand({ teamId, command: { action, agentId } });
  }
}

export async function respondAndResolveApproval(
  api: Api,
  endpointId: string,
  requestId: string,
  decision: RuntimeApprovalDecision,
  resolve: (endpointId: string, requestId: string) => void
): Promise<void> {
  await api.agentRuntime.respondApproval({ endpointId, requestId, decision });
  resolve(endpointId, requestId);
}
