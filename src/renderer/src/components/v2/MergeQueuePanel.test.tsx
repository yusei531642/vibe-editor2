import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { MergeCandidateSnapshot, WorktreeManagerSnapshot } from '../../../../types/shared';
import { MergeQueuePanel } from './MergeQueuePanel';
import { CandidateForm } from './TeamInspector';

const harness = vi.hoisted(() => ({
  context: {} as Record<string, unknown>
}));

vi.mock('../../lib/i18n', () => ({
  useT: () => (key: string) => key
}));

vi.mock('./TeamProjectionProvider', () => ({
  useTeamProjection: () => harness.context
}));

function candidate(
  status: MergeCandidateSnapshot['status'],
  conflict: MergeCandidateSnapshot['conflict'] = null
): MergeCandidateSnapshot {
  return {
    id: 'candidate-1',
    teamId: 'team-1',
    agentId: 'worker-1',
    commit: 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
    evidence: 'npm test: pass',
    baseCommit: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
    changedPaths: ['src/feature.ts'],
    queuePosition: 1,
    status,
    conflict
  };
}

function snapshot(item: MergeCandidateSnapshot): WorktreeManagerSnapshot {
  return {
    teamId: 'team-1',
    assignments: [],
    candidates: [item],
    reviewRequired: true,
    integrationInProgress: false
  };
}

afterEach(cleanup);

describe('MergeQueuePanel state transitions', () => {
  it('requires review before enabling the user-triggered integrate action', async () => {
    const runWorktreeCommand = vi.fn().mockResolvedValue(true);
    harness.context = {
      worktreeSnapshot: snapshot(candidate('pendingReview')),
      runWorktreeCommand
    };
    const { rerender } = render(<MergeQueuePanel />);
    expect(screen.getByRole('button', { name: 'v2.mergeQueue.integrate' })).toBeDisabled();

    fireEvent.click(screen.getByRole('button', { name: 'v2.mergeQueue.approve' }));
    await waitFor(() =>
      expect(runWorktreeCommand).toHaveBeenCalledWith({
        action: 'review',
        candidateId: 'candidate-1',
        decision: 'approve'
      })
    );

    harness.context = {
      worktreeSnapshot: snapshot(candidate('approved')),
      runWorktreeCommand
    };
    rerender(<MergeQueuePanel />);
    const integrate = screen.getByRole('button', { name: 'v2.mergeQueue.integrate' });
    expect(integrate).toBeEnabled();
    fireEvent.click(integrate);
    await waitFor(() =>
      expect(runWorktreeCommand).toHaveBeenCalledWith({
        action: 'integrate',
        candidateId: 'candidate-1'
      })
    );
  });

  it('surfaces conflict path, updated base commit, and candidate commit', () => {
    harness.context = {
      worktreeSnapshot: snapshot(
        candidate('conflict', {
          paths: ['src/conflicted.ts'],
          baseCommit: 'cccccccccccccccccccccccccccccccccccccccc',
          candidateCommit: 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'
        })
      ),
      runWorktreeCommand: vi.fn().mockResolvedValue(true)
    };
    render(<MergeQueuePanel />);
    const inspector = screen.getByRole('region', { name: 'v2.mergeQueue.conflict' });
    expect(inspector).toHaveTextContent('src/conflicted.ts');
    expect(inspector).toHaveTextContent('cccccccccccccccccccccccccccccccccccccccc');
    expect(inspector).toHaveTextContent('bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb');
    expect(screen.getByRole('button', { name: 'v2.mergeQueue.integrate' })).toBeDisabled();
  });

  it('allows the leader to cancel a queued candidate', async () => {
    const runWorktreeCommand = vi.fn().mockResolvedValue(true);
    harness.context = {
      worktreeSnapshot: snapshot(candidate('pendingReview')),
      runWorktreeCommand
    };
    render(<MergeQueuePanel />);
    fireEvent.click(screen.getByRole('button', { name: 'v2.mergeQueue.cancel' }));
    await waitFor(() =>
      expect(runWorktreeCommand).toHaveBeenCalledWith({
        action: 'cancel',
        candidateId: 'candidate-1'
      })
    );
  });

  it('disables enqueue in flight and clears evidence after success', async () => {
    let resolveCommand: ((value: boolean) => void) | undefined;
    const runWorktreeCommand = vi.fn().mockReturnValue(
      new Promise<boolean>((resolve) => {
        resolveCommand = resolve;
      })
    );
    harness.context = { runWorktreeCommand };
    render(<CandidateForm agentId="worker-1" />);
    const evidence = screen.getByLabelText('v2.worktree.evidence');
    fireEvent.change(evidence, { target: { value: 'cargo test: pass' } });
    fireEvent.click(screen.getByRole('button', { name: 'v2.worktree.enqueue' }));
    expect(screen.getByRole('button', { name: 'v2.worktree.enqueue' })).toBeDisabled();
    expect(runWorktreeCommand).toHaveBeenCalledWith({
      action: 'enqueue',
      agentId: 'worker-1',
      evidence: 'cargo test: pass'
    });
    resolveCommand?.(true);
    await waitFor(() => expect(evidence).toHaveValue(''));
    expect(screen.getByRole('button', { name: 'v2.worktree.enqueue' })).toBeEnabled();
  });
});
