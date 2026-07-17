import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState
} from 'react';
import type { RuntimeApprovalDecision } from '../../../../types/agent-runtime';
import type {
  Team,
  TeamOrchestrationState,
  TeamProjectionSnapshot
} from '../../../../types/shared';
import { useProject } from '../../lib/app-state-context';
import { useRecruitLifecycleProjection } from '../../lib/recruit-lifecycle-projection';
import {
  buildTeamProjection,
  type TeamAgentProjection,
  type TeamProjection
} from '../../lib/team-projection';
import {
  dispatchTeamAgentAction,
  respondAndResolveApproval,
  type TeamAgentAction
} from '../../lib/team-actions';
import { agentPayloadOf, useCanvasStore } from '../../stores/canvas';
import { useRuntimeStore } from '../../stores/runtime';

interface TeamProjectionContextValue {
  projection: TeamProjection;
  selectedAgent: TeamAgentProjection | null;
  selectedAgentId: string | null;
  inspectorOpen: boolean;
  approvalsOpen: boolean;
  terminalAgentId: string | null;
  error: string | null;
  selectAgent: (agentId: string) => void;
  setInspectorOpen: (open: boolean) => void;
  setApprovalsOpen: (open: boolean) => void;
  openInspector: (agentId?: string) => void;
  openTerminal: (agentId: string) => void;
  dispatchAgentAction: (
    agentId: string,
    action: TeamAgentAction,
    input?: string
  ) => Promise<void>;
  broadcast: (message: string) => Promise<void>;
  respondApproval: (
    endpointId: string,
    requestId: string,
    decision: RuntimeApprovalDecision
  ) => Promise<void>;
}

const EMPTY_PROJECTION: TeamProjection = {
  teamId: '',
  agents: [],
  approvals: [],
  activity: [],
  tasks: [],
  reports: [],
  runtimeDroppedCount: 0
};

const TeamProjectionContext = createContext<TeamProjectionContextValue | null>(null);

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function projectBufferedEvents(
  snapshot: TeamProjectionSnapshot,
  replayedEvents: Set<string>
): void {
  const store = useRuntimeStore.getState();
  // Rust の buffer 順は epoch をまたいだ発生順。sequence は登録 epoch ごとに 1 へ戻るため、
  // sequence sort すると旧/new epoch が混ざる。snapshot の canonical order を維持する。
  for (const event of snapshot.runtimeEvents) {
    const current = useRuntimeStore.getState().byEndpoint[event.endpointId];
    const eventKey = `${event.endpointId}\u0000${event.sequence}\u0000${event.timestamp}`;
    const alreadyProjected = current?.eventHistory.some(
      (projected) =>
        projected.sequence === event.sequence && projected.timestamp === event.timestamp
    );
    if (replayedEvents.has(eventKey) || alreadyProjected) {
      replayedEvents.add(eventKey);
      continue;
    }
    const startsEpoch =
      event.payload.type === 'lifecycle' && event.payload.state === 'spawning';
    if (!current || event.sequence > current.lastSequence || startsEpoch) {
      store.projectEvent(event);
    }
    replayedEvents.add(eventKey);
  }
}

