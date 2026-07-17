import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ApprovalCenter } from './ApprovalCenter';
import { TeamInspector } from './TeamInspector';

const harness = vi.hoisted(() => ({
  context: {} as Record<string, unknown>
}));

vi.mock('../../lib/i18n', () => ({
  useT: () => (key: string) => key
}));

vi.mock('./TeamProjectionProvider', () => ({
  useTeamProjection: () => harness.context
}));

const approval = {
  endpointId: 'endpoint-1', agentId: 'agent-1', agentTitle: 'Agent One', requestedAt: '2026-07-17T00:00:00Z',
  requestId: 'request-1', method: 'command/requestApproval', reason: 'run tests', command: 'npm test', cwd: null
};

describe('Team controls keyboard interaction', () => {
  beforeEach(() => {
    vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
      callback(0);
      return 1;
    });
  });
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it('moves pending approval focus with ArrowDown and exposes all decisions as buttons', () => {
    harness.context = {
      approvalsOpen: true,
      setApprovalsOpen: vi.fn(),
      projection: { approvals: [approval, { ...approval, requestId: 'request-2', agentTitle: 'Agent Two' }] },
      respondApproval: vi.fn()
    };
    render(<ApprovalCenter />);
    const items = screen.getAllByRole('listitem');
    items[0].focus();
    fireEvent.keyDown(items[0], { key: 'ArrowDown' });
    expect(items[1]).toHaveFocus();
    expect(screen.getAllByRole('button', { name: 'v2.approval.accept' })).toHaveLength(2);
    expect(screen.getAllByRole('button', { name: 'v2.approval.acceptSession' })).toHaveLength(2);
    expect(screen.getAllByRole('button', { name: 'v2.approval.decline' })).toHaveLength(2);
    expect(screen.getAllByRole('button', { name: 'v2.approval.cancel' })).toHaveLength(2);
  });

  it('uses roving tabs in Inspector and opens Terminal only from the explicit action', () => {
    const openTerminal = vi.fn();
    harness.context = {
      inspectorOpen: true,
      setInspectorOpen: vi.fn(),
      selectedAgent: {
        agentId: 'agent-1', title: 'Agent One', status: 'ready', task: null,
        endpoint: null, changedFiles: [], worktree: { label: 'Phase 6' }, runtime: null
      },
      projection: { runtimeDroppedCount: 0 },
      openTerminal
    };
    render(<TeamInspector />);
    const diffTab = screen.getByRole('tab', { name: 'v2.inspector.tab.diff' });
    diffTab.focus();
    fireEvent.keyDown(diffTab, { key: 'ArrowRight' });
    expect(screen.getByRole('tab', { name: 'v2.inspector.tab.test' })).toHaveFocus();
    fireEvent.click(screen.getByRole('button', { name: 'v2.inspector.openTerminal' }));
    expect(openTerminal).toHaveBeenCalledWith('agent-1');
  });
});
