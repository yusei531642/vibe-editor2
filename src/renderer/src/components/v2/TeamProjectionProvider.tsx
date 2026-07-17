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
  TeamProjectionSnapshot,
  TeamRuntimeEventCursor,
  WorktreeCommand,
  WorktreeManagerSnapshot
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
  respondAndResolveTeamApproval,
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
    agentId: string,
    endpointId: string,
    requestId: string,
    decision: RuntimeApprovalDecision
  ) => Promise<void>;
  worktreeSnapshot: WorktreeManagerSnapshot | null;
  runWorktreeCommand: (command: WorktreeCommand) => Promise<boolean>;
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

const MISSING_PROVIDER_ERROR = 'TeamProjectionProvider is missing';

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

function cursorKey(cursor: TeamRuntimeEventCursor): string {
  return `${cursor.endpointId}\u0000${cursor.sequence}\u0000${cursor.timestamp}`;
}

function pruneReplayedEvents(
  snapshot: TeamProjectionSnapshot,
  replayedEvents: Set<string>
): void {
  const retained = new Set(snapshot.retainedEventCursors.map(cursorKey));
  for (const key of replayedEvents) {
    if (!retained.has(key)) replayedEvents.delete(key);
  }
}

function latestCursors(snapshot: TeamProjectionSnapshot): TeamRuntimeEventCursor[] {
  const cursors = new Map<string, TeamRuntimeEventCursor>();
  for (const cursor of snapshot.retainedEventCursors) cursors.set(cursor.endpointId, cursor);
  return [...cursors.values()];
}

function snapshotsEqual(
  previous: TeamProjectionSnapshot | null,
  next: TeamProjectionSnapshot
): boolean {
  if (!previous) return false;
  return (
    previous.teamId === next.teamId &&
    previous.runtimeDroppedCount === next.runtimeDroppedCount &&
    JSON.stringify(previous.endpoints) === JSON.stringify(next.endpoints)
  );
}

function valuesEqual<T>(previous: T, next: T): boolean {
  return JSON.stringify(previous) === JSON.stringify(next);
}

export function TeamProjectionProvider({
  team,
  teamSceneCommitted,
  children
}: {
  team: Team;
  teamSceneCommitted: boolean;
  children: React.ReactNode;
}): JSX.Element {
  const { projectRoot } = useProject();
  const recruits = useRecruitLifecycleProjection();
  const nodes = useCanvasStore((state) => state.nodes);
  const runtimeByEndpoint = useRuntimeStore((state) => state.byEndpoint);
  const resolveApproval = useRuntimeStore((state) => state.resolveApproval);
  const [snapshot, setSnapshot] = useState<TeamProjectionSnapshot | null>(null);
  const [orchestration, setOrchestration] = useState<TeamOrchestrationState | null>(null);
  const [worktreeSnapshot, setWorktreeSnapshot] = useState<WorktreeManagerSnapshot | null>(null);
  const [selectedAgentId, setSelectedAgentId] = useState<string | null>(null);
  const [inspectorOpen, setInspectorOpen] = useState(false);
  const [approvalsOpen, setApprovalsOpen] = useState(false);
  const [terminalAgentId, setTerminalAgentId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const replayedSnapshotEvents = useRef(new Set<string>());
  const snapshotCursors = useRef<TeamRuntimeEventCursor[]>([]);

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
        runtimeByEndpoint,
        worktreeSnapshot
      }),
    [members, orchestration, recruits, runtimeByEndpoint, snapshot, team.id, worktreeSnapshot]
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
    const refreshProjection = async (): Promise<string | null> => {
      try {
        const [nextSnapshot, nextOrchestration] = await Promise.all([
        window.api.team.projectionSnapshot({
          teamId: team.id,
          sinceSequence: snapshotCursors.current
        }),
        projectRoot
          ? window.api.teamState.read(projectRoot, team.id)
          : Promise.resolve(null)
        ]);
        projectBufferedEvents(nextSnapshot, replayedSnapshotEvents.current);
        pruneReplayedEvents(nextSnapshot, replayedSnapshotEvents.current);
        snapshotCursors.current = latestCursors(nextSnapshot);
        setSnapshot((previous) => snapshotsEqual(previous, nextSnapshot) ? previous : nextSnapshot);
        setOrchestration((previous) =>
          valuesEqual(previous, nextOrchestration) ? previous : nextOrchestration
        );
        return null;
      } catch (refreshError) {
        return messageOf(refreshError);
      }
    };
    const refreshWorktrees = async (): Promise<string | null> => {
      if (!projectRoot) {
        setWorktreeSnapshot(null);
        return null;
      }
      try {
        const nextWorktrees = await window.api.worktree.snapshot({
          projectRoot,
          teamId: team.id
        });
        setWorktreeSnapshot((previous) =>
          valuesEqual(previous, nextWorktrees) ? previous : nextWorktrees
        );
        return null;
      } catch (refreshError) {
        return messageOf(refreshError);
      }
    };
    const [projectionError, worktreeError] = await Promise.all([
      refreshProjection(),
      refreshWorktrees()
    ]);
    setError(worktreeError ?? projectionError);
  }, [projectRoot, team.id]);

  useEffect(() => {
    let timer: number | null = null;
    const stop = (): void => {
      if (timer !== null) window.clearInterval(timer);
      timer = null;
    };
    const start = (): void => {
      stop();
      if (!teamSceneCommitted || document.hidden) return;
      void refresh();
      timer = window.setInterval(() => void refresh(), 3_000);
    };
    const handleVisibilityChange = (): void => start();
    document.addEventListener('visibilitychange', handleVisibilityChange);
    start();
    return () => {
      stop();
      document.removeEventListener('visibilitychange', handleVisibilityChange);
    };
  }, [refresh, teamSceneCommitted]);

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
      agentId: string,
      endpointId: string,
      requestId: string,
      decision: RuntimeApprovalDecision
    ): Promise<void> => {
      await respondAndResolveTeamApproval(
        window.api,
        team.id,
        agentId,
        endpointId,
        requestId,
        decision,
        resolveApproval
      );
    },
    [resolveApproval, team.id]
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

  const runWorktreeCommand = useCallback(
    async (command: WorktreeCommand): Promise<boolean> => {
      if (!projectRoot) {
        setError('No active project');
        return false;
      }
      try {
        const result = await window.api.worktree.command({
          projectRoot,
          teamId: team.id,
          command
        });
        setWorktreeSnapshot(result.snapshot);
        setError(null);
        return true;
      } catch (commandError) {
        setError(messageOf(commandError));
        return false;
      }
    },
    [projectRoot, team.id]
  );

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
      respondApproval,
      worktreeSnapshot,
      runWorktreeCommand
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
      runWorktreeCommand,
      selectedAgent,
      selectedAgentId,
      terminalAgentId,
      worktreeSnapshot
    ]
  );

  return <TeamProjectionContext.Provider value={value}>{children}</TeamProjectionContext.Provider>;
}

const NO_PROVIDER_CONTEXT: TeamProjectionContextValue = {
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
  dispatchAgentAction: () => Promise.reject(new Error(MISSING_PROVIDER_ERROR)),
  broadcast: () => Promise.reject(new Error(MISSING_PROVIDER_ERROR)),
  respondApproval: () => Promise.reject(new Error(MISSING_PROVIDER_ERROR)),
  worktreeSnapshot: null,
  runWorktreeCommand: () => Promise.resolve(false)
};

export function useTeamProjection(): TeamProjectionContextValue {
  const context = useContext(TeamProjectionContext);
  return context ?? NO_PROVIDER_CONTEXT;
}
