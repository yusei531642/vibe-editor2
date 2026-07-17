import { create } from 'zustand';
import type { TeamProjectionSnapshot } from '../../../types/shared';

interface SessionRestoreState {
  snapshot: TeamProjectionSnapshot | null;
  setSnapshot: (snapshot: TeamProjectionSnapshot | null) => void;
}

export const useSessionRestoreStore = create<SessionRestoreState>((set) => ({
  snapshot: null,
  setSnapshot: (snapshot) => set({ snapshot })
}));
