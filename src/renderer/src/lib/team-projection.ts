/** Issue #26: runtime / recruit / task / diff / approval を agent 単位へ統合する純粋 projection。 */
import type {
  TeamOrchestrationState,
  TeamProjectionSnapshot,
  TeamRuntimeEndpointSnapshot,
  TeamTaskSnapshot,
  WorktreeAssignmentSnapshot,
  WorktreeManagerSnapshot
} from '../../../types/shared';
import type { RecruitProjection } from './recruit-lifecycle-projection';
import type {
  RuntimeApprovalRequest,
  RuntimeEndpointProjection,
  RuntimeToolUse,
  RuntimeUsage
} from '../stores/runtime';

export type TeamAgentStatus = 'spawning' | 'ready' | 'running' | 'failed' | 'terminated';

export interface TeamProjectionMember {
  cardId: string;
  agentId: string;
  title: string;
  roleProfileId: string;
}

export interface TeamApprovalProjection extends RuntimeApprovalRequest {
  endpointId: string;
  agentId: string;
  agentTitle: string;
  requestedAt: string;
}

export interface TeamActivityItem {
  id: string;
  agentId: string | null;
  kind: 'recruit' | 'task' | 'report' | 'approval' | 'message' | 'error';
  message: string;
  timestamp: string;
}

export interface TeamAgentProjection {
  agentId: string;
  cardId: string | null;
  title: string;
  roleProfileId: string;
  status: TeamAgentStatus;
  recruit: RecruitProjection | null;
  task: TeamTaskSnapshot | null;
  endpoint: TeamRuntimeEndpointSnapshot | null;
  runtime: RuntimeEndpointProjection | null;
  changedFiles: string[];
  latestTool: RuntimeToolUse | null;
  latestDiff: string | null;
  latestUsage: RuntimeUsage | null;
  approvals: TeamApprovalProjection[];
  worktree: WorktreeAssignmentSnapshot | null;
}

export interface TeamProjection {
  teamId: string;
  agents: TeamAgentProjection[];
  approvals: TeamApprovalProjection[];
  activity: TeamActivityItem[];
  tasks: TeamTaskSnapshot[];
  reports: Array<{ id: string; fromAgentId: string; summary: string; timestamp: string }>;
  runtimeDroppedCount: number;
}

export interface BuildTeamProjectionInput {
  teamId: string;
  members: TeamProjectionMember[];
  snapshot: TeamProjectionSnapshot | null;
  orchestration: TeamOrchestrationState | null;
  recruits: RecruitProjection[];
  runtimeByEndpoint: Record<string, RuntimeEndpointProjection>;
  worktreeSnapshot: WorktreeManagerSnapshot | null;
}

/** diff 文字列は immutable なので解析結果を cache する (PR #36 レビュー: 3 秒 poll ×
 * 最大 200 diffs × 行数の再計算を防ぐ)。history cap があるため世代管理は上限 clear で足りる。 */
const CHANGED_FILES_CACHE_LIMIT = 512;
const changedFilesCache = new Map<string, string[]>();

export function changedFilesFromDiff(diff: string): string[] {
  const cached = changedFilesCache.get(diff);
  if (cached) return cached;
  const parsed = parseChangedFilesFromDiff(diff);
  if (changedFilesCache.size >= CHANGED_FILES_CACHE_LIMIT) changedFilesCache.clear();
  changedFilesCache.set(diff, parsed);
  return parsed;
}

function parseChangedFilesFromDiff(diff: string): string[] {
  const paths = new Set<string>();
  for (const line of diff.split('\n')) {
    const gitMatch = /^diff --git a\/(.+?) b\/(.+)$/.exec(line);
    if (gitMatch) {
      paths.add(gitMatch[2]);
      continue;
    }
    const fileMatch = /^\+\+\+ b\/(.+)$/.exec(line);
    if (fileMatch && fileMatch[1] !== '/dev/null') paths.add(fileMatch[1]);
  }
  return [...paths];
}

