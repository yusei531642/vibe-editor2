import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { TeamAgentProjection } from '../../../../../lib/team-projection';
import { AgentChatSurface } from '../AgentChatSurface';

const catalog = vi.hoisted(() => ({ models: [] as Array<{
  id: string; label: string; supportedEfforts: string[]; defaultEffort: string;
}> }));
vi.mock('../../../../../lib/hooks/use-v2-runtime-catalog', () => ({
  useV2RuntimeCatalog: () => ({ models: catalog.models, loading: false, error: null })
}));

function agent(overrides: Partial<TeamAgentProjection> = {}): TeamAgentProjection {
  return {
    agentId: 'worker-1', cardId: 'card-1', title: 'Worker', roleProfileId: 'worker',
    status: 'ready', recruit: null, task: null, endpoint: null, runtime: null,
    changedFiles: [], latestTool: null, latestDiff: null, latestUsage: null,
    approvals: [], worktree: null, ...overrides
  };
}

const endpoint = (live: boolean): NonNullable<TeamAgentProjection['endpoint']> => ({
  teamId: 'team-1', agentId: 'worker-1', endpointId: 'native-1', backend: 'native',
  sessionId: null, taskIds: [], live, provider: 'claude-native', restoreState: live ? 'live' : 'reconnectable'
});

describe('AgentChatSurface keyboard submit', () => {
  beforeEach(() => { catalog.models = []; });

  it('Enter 送信にも空入力・busy・unavailable のガードを適用する', () => {
    const onSubmit = vi.fn();
    const props = {
      agent: agent(), instruction: '', busyAction: null, confirmingDismiss: false,
      onInstructionChange: vi.fn(), onRuntimePatch: vi.fn(), onSubmit,
      onAction: vi.fn(), onInspect: vi.fn(), onConfirmingDismissChange: vi.fn(),
      t: (key: string) => key
    };
    const { rerender } = render(<AgentChatSurface {...props} />);
    const input = screen.getByRole('textbox', { name: 'v2.team.card.steerInput' });

    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSubmit).not.toHaveBeenCalled();

    rerender(<AgentChatSurface {...props} instruction="continue" busyAction="stop" />);
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSubmit).not.toHaveBeenCalled();

    rerender(<AgentChatSurface {...props} agent={agent({ endpoint: endpoint(false) })} instruction="continue" />);
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSubmit).not.toHaveBeenCalled();

    rerender(<AgentChatSurface {...props} agent={agent({ endpoint: endpoint(true) })} instruction="continue" />);
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSubmit).toHaveBeenCalledTimes(1);
  });

  it('catalog の既定 model/effort を未確定 payload へ同期する', async () => {
    catalog.models = [{
      id: 'claude-fable-5', label: 'Fable', supportedEfforts: ['high'], defaultEffort: 'high'
    }];
    const onRuntimePatch = vi.fn();
    render(<AgentChatSurface
      agent={agent()}
      payload={{ runtimeProvider: 'claude-native' }}
      instruction=""
      busyAction={null}
      confirmingDismiss={false}
      onInstructionChange={vi.fn()}
      onRuntimePatch={onRuntimePatch}
      onSubmit={vi.fn()}
      onAction={vi.fn()}
      onInspect={vi.fn()}
      onConfirmingDismissChange={vi.fn()}
      t={(key) => key}
    />);

    await waitFor(() => expect(onRuntimePatch).toHaveBeenCalledWith({
      runtimeModel: 'claude-fable-5', runtimeEffort: 'high'
    }));
  });
});
