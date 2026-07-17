import { act, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type {
  RecruitCancelledPayload,
  RecruitLifecyclePayload,
  RecruitRequestPayload
} from '../../../types/shared';
import { useRecruitLifecycleProjection } from './recruit-lifecycle-projection';

const eventHarness = vi.hoisted(() => ({
  listeners: new Map<string, (event: { payload: unknown }) => void>()
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async (event: string, callback: (event: { payload: unknown }) => void) => {
    eventHarness.listeners.set(event, callback);
    return () => eventHarness.listeners.delete(event);
  })
}));

function ProjectionHarness(): JSX.Element {
  const recruits = useRecruitLifecycleProjection();
  return (
    <div>
      {recruits.map((recruit) => (
        <output
          key={recruit.agentId}
          data-testid={recruit.agentId}
          data-state={recruit.state}
          data-sequence={recruit.sequence}
          data-exiting={recruit.exiting}
        >
          {recruit.roleProfileId}
        </output>
      ))}
    </div>
  );
}

function emit<T>(event: string, payload: T): void {
  const listener = eventHarness.listeners.get(event);
  if (!listener) throw new Error(`${event} listener is not registered`);
  listener({ payload });
}

const request: RecruitRequestPayload = {
  teamId: 'team-1',
  requesterAgentId: 'leader-1',
  requesterRole: 'leader',
  newAgentId: 'worker-1',
  roleProfileId: 'programmer',
  engine: 'codex',
  runtimeProvider: 'codex-native'
};

function lifecycle(sequence: number, state: RecruitLifecyclePayload['state']): RecruitLifecyclePayload {
  return {
    teamId: 'team-1',
    agentId: 'worker-1',
    roleProfileId: 'programmer',
    sequence,
    state,
    endpointId: null,
    sessionId: null,
    taskIds: [],
    reason: state === 'failed' ? 'spawn failed' : null
  };
}

describe('useRecruitLifecycleProjection', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    eventHarness.listeners.clear();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('requestを即時placeholderへ投影し、out-of-order sequenceを破棄する', async () => {
    render(<ProjectionHarness />);
    await act(async () => undefined);
    act(() => emit<RecruitRequestPayload>('team:recruit-request', request));
    expect(screen.getByTestId('worker-1')).toHaveAttribute('data-state', 'requested');

    act(() => emit('team:recruit-lifecycle', lifecycle(2, 'spawning')));
    act(() => emit('team:recruit-lifecycle', lifecycle(1, 'requested')));
    expect(screen.getByTestId('worker-1')).toHaveAttribute('data-state', 'spawning');
    expect(screen.getByTestId('worker-1')).toHaveAttribute('data-sequence', '2');

    act(() => emit('team:recruit-lifecycle', lifecycle(3, 'ready')));
    expect(screen.getByTestId('worker-1')).toHaveAttribute('data-state', 'ready');
    expect(screen.getByTestId('worker-1')).toHaveAttribute('data-exiting', 'false');
  });

  it('failed/cancelledをwithdraw状態へ投影してmotion完了後に除去する', async () => {
    render(<ProjectionHarness />);
    await act(async () => undefined);
    act(() => emit<RecruitRequestPayload>('team:recruit-request', request));
    act(() => emit('team:recruit-lifecycle', lifecycle(4, 'failed')));
    expect(screen.getByTestId('worker-1')).toHaveAttribute('data-exiting', 'true');
    act(() => vi.advanceTimersByTime(280));
    expect(screen.queryByTestId('worker-1')).toBeNull();

    act(() => emit<RecruitRequestPayload>('team:recruit-request', request));
    act(() =>
      emit<RecruitCancelledPayload>('team:recruit-cancelled', {
        newAgentId: 'worker-1',
        reason: 'cancelled by leader'
      })
    );
    expect(screen.getByTestId('worker-1')).toHaveAttribute('data-state', 'cancelled');
  });
});
