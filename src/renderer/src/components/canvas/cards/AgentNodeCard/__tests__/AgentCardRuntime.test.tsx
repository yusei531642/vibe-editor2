import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { AgentCardRuntime } from '../AgentCardRuntime';

const harness = vi.hoisted(() => ({
  dispatch: vi.fn().mockResolvedValue(undefined),
  inspect: vi.fn()
}));

vi.mock('../../../../../lib/i18n', () => ({
  useT: () => (key: string) => key
}));

vi.mock('../../../../v2/TeamProjectionProvider', () => ({
  useTeamProjection: () => ({
    projection: {
      agents: [{
        agentId: 'agent-1', status: 'running', task: { description: 'Implement cards' },
        latestTool: { toolName: 'apply_patch', status: 'completed' }, latestDiff: null, latestUsage: null
      }]
    },
    dispatchAgentAction: harness.dispatch,
    openInspector: harness.inspect
  })
}));

describe('AgentCardRuntime actions', () => {
  afterEach(() => {
    cleanup();
    harness.dispatch.mockClear();
    harness.inspect.mockClear();
  });

  it('dispatches steer, pause, stop and confirmed dismiss from card controls', async () => {
    render(<AgentCardRuntime agentId="agent-1" />);
    fireEvent.change(screen.getByRole('textbox', { name: 'v2.team.card.steerInput' }), {
      target: { value: 'continue with tests' }
    });
    fireEvent.click(screen.getByRole('button', { name: 'v2.team.card.steer' }));
    await waitFor(() => expect(harness.dispatch).toHaveBeenCalledWith('agent-1', 'steer', 'continue with tests'));
    fireEvent.click(screen.getByRole('button', { name: 'v2.team.card.pause' }));
    await waitFor(() => expect(harness.dispatch).toHaveBeenCalledWith('agent-1', 'interrupt', ''));
    fireEvent.click(screen.getByRole('button', { name: 'v2.team.card.stop' }));
    await waitFor(() => expect(harness.dispatch).toHaveBeenCalledWith('agent-1', 'stop', ''));
    fireEvent.click(screen.getByRole('button', { name: 'v2.team.card.dismiss' }));
    fireEvent.click(screen.getByRole('button', { name: 'v2.team.card.confirmDismiss' }));
    await waitFor(() => expect(harness.dispatch).toHaveBeenCalledWith('agent-1', 'dismiss', ''));
  });

  it('opens the selected agent in Inspector', () => {
    render(<AgentCardRuntime agentId="agent-1" />);
    fireEvent.click(screen.getByRole('button', { name: 'v2.team.card.inspect' }));
    expect(harness.inspect).toHaveBeenCalledWith('agent-1');
  });
});
