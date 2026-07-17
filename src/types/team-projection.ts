import type { RuntimeEventEnvelope } from './agent-runtime';

/** Issue #26: TeamHub が認可済み team member に保持する runtime binding の snapshot。 */
export interface TeamRuntimeEndpointSnapshot {
  teamId: string;
  agentId: string;
  endpointId: string;
  backend: 'native' | 'pty';
  sessionId: string | null;
  taskIds: number[];
  live: boolean;
}

/** Team Card / Inspector の初期同期用 snapshot。 */
export interface TeamProjectionSnapshot {
  teamId: string;
  endpoints: TeamRuntimeEndpointSnapshot[];
  runtimeEvents: RuntimeEventEnvelope[];
  runtimeDroppedCount: number;
}

export type TeamMemberCommand =
  | { action: 'send'; agentId?: string | null; message: string }
  | { action: 'interrupt'; agentId: string }
  | { action: 'stop'; agentId: string }
  | { action: 'dismiss'; agentId: string };

export interface TeamMemberCommandRequest {
  teamId: string;
  command: TeamMemberCommand;
}

export interface TeamMemberCommandResult {
  action: TeamMemberCommand['action'];
  affectedAgentIds: string[];
}
