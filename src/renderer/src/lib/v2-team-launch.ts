import type { RuntimeEngine, RuntimePermission } from '../../../types/agent-runtime';
import type { AgentCardPayload } from '../stores/canvas';

interface V2TeamLaunchRequest {
  projectRoot: string;
  teamName: string;
  initialMessage: string;
  engine: RuntimeEngine;
  model: string;
  effort: string;
  permission: RuntimePermission;
  setupTeamMcp: (
    cwd: string,
    teamId: string,
    teamName: string,
    members: Array<{ agentId: string; role: string; agent: RuntimeEngine }>
  ) => Promise<unknown>;
  addCard: (card: {
    type: 'agent';
    title: string;
    position: { x: number; y: number };
    payload: AgentCardPayload;
  }) => string;
  selectTeam: (teamId: string) => void;
  requestTeamScene: () => void;
}

export async function launchV2Team(request: V2TeamLaunchRequest): Promise<string> {
  const teamId = `team-${crypto.randomUUID()}`;
  const agentId = `leader-0-${teamId}`;
  const setup = await request.setupTeamMcp(request.projectRoot, teamId, request.teamName, [
    { agentId, role: 'leader', agent: request.engine }
  ]);
  if (setup && typeof setup === 'object' && 'ok' in setup && setup.ok === false) {
    const message = 'error' in setup && typeof setup.error === 'string'
      ? setup.error
      : 'TeamHub setup failed';
    throw new Error(message);
  }
  request.addCard({
    type: 'agent',
    title: request.teamName,
    position: { x: 120, y: 140 },
    payload: {
      agent: request.engine,
      runtimeProvider: request.engine === 'claude' ? 'claude-native' : 'codex-native',
      runtimeModel: request.model || undefined,
      runtimeEffort: request.effort || undefined,
      runtimePermission: request.permission,
      role: 'leader',
      roleProfileId: 'leader',
      teamId,
      teamName: request.teamName,
      agentId,
      cwd: request.projectRoot,
      initialMessage: request.initialMessage
    }
  });
  request.selectTeam(teamId);
  request.requestTeamScene();
  return teamId;
}
