import { useMemo } from 'react';
import type { Edge, Node } from '@xyflow/react';
import { agentPayloadOf, cardTeamId, type CardData } from '../stores/canvas';
import { useCanvasEdges, useCanvasNodes } from '../stores/canvas-selectors';

/** Team scene では選択中 team の GUI カードと、その内部 edge だけを描画する。 */
export function filterCanvasGraphForTeam(
  nodes: Node<CardData>[],
  edges: Edge[],
  teamId?: string
): { nodes: Node<CardData>[]; edges: Edge[] } {
  if (!teamId) return { nodes, edges };
  const scopedNodes = nodes.filter((node) => {
    if (cardTeamId(node.data) !== teamId || node.data.cardType === 'terminal') return false;
    return agentPayloadOf(node.data)?.runtimeProvider !== 'pty';
  });
  const scopedIds = new Set(scopedNodes.map((node) => node.id));
  return {
    nodes: scopedNodes,
    edges: edges.filter((edge) => scopedIds.has(edge.source) && scopedIds.has(edge.target))
  };
}

export function useTeamCanvasGraph(teamId?: string): { nodes: Node<CardData>[]; edges: Edge[] } {
  const nodes = useCanvasNodes();
  const edges = useCanvasEdges();
  return useMemo(() => filterCanvasGraphForTeam(nodes, edges, teamId), [edges, nodes, teamId]);
}

export function filterCanvasAgents(nodes: Node<CardData>[], teamId?: string): Node<CardData>[] {
  return nodes.filter((node) => {
    const payload = agentPayloadOf(node.data);
    return payload !== undefined && (!teamId || payload.teamId === teamId);
  });
}

export function findTeamAgentNode(
  nodes: Node<CardData>[],
  agentId: string,
  teamId?: string
): Node<CardData> | undefined {
  return nodes.find((node) => {
    const payload = agentPayloadOf(node.data);
    return payload?.agentId === agentId && (!teamId || payload.teamId === teamId);
  });
}
