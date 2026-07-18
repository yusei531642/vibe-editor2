import { describe, expect, it } from 'vitest';
import type { Edge, Node } from '@xyflow/react';
import { filterCanvasGraphForTeam } from '../../../lib/canvas-team-scope';
import type { CardData } from '../../../stores/canvas';

function node(id: string, teamId: string, cardType: 'agent' | 'terminal' = 'agent'): Node<CardData> {
  return {
    id,
    type: cardType,
    position: { x: 0, y: 0 },
    data: {
      cardType,
      title: id,
      payload: { teamId, agentId: id, agent: 'claude' }
    } as CardData
  };
}

function legacyPtyAgent(id: string, teamId: string): Node<CardData> {
  const item = node(id, teamId);
  if (item.data.cardType === 'agent' && item.data.payload) {
    item.data.payload.runtimeProvider = 'pty';
  }
  return item;
}

describe('Team Canvas scope', () => {
  it('選択中 team の GUI カードと内部 edge だけを描画する', () => {
    const nodes = [node('leader-a', 'team-a'), node('worker-a', 'team-a'), node('leader-b', 'team-b')];
    const edges: Edge[] = [
      { id: 'inside', source: 'leader-a', target: 'worker-a' },
      { id: 'outside', source: 'leader-a', target: 'leader-b' }
    ];

    const scoped = filterCanvasGraphForTeam(nodes, edges, 'team-a');

    expect(scoped.nodes.map((item) => item.id)).toEqual(['leader-a', 'worker-a']);
    expect(scoped.edges.map((item) => item.id)).toEqual(['inside']);
  });

  it('同じ team の旧 terminal / PTY Agent カードも Team scene には表示しない', () => {
    const scoped = filterCanvasGraphForTeam(
      [
        node('leader', 'team-a'),
        node('legacy-terminal', 'team-a', 'terminal'),
        legacyPtyAgent('legacy-pty-agent', 'team-a')
      ],
      [],
      'team-a'
    );

    expect(scoped.nodes.map((item) => item.id)).toEqual(['leader']);
  });
});
