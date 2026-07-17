import type {
  WorktreeCommandRequest,
  WorktreeCommandResult,
  WorktreeManagerSnapshot,
  WorktreeSnapshotRequest
} from '../../../../types/shared';
import { invokeCommand } from './command-error';

export const worktree = {
  snapshot: (request: WorktreeSnapshotRequest): Promise<WorktreeManagerSnapshot> =>
    invokeCommand('worktree_manager_snapshot', { request }),
  command: (request: WorktreeCommandRequest): Promise<WorktreeCommandResult> =>
    invokeCommand('worktree_manager_command', { request })
};
