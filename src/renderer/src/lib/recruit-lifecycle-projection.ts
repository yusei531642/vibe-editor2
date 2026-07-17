import { useEffect, useReducer, useRef } from 'react';
import { subscribeEvent } from './subscribe-event';
import type {
  RecruitCancelledPayload,
  RecruitLifecyclePayload,
  RecruitLifecycleState,
  RecruitRequestPayload
} from '../../../types/shared';

export interface RecruitProjection {
  agentId: string;
  teamId: string;
  roleProfileId: string;
  sequence: number;
  state: RecruitLifecycleState;
  reason: string | null;
  exiting: boolean;
}

export type RecruitProjectionAction =
  | { type: 'request'; payload: RecruitRequestPayload }
  | { type: 'lifecycle'; payload: RecruitLifecyclePayload }
  | { type: 'cancelled'; payload: RecruitCancelledPayload }
  | { type: 'remove'; agentId: string };

export type RecruitProjectionState = Record<string, RecruitProjection>;

/** sequence が後退・重複する lifecycle event は UI state へ投影しない。 */
export function projectRecruitLifecycle(
  state: RecruitProjectionState,
  action: RecruitProjectionAction
): RecruitProjectionState {
  if (action.type === 'request') {
    const payload = action.payload;
    const current = state[payload.newAgentId];
    // 進行中 entry は維持するが、terminal / 撤収中の残骸は新 epoch で上書きする
    // (cancelled が書いた MAX_SAFE_INTEGER sequence を持ち越さない、PR #35 レビュー)。
    if (
      current &&
      !current.exiting &&
      current.state !== 'failed' &&
      current.state !== 'cancelled'
    ) {
      return state;
    }
    return {
      ...state,
      [payload.newAgentId]: {
        agentId: payload.newAgentId,
        teamId: payload.teamId,
        roleProfileId: payload.roleProfileId,
        sequence: -1,
        state: 'requested',
        reason: null,
        exiting: false
      }
    };
  }

  if (action.type === 'lifecycle') {
    const payload = action.payload;
    const current = state[payload.agentId];
    if (current && payload.sequence <= current.sequence) return state;
    const exiting = payload.state === 'failed' || payload.state === 'cancelled';
    return {
      ...state,
      [payload.agentId]: {
        agentId: payload.agentId,
        teamId: payload.teamId,
        roleProfileId: payload.roleProfileId,
        sequence: payload.sequence,
        state: payload.state,
        reason: payload.reason,
        exiting
      }
    };
  }

  if (action.type === 'cancelled') {
    const current = state[action.payload.newAgentId];
    if (!current) return state;
    return {
      ...state,
      [current.agentId]: {
        ...current,
        sequence: Number.MAX_SAFE_INTEGER,
        state: 'cancelled',
        reason: action.payload.reason,
        exiting: true
      }
    };
  }

  if (!(action.agentId in state)) return state;
  const next = { ...state };
  delete next[action.agentId];
  return next;
}

const WITHDRAW_MS = 280;

export function useRecruitLifecycleProjection(): RecruitProjection[] {
  const [state, dispatch] = useReducer(projectRecruitLifecycle, {});
  const withdrawTimers = useRef(new Map<string, number>());
  const latestSequence = useRef(new Map<string, number>());

  useEffect(() => {
    const timers = withdrawTimers.current;
    const sequences = latestSequence.current;
    const unlistens = [
      subscribeEvent<RecruitRequestPayload>('team:recruit-request', (payload) => {
        // 新しい recruit-request は新 epoch: 同一 agentId の再採用が前回の
        // sequence (特に cancelled 時の MAX_SAFE_INTEGER 番兵) にブロックされないよう
        // 必ず reset する (PR #35 レビュー)。
        sequences.set(payload.newAgentId, -1);
        // 前回 failed/cancelled の withdraw timer が残っていると、新 epoch の card を
        // WITHDRAW_MS 後に消してしまうため解除する (PR #35 レビュー)。
        const pendingTimer = timers.get(payload.newAgentId);
        if (pendingTimer !== undefined) {
          window.clearTimeout(pendingTimer);
          timers.delete(payload.newAgentId);
        }
        dispatch({ type: 'request', payload });
      }),
      subscribeEvent<RecruitLifecyclePayload>('team:recruit-lifecycle', (payload) => {
        const seen = sequences.get(payload.agentId) ?? -1;
        if (payload.sequence <= seen) return;
        sequences.set(payload.agentId, payload.sequence);
        const oldTimer = timers.get(payload.agentId);
        if (oldTimer !== undefined) window.clearTimeout(oldTimer);
        timers.delete(payload.agentId);
        dispatch({ type: 'lifecycle', payload });
        if (payload.state === 'failed' || payload.state === 'cancelled') {
          const timer = window.setTimeout(() => {
            timers.delete(payload.agentId);
            sequences.delete(payload.agentId);
            dispatch({ type: 'remove', agentId: payload.agentId });
          }, WITHDRAW_MS);
          timers.set(payload.agentId, timer);
        }
      }),
      subscribeEvent<RecruitCancelledPayload>('team:recruit-cancelled', (payload) => {
        sequences.set(payload.newAgentId, Number.MAX_SAFE_INTEGER);
        dispatch({ type: 'cancelled', payload });
        const oldTimer = timers.get(payload.newAgentId);
        if (oldTimer !== undefined) window.clearTimeout(oldTimer);
        const timer = window.setTimeout(() => {
          timers.delete(payload.newAgentId);
          sequences.delete(payload.newAgentId);
          dispatch({ type: 'remove', agentId: payload.newAgentId });
        }, WITHDRAW_MS);
        timers.set(payload.newAgentId, timer);
      })
    ];

    return () => {
      unlistens.forEach((unsubscribe) => unsubscribe());
      timers.forEach((timer) => window.clearTimeout(timer));
      timers.clear();
      sequences.clear();
    };
  }, []);

  return Object.values(state);
}
