/** Issue #27: renderer-visible projection. Managed filesystem paths never cross IPC. */
export interface WorktreeAssignmentSnapshot {
  teamId: string;
  agentId: string;
  branchName: string;
  baseBranch: string;
  baseCommit: string;
  headCommit: string;
  clean: boolean;
  cleanupEligible: boolean;
}

export type MergeCandidateStatus =
  | 'pendingReview'
  | 'approved'
  | 'changesRequested'
  | 'integrating'
  | 'integrated'
  | 'conflict'
  | 'failed'
  | 'cancelled';

export interface MergeConflictSnapshot {
  paths: string[];
  baseCommit: string;
  candidateCommit: string;
}

export interface MergeCandidateSnapshot {
  id: string;
  teamId: string;
  agentId: string;
  commit: string;
  evidence: string;
  baseCommit: string;
  changedPaths: string[];
  queuePosition: number;
  status: MergeCandidateStatus;
  conflict: MergeConflictSnapshot | null;
}

export interface WorktreeManagerSnapshot {
  teamId: string;
  assignments: WorktreeAssignmentSnapshot[];
  candidates: MergeCandidateSnapshot[];
  reviewRequired: true;
  integrationInProgress: boolean;
}

export interface WorktreeSnapshotRequest {
  projectRoot: string;
  teamId: string;
}

export type WorktreeCommand =
  | { action: 'create'; agentId: string }
  | { action: 'resume'; agentId: string }
  | { action: 'enqueue'; agentId: string; evidence: string }
  | { action: 'review'; candidateId: string; decision: 'approve' | 'requestChanges' }
  | { action: 'integrate'; candidateId: string }
  | { action: 'cleanup'; agentId: string }
  | { action: 'cancel'; candidateId: string };

export interface WorktreeCommandRequest extends WorktreeSnapshotRequest {
  command: WorktreeCommand;
}

export interface WorktreeCommandResult {
  action: WorktreeCommand['action'];
  snapshot: WorktreeManagerSnapshot;
}