function latest<T>(values: T[]): T | null {
  return values.at(-1) ?? null;
}

function chooseEndpoint(
  endpoints: TeamRuntimeEndpointSnapshot[],
  recruit: RecruitProjection | null
): TeamRuntimeEndpointSnapshot | null {
  const live = endpoints.filter((endpoint) => endpoint.live);
  const preferred = live.find((endpoint) => endpoint.backend === 'native') ?? live[0];
  if (preferred) return preferred;
  const fallback = endpoints.find((endpoint) => endpoint.backend === 'native') ?? endpoints[0];
  if (fallback) return fallback;
  if (!recruit?.endpointId) return null;
  return {
    teamId: recruit.teamId,
    agentId: recruit.agentId,
    endpointId: recruit.endpointId,
    backend: recruit.endpointId.startsWith('team-pty-') ? 'pty' : 'native',
    sessionId: recruit.sessionId,
    taskIds: recruit.taskIds,
    live: recruit.state === 'ready',
    provider: recruit.endpointId.startsWith('team-pty-') ? 'pty' : 'native',
    restoreState: 'live'
  };
}

function deriveStatus(
  recruit: RecruitProjection | null,
  runtime: RuntimeEndpointProjection | null,
  task: TeamTaskSnapshot | null,
  endpoint: TeamRuntimeEndpointSnapshot | null
): TeamAgentStatus {
  if (endpoint?.restoreState === 'terminated') return 'terminated';
  if (
    recruit?.state === 'failed' ||
    recruit?.state === 'cancelled' ||
    runtime?.lifecycle === 'failed' ||
    runtime?.lifecycle === 'exited'
  ) {
    return 'failed';
  }
  if (
    recruit &&
    (recruit.state === 'requested' ||
      recruit.state === 'spawning' ||
      recruit.state === 'handshaking')
  ) {
    return 'spawning';
  }
  if (
    task?.status === 'in_progress' ||
    Boolean(runtime?.currentMessage) ||
    runtime?.lastKind === 'toolUse' ||
    runtime?.lastKind === 'diff' ||
    runtime?.lastKind === 'approvalRequest'
  ) {
    return 'running';
  }
  return 'ready';
}

function approvalTimestamp(runtime: RuntimeEndpointProjection, requestId: string): string {
  for (let index = runtime.eventHistory.length - 1; index >= 0; index -= 1) {
    const event = runtime.eventHistory[index];
    if (event.payload.type === 'approvalRequest' && event.payload.requestId === requestId) {
      return event.timestamp;
    }
  }
  return new Date(0).toISOString();
}

