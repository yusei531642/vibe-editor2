import { useEffect, useMemo } from 'react';
import { Bot, CircleAlert, LoaderCircle, UserRound } from 'lucide-react';
import { Canvas, type CanvasActions } from '../canvas/Canvas';
import { useProject } from '../../lib/app-state-context';
import { useT } from '../../lib/i18n';
import {
  agentPayloadOf,
  useCanvasStore
} from '../../stores/canvas';
import type { Team } from '../../../../types/shared';
import type { RecruitProjection } from '../../lib/recruit-lifecycle-projection';
import { TeamCommandBar } from './TeamCommandBar';
import { ApprovalCenter } from './ApprovalCenter';
import { TeamInspector } from './TeamInspector';
import { TeamActivityFeed } from './TeamActivityFeed';
import { useTeamProjection } from './TeamProjectionProvider';
import { useSemanticEdgeStore } from '../../stores/semantic-edges';
import '../../styles/components/canvas.css';
import '../../styles/components/canvas-agent-card.css';
import '../../styles/components/team-control.css';

function RecruitLifecycleLayer({ teamId }: { teamId?: string }): JSX.Element {
  const { projection } = useTeamProjection();
  const recruits = projection.agents
    .map((agent) => agent.recruit)
    .filter(
      (recruit): recruit is RecruitProjection =>
        recruit !== null && (teamId === undefined || recruit.teamId === teamId)
    );
  const t = useT();

  return (
    <aside className="workspace-recruits" aria-live="polite" aria-label={t('v2.team.recruits')}>
      {recruits.map((recruit) => {
        const terminal = recruit.state === 'failed' || recruit.state === 'cancelled';
        const Icon = terminal ? CircleAlert : recruit.state === 'ready' ? Bot : LoaderCircle;
        return (
          <article
            key={recruit.agentId}
            className="workspace-recruit-card glass-surface"
            data-lifecycle-state={recruit.state}
            data-exiting={recruit.exiting || undefined}
          >
            <Icon size={20} strokeWidth={1.75} aria-hidden="true" />
            <div>
              <strong>{recruit.roleProfileId}</strong>
              <span>{t(`v2.recruit.${recruit.state}`)}</span>
              {recruit.reason ? <small>{recruit.reason}</small> : null}
            </div>
          </article>
        );
      })}
    </aside>
  );
}

function randomAgentId(prefix: string): string {
  return `${prefix}-${crypto.randomUUID()}`;
}