export function TeamProjectionProvider({
  team,
  children
}: {
  team: Team;
  children: React.ReactNode;
}): JSX.Element {
  const { projectRoot } = useProject();
  const recruits = useRecruitLifecycleProjection();
  const nodes = useCanvasStore((state) => state.nodes);
  const runtimeByEndpoint = useRuntimeStore((state) => state.byEndpoint);
  const resolveApproval = useRuntimeStore((state) => state.resolveApproval);
  const [snapshot, setSnapshot] = useState<TeamProjectionSnapshot | null>(null);
  const [orchestration, setOrchestration] = useState<TeamOrchestrationState | null>(null);
  const [selectedAgentId, setSelectedAgentId] = useState<string | null>(null);
  const [inspectorOpen, setInspectorOpen] = useState(false);
  const [approvalsOpen, setApprovalsOpen] = useState(false);
  const [terminalAgentId, setTerminalAgentId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const replayedSnapshotEvents = useRef(new Set<string>());

  const members = useMemo(
    () =>
      nodes.flatMap((node) => {
        const payload = agentPayloadOf(node.data);
        if (!payload?.agentId || payload.teamId !== team.id) return [];
        return [
          {
            cardId: node.id,
            agentId: payload.agentId,
            title: node.data.title,
            roleProfileId: payload.roleProfileId ?? payload.role ?? 'agent'
          }
        ];
      }),
    [nodes, team.id]
  );

  const projection = useMemo(
    () =>
      buildTeamProjection({
        teamId: team.id,
        members,
        snapshot,
        orchestration,
        recruits,
        runtimeByEndpoint
      }),
    [members, orchestration, recruits, runtimeByEndpoint, snapshot, team.id]
  );

  useEffect(() => {
    const canvasSelected = nodes.find((node) => node.selected && agentPayloadOf(node.data)?.teamId === team.id);
    const agentId = canvasSelected ? agentPayloadOf(canvasSelected.data)?.agentId : null;
    if (agentId) setSelectedAgentId(agentId);
  }, [nodes, team.id]);

  useEffect(() => {
    if (
      selectedAgentId &&
      projection.agents.some((agent) => agent.agentId === selectedAgentId)
    ) {
      return;
    }
    setSelectedAgentId(projection.agents[0]?.agentId ?? null);
  }, [projection.agents, selectedAgentId]);

  const refresh = useCallback(async () => {
    try {
      const [nextSnapshot, nextOrchestration] = await Promise.all([
        window.api.team.projectionSnapshot(team.id),
        projectRoot
          ? window.api.teamState.read(projectRoot, team.id)
          : Promise.resolve(null)
      ]);
      projectBufferedEvents(nextSnapshot, replayedSnapshotEvents.current);
      setSnapshot(nextSnapshot);
      setOrchestration(nextOrchestration);
      setError(null);
    } catch (refreshError) {
      setError(messageOf(refreshError));
    }
  }, [projectRoot, team.id]);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), 3_000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const endpointIds = useMemo(() => {
    const values = new Set(snapshot?.endpoints.map((endpoint) => endpoint.endpointId) ?? []);
    for (const recruit of recruits) {
      if (recruit.teamId === team.id && recruit.endpointId) values.add(recruit.endpointId);
    }
    return [...values].sort();
  }, [recruits, snapshot?.endpoints, team.id]);
  const endpointSignature = endpointIds.join('\u0000');

  useEffect(() => {
    let disposed = false;
    const unlistens: (() => void)[] = [];
    void Promise.all(
      endpointIds.map(async (endpointId) => {
        const unsubscribe = await window.api.agentRuntime.onEventReady(
          endpointId,
          useRuntimeStore.getState().projectEvent
        );
        if (disposed) unsubscribe();
        else unlistens.push(unsubscribe);
      })
    ).catch((subscribeError) => {
      if (!disposed) setError(messageOf(subscribeError));
    });
    return () => {
      disposed = true;
      unlistens.forEach((unsubscribe) => unsubscribe());
    };
    // endpointSignature is the stable identity; endpointIds is reconstructed from it.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [endpointSignature]);

  const selectedAgent =
    projection.agents.find((agent) => agent.agentId === selectedAgentId) ?? null;

  const dispatchAgentAction = useCallback(
    async (agentId: string, action: TeamAgentAction, input = ''): Promise<void> => {
      const agent = projection.agents.find((candidate) => candidate.agentId === agentId) ?? null;
      await dispatchTeamAgentAction(window.api, team.id, agent, agentId, action, input);
    },
    [projection.agents, team.id]
  );

  const broadcast = useCallback(
    async (message: string): Promise<void> => {
      const trimmed = message.trim();
      if (!trimmed) throw new Error('broadcast message is empty');
      await window.api.team.memberCommand({
        teamId: team.id,
        command: { action: 'send', agentId: null, message: trimmed }
      });
    },
    [team.id]
  );

  const respondApproval = useCallback(
    async (
      endpointId: string,
      requestId: string,
      decision: RuntimeApprovalDecision
    ): Promise<void> => {
      await respondAndResolveApproval(
        window.api,
        endpointId,
        requestId,
        decision,
        resolveApproval
      );
    },
    [resolveApproval]
  );

  const openInspector = useCallback(
    (agentId?: string) => {
      if (agentId) setSelectedAgentId(agentId);
      setInspectorOpen(true);
    },
    []
  );
  const openTerminal = useCallback((agentId: string) => {
    setSelectedAgentId(agentId);
    setInspectorOpen(true);
    setTerminalAgentId(agentId);
  }, []);

  const value = useMemo<TeamProjectionContextValue>(
    () => ({
      projection,
      selectedAgent,
      selectedAgentId,
      inspectorOpen,
      approvalsOpen,
      terminalAgentId,
      error,
      selectAgent: setSelectedAgentId,
      setInspectorOpen,
      setApprovalsOpen,
      openInspector,
      openTerminal,
      dispatchAgentAction,
      broadcast,
      respondApproval
    }),
    [
      approvalsOpen,
      broadcast,
      dispatchAgentAction,
      error,
      inspectorOpen,
      openInspector,
      openTerminal,
      projection,
      respondApproval,
      selectedAgent,
      selectedAgentId,
      terminalAgentId
    ]
  );

  return <TeamProjectionContext.Provider value={value}>{children}</TeamProjectionContext.Provider>;
}

export function useTeamProjection(): TeamProjectionContextValue {
  const context = useContext(TeamProjectionContext);
  if (!context) {
    return {
      projection: EMPTY_PROJECTION,
      selectedAgent: null,
      selectedAgentId: null,
      inspectorOpen: false,
      approvalsOpen: false,
      terminalAgentId: null,
      error: null,
      selectAgent: () => undefined,
      setInspectorOpen: () => undefined,
      setApprovalsOpen: () => undefined,
      openInspector: () => undefined,
      openTerminal: () => undefined,
      dispatchAgentAction: () => Promise.reject(new Error('TeamProjectionProvider is missing')),
      broadcast: () => Promise.reject(new Error('TeamProjectionProvider is missing')),
      respondApproval: () => Promise.reject(new Error('TeamProjectionProvider is missing'))
    };
  }
  return context;
}