export function buildTeamProjection(input: BuildTeamProjectionInput): TeamProjection {
  const tasks = input.orchestration?.tasks ?? [];
  const reports = [
    ...(input.orchestration?.workerReports ?? []).map((report) => ({
      id: report.id,
      fromAgentId: report.fromAgentId,
      summary: report.summary,
      timestamp: report.createdAt
    })),
    ...(input.orchestration?.teamReports ?? []).map((report) => ({
      id: report.id,
      fromAgentId: report.fromAgentId,
      summary: report.summary,
      timestamp: report.createdAt
    }))
  ];
  const recruits = input.recruits.filter((recruit) => recruit.teamId === input.teamId);
  const memberMap = new Map(input.members.map((member) => [member.agentId, member]));
  for (const recruit of recruits) {
    if (!memberMap.has(recruit.agentId)) {
      memberMap.set(recruit.agentId, {
        cardId: '',
        agentId: recruit.agentId,
        title: recruit.roleProfileId,
        roleProfileId: recruit.roleProfileId
      });
    }
  }
  const endpoints = input.snapshot?.endpoints ?? [];
  for (const endpoint of endpoints) {
    if (!memberMap.has(endpoint.agentId)) {
      memberMap.set(endpoint.agentId, {
        cardId: '',
        agentId: endpoint.agentId,
        title: endpoint.agentId,
        roleProfileId: 'agent'
      });
    }
  }

  const agents = [...memberMap.values()].map<TeamAgentProjection>((member) => {
    const recruit = recruits.find((item) => item.agentId === member.agentId) ?? null;
    const endpoint = chooseEndpoint(
      endpoints.filter((item) => item.agentId === member.agentId),
      recruit
    );
    const runtime = endpoint ? input.runtimeByEndpoint[endpoint.endpointId] ?? null : null;
    const assignedTasks = tasks.filter((task) => task.assignedTo === member.agentId);
    const task =
      assignedTasks.find((item) => item.status === 'in_progress') ?? latest(assignedTasks);
    const approvals = (runtime?.approvalRequests ?? []).map((approval) => ({
      ...approval,
      endpointId: endpoint?.endpointId ?? '',
      agentId: member.agentId,
      agentTitle: member.title,
      requestedAt: runtime ? approvalTimestamp(runtime, approval.requestId) : new Date(0).toISOString()
    }));
    const changedFiles = [
      ...(task?.targetPaths ?? []),
      ...(runtime?.diffs.flatMap(changedFilesFromDiff) ?? [])
    ];
    return {
      agentId: member.agentId,
      cardId: member.cardId || null,
      title: member.title,
      roleProfileId: member.roleProfileId,
      status: deriveStatus(recruit, runtime, task, endpoint),
      recruit,
      task,
      endpoint,
      runtime,
      changedFiles: [...new Set(changedFiles)],
      latestTool: latest(runtime?.toolUses ?? []),
      latestDiff: latest(runtime?.diffs ?? []),
      latestUsage: latest(runtime?.usage ?? []),
      approvals,
      worktree:
        input.worktreeSnapshot?.assignments.find(
          (assignment) => assignment.agentId === member.agentId
        ) ?? null
    };
  });

  const approvals = agents.flatMap((agent) => agent.approvals);
  const activity: TeamActivityItem[] = [];
  for (const recruit of recruits) {
    activity.push({
      id: `recruit:${recruit.agentId}:${recruit.sequence}`,
      agentId: recruit.agentId,
      kind: 'recruit',
      message: `${recruit.roleProfileId}: ${recruit.state}`,
      timestamp: recruit.observedAt
    });
  }
  for (const task of tasks) {
    activity.push({
      id: `task:${task.id}:${task.updatedAt ?? task.createdAt}`,
      agentId: task.assignedTo,
      kind: 'task',
      message: `#${task.id} ${task.status}: ${task.description}`,
      timestamp: task.updatedAt ?? task.createdAt
    });
  }
  for (const approval of approvals) {
    activity.push({
      id: `approval:${approval.endpointId}:${approval.requestId}`,
      agentId: approval.agentId,
      kind: 'approval',
      message: approval.reason ?? approval.method,
      timestamp: approval.requestedAt
    });
  }
  for (const report of reports) {
    activity.push({
      id: `report:${report.id}`,
      agentId: report.fromAgentId,
      kind: 'report',
      message: report.summary,
      timestamp: report.timestamp
    });
  }
  for (const agent of agents) {
    for (const event of agent.runtime?.eventHistory ?? []) {
      if (event.payload.type === 'messageComplete') {
        activity.push({
          id: `message:${event.endpointId}:${event.epoch}:${event.sequence}`,
          agentId: agent.agentId,
          kind: 'message',
          message: event.payload.message,
          timestamp: event.timestamp
        });
      } else if (event.payload.type === 'error') {
        activity.push({
          id: `error:${event.endpointId}:${event.epoch}:${event.sequence}`,
          agentId: agent.agentId,
          kind: 'error',
          message: event.payload.message,
          timestamp: event.timestamp
        });
      }
    }
  }
  activity.sort((left, right) => right.timestamp.localeCompare(left.timestamp));
  return {
    teamId: input.teamId,
    agents,
    approvals,
    activity,
    tasks,
    reports,
    runtimeDroppedCount: input.snapshot?.runtimeDroppedCount ?? 0
  };
}
