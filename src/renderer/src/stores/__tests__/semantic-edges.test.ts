import { beforeEach, describe, expect, it } from 'vitest';
import { useSemanticEdgeStore } from '../semantic-edges';

describe('semantic edge store', () => {
  beforeEach(() => useSemanticEdgeStore.setState({ seen: new Set<string>() }));

  it('suppresses an edge after the first pulse across component consumers', () => {
    expect(useSemanticEdgeStore.getState().markSeen('delegation:team-1:task-1')).toBe(true);
    expect(useSemanticEdgeStore.getState().markSeen('delegation:team-1:task-1')).toBe(false);
  });
});
