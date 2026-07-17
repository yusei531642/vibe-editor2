/** Issue #26: task/report edge の再 pulse を component remount 間で抑止する runtime store。 */
import { create } from 'zustand';

const SEMANTIC_EDGE_HISTORY_LIMIT = 1_000;

interface SemanticEdgeState {
  seen: Set<string>;
  markSeen: (edgeId: string) => boolean;
}

export const useSemanticEdgeStore = create<SemanticEdgeState>((set, get) => ({
  seen: new Set<string>(),
  markSeen: (edgeId) => {
    if (get().seen.has(edgeId)) return false;
    const seen = new Set(get().seen);
    seen.add(edgeId);
    while (seen.size > SEMANTIC_EDGE_HISTORY_LIMIT) {
      const oldest = seen.values().next().value;
      if (oldest === undefined) break;
      seen.delete(oldest);
    }
    set({ seen });
    return true;
  }
}));