export function TeamWorkspaceScene({ team }: { team: Team }): JSX.Element {
  const { projectRoot } = useProject();
  const t = useT();
  const addCard = useCanvasStore((state) => state.addCard);
  const pulseEdge = useCanvasStore((state) => state.pulseEdge);
  const markSemanticEdgeSeen = useSemanticEdgeStore((state) => state.markSeen);
  const { projection } = useTeamProjection();
  const hasLeader = useCanvasStore((state) =>
    state.nodes.some((node) => {
      const payload = agentPayloadOf(node.data);
      return (
        payload?.teamId === team.id &&
        (payload.roleProfileId === 'leader' || payload.role === 'leader')
      );
    })
  );

  const actions = useMemo<CanvasActions>(
    () => ({
      addClaude: () =>
        addCard({
          type: 'agent',
          title: 'Claude',
          payload: { agent: 'claude', teamId: team.id, teamName: team.name }
        }),
      addCodex: () =>
        addCard({
          type: 'agent',
          title: 'Codex',
          payload: { agent: 'codex', teamId: team.id, teamName: team.name }
        }),
      addApiAgent: () =>
        addCard({
          type: 'apiAgent',
          title: 'API Agent',
          payload: {
            agentId: randomAgentId('api'),
            teamId: team.id,
            teamName: team.name,
            configured: false
          }
        }),
      addCustomAgent: (agentId) =>
        addCard({
          type: 'agent',
          title: agentId,
          payload: {
            agentConfigId: agentId,
            agentId: randomAgentId(agentId),
            teamId: team.id,
            teamName: team.name
          }
        }),
      addFileTree: () =>
        addCard({ type: 'fileTree', title: t('v2.team.fileTree'), payload: { projectRoot } }),
      addChanges: () =>
        addCard({ type: 'changes', title: t('v2.team.changes'), payload: { projectRoot } }),
      addEditor: () =>
        addCard({
          type: 'editor',
          title: t('canvas.card.editor'),
          payload: { projectRoot, relPath: '' }
        }),
      spawnDefaultTeam: () =>
        addCard({
          type: 'agent',
          title: team.name,
          payload: {
            agent: 'claude',
            agentId: randomAgentId('leader'),
            role: 'leader',
            roleProfileId: 'leader',
            teamId: team.id,
            teamName: team.name
          },
          position: { x: 120, y: 140 }
        })
    }),
    [addCard, projectRoot, t, team.id, team.name]
  );

  useEffect(() => {
    const nodes = useCanvasStore.getState().nodes;
    const teamNodes = nodes.filter((node) => agentPayloadOf(node.data)?.teamId === team.id);
    const leader = teamNodes.find((node) => {
      const payload = agentPayloadOf(node.data);
      return (payload?.roleProfileId ?? payload?.role) === 'leader';
    });
    if (!leader) return;
    for (const task of projection.tasks) {
      const edgeId = `delegation:${team.id}:${task.id}`;
      const target = teamNodes.find((node) => {
        const payload = agentPayloadOf(node.data);
        return payload?.agentId === task.assignedTo || payload?.roleProfileId === task.assignedTo;
      });
      if (!target || target.id === leader.id) continue;
      if (!markSemanticEdgeSeen(edgeId)) continue;
      pulseEdge({
        id: edgeId,
        source: leader.id,
        target: target.id,
        type: 'handoff',
        data: {
          semantic: 'delegation',
          preview: `Task #${task.id}`,
          fromRole: 'leader',
          color: 'var(--accent)'
        }
      }, 60_000);
    }
    for (const report of projection.reports) {
      const edgeId = `report:${team.id}:${report.id}`;
      const source = teamNodes.find(
        (node) => agentPayloadOf(node.data)?.agentId === report.fromAgentId
      );
      if (!source || source.id === leader.id) continue;
      if (!markSemanticEdgeSeen(edgeId)) continue;
      pulseEdge({
        id: edgeId,
        source: source.id,
        target: leader.id,
        type: 'handoff',
        data: {
          semantic: 'report',
          preview: report.summary,
          fromRole: 'report',
          color: 'var(--success)'
        }
      }, 60_000);
    }
  }, [markSemanticEdgeSeen, projection.reports, projection.tasks, pulseEdge, team.id]);

  return (
    <section className="workspace-team-scene" aria-label={t('v2.team.canvas')}>
      <header className="workspace-team-scene__header">
        <div>
          <UserRound size={20} strokeWidth={1.75} aria-hidden="true" />
          <span>{team.name}</span>
        </div>
        <span>{t('v2.team.canvas')}</span>
      </header>
      <TeamCommandBar />
      <div className="workspace-team-scene__canvas">
        <Canvas actions={actions} />
      </div>
      {!hasLeader ? (
        <div
          className="workspace-team-leader glass-surface"
          data-workspace-leader=""
          data-workspace-team-id={team.id}
          aria-label={t('v2.team.leader')}
        >
          <UserRound size={20} strokeWidth={1.75} aria-hidden="true" />
          <div>
            <strong>{team.name}</strong>
            <span>{t('v2.team.leader')}</span>
          </div>
        </div>
      ) : null}
      <RecruitLifecycleLayer teamId={team.id} />
      <TeamActivityFeed />
      <TeamInspector />
      <ApprovalCenter />
    </section>
  );
}

export { RecruitLifecycleLayer };
